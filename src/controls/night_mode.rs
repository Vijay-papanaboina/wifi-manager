use std::collections::HashMap;
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use memmap2::MmapMut;
use rustix::fs::{MemfdFlags, memfd_create};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    protocol::{wl_output::WlOutput, wl_registry::WlRegistry},
};
use wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
    zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
};

#[derive(Default)]
struct AppState {
    outputs: Vec<WlOutput>,
    gamma_manager: Option<ZwlrGammaControlManagerV1>,
    gamma_controls: Vec<ZwlrGammaControlV1>,
    gamma_sizes: HashMap<ZwlrGammaControlV1, u32>,
}

impl Dispatch<WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wayland_client::protocol::wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_registry::Event::Global { name, interface, .. } = event
        {
            if interface == "wl_output" {
                state.outputs.push(registry.bind::<WlOutput, _, _>(name, 1, qh, ()));
            } else if interface == "zwlr_gamma_control_manager_v1" {
                state.gamma_manager =
                    Some(registry.bind::<ZwlrGammaControlManagerV1, _, _>(name, 1, qh, ()));
            }
        }
    }
}

impl Dispatch<WlOutput, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &WlOutput,
        _: <WlOutput as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrGammaControlManagerV1, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &ZwlrGammaControlManagerV1,
        _: <ZwlrGammaControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrGammaControlV1, ()> for AppState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrGammaControlV1,
        event: <ZwlrGammaControlV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_gamma_control_v1::Event::GammaSize { size } => {
                state.gamma_sizes.insert(proxy.clone(), size);
            }
            zwlr_gamma_control_v1::Event::Failed => {
                log::warn!("wl_output rejected gamma control; removing from state.");
                state.gamma_sizes.remove(proxy);
            }
            _ => {}
        }
    }
}

/// Default timeout for Wayland thread initialization.
pub(crate) const NIGHT_MODE_INIT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(1000);

/// Manages a background worker that applies color temperatures for night mode.
///
/// Two backends are supported, chosen automatically in [`NightModeManager::new`]:
/// - `wlr-gamma-control` (Sway, Hyprland, River, other wlroots compositors)
/// - KDE Plasma Night Light (KWin does not implement `wlr-gamma-control`)
pub(crate) struct NightModeManager {
    sender: Option<mpsc::Sender<f64>>,
    worker_handle: Option<thread::JoinHandle<()>>,
    external_state: Option<NightModeState>,
}

/// Night Mode state owned by the desktop rather than by this app
/// (currently only reported by the KDE backend).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NightModeState {
    pub(crate) enabled: bool,
    pub(crate) temperature: f64,
}

impl NightModeManager {
    /// Initializes a new NightModeManager. Tries the `wlr-gamma-control` backend
    /// first and falls back to the KDE backend when running under Plasma.
    pub(crate) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        match Self::new_wlr() {
            Ok(manager) => Ok(manager),
            Err(wlr_err) => {
                if kde_backend_available() {
                    log::info!(
                        "wlr-gamma-control unavailable ({wlr_err}); using KDE Night Light backend"
                    );
                    Self::new_kde()
                } else {
                    Err(wlr_err)
                }
            }
        }
    }

    /// Spawns a background thread to lock and manipulate the `wlr_gamma_control_v1`
    /// Wayland protocol outputs.
    fn new_wlr() -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel();

        // Initial temperature
        let initial_temp = 6500.0;
        let current_temp =
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(initial_temp as u32));
        let thread_temp = std::sync::Arc::clone(&current_temp);

        let handle = thread::spawn(move || {
            if let Err(e) = run_wayland_thread(rx, init_tx, initial_temp, thread_temp) {
                log::error!("Night mode wayland thread exited: {}", e);
            }
        });

        // Block on initialization status from Wayland thread
        match init_rx.recv_timeout(NIGHT_MODE_INIT_TIMEOUT) {
            Ok(Ok(())) => Ok(NightModeManager {
                sender: Some(tx),
                worker_handle: Some(handle),
                external_state: None,
            }),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(format!("Night mode wayland thread initialization failed: {}", e).into()),
        }
    }

    /// Reads KDE's current Night Light settings, then spawns the KDE worker thread.
    /// Nothing is written to `kwinrc` until a temperature is actually requested.
    fn new_kde() -> Result<Self, Box<dyn std::error::Error>> {
        let settings = read_kde_settings()
            .map_err(|error| format!("Could not read KDE Night Light settings: {error}"))?;
        let external_state = Some(settings.ui_state());
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || run_kde_thread(rx));
        Ok(NightModeManager { sender: Some(tx), worker_handle: Some(handle), external_state })
    }

    /// Sends a new color temperature (in Kelvin) down the channel to the worker thread.
    pub(crate) fn set_temperature(&self, temp: f64) -> Result<(), mpsc::SendError<f64>> {
        if let Some(tx) = &self.sender {
            match tx.send(temp) {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            }
        } else {
            Err(mpsc::SendError(temp))
        }
    }

    /// State read from the desktop at startup, if the desktop owns Night Mode.
    /// `None` means this app owns the state (wlr backend).
    pub(crate) fn external_state(&self) -> Option<NightModeState> {
        self.external_state
    }
}

