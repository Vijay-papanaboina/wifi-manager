//! Bluetooth controller — initial setup and GTK signal wiring.
//!
//! Pure event-handler setup: tab toggle, power switch, and device row clicks.
//! Scan logic lives in `bt_scanning`; shared helpers live in `bt_helpers`.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::bluetooth_device::BluetoothDevice;
use crate::dbus::bluetooth_manager::BluetoothManager;
use crate::ui::window::PanelWidgets;

use super::AppState;
use super::bt_helpers::{
    clear_bt_list, get_bt, refresh_bt_list,
};
use super::bt_scanning::{
    run_bt_scan_burst, start_bt_background_tasks, ManualBtScanUi,
};

// Re-export scanning entry-points used by mod.rs
pub(super) use super::bt_scanning::{
    resume_bt_background_tasks, run_manual_scan, stop_bt_background_tasks, stop_bt_discovery,
};

const BT_MANUAL_SCAN_WINDOW_MS: u64 = 5000;

/// Set up all Bluetooth UI event handlers.
///
/// Hides the BT tab entirely when no Bluetooth adapter is available.
pub(super) fn setup_bluetooth(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
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
                bt_tab.set_visible(false);
                bt_spinner.set_visible(false);
                log::info!("No Bluetooth adapter found — BT tab hidden");
                return;
            }
        };

        log::info!("Bluetooth adapter available — BT tab enabled");
        state.borrow_mut().bluetooth = Some(bt.clone());

        // ── BT tab activation ──────────────────────────────────────────────
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
                    stop_bt_background_tasks(&state);
                    let state = Rc::clone(&state);
                    let btn = btn.clone();
                    glib::spawn_future_local(async move {
                        if !btn.is_active() {
                            if let Some(bt) = get_bt(&state) {
                                let _ = bt.stop_discovery().await;
                            }
                        }
                    });
                    return;
                }

                title.set_text("Bluetooth");
                scan_btn.set_tooltip_text(Some("Scan for devices"));
                switch.set_tooltip_text(Some("Enable/Disable Bluetooth"));

                let state = Rc::clone(&state);
                let bt_list_box = bt_list_box.clone();
                let bt_spinner = bt_spinner.clone();
                let bt_scroll = bt_scroll.clone();
                let status = status.clone();
                let switch = switch.clone();
                let state_for_bg = Rc::clone(&state);
                let bt_tab_for_bg = btn.clone();

                glib::spawn_future_local(async move {
                    let bt = match get_bt(&state) {
                        Some(bt) => bt,
                        None => return,
                    };

                    let powered = match bt.is_powered().await {
                        Ok(p) => p,
                        Err(e) => {
                            log::error!("Failed to get BT power state: {e}");
                            true
                        }
                    };
                    if bt_tab_for_bg.is_active() {
                        switch.set_active(powered);
                    }

                    if !powered {
                        status.set_text("Bluetooth disabled");
                        bt_spinner.set_visible(false);
                        bt_spinner.set_spinning(false);
                        bt_scroll.set_visible(true);
                        stop_bt_background_tasks(&state);
                        clear_bt_list(&state, &bt_list_box, &status, "Bluetooth disabled");
                        return;
                    }

                    bt_spinner.set_visible(false);
                    bt_spinner.set_spinning(false);
                    bt_scroll.set_visible(true);

                    refresh_bt_list(&state, &bt_list_box, &status).await;
                    start_bt_background_tasks(state_for_bg, bt_tab_for_bg, bt_list_box, status);
                });
            });
        }

        // ── BT power toggle ────────────────────────────────────────────────
        {
            let state = Rc::clone(&state);
            let bt_list_box = bt_list_box.clone();
            let bt_spinner = bt_spinner.clone();
            let bt_scroll = bt_scroll.clone();
            let status = status.clone();
            let bt_tab_c = bt_tab.clone();
            let scan_btn = scan_btn.clone();

            switch.connect_state_set(move |switch, enabled| {
                if !bt_tab_c.is_active() {
                    return glib::Propagation::Proceed;
                }

                let bt_tab_c = bt_tab_c.clone();
                let scan_btn = scan_btn.clone();
                let state = Rc::clone(&state);
                let bt_list_box = bt_list_box.clone();
                let bt_spinner = bt_spinner.clone();
                let bt_scroll = bt_scroll.clone();
                let status = status.clone();
                let switch = switch.clone();

                switch.set_sensitive(false);
                state.borrow_mut().bt_power_transition_in_progress = true;

                glib::spawn_future_local(async move {
                    let bt = match get_bt(&state) {
                        Some(bt) => bt,
                        None => {
                            if bt_tab_c.is_active() {
                                switch.set_active(!enabled);
                                status.set_text("Bluetooth unavailable");
                            }
                            state.borrow_mut().bt_power_transition_in_progress = false;
                            switch.set_sensitive(true);
                            return;
                        }
                    };

                    match bt.set_powered(enabled).await {
                        Ok(_) => {
                            if enabled {
                                if bt_tab_c.is_active() {
                                    status.set_text("Bluetooth enabled");
                                    scan_btn.set_sensitive(false);
                                }
                                let task_list_box = bt_list_box.clone();
                                let task_status = status.clone();
                                let task_tab = bt_tab_c.clone();
                                run_bt_scan_burst(
                                    Rc::clone(&state),
                                    bt_list_box.clone(),
                                    status.clone(),
                                    bt_tab_c.clone(),
                                    Some(ManualBtScanUi {
                                        scan_btn: scan_btn.clone(),
                                        spinner: bt_spinner,
                                        scroll: bt_scroll,
                                    }),
                                    BT_MANUAL_SCAN_WINDOW_MS,
                                )
                                .await;

                                // Powering BT back on must restore the
                                // background scan/live-refresh loop too.
                                let actual = bt.is_powered().await.unwrap_or(false);
                                if task_tab.is_active() && actual {
                                    start_bt_background_tasks(
                                        Rc::clone(&state),
                                        task_tab,
                                        task_list_box,
                                        task_status,
                                    );
                                } else if !actual {
                                    stop_bt_background_tasks(&state);
                                    if bt_tab_c.is_active() {
                                        clear_bt_list(
                                            &state,
                                            &bt_list_box,
                                            &status,
                                            "Bluetooth disabled",
                                        );
                                    }
                                }
                            } else {
                                stop_bt_background_tasks(&state);
                                if bt_tab_c.is_active() {
                                    clear_bt_list(
                                        &state,
                                        &bt_list_box,
                                        &status,
                                        "Bluetooth disabled",
                                    );
                                }
                                let _ = bt.stop_discovery().await;
                            }
                            // A successful setter can still race with an
                            // external power change. Reflect the property
                            // that is actually present after the operation.
                            let actual = bt.is_powered().await.unwrap_or(enabled);
                            if bt_tab_c.is_active() {
                                switch.set_active(actual);
                            }
                        }
                        Err(e) => {
                            log::error!("BT power toggle failed: {e}");
                            // Restore the last requested state when the
                            // backend rejects the operation. A direct
                            // property update does not emit state-set.
                            let actual = bt.is_powered().await.unwrap_or(!enabled);
                            if !actual {
                                stop_bt_background_tasks(&state);
                                if bt_tab_c.is_active() {
                                    clear_bt_list(
                                        &state,
                                        &bt_list_box,
                                        &status,
                                        "Bluetooth disabled",
                                    );
                                }
                                let _ = bt.stop_discovery().await;
                            }
                            if bt_tab_c.is_active() {
                                switch.set_active(actual);
                                status.set_text("Toggle failed");
                            }
                        }
                    }
                    state.borrow_mut().bt_power_transition_in_progress = false;
                    switch.set_sensitive(true);
                });

                glib::Propagation::Proceed
            });
        }

        // ── Device row click: connect / disconnect / pair ──────────────────
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
                        let dev_path = st.bt_row_paths.get(index).and_then(|v| v.clone());
                        let dev = dev_path.and_then(|path| {
                            st.bt_devices
                                .iter()
                                .find(|d| d.device_path == path)
                                .cloned()
                        });
                        let bt = st.bluetooth.clone();
                        (dev, bt)
                    };

                    let (Some(device), Some(bt)) = (device, bt) else {
                        return;
                    };

                    handle_device_row_click(state, status, bt_list_box, device, bt).await;
                });
            });
        }
    });
}

