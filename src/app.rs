//! Application controller — bridges the GTK4 UI and the D-Bus backend.
//!
//! Handles: scan triggers, network list population, connect/disconnect actions,
//! WiFi toggle, and password entry flow.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::access_point::{Network, SecurityType};
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

/// Set up all event handlers and kick off the initial scan.
pub fn setup(widgets: &PanelWidgets, wifi: WifiManager) {
    let state = Rc::new(RefCell::new(AppState {
        wifi,
        networks: Vec::new(),
        selected_index: None,
    }));

    setup_scan_button(widgets, Rc::clone(&state));
    setup_wifi_toggle(widgets, Rc::clone(&state));
    setup_network_click(widgets, Rc::clone(&state));
    setup_password_actions(widgets, Rc::clone(&state));
    setup_initial_state(widgets, Rc::clone(&state));
}

/// Initial state: check WiFi status and trigger first scan.
fn setup_initial_state(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let switch = widgets.wifi_switch.clone();
    let status = widgets.status_label.clone();
    let list_box = widgets.network_list_box.clone();
    let state2 = Rc::clone(&state);

    glib::spawn_future_local(async move {
        let st = state2.borrow();

        // Set WiFi switch to current state
        match st.wifi.is_wifi_enabled().await {
            Ok(enabled) => switch.set_active(enabled),
            Err(e) => log::error!("Failed to get WiFi state: {e}"),
        }

        // Trigger initial scan
        if let Err(e) = st.wifi.request_scan().await {
            log::warn!("Initial scan failed: {e}");
        }

        // Brief delay to let NM populate APs after scan
        drop(st);
        glib::timeout_future(std::time::Duration::from_millis(1500)).await;

        refresh_list(&state2, &list_box, &status).await;
    });
}

/// Wire the scan button to trigger a scan and refresh the list.
fn setup_scan_button(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();
    let scan_btn = widgets.scan_button.clone();

    scan_btn.connect_clicked(move |btn| {
        btn.set_sensitive(false);
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let btn = btn.clone();

        glib::spawn_future_local(async move {
            {
                let st = state.borrow();
                if let Err(e) = st.wifi.request_scan().await {
                    log::error!("Scan failed: {e}");
                    status.set_text("Scan failed");
                    btn.set_sensitive(true);
                    return;
                }
            }

            // Wait for scan results
            glib::timeout_future(std::time::Duration::from_millis(1500)).await;
            refresh_list(&state, &list_box, &status).await;
            btn.set_sensitive(true);
        });
    });
}

/// Wire the WiFi toggle switch.
fn setup_wifi_toggle(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();

    widgets.wifi_switch.connect_state_set(move |_switch, enabled| {
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();

        glib::spawn_future_local(async move {
            let result = {
                let st = state.borrow();
                st.wifi.set_wifi_enabled(enabled).await
            };

            match result {
                Ok(_) => {
                    if enabled {
                        status.set_text("WiFi enabled");
                        // Wait a moment then scan
                        glib::timeout_future(std::time::Duration::from_millis(2000)).await;
                        {
                            let st = state.borrow();
                            let _ = st.wifi.request_scan().await;
                        }
                        glib::timeout_future(std::time::Duration::from_millis(1500)).await;
                        refresh_list(&state, &list_box, &status).await;
                    } else {
                        status.set_text("WiFi disabled");
                        network_list::populate_network_list(&list_box, &[]);
                    }
                }
                Err(e) => {
                    log::error!("WiFi toggle failed: {e}");
                    status.set_text("Toggle failed");
                }
            }
        });

        glib::Propagation::Proceed
    });
}