impl Drop for NightModeManager {
    fn drop(&mut self) {
        let _ = self.sender.take();
        if let Some(handle) = self.worker_handle.take()
            && let Err(e) = handle.join()
        {
            log::error!("Failed to join night mode worker thread: {:?}", e);
        }
    }
}

/// Converts a given color temperature in Kelvin to RGB multipliers between 0.0 and 1.0.
/// Includes clamping to mathematically guard against invalid log functions.
fn color_temp_to_rgb(temp: f64) -> (f64, f64, f64) {
    // Clamp temperature to a sensible range (1000K to 40000K) to ensure
    // we never take the log of a non-positive number.
    let temp = temp.clamp(1000.0, 40000.0) / 100.0;

    let red = if temp <= 66.0 {
        255.0
    } else {
        let r = temp - 60.0;
        let r = 329.698727446 * (r.powf(-0.1332047592));
        r.clamp(0.0, 255.0)
    };

    let green = if temp <= 66.0 {
        let g = temp;
        let g = 99.4708025861 * g.ln() - 161.1195681661;
        g.clamp(0.0, 255.0)
    } else {
        let g = temp - 60.0;
        let g = 288.1221695283 * (g.powf(-0.0755148492));
        g.clamp(0.0, 255.0)
    };

    let blue = if temp >= 66.0 {
        255.0
    } else if temp <= 19.0 {
        0.0
    } else {
        let b = temp - 10.0;
        let b = 138.5177312231 * b.ln() - 305.0447927307;
        b.clamp(0.0, 255.0)
    };

    (red / 255.0, green / 255.0, blue / 255.0)
}

/// Runs the blocking event queue in the background. Listens for temperature changes
/// via MPSC and translates them into live Wayland gamma adjustments.
fn run_wayland_thread(
    rx: mpsc::Receiver<f64>,
    init_tx: mpsc::Sender<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
    initial_temp: f64,
    current_temp: std::sync::Arc<std::sync::atomic::AtomicU32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            let _ = init_tx.send(Err(e.into()));
            return Err("Failed to connect to wayland".into());
        }
    };

    let display = conn.display();
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let _registry = display.get_registry(&qh, ());
    let mut state = AppState::default();

    // Roundtrip to get globals
    if let Err(e) = event_queue.roundtrip(&mut state) {
        let _ = init_tx.send(Err(e.into()));
        return Err("Failed roundtrip".into());
    }

    let manager = match state.gamma_manager.as_ref() {
        Some(m) => m,
        None => {
            let _ = init_tx.send(Err("zwlr_gamma_control_manager_v1 not available".into()));
            return Err("zwlr_gamma_control_manager_v1 not available".into());
        }
    };

    for output in &state.outputs {
        let control = manager.get_gamma_control(output, &qh, ());
        state.gamma_controls.push(control);
    }

    // Roundtrip to get gamma sizes
    if let Err(e) = event_queue.roundtrip(&mut state) {
        let _ = init_tx.send(Err(e.into()));
        return Err("Failed roundtrip for sizes".into());
    }

    // Inform main thread that initialization succeeded.
    let _ = init_tx.send(Ok(()));

    // Apply initial temperature, or use an incoming slider update if one arrived during boot
    let applied_temp = match rx.recv_timeout(std::time::Duration::from_millis(50)) {
        Ok(temp) => temp,
        Err(_) => initial_temp,
    };

    // Keep File Descriptors for the applied gamma ramps alive for the compositor's lifetime
    let mut _active_gamma_files = Vec::new();
    match apply_gamma_ramps(&state, applied_temp) {
        Ok(files) => {
            let _ = event_queue.roundtrip(&mut state);
            _active_gamma_files = files;
            current_temp.store(applied_temp.round() as u32, std::sync::atomic::Ordering::SeqCst);
        }
        Err(e) => {
            log::warn!("Failed to apply initial gamma ramps (temp: {}): {}", applied_temp, e);
        }
    }

    loop {
        // We do a brief dispatch to handle ping/pongs but mainly block on rx with a timeout.
        match rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(temp) => match apply_gamma_ramps(&state, temp) {
                Ok(files) => {
                    let _ = event_queue.roundtrip(&mut state);
                    _active_gamma_files = files;
                    current_temp.store(temp.round() as u32, std::sync::atomic::Ordering::SeqCst);
                }
                Err(e) => {
                    log::warn!("Failed to apply gamma ramps: {}", e);
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Keep connection alive, dispatch any pending events from compositor
                let _ = event_queue.dispatch_pending(&mut state);
                let _ = event_queue.flush();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break; // UI dropped the sender, exit thread
            }
        }
    }

    // Explicitly destroy all gamma controls before the thread dies
    // This hands the lock back to the compositor so we don't permanently brick Night Mode
    for control in &state.gamma_controls {
        control.destroy();
    }

    // Flush the destroy commands to the Wayland socket immediately
    let _ = event_queue.roundtrip(&mut state);

    Ok(())
}