/// Execute the connect / disconnect / pair flow for a tapped device row.
async fn handle_device_row_click(
    state: Rc<RefCell<AppState>>,
    status: gtk4::Label,
    bt_list_box: gtk4::ListBox,
    device: BluetoothDevice,
    bt: BluetoothManager,
) {
    // Mark a device as pending and show a status update.
    let set_pending = |state: &Rc<RefCell<AppState>>,
                       status: &gtk4::Label,
                       bt_list_box: &gtk4::ListBox,
                       device: &BluetoothDevice,
                       pending_label: &str,
                       status_prefix: &str| {
        {
            let mut st = state.borrow_mut();
            st.bt_pending
                .insert(device.device_path.clone(), pending_label.to_string());
        }
        status.set_text(&format!("{} {}...", status_prefix, device.display_name));
        glib::spawn_future_local({
            let state = Rc::clone(state);
            let bt_list_box = bt_list_box.clone();
            let status = status.clone();
            async move {
                refresh_bt_list(&state, &bt_list_box, &status).await;
            }
        });
    };

    // Clear a pending entry and refresh the list.
    let clear_pending = |state: &Rc<RefCell<AppState>>,
                         bt_list_box: &gtk4::ListBox,
                         status: &gtk4::Label,
                         device: &BluetoothDevice| {
        let mut st = state.borrow_mut();
        st.bt_pending.remove(&device.device_path);
        glib::spawn_future_local({
            let state = Rc::clone(state);
            let bt_list_box = bt_list_box.clone();
            let status = status.clone();
            async move {
                refresh_bt_list(&state, &bt_list_box, &status).await;
            }
        });
    };

    if device.connected {
        set_pending(&state, &status, &bt_list_box, &device, "Disconnecting", "Disconnecting");
        match bt.disconnect_device(&device.device_path).await {
            Ok(_) => {
                glib::timeout_future(std::time::Duration::from_millis(500)).await;
                clear_pending(&state, &bt_list_box, &status, &device);
                refresh_bt_list(&state, &bt_list_box, &status).await;
            }
            Err(e) => {
                log::error!("BT disconnect failed: {e}");
                status.set_text("Disconnect failed");
                clear_pending(&state, &bt_list_box, &status, &device);
            }
        }
    } else if device.paired {
        set_pending(&state, &status, &bt_list_box, &device, "Connecting", "Connecting to");
        match bt.connect_device(&device.device_path).await {
            Ok(_) => {
                glib::timeout_future(std::time::Duration::from_millis(1000)).await;
                clear_pending(&state, &bt_list_box, &status, &device);
                refresh_bt_list(&state, &bt_list_box, &status).await;
            }
            Err(e) => {
                log::error!("BT connect failed: {e}");
                status.set_text("Connection failed");
                clear_pending(&state, &bt_list_box, &status, &device);
            }
        }
    } else {
        // "Just Works" pair then connect
        set_pending(&state, &status, &bt_list_box, &device, "Pairing", "Pairing with");
        match bt.pair_device(&device.device_path).await {
            Ok(_) => {
                let _ = bt.trust_device(&device.device_path, true).await;
                set_pending(&state, &status, &bt_list_box, &device, "Connecting", "Connecting to");
                let _ = bt.connect_device(&device.device_path).await;
                glib::timeout_future(std::time::Duration::from_millis(1000)).await;
                clear_pending(&state, &bt_list_box, &status, &device);
                refresh_bt_list(&state, &bt_list_box, &status).await;
            }
            Err(e) => {
                log::error!("BT pairing failed: {e}");
                status.set_text("Pairing failed — try bluetoothctl");
                clear_pending(&state, &bt_list_box, &status, &device);
            }
        }
    }
}
