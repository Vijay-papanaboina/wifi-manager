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
mod hotspot;
mod live_updates;
mod scanning;
mod shortcuts;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::dbus::access_point::Network;
use crate::dbus::bluetooth_device::BluetoothDevice;
use crate::dbus::bluetooth_manager::BluetoothManager;
use crate::dbus::hotspot_manager::HotspotManager;
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
    /// Hotspot manager.
    hotspot: HotspotManager,
}

/// Set up all event handlers, kick off the initial scan, start live updates,
/// and wire scan-on-show polling.
pub fn setup(
    widgets: &PanelWidgets,
    wifi: WifiManager,
    scan_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
    panel_state: crate::daemon::PanelState,
) {
    let hotspot = HotspotManager::new(wifi.connection().clone(), wifi.device_path());

    let state = Rc::new(RefCell::new(AppState {
        wifi,
        networks: Vec::new(),
        selected_index: None,
        bluetooth: None,
        bt_devices: Vec::new(),
        hotspot,
    }));

    scanning::setup_scan_button(widgets, Rc::clone(&state));
    connection::setup_wifi_toggle(widgets, Rc::clone(&state));
    connection::setup_network_click(widgets, Rc::clone(&state));
    connection::setup_password_actions(widgets, Rc::clone(&state));
    live_updates::setup_live_updates(widgets, Rc::clone(&state));
    scanning::setup_scan_on_show(widgets, Rc::clone(&state), scan_requested);
    bluetooth::setup_bluetooth(widgets, Rc::clone(&state));
    bt_live_updates::setup_bt_live_updates(widgets, Rc::clone(&state));
    hotspot::setup_hotspot(widgets, Rc::clone(&state));
    setup_bt_scan_button(widgets, Rc::clone(&state));
    setup_wifi_tab_sync(widgets, Rc::clone(&state));
    let reload_requested = panel_state.reload_requested.clone();
    shortcuts::setup_escape_key(widgets, panel_state);
    shortcuts::setup_reload_on_request(widgets, Rc::clone(&state), reload_requested);
    scanning::setup_initial_state(widgets, Rc::clone(&state));
}

/// Clone the WifiManager out of the RefCell (avoids holding borrow across await).
fn get_wifi(state: &Rc<RefCell<AppState>>) -> WifiManager {
    state.borrow().wifi.clone()
}

/// Refresh the network list from D-Bus and update the UI.
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

/// Wire the scan button to also trigger BT discovery when on BT tab.
fn setup_bt_scan_button(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let bt_tab = widgets.bt_tab.clone();
    let bt_list_box = widgets.bt_list_box.clone();
    let bt_spinner = widgets.bt_spinner.clone();
    let bt_scroll = widgets.bt_scroll.clone();
    let status = widgets.status_label.clone();
    let scan_btn = widgets.scan_button.clone();

    // We prepend a handler that checks if BT tab is active.
    // If so, it does BT scan instead of WiFi scan.
    scan_btn.connect_clicked(move |btn| {
        if !bt_tab.is_active() {
            return; // Let the WiFi scan handler deal with it
        }

        btn.set_sensitive(false);
        let state = Rc::clone(&state);
        let bt_list_box = bt_list_box.clone();
        let bt_spinner = bt_spinner.clone();
        let bt_scroll = bt_scroll.clone();
        let status = status.clone();
        let btn = btn.clone();

        bt_spinner.set_visible(true);
        bt_spinner.set_spinning(true);
        bt_scroll.set_visible(false);

        gtk4::glib::spawn_future_local(async move {
            if let Some(bt) = state.borrow().bluetooth.clone() {
                if let Err(e) = bt.start_discovery().await {
                    log::warn!("BT scan failed: {e}");
                }
                gtk4::glib::timeout_future(std::time::Duration::from_millis(2000)).await;
                bluetooth::refresh_bt_list(&state, &bt_list_box, &status).await;
            }

            bt_spinner.set_spinning(false);
            bt_spinner.set_visible(false);
            bt_scroll.set_visible(true);
            btn.set_sensitive(true);
        });
    });
}

/// Sync the toggle switch to WiFi power state when WiFi tab is activated.
fn setup_wifi_tab_sync(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let wifi_tab = widgets.wifi_tab.clone();
    let switch = widgets.wifi_switch.clone();
    let title = widgets.title_label.clone();
    let status = widgets.status_label.clone();
    let list_box = widgets.network_list_box.clone();
    let scan_btn = widgets.scan_button.clone();

    wifi_tab.connect_toggled(move |btn| {
        if !btn.is_active() {
            return;
        }

        title.set_text("Wi-Fi");
        switch.set_tooltip_text(Some("Enable/Disable Wi-Fi"));
        scan_btn.set_tooltip_text(Some("Scan for networks"));

        let state = Rc::clone(&state);
        let switch = switch.clone();
        let status = status.clone();
        let list_box = list_box.clone();

        gtk4::glib::spawn_future_local(async move {
            let wifi = get_wifi(&state);

            // Sync switch to actual WiFi power state
            match wifi.is_wifi_enabled().await {
                Ok(enabled) => switch.set_active(enabled),
                Err(e) => log::error!("Failed to get WiFi state on tab switch: {e}"),
            }

            // Refresh network list
            refresh_list(&state, &list_box, &status).await;
        });
    });
}