/// Manually maps RGB mathematical structures into a shared `memfd`,
/// attaching the resulting `File` bytes directly to the Wayland output gamma controller.
#[allow(unsafe_code)]
fn apply_gamma_ramps(
    state: &AppState,
    temp: f64,
) -> Result<Vec<std::fs::File>, Box<dyn std::error::Error>> {
    let (r_mult, g_mult, b_mult) = color_temp_to_rgb(temp);
    let mut new_files = Vec::new();

    for control in &state.gamma_controls {
        let size = *state.gamma_sizes.get(control).unwrap_or(&0) as usize;
        if size == 0 {
            continue;
        }

        let bytes_per_ramp = size * 2;
        let total_bytes = bytes_per_ramp * 3;

        let fd = memfd_create("gamma_ramp", MemfdFlags::CLOEXEC)?;
        let file = std::fs::File::from(fd);
        file.set_len(total_bytes as u64)?;

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        for i in 0..size {
            let progress = if size == 1 { 1.0 } else { i as f64 / (size - 1) as f64 };
            let val = (progress * 65535.0) as u16;

            let r = (val as f64 * r_mult) as u16;
            let g = (val as f64 * g_mult) as u16;
            let b = (val as f64 * b_mult) as u16;

            let r_bytes = r.to_ne_bytes();
            let g_bytes = g.to_ne_bytes();
            let b_bytes = b.to_ne_bytes();

            mmap[i * 2] = r_bytes[0];
            mmap[i * 2 + 1] = r_bytes[1];

            mmap[bytes_per_ramp + i * 2] = g_bytes[0];
            mmap[bytes_per_ramp + i * 2 + 1] = g_bytes[1];

            mmap[bytes_per_ramp * 2 + i * 2] = b_bytes[0];
            mmap[bytes_per_ramp * 2 + i * 2 + 1] = b_bytes[1];
        }

        mmap.flush()?;
        control.set_gamma(file.as_fd());
        new_files.push(file);
    }

    Ok(new_files)
}

// ── KDE Plasma backend ─────────────────────────────────────────────────────
//
// KWin does not implement `wlr-gamma-control`, and its D-Bus NightLight
// interface is read-only apart from a temporary `preview`. The reliable way to
// drive Night Light is the [NightColor] group of `kwinrc`, written with
// `kwriteconfig6 --notify` and read with `kreadconfig6`.
//
// Semantics: the panel's Night Mode toggle means "warm now, constant", i.e. a
// manual override of KDE's schedule.
//   * Switching on  : remember KDE's previous Active/Mode (if it was not
//                     already manual), then write NightTemperature,
//                     Mode=Constant and Active=true.
//   * Switching off : restore the remembered Active/Mode, unless the user has
//                     changed them in KDE in the meantime. With nothing to
//                     restore, write Active=false.
//   * Slider        : same as switching on, with the new temperature.
// Re-read KDE's settings before overriding or restoring its schedule.
// `NightTemperature` is shared with KDE's schedule and is not restored. The
// remembered settings live in memory only, so restarting the app while the
// override is on forgets them.

/// Neutral temperature: at or above this, the override is switched off.
const KDE_NEUTRAL_KELVIN: u32 = 6500;
/// Lowest temperature KDE accepts.
const KDE_MIN_KELVIN: u32 = 1000;
/// KDE's default `NightTemperature`.
const KDE_DEFAULT_KELVIN: u32 = 4500;
/// `Mode` value for "always on".
const KDE_MODE_CONSTANT: &str = "Constant";
/// Upper bound for a single kwriteconfig6/kreadconfig6 invocation.
const KDE_COMMAND_TIMEOUT: Duration = Duration::from_secs(1);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// How long to wait for captured output once the child is gone.
const COMMAND_OUTPUT_GRACE: Duration = Duration::from_millis(200);
const COMMAND_OUTPUT_LIMIT: u64 = 4096;

/// Why an external command did not succeed.
#[derive(Debug)]
enum CommandError {
    Spawn(io::Error),
    Wait(io::Error),
    Timeout { after: Duration, stderr: String },
    Failed { status: ExitStatus, stderr: String },
}

