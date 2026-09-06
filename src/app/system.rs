//! Background collector and GTK controller for generic Linux metrics.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use gtk4::glib;

use crate::domain::system::{
    CpuCounters, GpuSnapshot, NetworkCounters, SystemSnapshot, TemperatureSource,
    cpu_usage_percent, format_memory_usage, memory_usage_bytes, network_rates, parse_cpu_counters,
    parse_memory_info, parse_network_counters, swap_usage_bytes,
};
use crate::ui::window::PanelWidgets;

const PROC_STAT: &str = "/proc/stat";
const PROC_MEMINFO: &str = "/proc/meminfo";
const PROC_NET_DEV: &str = "/proc/net/dev";
const PROC_UPTIME: &str = "/proc/uptime";
const THERMAL_ROOT: &str = "/sys/class/thermal";
const HWMON_ROOT: &str = "/sys/class/hwmon";
const DRM_ROOT: &str = "/sys/class/drm";

#[derive(Debug, Clone)]
struct CounterSample {
    cpu: Option<CpuCounters>,
    network: Option<NetworkCounters>,
    at: Instant,
}

#[derive(Default)]
struct SystemCollector {
    previous: Option<CounterSample>,
}

impl SystemCollector {
    fn sample(&mut self) -> SystemSnapshot {
        let now = Instant::now();
        let cpu = read_file(PROC_STAT).and_then(|contents| parse_cpu_counters(&contents));
        let memory = read_file(PROC_MEMINFO).and_then(|contents| parse_memory_info(&contents));
        let network = read_file(PROC_NET_DEV).map(|contents| parse_network_counters(&contents));
        let uptime_seconds = read_file(PROC_UPTIME).and_then(|contents| {
            contents
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| value as u64)
        });

        let (cpu_usage, rx_rate, tx_rate) = match (self.previous.as_ref(), cpu, network.clone()) {
            (Some(previous), current_cpu, Some(current_network)) => {
                let elapsed = now.duration_since(previous.at).as_secs_f64();
                let cpu_usage = match (previous.cpu, current_cpu) {
                    (Some(previous), Some(current)) => cpu_usage_percent(previous, current),
                    _ => None,
                };
                let (rx_rate, tx_rate) = previous
                    .network
                    .as_ref()
                    .map(|previous| network_rates(&current_network, previous, elapsed))
                    .unwrap_or((None, None));
                (cpu_usage, rx_rate, tx_rate)
            }
            _ => (None, None, None),
        };

        self.previous = Some(CounterSample { cpu, network, at: now });

        let memory_usage = memory.and_then(memory_usage_bytes);
        let swap_usage = memory.and_then(swap_usage_bytes);
        let root_used = root_filesystem_usage();
        let temperature = cpu_temperature();
        let gpu = gpu_snapshot();

        SystemSnapshot {
            cpu_usage_percent: cpu_usage,
            cpu_temperature_c: temperature,
            memory_used_percent: memory.and_then(crate::domain::system::memory_used_percent),
            memory_used_bytes: memory_usage.map(|(used, _)| used),
            memory_total_bytes: memory_usage.map(|(_, total)| total),
            swap_used_percent: memory.and_then(crate::domain::system::swap_used_percent),
            swap_used_bytes: swap_usage.map(|(used, _)| used),
            swap_total_bytes: swap_usage.map(|(_, total)| total),
            root_used_percent: root_used,
            rx_bytes_per_second: rx_rate,
            tx_bytes_per_second: tx_rate,
            uptime_seconds,
            gpu,
        }
    }
}