/// Wire network row clicks to connect or show password dialog.
fn setup_network_click(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let revealer = widgets.password_revealer.clone();
    let entry = widgets.password_entry.clone();
    let error_label = widgets.error_label.clone();
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();

    widgets.network_list_box.connect_row_activated(move |_list, row| {
        let index = row.index() as usize;
        let state = Rc::clone(&state);
        let revealer = revealer.clone();
        let entry = entry.clone();
        let error_label = error_label.clone();
        let list_box = list_box.clone();
        let status = status.clone();

        glib::spawn_future_local(async move {
            let network = {
                let st = state.borrow();
                st.networks.get(index).cloned()
            };

            let Some(network) = network else {
                return;
            };

            if network.is_connected {
                // Disconnect
                status.set_text(&format!("Disconnecting from {}...", network.ssid));
                let result = {
                    let st = state.borrow();
                    st.wifi.disconnect().await
                };
                match result {
                    Ok(_) => {
                        glib::timeout_future(std::time::Duration::from_millis(500)).await;
                        refresh_list(&state, &list_box, &status).await;
                    }
                    Err(e) => {
                        log::error!("Disconnect failed: {e}");
                        status.set_text("Disconnect failed");
                    }
                }
            } else if network.is_saved || network.security == SecurityType::Open {
                // Connect directly (no password needed)
                status.set_text(&format!("Connecting to {}...", network.ssid));
                let result = {
                    let st = state.borrow();
                    st.wifi.connect_to_network(&network, None).await
                };
                match result {
                    Ok(_) => {
                        glib::timeout_future(std::time::Duration::from_millis(2000)).await;
                        refresh_list(&state, &list_box, &status).await;
                    }
                    Err(e) => {
                        log::error!("Connect failed: {e}");
                        status.set_text(&format!("Failed: {}", e));
                    }
                }
            } else {
                // Show password dialog
                state.borrow_mut().selected_index = Some(index);
                error_label.set_visible(false);
                entry.set_text("");
                revealer.set_reveal_child(true);
                entry.grab_focus();
            }
        });
    });
}

/// Wire password dialog connect/cancel buttons and Enter key.
fn setup_password_actions(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let revealer = widgets.password_revealer.clone();
    let entry = widgets.password_entry.clone();
    let error_label = widgets.error_label.clone();
    let list_box = widgets.network_list_box.clone();
    let status_label = widgets.status_label.clone();

    // Cancel button — hide the password section
    {
        let revealer = revealer.clone();
        widgets.cancel_button.connect_clicked(move |_| {
            revealer.set_reveal_child(false);
        });
    }

    // Connect button
    {
        let state = Rc::clone(&state);
        let revealer = revealer.clone();
        let entry = entry.clone();
        let error_label = error_label.clone();
        let list_box = list_box.clone();
        let status = status_label.clone();

        widgets.connect_button.connect_clicked(move |btn| {
            let password = entry.text().to_string();
            if password.is_empty() {
                error_label.set_text("Password cannot be empty");
                error_label.set_visible(true);
                return;
            }

            btn.set_sensitive(false);
            let state = Rc::clone(&state);
            let revealer = revealer.clone();
            let error_label = error_label.clone();
            let list_box = list_box.clone();
            let status = status.clone();
            let btn = btn.clone();

            glib::spawn_future_local(async move {
                let network = {
                    let st = state.borrow();
                    st.selected_index.and_then(|i| st.networks.get(i).cloned())
                };

                let Some(network) = network else {
                    btn.set_sensitive(true);
                    return;
                };

                status.set_text(&format!("Connecting to {}...", network.ssid));

                let result = {
                    let st = state.borrow();
                    st.wifi.connect_to_network(&network, Some(&password)).await
                };

                match result {
                    Ok(_) => {
                        revealer.set_reveal_child(false);
                        glib::timeout_future(std::time::Duration::from_millis(2000)).await;
                        refresh_list(&state, &list_box, &status).await;
                    }
                    Err(e) => {
                        log::error!("Connect with password failed: {e}");
                        error_label.set_text("Connection failed — check password");
                        error_label.set_visible(true);
                    }
                }
                btn.set_sensitive(true);
            });
        });
    }

    // Enter key in password entry triggers connect
    {
        let connect_btn = widgets.connect_button.clone();
        widgets.password_entry.connect_activate(move |_| {
            connect_btn.emit_clicked();
        });
    }
}

/// Refresh the network list from D-Bus and update the UI.
async fn refresh_list(
    state: &Rc<RefCell<AppState>>,
    list_box: &gtk4::ListBox,
    status: &gtk4::Label,
) {
    let networks = {
        let st = state.borrow();
        st.wifi.get_networks().await
    };

    match networks {
        Ok(nets) => {
            // Update status with connected network
            let connected = nets.iter().find(|n| n.is_connected);
            match connected {
                Some(n) => status.set_text(&format!("Connected to {}", n.ssid)),
                None => status.set_text("Not connected"),
            }

            network_list::populate_network_list(list_box, &nets);
            log::info!("Network list refreshed: {} networks", nets.len());
            state.borrow_mut().networks = nets;
        }
        Err(e) => {
            log::error!("Failed to get networks: {e}");
            status.set_text("Failed to load networks");
        }
    }
}
