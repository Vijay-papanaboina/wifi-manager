//! Pure parsers and counter math for the generic Linux System page.

/// Aggregate CPU counters from the first `cpu` line in `/proc/stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CpuCounters {
    pub(crate) total: u64,
    pub(crate) idle: u64,
}

pub(crate) fn parse_cpu_counters(contents: &str) -> Option<CpuCounters> {
    let line = contents.lines().find(|line| {
        let mut fields = line.split_whitespace();
        fields.next() == Some("cpu")
    })?;
    let mut fields = line.split_whitespace();
    fields.next();
    let values = fields.map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().ok()?;
    if values.len() < 4 {
        return None;
    }
    let total = values.iter().copied().fold(0_u64, u64::saturating_add);
    let idle = values[3].saturating_add(values.get(4).copied().unwrap_or(0));
    Some(CpuCounters { total, idle })
}

/// Compute busy CPU percentage from monotonic counters. A reset or malformed
/// delta is intentionally unavailable rather than a misleading spike.
pub(crate) fn cpu_usage_percent(previous: CpuCounters, current: CpuCounters) -> Option<f64> {
    let total_delta = current.total.checked_sub(previous.total)?;
    let idle_delta = current.idle.checked_sub(previous.idle)?;
    if total_delta == 0 || idle_delta > total_delta {
        return None;
    }
    Some(((total_delta - idle_delta) as f64 / total_delta as f64 * 100.0).clamp(0.0, 100.0))
}

/// Parsed memory quantities in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MemoryInfo {
    pub(crate) total_bytes: u64,
    pub(crate) available_bytes: u64,
    pub(crate) swap_total_bytes: u64,
    pub(crate) swap_free_bytes: u64,
}

pub(crate) fn parse_memory_info(contents: &str) -> Option<MemoryInfo> {
    let mut total = None;
    let mut available = None;
    let mut swap_total = 0;
    let mut swap_free = 0;

    for line in contents.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let mut fields = rest.split_whitespace();
        let Some(value) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        let unit = fields.next().unwrap_or("");
        let multiplier = match unit {
            "kB" => 1024,
            "MB" => 1024 * 1024,
            "GB" => 1024 * 1024 * 1024,
            _ => 1,
        };
        let value = value.saturating_mul(multiplier);
        match name.trim() {
            "MemTotal" => total = Some(value),
            "MemAvailable" => available = Some(value),
            "SwapTotal" => swap_total = value,
            "SwapFree" => swap_free = value,
            _ => {}
        }
    }

    Some(MemoryInfo {
        total_bytes: total?,
        available_bytes: available?,
        swap_total_bytes: swap_total,
        swap_free_bytes: swap_free,
    })
}

pub(crate) fn memory_used_percent(memory: MemoryInfo) -> Option<f64> {
    if memory.total_bytes == 0 || memory.available_bytes > memory.total_bytes {
        return None;
    }
    Some((1.0 - memory.available_bytes as f64 / memory.total_bytes as f64) * 100.0)
}

pub(crate) fn swap_used_percent(memory: MemoryInfo) -> Option<f64> {
    if memory.swap_total_bytes == 0 || memory.swap_free_bytes > memory.swap_total_bytes {
        return None;
    }
    Some((1.0 - memory.swap_free_bytes as f64 / memory.swap_total_bytes as f64) * 100.0)
}

/// Return used and total physical memory bytes using MemAvailable rather than
/// summing process categories, which avoids double-counting the kernel cache.
pub(crate) fn memory_usage_bytes(memory: MemoryInfo) -> Option<(u64, u64)> {
    if memory.total_bytes == 0 || memory.available_bytes > memory.total_bytes {
        return None;
    }
    Some((memory.total_bytes - memory.available_bytes, memory.total_bytes))
}

/// Return used and total swap bytes, omitting systems with no configured swap.
pub(crate) fn swap_usage_bytes(memory: MemoryInfo) -> Option<(u64, u64)> {
    if memory.swap_total_bytes == 0 || memory.swap_free_bytes > memory.swap_total_bytes {
        return None;
    }
    Some((memory.swap_total_bytes - memory.swap_free_bytes, memory.swap_total_bytes))
}