/// Start the non-blocking system sampler. Filesystem reads happen on a worker
/// thread; only the channel-draining GTK callback mutates widgets.
pub(crate) fn setup(widgets: &PanelWidgets, panel_visible: Arc<AtomicBool>) {
    let collector = Arc::new(Mutex::new(SystemCollector::default()));
    let (sender, receiver) = mpsc::channel::<SystemSnapshot>();
    let in_flight = Arc::new(AtomicBool::new(false));

    let system = widgets.system.clone();
    let home = widgets.home.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        while let Ok(snapshot) = receiver.try_recv() {
            system.render(&snapshot);
            if let (Some(cpu), Some(used), Some(total)) = (
                snapshot.cpu_usage_percent,
                snapshot.memory_used_bytes,
                snapshot.memory_total_bytes,
            ) {
                if let Some(memory) = format_memory_usage(used, total) {
                    home.set_system_status(Some(&format!("CPU {cpu:.0}% · RAM {memory}")));
                } else {
                    home.set_system_status(Some("System metrics loading…"));
                }
            } else {
                home.set_system_status(Some("System metrics loading…"));
            }
        }
        glib::ControlFlow::Continue
    });

    let schedule = {
        let collector = Arc::clone(&collector);
        let sender = sender.clone();
        let in_flight = Arc::clone(&in_flight);
        move || {
            if in_flight.swap(true, Ordering::AcqRel) {
                return;
            }
            let collector = Arc::clone(&collector);
            let sender = sender.clone();
            let in_flight = Arc::clone(&in_flight);
            std::thread::spawn(move || {
                let snapshot =
                    collector.lock().map(|mut collector| collector.sample()).unwrap_or_default();
                let _ = sender.send(snapshot);
                in_flight.store(false, Ordering::Release);
            });
        }
    };

    // Take one immediate sample for Home and the detail page, then sample at
    // the requested cadence while the panel is visible.
    schedule();
    let schedule_periodic = schedule;
    glib::timeout_add_local(Duration::from_secs(2), move || {
        if panel_visible.load(Ordering::Relaxed) {
            schedule_periodic();
        }
        glib::ControlFlow::Continue
    });
}

fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn root_filesystem_usage() -> Option<f64> {
    let stats = rustix::fs::statvfs("/").ok()?;
    let block_size = if stats.f_frsize > 0 { stats.f_frsize } else { stats.f_bsize };
    let total = stats.f_blocks.checked_mul(block_size)?;
    let available = stats.f_bavail.checked_mul(block_size)?;
    if total == 0 || available > total {
        return None;
    }
    Some((1.0 - available as f64 / total as f64) * 100.0)
}

fn cpu_temperature() -> Option<f64> {
    let mut sources = Vec::new();
    collect_thermal_sources(Path::new(THERMAL_ROOT), &mut sources);
    collect_hwmon_sources(Path::new(HWMON_ROOT), &mut sources, false);
    crate::domain::system::choose_temperature(&sources)
}

fn collect_thermal_sources(root: &Path, sources: &mut Vec<TemperatureSource>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(label) = read_trimmed(path.join("type")) else { continue };
        let Some(value) =
            read_trimmed(path.join("temp")).and_then(|value| value.parse::<f64>().ok())
        else {
            continue;
        };
        let Some(celsius) = millidegrees_to_celsius(value) else { continue };
        sources.push(TemperatureSource {
            label_matches: crate::domain::system::temperature_label_matches(&label, false),
            celsius,
        });
    }
}

fn collect_hwmon_sources(root: &Path, sources: &mut Vec<TemperatureSource>, gpu_only: bool) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = read_trimmed(path.join("name")).unwrap_or_default();
        let Ok(files) = std::fs::read_dir(&path) else { continue };
        for file in files.flatten() {
            let file_path = file.path();
            let Some(file_name) = file_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !file_name.starts_with("temp") || !file_name.ends_with("_input") {
                continue;
            }
            let index = file_name.trim_start_matches("temp").trim_end_matches("_input");
            let label = read_trimmed(path.join(format!("temp{index}_label")))
                .or_else(|| (!name.is_empty()).then_some(name.clone()))
                .unwrap_or_default();
            let Some(value) = read_trimmed(&file_path).and_then(|value| value.parse::<f64>().ok())
            else {
                continue;
            };
            let Some(celsius) = millidegrees_to_celsius(value) else { continue };
            sources.push(TemperatureSource {
                label_matches: crate::domain::system::temperature_label_matches(&label, gpu_only),
                celsius,
            });
        }
    }
}

fn millidegrees_to_celsius(value: f64) -> Option<f64> {
    let celsius = if value.abs() > 1_000.0 { value / 1_000.0 } else { value };
    celsius.is_finite().then_some(celsius)
}

fn gpu_snapshot() -> Option<GpuSnapshot> {
    let Ok(entries) = std::fs::read_dir(DRM_ROOT) else { return None };
    let mut utilization = None;
    let mut sources = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue };
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        if let Some(value) = read_trimmed(path.join("device/gpu_busy_percent"))
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && (0.0..=100.0).contains(value))
        {
            utilization = Some(value);
        }
        collect_hwmon_sources(&path.join("device/hwmon"), &mut sources, true);
    }
    let temperature = crate::domain::system::choose_temperature(&sources);
    let gpu = GpuSnapshot { utilization_percent: utilization, temperature_c: temperature };
    gpu.usable().then_some(gpu)
}

fn read_trimmed(path: impl Into<PathBuf>) -> Option<String> {
    std::fs::read_to_string(path.into()).ok().map(|value| value.trim().to_string())
}
