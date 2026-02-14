//! D-Bus daemon service — exposes Toggle/Show/Hide methods on the session bus.
//!
//! This allows `wifi-manager --toggle` to control a running instance.
//! The interface is registered at `com.github.wifi_manager.WifiManager`
//! on the session bus at path `/com/github/wifi_manager/WifiManager`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use zbus::interface;

/// The D-Bus interface name and path.
pub const DBUS_NAME: &str = "com.github.wifi_manager.Daemon";
pub const DBUS_PATH: &str = "/com/github/wifi_manager/Daemon";

/// Thread-safe callback type for toggling visibility from D-Bus thread.
type ToggleFn = Arc<dyn Fn(bool) + Send + Sync>;

/// State shared between the D-Bus service and the GTK window.
/// Must be Send + Sync because zbus runs on its own async runtime.
#[derive(Clone)]
pub struct PanelState {
    /// Whether the panel is currently visible.
    pub visible: Arc<AtomicBool>,
    /// Flag set by show() — polled by GTK main thread to trigger scan-on-show.
    pub scan_requested: Arc<AtomicBool>,
    /// Callback to toggle visibility — dispatches to GTK main thread.
    toggle_fn: ToggleFn,
}

impl PanelState {
    pub fn new(toggle_fn: impl Fn(bool) + Send + Sync + 'static) -> Self {
        Self {
            visible: Arc::new(AtomicBool::new(false)),
            scan_requested: Arc::new(AtomicBool::new(false)),
            toggle_fn: Arc::new(toggle_fn),
        }
    }

    pub fn show(&self) {
        self.visible.store(true, Ordering::Relaxed);
        self.scan_requested.store(true, Ordering::Relaxed);
        (self.toggle_fn)(true);
    }

    pub fn hide(&self) {
        self.visible.store(false, Ordering::Relaxed);
        (self.toggle_fn)(false);
    }

    pub fn toggle(&self) {
        if self.visible.load(Ordering::Relaxed) {
            self.hide();
        } else {
            self.show();
        }
    }
}

/// D-Bus interface implementation — exposed on the session bus.
struct DaemonInterface {
    state: PanelState,
}

#[interface(name = "com.github.wifi_manager.Daemon")]
impl DaemonInterface {
    /// Toggle panel visibility.
    fn toggle(&self) {
        log::info!("D-Bus Toggle() called");
        self.state.toggle();
    }

    /// Show the panel.
    fn show(&self) {
        log::info!("D-Bus Show() called");
        self.state.show();
    }

    /// Hide the panel.
    fn hide(&self) {
        log::info!("D-Bus Hide() called");
        self.state.hide();
    }

    /// Check if the panel is visible.
    #[zbus(property)]
    fn visible(&self) -> bool {
        self.state.visible.load(Ordering::Relaxed)
    }
}

/// Register the D-Bus service on the session bus.
/// Returns the connection (keep alive for the daemon's lifetime).
pub async fn register_service(state: PanelState) -> zbus::Result<zbus::Connection> {
    let iface = DaemonInterface { state };

    let conn = zbus::connection::Builder::session()?
        .name(DBUS_NAME)?
        .serve_at(DBUS_PATH, iface)?
        .build()
        .await?;

    log::info!("D-Bus daemon service registered: {DBUS_NAME}");
    Ok(conn)
}

/// Check if another instance is already running (name is taken on session bus).
pub async fn is_instance_running() -> bool {
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(_) => return false,
    };

    let result = conn
        .call_method(
            Some(DBUS_NAME),
            DBUS_PATH,
            Some("org.freedesktop.DBus.Peer"),
            "Ping",
            &(),
        )
        .await;

    result.is_ok()
}

/// Send a Toggle() call to the running daemon instance.
pub async fn send_toggle() -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;

    conn.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_NAME), "Toggle", &())
        .await?;

    log::info!("Toggle sent to running instance");
    Ok(())
}
