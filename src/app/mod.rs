//! Application controller — bridges the GTK4 UI and the D-Bus backend.
//!
//! Split into sub-modules:
//! - `scanning` — scan-on-show, initial scan, scan button
//! - `connection` — WiFi toggle, network click, password dialog
//! - `live_updates` — D-Bus signal subscriptions for real-time changes
//! - `shortcuts` — Escape key, reload polling

mod bluetooth;
mod bt_live_updates;
mod connection;
mod controls;
mod live_updates;
mod scanning;
mod shortcuts;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::access_point::Network;
use crate::dbus::bluetooth_device::BluetoothDevice;
use crate::dbus::bluetooth_manager::BluetoothManager;
use crate::dbus::network_manager::WifiManager;
use crate::ui::network_list;
use crate::ui::window::PanelWidgets;

/// Shared application state accessible from GTK callbacks.
struct AppState {
    wifi: WifiManager,
    /// The network list — refreshed on scan.
    networks: Vec<Network>,
    /// Index of the currently selected network (for password entry).
    selected_index: Option<usize>,
    /// Bluetooth manager (None if no adapter found).
    bluetooth: Option<BluetoothManager>,
    /// Bluetooth device list — refreshed on BT scan.
    bt_devices: Vec<BluetoothDevice>,
    /// Whether a Bluetooth scan is currently running.
    bt_scan_in_progress: bool,
    /// Periodic auto-scan timer for Bluetooth (when BT tab is active).
    bt_auto_scan_source: Option<glib::SourceId>,
    /// Periodic refresh timer for Bluetooth list (when BT tab is active).
    bt_live_refresh_source: Option<glib::SourceId>,
    /// Whether Bluetooth auto-scan loop is active.
    bt_auto_scan_active: bool,
    /// Whether a Bluetooth device menu is open (avoid refresh to prevent popover closing).
    bt_menu_open: bool,
    /// Whether a Wi-Fi scan is currently running.
    wifi_scan_in_progress: bool,
    /// Periodic auto-scan timer for Wi-Fi (when Wi-Fi tab is active).
    wifi_auto_scan_source: Option<glib::SourceId>,
}

/// Initialize application UI behavior and start initial background activity.
///
/// Creates the shared application state, registers all UI event handlers and shortcuts,
/// wires scan-on-show polling, and starts the initial Wi‑Fi auto-scan when the Wi‑Fi tab is active.
///
/// # Examples
///
/// ```no_run
/// use std::sync::{Arc, atomic::AtomicBool};
/// // Assume `widgets`, `wifi_manager`, and `panel_state` are constructed elsewhere.
/// let widgets: PanelWidgets = /* ... */;
/// let wifi_manager: WifiManager = /* ... */;
/// let scan_requested = Arc::new(AtomicBool::new(false));
/// let panel_state: crate::daemon::PanelState = /* ... */;
/// setup(&widgets, wifi_manager, scan_requested, panel_state);
/// ```
pub fn setup(
    widgets: &PanelWidgets,
    wifi: WifiManager,
    scan_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
    panel_state: crate::daemon::PanelState,
) {
    let state = Rc::new(RefCell::new(AppState {
        wifi,
        networks: Vec::new(),
        selected_index: None,
        bluetooth: None,
        bt_devices: Vec::new(),
        bt_scan_in_progress: false,
        bt_auto_scan_source: None,
        bt_live_refresh_source: None,
        bt_auto_scan_active: false,
        bt_menu_open: false,
        wifi_scan_in_progress: false,
        wifi_auto_scan_source: None,
    }));

    connection::setup_wifi_toggle(widgets, Rc::clone(&state));
    connection::setup_network_click(widgets, Rc::clone(&state));
    connection::setup_password_actions(widgets, Rc::clone(&state));
    live_updates::setup_live_updates(widgets, Rc::clone(&state));
    scanning::setup_scan_on_show(widgets, Rc::clone(&state), scan_requested);
    bluetooth::setup_bluetooth(widgets, Rc::clone(&state));
    bt_live_updates::setup_bt_live_updates(widgets, Rc::clone(&state));
    setup_scan_button_dispatch(widgets, Rc::clone(&state));
    setup_wifi_tab_sync(widgets, Rc::clone(&state));
    if widgets.wifi_tab.is_active() {
        scanning::start_wifi_auto_scan(
            Rc::clone(&state),
            widgets.wifi_tab.clone(),
            widgets.network_list_box.clone(),
            widgets.status_label.clone(),
        );
    }
    let reload_requested = panel_state.reload_requested.clone();
    shortcuts::setup_escape_key(widgets, panel_state);
    shortcuts::setup_reload_on_request(widgets, Rc::clone(&state), reload_requested);
    scanning::setup_initial_state(widgets, Rc::clone(&state));
    controls::setup_controls(widgets);
}

/// Clone the WifiManager out of the RefCell (avoids holding borrow across await).
fn get_wifi(state: &Rc<RefCell<AppState>>) -> WifiManager {
    state.borrow().wifi.clone()
}