fn write_stderr(f: &mut std::fmt::Formatter<'_>, stderr: &str) -> std::fmt::Result {
    if stderr.is_empty() { Ok(()) } else { write!(f, ": {stderr}") }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Spawn(e) => write!(f, "failed to start command: {e}"),
            CommandError::Wait(e) => write!(f, "failed to wait for command: {e}"),
            CommandError::Timeout { after, stderr } => {
                write!(f, "command timed out after {after:?} and was killed")?;
                write_stderr(f, stderr)
            }
            CommandError::Failed { status, stderr } => {
                write!(f, "command exited with {status}")?;
                write_stderr(f, stderr)
            }
        }
    }
}

/// Reads a pipe on its own thread (so a chatty child can never block on a full
/// pipe) and delivers the first `COMMAND_OUTPUT_LIMIT` bytes at EOF.
fn spawn_reader<R: Read + Send + 'static>(mut pipe: R) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut pipe).take(COMMAND_OUTPUT_LIMIT).read_to_end(&mut buf);
        let _ = io::copy(&mut pipe, &mut io::sink());
        let _ = tx.send(String::from_utf8_lossy(&buf).trim().to_owned());
    });
    rx
}

fn collect_output(rx: Option<mpsc::Receiver<String>>) -> String {
    rx.and_then(|rx| rx.recv_timeout(COMMAND_OUTPUT_GRACE).ok()).unwrap_or_default()
}

/// Runs `cmd` and returns its trimmed stdout. A child that outlives `timeout`
/// is killed and reaped. stderr is captured for error reporting.
fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<String, CommandError> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(CommandError::Spawn)?;
    let stdout_rx = child.stdout.take().map(spawn_reader);
    let stderr_rx = child.stderr.take().map(spawn_reader);
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = collect_output(stdout_rx);
                let stderr = collect_output(stderr_rx);
                return if status.success() {
                    Ok(stdout)
                } else {
                    Err(CommandError::Failed { status, stderr })
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                // Kill, then reap so no zombie process is left behind.
                let _ = child.kill();
                let _ = child.wait();
                let stderr = collect_output(stderr_rx);
                return Err(CommandError::Timeout { after: timeout, stderr });
            }
            Ok(None) => thread::sleep(COMMAND_POLL_INTERVAL),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CommandError::Wait(e));
            }
        }
    }
}

/// Writes one key of the `[NightColor]` group in `kwinrc` and notifies KWin.
fn kde_write(key: &str, value: &str) -> Result<(), CommandError> {
    let mut cmd = Command::new("kwriteconfig6");
    cmd.args(["--file", "kwinrc", "--group", "NightColor", "--key", key, "--notify"]);
    if key == "Mode" && value.is_empty() {
        // Restore an unset mode using this Plasma version's default.
        cmd.arg("--delete");
    } else {
        cmd.arg(value);
    }
    run_with_timeout(&mut cmd, KDE_COMMAND_TIMEOUT).map(|_| ())
}

/// Reads one key of the `[NightColor]` group in `kwinrc`.
fn kde_read(key: &str, default: &str) -> Result<String, CommandError> {
    let mut cmd = Command::new("kreadconfig6");
    cmd.args(["--file", "kwinrc", "--group", "NightColor", "--key", key, "--default", default]);
    run_with_timeout(&mut cmd, KDE_COMMAND_TIMEOUT)
}

/// True when running under KDE and both config tools can be executed.
fn kde_backend_available() -> bool {
    let is_kde = std::env::var("XDG_CURRENT_DESKTOP")
        .map(|v| v.split(':').any(|part| part.eq_ignore_ascii_case("KDE")))
        .unwrap_or(false);
    if !is_kde {
        return false;
    }
    ["kwriteconfig6", "kreadconfig6"].iter().all(|tool| {
        match run_with_timeout(Command::new(tool).arg("--help"), KDE_COMMAND_TIMEOUT) {
            Ok(_) => true,
            Err(e) => {
                log::debug!("KDE backend unavailable: {tool}: {e}");
                false
            }
        }
    })
}

/// KDE's `[NightColor]` settings that matter to this backend.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KdeSettings {
    active: bool,
    /// Empty means unset, so restoration must leave KDE's default intact.
    mode: String,
    temperature: u32,
}

impl KdeSettings {
    /// Always-on Night Light, i.e. what the panel's toggle means by "on".
    fn is_manual(&self) -> bool {
        self.active && self.mode == KDE_MODE_CONSTANT
    }

    fn ui_state(&self) -> NightModeState {
        NightModeState { enabled: self.is_manual(), temperature: f64::from(self.temperature) }
    }
}

fn parse_kde_bool(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("true")
}

fn parse_kde_temperature(value: &str) -> u32 {
    value
        .trim()
        .parse::<u32>()
        .unwrap_or(KDE_DEFAULT_KELVIN)
        .clamp(KDE_MIN_KELVIN, KDE_NEUTRAL_KELVIN)
}