pub(crate) fn format_memory_usage(used: u64, total: u64) -> Option<String> {
    if total == 0 || used > total {
        return None;
    }
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut divisor = 1_u64;
    let mut unit = 0;
    while total / divisor >= 1024 && unit < UNITS.len() - 1 {
        divisor = divisor.saturating_mul(1024);
        unit += 1;
    }
    if unit == 0 {
        Some(format!("{used} / {total} B"))
    } else {
        Some(format!(
            "{:.1} / {:.1} {}",
            used as f64 / divisor as f64,
            total as f64 / divisor as f64,
            UNITS[unit]
        ))
    }
}

/// Aggregate non-loopback network byte counters from `/proc/net/dev`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct NetworkCounters {
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
    pub(crate) interfaces: Vec<String>,
}

pub(crate) fn parse_network_counters(contents: &str) -> NetworkCounters {
    let mut result = NetworkCounters::default();
    for line in contents.lines() {
        let Some((name, values)) = line.split_once(':') else { continue };
        let name = name.trim();
        if name == "lo" {
            continue;
        }
        let fields = values.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 9 {
            continue;
        }
        let Ok(rx) = fields[0].parse::<u64>() else { continue };
        let Ok(tx) = fields[8].parse::<u64>() else { continue };
        result.rx_bytes = result.rx_bytes.saturating_add(rx);
        result.tx_bytes = result.tx_bytes.saturating_add(tx);
        result.interfaces.push(name.to_string());
    }
    result.interfaces.sort();
    result
}

/// Convert a monotonic byte-counter delta into bytes per second, returning no
/// value if an interface disappeared/reset or elapsed time is invalid.
pub(crate) fn counter_rate(current: u64, previous: u64, elapsed_seconds: f64) -> Option<f64> {
    if elapsed_seconds <= 0.0 || !elapsed_seconds.is_finite() {
        return None;
    }
    Some(current.checked_sub(previous)? as f64 / elapsed_seconds)
}

pub(crate) fn network_rates(
    current: &NetworkCounters,
    previous: &NetworkCounters,
    elapsed_seconds: f64,
) -> (Option<f64>, Option<f64>) {
    if current.interfaces != previous.interfaces {
        return (None, None);
    }
    (
        counter_rate(current.rx_bytes, previous.rx_bytes, elapsed_seconds),
        counter_rate(current.tx_bytes, previous.tx_bytes, elapsed_seconds),
    )
}

/// Round an uptime duration up before splitting hours and minutes. This keeps
/// `3599` at `1h 0m` and never emits `1h 60m`.
pub(crate) fn format_duration(seconds: u64) -> String {
    let total_minutes = seconds.div_ceil(60);
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours > 0 { format!("{hours}h {minutes}m") } else { format!("{minutes}m") }
}

/// A generic labeled temperature source (thermal zone or hwmon).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TemperatureSource {
    pub(crate) label_matches: bool,
    pub(crate) celsius: f64,
}

pub(crate) fn choose_temperature(sources: &[TemperatureSource]) -> Option<f64> {
    sources
        .iter()
        .find(|source| source.label_matches && source.celsius.is_finite())
        .map(|source| source.celsius)
}