/// Refreshes the stored network list and updates the UI to reflect current connections.
///
/// This function queries the Wi‑Fi manager for available networks, updates the status label
/// to show the SSID of the connected network or "Not connected", repopulates the provided
/// list box with the retrieved networks using the current configuration, and stores the
/// fetched network list in the shared application state. If the query fails, the status
/// label is set to "Failed to load networks".
///
/// # Examples
///
/// ```no_run
/// # use std::rc::Rc;
/// # use std::cell::RefCell;
/// # async fn example() {
/// // `state`, `list_box`, and `status` should be initialized GTK objects in real usage.
/// refresh_list(&state, &list_box, &status).await;
/// # }
/// ```
async fn refresh_list(
    state: &Rc<RefCell<AppState>>,
    list_box: &gtk4::ListBox,
    status: &gtk4::Label,
) {
    let wifi = get_wifi(state);
    let networks = wifi.get_networks().await;

    match networks {
        Ok(nets) => {
            // Update status with connected network
            let connected = nets.iter().find(|n| n.is_connected);
            match connected {
                Some(n) => status.set_text(&format!("Connected to {}", n.ssid)),
                None => status.set_text("Not connected"),
            }

            let config = crate::config::Config::load();
            network_list::populate_network_list(list_box, &nets, &config, &wifi, status);
            log::info!("Network list refreshed: {} networks", nets.len());
            state.borrow_mut().networks = nets;
        }
        Err(e) => {
            log::error!("Failed to get networks: {e}");
            status.set_text("Failed to load networks");
        }
    }
}

/// Route the scan button click to the active tab's scan handler.
///
/// Wires the panel's scan button so that a click invokes the Bluetooth manual scan
/// when the Bluetooth tab is active, otherwise invokes the Wi‑Fi manual scan. The
/// function captures and forwards the relevant UI widgets and shared AppState to
/// the appropriate handler.
///
/// # Examples
///
/// ```
/// // Given `widgets: PanelWidgets` and `state: Rc<RefCell<AppState>>` already created:
/// setup_scan_button_dispatch(&widgets, state.clone());
/// // Clicking the scan button will now dispatch to the correct scan flow based on the active tab.
/// ```
fn setup_scan_button_dispatch(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let scan_btn = widgets.scan_button.clone();
    let bt_tab = widgets.bt_tab.clone();
    let bt_list_box = widgets.bt_list_box.clone();
    let bt_spinner = widgets.bt_spinner.clone();
    let bt_scroll = widgets.bt_scroll.clone();
    let wifi_list_box = widgets.network_list_box.clone();
    let wifi_spinner = widgets.spinner.clone();
    let wifi_scroll = widgets.network_scroll.clone();
    let status = widgets.status_label.clone();

    let scan_btn_cb = scan_btn.clone();
    scan_btn.connect_clicked(move |_btn| {
        if bt_tab.is_active() {
            bluetooth::run_manual_scan(
                Rc::clone(&state),
                bt_tab.clone(),
                bt_list_box.clone(),
                status.clone(),
                scan_btn_cb.clone(),
                bt_spinner.clone(),
                bt_scroll.clone(),
            );
        } else {
            scanning::run_manual_scan(
                Rc::clone(&state),
                wifi_list_box.clone(),
                status.clone(),
                scan_btn_cb.clone(),
                wifi_spinner.clone(),
                wifi_scroll.clone(),
            );
        }
    });
}

/// Synchronize Wi‑Fi UI and scanning behavior when the Wi‑Fi tab is toggled.
///
/// When the Wi‑Fi tab becomes active this updates the tab title and tooltips,
/// queries the hardware Wi‑Fi power state and sets the UI switch accordingly,
/// refreshes the displayed network list, and starts the periodic Wi‑Fi auto‑scan.
/// When the tab is deactivated it stops any ongoing Wi‑Fi auto‑scan.
///
/// # Parameters
///
/// - `widgets`: UI widgets for the panel (Wi‑Fi tab, switch, labels, list, scan button).
/// - `state`: shared application state containing the Wi‑Fi manager and scan state.
///
/// # Examples
///
/// ```no_run
/// // `widgets` and `state` would be created during application setup.
/// setup_wifi_tab_sync(&widgets, Rc::new(RefCell::new(app_state)));
/// ```
fn setup_wifi_tab_sync(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let wifi_tab = widgets.wifi_tab.clone();
    let switch = widgets.wifi_switch.clone();
    let title = widgets.title_label.clone();
    let status = widgets.status_label.clone();
    let list_box = widgets.network_list_box.clone();
    let scan_btn = widgets.scan_button.clone();

    wifi_tab.connect_toggled(move |btn| {
        if !btn.is_active() {
            scanning::stop_wifi_auto_scan(&state);
            return;
        }

        title.set_text("Wi-Fi");
        switch.set_tooltip_text(Some("Enable/Disable Wi-Fi"));
        scan_btn.set_tooltip_text(Some("Scan for networks"));

        let state_for_refresh = Rc::clone(&state);
        let switch = switch.clone();
        let status = status.clone();
        let list_box = list_box.clone();
        let status_for_refresh = status.clone();
        let list_box_for_refresh = list_box.clone();

        gtk4::glib::spawn_future_local(async move {
            let wifi = get_wifi(&state_for_refresh);

            // Sync switch to actual WiFi power state
            match wifi.is_wifi_enabled().await {
                Ok(enabled) => switch.set_active(enabled),
                Err(e) => log::error!("Failed to get WiFi state on tab switch: {e}"),
            }

            // Refresh network list
            refresh_list(&state_for_refresh, &list_box_for_refresh, &status_for_refresh).await;
        });

        scanning::start_wifi_auto_scan(
            Rc::clone(&state),
            btn.clone(),
            list_box.clone(),
            status.clone(),
        );
    });
}
