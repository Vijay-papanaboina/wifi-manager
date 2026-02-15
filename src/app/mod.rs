//! Application controller — bridges the GTK4 UI and the D-Bus backend.
//!
//! Split into sub-modules:
//! - `scanning` — scan-on-show, initial scan, scan button
//! - `connection` — WiFi toggle, network click, password dialog
//! - `live_updates` — D-Bus signal subscriptions for real-time changes
//! - `shortcuts` — Escape key, reload polling

mod connection;
mod live_updates;
mod scanning;
mod shortcuts;

use std::cell::RefCell;
use std::rc::Rc;

use crate::dbus::access_point::Network;
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
}

/// Set up all event handlers, kick off the initial scan, start live updates,
/// and wire scan-on-show polling.
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
    }));

    scanning::setup_scan_button(widgets, Rc::clone(&state));
    connection::setup_wifi_toggle(widgets, Rc::clone(&state));
    connection::setup_network_click(widgets, Rc::clone(&state));
    connection::setup_password_actions(widgets, Rc::clone(&state));
    live_updates::setup_live_updates(widgets, Rc::clone(&state));
    scanning::setup_scan_on_show(widgets, Rc::clone(&state), scan_requested);
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