/// Recognize generic Linux CPU/SoC thermal and hwmon driver labels. These
/// are capability labels, not machine-specific device configuration.
pub(crate) fn temperature_label_matches(label: &str, gpu_only: bool) -> bool {
    let label = label.to_ascii_lowercase();
    if gpu_only {
        ["gpu", "graphics", "video"].iter().any(|part| label.contains(part))
    } else {
        ["cpu", "soc", "package", "pkg", "core", "coretemp", "k10temp", "zenpower"]
            .iter()
            .any(|part| label.contains(part))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GpuSnapshot {
    pub(crate) utilization_percent: Option<f64>,
    pub(crate) temperature_c: Option<f64>,
}

impl GpuSnapshot {
    pub(crate) fn usable(self) -> bool {
        self.utilization_percent.is_some() || self.temperature_c.is_some()
    }
}

/// Values rendered by the UI after one background sample.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SystemSnapshot {
    pub(crate) cpu_usage_percent: Option<f64>,
    pub(crate) cpu_temperature_c: Option<f64>,
    pub(crate) memory_used_percent: Option<f64>,
    pub(crate) memory_used_bytes: Option<u64>,
    pub(crate) memory_total_bytes: Option<u64>,
    pub(crate) swap_used_percent: Option<f64>,
    pub(crate) swap_used_bytes: Option<u64>,
    pub(crate) swap_total_bytes: Option<u64>,
    pub(crate) root_used_percent: Option<f64>,
    pub(crate) rx_bytes_per_second: Option<f64>,
    pub(crate) tx_bytes_per_second: Option<f64>,
    pub(crate) uptime_seconds: Option<u64>,
    pub(crate) gpu: Option<GpuSnapshot>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cpu_and_rejects_counter_reset() {
        let first = parse_cpu_counters("cpu 10 0 20 30 5\ncpu0 1 2 3 4").unwrap();
        let second = parse_cpu_counters("cpu 20 0 30 40 5").unwrap();
        assert_eq!(first.total, 65);
        assert!(cpu_usage_percent(first, second).is_some());
        assert!(cpu_usage_percent(second, first).is_none());
    }

    #[test]
    fn parses_available_memory_and_swap() {
        let memory = parse_memory_info(
            "MemTotal:       100 kB\nMemAvailable:    40 kB\nSwapTotal: 10 kB\nSwapFree: 4 kB\n",
        )
        .unwrap();
        assert_eq!(memory.total_bytes, 100 * 1024);
        assert_eq!(memory.available_bytes, 40 * 1024);
        assert!((memory_used_percent(memory).unwrap() - 60.0).abs() < f64::EPSILON);
        assert!((swap_used_percent(memory).unwrap() - 60.0).abs() < f64::EPSILON);
        assert_eq!(memory_usage_bytes(memory), Some((60 * 1024, 100 * 1024)));
        assert_eq!(swap_usage_bytes(memory), Some((6 * 1024, 10 * 1024)));
    }

    #[test]
    fn network_parser_excludes_loopback_and_counter_rate_handles_reset() {
        let counters = parse_network_counters(
            "Inter-| Receive | Transmit\n lo: 100 0 0 0 0 0 0 0 200\n eth0: 10 0 0 0 0 0 0 0 20\n",
        );
        assert_eq!(
            counters,
            NetworkCounters { rx_bytes: 10, tx_bytes: 20, interfaces: vec!["eth0".to_string()] }
        );
        assert_eq!(counter_rate(30, 10, 2.0), Some(10.0));
        assert_eq!(counter_rate(10, 30, 2.0), None);
        let changed = NetworkCounters { interfaces: vec!["wlan0".to_string()], ..counters };
        assert_eq!(network_rates(&changed, &counters, 2.0), (None, None));
    }

    #[test]
    fn duration_rounding_is_stable() {
        assert_eq!(format_duration(59), "1m");
        assert_eq!(format_duration(3599), "1h 0m");
        assert_eq!(format_duration(7199), "2h 0m");
    }

    #[test]
    fn temperature_requires_a_generic_matching_label() {
        let sources = [
            TemperatureSource { label_matches: false, celsius: 99.0 },
            TemperatureSource { label_matches: true, celsius: 42.5 },
        ];
        assert_eq!(choose_temperature(&sources), Some(42.5));
        assert!(!GpuSnapshot { utilization_percent: None, temperature_c: None }.usable());
        assert!(temperature_label_matches("coretemp", false));
        assert!(temperature_label_matches("k10temp", false));
        assert!(temperature_label_matches("zenpower", false));
        assert!(!temperature_label_matches("coretemp", true));
    }

    #[test]
    fn formats_memory_with_compact_binary_units_and_rejects_invalid_ranges() {
        assert_eq!(format_memory_usage(0, 0), None);
        assert_eq!(format_memory_usage(0, 1024), Some("0.0 / 1.0 KB".to_string()));
        assert_eq!(format_memory_usage(6, 4), None);
        assert_eq!(format_memory_usage(1024, 2048), Some("1.0 / 2.0 KB".to_string()));
    }
}
