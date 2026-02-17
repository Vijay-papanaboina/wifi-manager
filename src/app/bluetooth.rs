//! Bluetooth controller — scan, power toggle, device click, and initial state.
//!
//! Mirrors the structure of `scanning.rs` and `connection.rs` for WiFi.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::bluetooth_manager::BluetoothManager;
use crate::ui::device_list;
use crate::ui::window::PanelWidgets;

use super::AppState;

/// Set up all Bluetooth UI event handlers.
///
/// If no Bluetooth adapter is available, the BT tab is hidden entirely.
pub(super) fn setup_bluetooth(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    // Try to initialize the BluetoothManager
    let bt_tab = widgets.bt_tab.clone();
    let bt_spinner = widgets.bt_spinner.clone();
    let bt_scroll = widgets.bt_scroll.clone();
    let bt_list_box = widgets.bt_list_box.clone();
    let status = widgets.status_label.clone();
    let switch = widgets.wifi_switch.clone();
    let scan_btn = widgets.scan_button.clone();
    let title = widgets.title_label.clone();

    glib::spawn_future_local(async move {
        let bt = match BluetoothManager::new().await {
            Some(bt) => bt,
            None => {
                // No Bluetooth adapter — hide the BT tab completely
                bt_tab.set_visible(false);
                bt_spinner.set_visible(false);
                log::info!("No Bluetooth adapter found — BT tab hidden");
                return;
            }
        };

        log::info!("Bluetooth adapter available — BT tab enabled");

        // Store the BT manager
        state.borrow_mut().bluetooth = Some(bt.clone());

        // ── BT tab activation: sync header and scan if powered ──
        {
            let state = Rc::clone(&state);
            let bt_list_box = bt_list_box.clone();
            let bt_spinner = bt_spinner.clone();
            let bt_scroll = bt_scroll.clone();
            let status = status.clone();
            let switch = switch.clone();
            let scan_btn = scan_btn.clone();
            let title = title.clone();

            bt_tab.connect_toggled(move |btn| {
                if !btn.is_active() {
                    return;
                }

                // Update header labels for BT context
                title.set_text("Bluetooth");
                scan_btn.set_tooltip_text(Some("Scan for devices"));
                switch.set_tooltip_text(Some("Enable/Disable Bluetooth"));

                let state = Rc::clone(&state);
                let bt_list_box = bt_list_box.clone();
                let bt_spinner = bt_spinner.clone();
                let bt_scroll = bt_scroll.clone();
                let status = status.clone();
                let switch = switch.clone();

                glib::spawn_future_local(async move {
                    let bt = match get_bt(&state) {
                        Some(bt) => bt,
                        None => return,
                    };

                    // Sync switch to actual BT power state
                    let powered = match bt.is_powered().await {
                        Ok(p) => p,
                        Err(e) => {
                            log::error!("Failed to get BT power state: {e}");
                            true // assume powered if we can't check
                        }
                    };
                    switch.set_active(powered);

                    if !powered {
                        // BT is off — show disabled state, no spinner
                        status.set_text("Bluetooth disabled");
                        bt_spinner.set_visible(false);
                        bt_scroll.set_visible(true);
                        device_list::populate_device_list(
                            &bt_list_box, &[], &bt, &status,
                        );
                        return;
                    }

                    // BT is on — discover and populate
                    bt_spinner.set_visible(true);
                    bt_spinner.set_spinning(true);
                    bt_scroll.set_visible(false);

                    if let Err(e) = bt.start_discovery().await {
                        log::warn!("BT discovery failed: {e}");
                    }
                    glib::timeout_future(std::time::Duration::from_millis(2000)).await;

                    refresh_bt_list(&state, &bt_list_box, &status).await;

                    bt_spinner.set_spinning(false);
                    bt_spinner.set_visible(false);
                    bt_scroll.set_visible(true);
                });
            });
        }

        // ── BT power toggle (when BT tab is active) ──
        {
            let state = Rc::clone(&state);
            let bt_list_box = bt_list_box.clone();
            let bt_spinner = bt_spinner.clone();
            let bt_scroll = bt_scroll.clone();
            let status = status.clone();
            let bt_tab_c = bt_tab.clone();

            switch.connect_state_set(move |_switch, enabled| {
                if !bt_tab_c.is_active() {
                    return glib::Propagation::Proceed;
                }

                let state = Rc::clone(&state);
                let bt_list_box = bt_list_box.clone();
                let bt_spinner = bt_spinner.clone();
                let bt_scroll = bt_scroll.clone();
                let status = status.clone();

                glib::spawn_future_local(async move {
                    let bt = match get_bt(&state) {
                        Some(bt) => bt,
                        None => return,
                    };

                    match bt.set_powered(enabled).await {
                        Ok(_) => {
                            if enabled {
                                status.set_text("Bluetooth enabled");
                                bt_spinner.set_visible(true);
                                bt_spinner.set_spinning(true);
                                bt_scroll.set_visible(false);

                                if let Err(e) = bt.start_discovery().await {
                                    log::warn!("BT discovery after power on failed: {e}");
                                }
                                glib::timeout_future(std::time::Duration::from_millis(2000))
                                    .await;
                                refresh_bt_list(&state, &bt_list_box, &status).await;

                                bt_spinner.set_spinning(false);
                                bt_spinner.set_visible(false);
                                bt_scroll.set_visible(true);
                            } else {
                                status.set_text("Bluetooth disabled");
                                device_list::populate_device_list(
                                    &bt_list_box,
                                    &[],
                                    &bt,
                                    &status,
                                );
                            }
                        }
                        Err(e) => {
                            log::error!("BT power toggle failed: {e}");
                            status.set_text("Toggle failed");
                        }
                    }
                });

                glib::Propagation::Proceed
            });
        }

        // ── Device row click: connect/disconnect ──
        {
            let state_c = Rc::clone(&state);
            let status_c = status.clone();
            let bt_list_box_c = bt_list_box.clone();

            bt_list_box.connect_row_activated(move |_list, row| {
                let index = row.index() as usize;
                let state = Rc::clone(&state_c);
                let status = status_c.clone();
                let bt_list_box = bt_list_box_c.clone();

                glib::spawn_future_local(async move {
                    let (device, bt) = {
                        let st = state.borrow();
                        let dev = st.bt_devices.get(index).cloned();
                        let bt = st.bluetooth.clone();
                        (dev, bt)
                    };

                    let (Some(device), Some(bt)) = (device, bt) else {
                        return;
                    };

                    if device.connected {
                        // Disconnect
                        status.set_text(&format!("Disconnecting {}...", device.display_name));
                        match bt.disconnect_device(&device.device_path).await {
                            Ok(_) => {
                                glib::timeout_future(std::time::Duration::from_millis(500)).await;
                                refresh_bt_list(&state, &bt_list_box, &status).await;
                            }
                            Err(e) => {
                                log::error!("BT disconnect failed: {e}");
                                status.set_text("Disconnect failed");
                            }
                        }
                    } else if device.paired {
                        // Connect (already paired)
                        status.set_text(&format!("Connecting to {}...", device.display_name));
                        match bt.connect_device(&device.device_path).await {
                            Ok(_) => {
                                glib::timeout_future(std::time::Duration::from_millis(1000)).await;
                                refresh_bt_list(&state, &bt_list_box, &status).await;
                            }
                            Err(e) => {
                                log::error!("BT connect failed: {e}");
                                status.set_text("Connection failed");
                            }
                        }
                    } else {
                        // Try "Just Works" pair + connect
                        status.set_text(&format!("Pairing with {}...", device.display_name));
                        match bt.pair_device(&device.device_path).await {
                            Ok(_) => {
                                let _ = bt.trust_device(&device.device_path, true).await;
                                status.set_text(&format!(
                                    "Connecting to {}...",
                                    device.display_name
                                ));
                                let _ = bt.connect_device(&device.device_path).await;
                                glib::timeout_future(std::time::Duration::from_millis(1000)).await;
                                refresh_bt_list(&state, &bt_list_box, &status).await;
                            }
                            Err(e) => {
                                log::error!("BT pairing failed: {e}");
                                status.set_text("Pairing failed — try bluetoothctl");
                            }
                        }
                    }
                });
            });
        }
    });
}

/// Extract BluetoothManager from AppState.
fn get_bt(state: &Rc<RefCell<AppState>>) -> Option<BluetoothManager> {
    state.borrow().bluetooth.clone()
}

/// Refresh the Bluetooth device list from D-Bus and update the UI.
pub(super) async fn refresh_bt_list(
    state: &Rc<RefCell<AppState>>,
    list_box: &gtk4::ListBox,
    status: &gtk4::Label,
) {
    let bt = match get_bt(state) {
        Some(bt) => bt,
        None => return,
    };

    match bt.get_devices().await {
        Ok(devices) => {
            // Update status
            let connected = devices.iter().find(|d| d.connected);
            match connected {
                Some(d) => status.set_text(&format!("Connected to {}", d.display_name)),
                None => status.set_text("Not connected"),
            }

            device_list::populate_device_list(list_box, &devices, &bt, status);
            log::info!("BT device list refreshed: {} devices", devices.len());
            state.borrow_mut().bt_devices = devices;
        }
        Err(e) => {
            log::error!("Failed to get BT devices: {e}");
            status.set_text("Failed to load devices");
        }
    }
}