fn read_kde_settings() -> Result<KdeSettings, CommandError> {
    let active = parse_kde_bool(&kde_read("Active", "false")?);
    let mode = kde_read("Mode", "")?;
    let temperature =
        parse_kde_temperature(&kde_read("NightTemperature", &KDE_DEFAULT_KELVIN.to_string())?);
    Ok(KdeSettings { active, mode, temperature })
}

/// What a requested temperature means for KDE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KdeTarget {
    /// 6500K or more: no color shift, switch the override off.
    Neutral,
    /// Warm override at this temperature (1000..6500K).
    Warm(u32),
}

fn kde_target(temp: f64) -> KdeTarget {
    if !temp.is_finite() {
        return KdeTarget::Neutral;
    }
    let kelvin =
        temp.round().clamp(f64::from(KDE_MIN_KELVIN), f64::from(KDE_NEUTRAL_KELVIN)) as u32;
    if kelvin >= KDE_NEUTRAL_KELVIN { KdeTarget::Neutral } else { KdeTarget::Warm(kelvin) }
}

#[derive(Default)]
struct KdeNightLight {
    /// KDE's settings from before the override was switched on.
    saved: Option<KdeSettings>,
    /// A restoration started but has not yet written both saved settings.
    restore_pending: bool,
}

impl KdeNightLight {
    fn apply(&mut self, temp: f64) -> Result<(), CommandError> {
        let mut read = || read_kde_settings();
        let mut write = |key: &str, value: &str| kde_write(key, value);
        self.apply_with_io(temp, &mut read, &mut write)
    }

    fn apply_with_io<R, W>(
        &mut self,
        temp: f64,
        read: &mut R,
        write: &mut W,
    ) -> Result<(), CommandError>
    where
        R: FnMut() -> Result<KdeSettings, CommandError>,
        W: FnMut(&str, &str) -> Result<(), CommandError>,
    {
        match kde_target(temp) {
            KdeTarget::Warm(kelvin) => self.enable_with_io(kelvin, read, write),
            KdeTarget::Neutral => self.disable_with_io(read, write),
        }
    }

    fn enable_with_io<R, W>(
        &mut self,
        kelvin: u32,
        read: &mut R,
        write: &mut W,
    ) -> Result<(), CommandError>
    where
        R: FnMut() -> Result<KdeSettings, CommandError>,
        W: FnMut(&str, &str) -> Result<(), CommandError>,
    {
        if self.restore_pending {
            self.disable_with_io(read, write)?;
        }

        let current = read()?;
        if !current.is_manual() {
            self.saved = Some(current);
        }
        // Temperature first so enabling never flashes the previous value.
        // Mode before Active so KDE's schedule cannot take over.
        write("NightTemperature", &kelvin.to_string())?;
        write("Mode", KDE_MODE_CONSTANT)?;
        if let Err(enable_error) = write("Active", "true") {
            if let Some(saved) = self.saved.clone() {
                self.restore_pending = true;
                match Self::write_saved_settings(&saved, write) {
                    Ok(()) => {
                        self.saved = None;
                        self.restore_pending = false;
                    }
                    Err(rollback_error) => {
                        log::warn!("Failed to roll back KDE Night Light enable: {rollback_error}");
                    }
                }
            }
            return Err(enable_error);
        }
        Ok(())
    }

    fn disable_with_io<R, W>(&mut self, read: &mut R, write: &mut W) -> Result<(), CommandError>
    where
        R: FnMut() -> Result<KdeSettings, CommandError>,
        W: FnMut(&str, &str) -> Result<(), CommandError>,
    {
        let Some(saved) = self.saved.clone() else {
            return write("Active", "false");
        };

        if !self.restore_pending {
            let current = match read() {
                Ok(current) => current,
                Err(e) => {
                    log::warn!("Could not read KDE settings before restoring: {e}");
                    return Err(e);
                }
            };
            if !current.is_manual() {
                self.saved = None;
                return Ok(());
            }
        }

        self.restore_pending = true;
        Self::write_saved_settings(&saved, write)?;
        self.saved = None;
        self.restore_pending = false;
        Ok(())
    }

    fn write_saved_settings<W>(saved: &KdeSettings, write: &mut W) -> Result<(), CommandError>
    where
        W: FnMut(&str, &str) -> Result<(), CommandError>,
    {
        let mode_result = write("Mode", &saved.mode);
        let active_result = write("Active", if saved.active { "true" } else { "false" });
        mode_result.and(active_result)
    }
}

/// Worker loop: waits for a request, collapses any requests that piled up
/// while the previous one was being written, then applies only the latest.
/// This gives live feedback while dragging without flooding kwriteconfig6.
fn run_kde_thread(rx: mpsc::Receiver<f64>) {
    let mut kde = KdeNightLight::default();
    while let Ok(mut temp) = rx.recv() {
        while let Ok(newer) = rx.try_recv() {
            temp = newer;
        }
        if let Err(e) = kde.apply(temp) {
            log::warn!("Failed to apply KDE Night Light ({temp}K): {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn kde(active: bool, mode: &str) -> KdeSettings {
        KdeSettings { active, mode: mode.to_owned(), temperature: 4500 }
    }

    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        cmd
    }

    // ── temperature mapping ──────────────────────────────────────

    #[test]
    fn neutral_at_and_above_6500k() {
        assert_eq!(kde_target(6500.0), KdeTarget::Neutral);
        assert_eq!(kde_target(6499.6), KdeTarget::Neutral); // rounds to 6500
        assert_eq!(kde_target(9000.0), KdeTarget::Neutral);
    }

    #[test]
    fn warm_below_6500k_is_rounded() {
        assert_eq!(kde_target(6499.4), KdeTarget::Warm(6499));
        assert_eq!(kde_target(4000.0), KdeTarget::Warm(4000));
        assert_eq!(kde_target(3000.4), KdeTarget::Warm(3000));
    }

    #[test]
    fn warm_is_clamped_to_kde_minimum() {
        assert_eq!(kde_target(500.0), KdeTarget::Warm(1000));
        assert_eq!(kde_target(-1.0), KdeTarget::Warm(1000));
    }

    #[test]
    fn non_finite_temperature_is_neutral() {
        assert_eq!(kde_target(f64::NAN), KdeTarget::Neutral);
        assert_eq!(kde_target(f64::INFINITY), KdeTarget::Neutral);
    }

    // ── KDE settings handling ────────────────────────────────────

    #[test]
    fn parses_kde_values_with_fallbacks() {
        assert!(parse_kde_bool("true"));
        assert!(parse_kde_bool(" True \n"));
        assert!(!parse_kde_bool("false"));
        assert!(!parse_kde_bool(""));
        assert_eq!(parse_kde_temperature("5500"), 5500);
        assert_eq!(parse_kde_temperature("garbage"), KDE_DEFAULT_KELVIN);
        assert_eq!(parse_kde_temperature("99999"), KDE_NEUTRAL_KELVIN);
        assert_eq!(parse_kde_temperature("10"), KDE_MIN_KELVIN);
    }

    #[test]
    fn manual_means_active_and_constant() {
        assert!(kde(true, "Constant").is_manual());
        assert!(!kde(false, "Constant").is_manual());
        assert!(!kde(true, "Automatic").is_manual());
    }

    #[test]
    fn ui_state_reflects_kde_settings() {
        let state =
            KdeSettings { active: true, mode: "Constant".into(), temperature: 5500 }.ui_state();
        assert_eq!(state, NightModeState { enabled: true, temperature: 5500.0 });
        let state = kde(true, "Automatic").ui_state();
        assert!(!state.enabled, "a scheduled Night Light is not a manual override");
    }

    #[derive(Debug)]
    struct FakeKdeIo {
        settings: KdeSettings,
        reads: usize,
        writes: usize,
        write_log: Vec<(String, String)>,
        fail_reads: Vec<usize>,
        fail_writes: Vec<usize>,
    }

    impl FakeKdeIo {
        fn new(settings: KdeSettings) -> Self {
            Self {
                settings,
                reads: 0,
                writes: 0,
                write_log: Vec::new(),
                fail_reads: Vec::new(),
                fail_writes: Vec::new(),
            }
        }

        fn read(&mut self) -> Result<KdeSettings, CommandError> {
            self.reads += 1;
            if self.fail_reads.contains(&self.reads) {
                return Err(injected_error("read"));
            }
            Ok(self.settings.clone())
        }

        fn write(&mut self, key: &str, value: &str) -> Result<(), CommandError> {
            self.writes += 1;
            self.write_log.push((key.to_owned(), value.to_owned()));
            if self.fail_writes.contains(&self.writes) {
                return Err(injected_error("write"));
            }
            match key {
                "Active" => self.settings.active = parse_kde_bool(value),
                "Mode" => self.settings.mode = value.to_owned(),
                "NightTemperature" => {
                    self.settings.temperature = parse_kde_temperature(value);
                }
                _ => unreachable!("unexpected KDE setting: {key}"),
            }
            Ok(())
        }
    }

    fn injected_error(operation: &str) -> CommandError {
        CommandError::Wait(io::Error::other(format!("injected {operation} failure")))
    }

    fn fake_io(settings: KdeSettings) -> Rc<RefCell<FakeKdeIo>> {
        Rc::new(RefCell::new(FakeKdeIo::new(settings)))
    }

    fn apply_fake(
        light: &mut KdeNightLight,
        fake: &Rc<RefCell<FakeKdeIo>>,
        temp: f64,
    ) -> Result<(), CommandError> {
        let read_state = Rc::clone(fake);
        let write_state = Rc::clone(fake);
        let mut read = move || read_state.borrow_mut().read();
        let mut write = move |key: &str, value: &str| write_state.borrow_mut().write(key, value);
        light.apply_with_io(temp, &mut read, &mut write)
    }

    fn disable_fake(
        light: &mut KdeNightLight,
        fake: &Rc<RefCell<FakeKdeIo>>,
    ) -> Result<(), CommandError> {
        let read_state = Rc::clone(fake);
        let write_state = Rc::clone(fake);
        let mut read = move || read_state.borrow_mut().read();
        let mut write = move |key: &str, value: &str| write_state.borrow_mut().write(key, value);
        light.disable_with_io(&mut read, &mut write)
    }

    #[test]
    fn failed_final_enable_rolls_back_the_original_schedule() {
        let original = KdeSettings { active: true, mode: "Automatic".into(), temperature: 5200 };
        let fake = fake_io(original.clone());
        fake.borrow_mut().fail_writes = vec![3];
        let mut light = KdeNightLight::default();

        assert!(apply_fake(&mut light, &fake, 3000.0).is_err());
        let state = &fake.borrow().settings;
        assert_eq!(state.mode, original.mode);
        assert_eq!(state.active, original.active);
        assert_eq!(fake.borrow().writes, 5, "rollback must attempt both saved settings");
    }

    #[test]
    fn failed_enable_rollback_stays_retryable() {
        let original = KdeSettings { active: true, mode: "Automatic".into(), temperature: 5200 };
        let fake = fake_io(original.clone());
        fake.borrow_mut().fail_writes = vec![3, 4];
        let mut light = KdeNightLight::default();

        assert!(apply_fake(&mut light, &fake, 3000.0).is_err());
        assert!(disable_fake(&mut light, &fake).is_ok());
        let state = &fake.borrow().settings;
        assert_eq!(state.mode, original.mode);
        assert_eq!(state.active, original.active);
    }

    #[test]
    fn failed_restore_retains_snapshot_for_successful_retry() {
        let original = KdeSettings { active: true, mode: "Automatic".into(), temperature: 5200 };
        let fake = fake_io(original.clone());
        fake.borrow_mut().fail_writes = vec![4];
        let mut light = KdeNightLight::default();
        apply_fake(&mut light, &fake, 3000.0).expect("warm override applies");

        assert!(disable_fake(&mut light, &fake).is_err());
        assert!(light.saved.is_some(), "failed restore must keep the snapshot");
        disable_fake(&mut light, &fake).expect("restore retry succeeds");
        let state = &fake.borrow().settings;
        assert_eq!(state.mode, original.mode);
        assert_eq!(state.active, original.active);
    }

    #[test]
    fn retry_after_mode_restore_failure_still_restores_active() {
        let original = KdeSettings { active: true, mode: "Automatic".into(), temperature: 5200 };
        let fake = fake_io(original.clone());
        fake.borrow_mut().fail_writes = vec![5];
        let mut light = KdeNightLight::default();
        apply_fake(&mut light, &fake, 3000.0).expect("warm override applies");

        assert!(disable_fake(&mut light, &fake).is_err());
        assert_eq!(fake.borrow().settings.mode, original.mode);
        disable_fake(&mut light, &fake).expect("restore retry succeeds");
        let state = &fake.borrow().settings;
        assert_eq!(state.mode, original.mode);
        assert_eq!(state.active, original.active);
    }

    #[test]
    fn external_settings_change_is_not_overwritten() {
        let fake =
            fake_io(KdeSettings { active: false, mode: "Automatic".into(), temperature: 5200 });
        let mut light = KdeNightLight::default();
        apply_fake(&mut light, &fake, 3000.0).expect("warm override applies");
        let writes_before_disable = fake.borrow().writes;
        fake.borrow_mut().settings =
            KdeSettings { active: false, mode: "Scheduled".into(), temperature: 4100 };

        disable_fake(&mut light, &fake).expect("external settings are left alone");
        assert_eq!(fake.borrow().settings.mode, "Scheduled");
        assert_eq!(fake.borrow().settings.temperature, 4100);
        assert_eq!(fake.borrow().writes, writes_before_disable);
        assert!(light.saved.is_none());
    }

    #[test]
    fn failed_disable_read_preserves_snapshot_until_retry() {
        let original = KdeSettings { active: true, mode: "Automatic".into(), temperature: 5200 };
        let fake = fake_io(original.clone());
        let mut light = KdeNightLight::default();
        apply_fake(&mut light, &fake, 3000.0).expect("warm override applies");
        fake.borrow_mut().fail_reads = vec![2];
        let writes_before_disable = fake.borrow().writes;

        assert!(disable_fake(&mut light, &fake).is_err());
        assert_eq!(fake.borrow().writes, writes_before_disable);
        assert!(light.saved.is_some(), "failed read must preserve the snapshot");
        disable_fake(&mut light, &fake).expect("restore retry succeeds");
        let state = &fake.borrow().settings;
        assert_eq!(state.mode, original.mode);
        assert_eq!(state.active, original.active);
    }

    #[test]
    fn warm_request_after_failed_rollback_restores_before_new_snapshot() {
        let original = KdeSettings { active: true, mode: "Automatic".into(), temperature: 5200 };
        let fake = fake_io(original.clone());
        fake.borrow_mut().fail_writes = vec![3, 4];
        let mut light = KdeNightLight::default();

        assert!(apply_fake(&mut light, &fake, 3000.0).is_err());
        assert!(light.restore_pending);
        assert!(light.saved.is_some());

        apply_fake(&mut light, &fake, 4200.0).expect("pending restore then warm apply succeeds");

        let io = fake.borrow();
        assert_eq!(io.write_log[5], ("Mode".into(), "Automatic".into()));
        assert_eq!(io.write_log[6], ("Active".into(), "true".into()));
        assert_eq!(io.write_log[7], ("NightTemperature".into(), "4200".into()));
        assert_eq!(io.settings.mode, KDE_MODE_CONSTANT);
        assert!(io.settings.active);
        drop(io);
        assert_eq!(light.saved.as_ref().map(|settings| settings.mode.as_str()), Some("Automatic"));
        assert!(!light.restore_pending);
    }

    #[test]
    fn initial_read_error_performs_no_writes() {
        let fake =
            fake_io(KdeSettings { active: false, mode: "Automatic".into(), temperature: 5200 });
        fake.borrow_mut().fail_reads = vec![1];
        let mut light = KdeNightLight::default();

        assert!(apply_fake(&mut light, &fake, 3000.0).is_err());
        assert_eq!(fake.borrow().writes, 0);
        assert!(light.saved.is_none());
    }

    #[test]
    fn unset_mode_is_preserved_when_restoring() {
        let fake = fake_io(kde(true, ""));
        let mut light = KdeNightLight::default();

        apply_fake(&mut light, &fake, 4000.0).expect("override applies");
        disable_fake(&mut light, &fake).expect("restore succeeds");

        assert!(fake.borrow().settings.mode.is_empty());
        assert!(fake.borrow().settings.active);
    }

    #[test]
    fn warm_request_remembers_a_new_external_schedule() {
        let fake = fake_io(kde(true, "Automatic"));
        let mut light = KdeNightLight::default();
        apply_fake(&mut light, &fake, 4000.0).expect("first override applies");
        fake.borrow_mut().settings = kde(true, "DarkLight");

        apply_fake(&mut light, &fake, 4200.0).expect("second override applies");
        disable_fake(&mut light, &fake).expect("restore succeeds");

        assert_eq!(fake.borrow().settings.mode, "DarkLight");
        assert!(fake.borrow().settings.active);
    }

    // ── command handling ─────────────────────────────────────────

    #[test]
    fn successful_command_returns_trimmed_stdout() {
        let out = run_with_timeout(&mut sh("printf '  hello \\n'"), Duration::from_secs(2));
        assert_eq!(out.expect("command succeeds"), "hello");
    }

    #[test]
    fn failed_command_reports_status_and_stderr() {
        let err = run_with_timeout(&mut sh("echo boom >&2; exit 3"), Duration::from_secs(2))
            .expect_err("non-zero exit must fail");
        match &err {
            CommandError::Failed { status, stderr } => {
                assert_eq!(status.code(), Some(3));
                assert_eq!(stderr, "boom");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(err.to_string().contains("boom"), "stderr must appear in the log message");
    }

    #[test]
    fn missing_binary_is_a_spawn_error() {
        let mut cmd = Command::new("definitely-not-a-real-binary-for-wifi-manager");
        let err = run_with_timeout(&mut cmd, Duration::from_secs(1)).expect_err("cannot spawn");
        assert!(matches!(err, CommandError::Spawn(_)));
    }

    #[test]
    fn timed_out_command_is_killed_and_reported() {
        let start = Instant::now();
        let err =
            run_with_timeout(&mut sh("echo late >&2; exec sleep 5"), Duration::from_millis(200))
                .expect_err("must time out");
        match &err {
            CommandError::Timeout { stderr, .. } => assert_eq!(stderr, "late"),
            other => panic!("expected Timeout, got {other:?}"),
        }
        assert!(start.elapsed() < Duration::from_secs(3), "the child must not be waited for");
        assert!(err.to_string().contains("timed out"));
    }
}
