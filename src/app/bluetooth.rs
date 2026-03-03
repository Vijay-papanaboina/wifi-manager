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

const BT_MANUAL_SCAN_WINDOW_MS: u64 = 5000;
const BT_AUTO_SCAN_WINDOW_MS: u64 = 10000;
const BT_AUTO_SCAN_COOLDOWN_MS: u64 = 10000;
const BT_LIVE_REFRESH_INTERVAL_MS: u64 = 2000;

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
                let state_for_bg = Rc::clone(&state);
                let bt_tab_for_bg = btn.clone();

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
                        bt_spinner.set_spinning(false);
                        bt_scroll.set_visible(true);
                        device_list::populate_device_list(
                            &bt_list_box,
                            &[],
                            no_op_remove(),
                            no_op_menu_active(),
                        );
                        stop_bt_background_tasks(&state);
                        return;
                    }

                    // BT is on — show list, refresh once, then start background tasks
                    bt_spinner.set_visible(false);
                    bt_spinner.set_spinning(false);
                    bt_scroll.set_visible(true);

                    refresh_bt_list(&state, &bt_list_box, &status).await;
                    start_bt_background_tasks(
                        state_for_bg,
                        bt_tab_for_bg,
                        bt_list_box,
                        status,
                    );
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
            let scan_btn = scan_btn.clone();

            switch.connect_state_set(move |_switch, enabled| {
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

                glib::spawn_future_local(async move {
                    let bt = match get_bt(&state) {
                        Some(bt) => bt,
                        None => return,
                    };

                    match bt.set_powered(enabled).await {
                        Ok(_) => {
                            if enabled {
                                status.set_text("Bluetooth enabled");
                                scan_btn.set_sensitive(false);
                                run_bt_scan_burst(
                                    Rc::clone(&state),
                                    bt_list_box,
                                    status,
                                    bt_tab_c.clone(),
                                    Some(ManualBtScanUi {
                                        scan_btn: scan_btn.clone(),
                                        spinner: bt_spinner,
                                        scroll: bt_scroll,
                                    }),
                                    BT_MANUAL_SCAN_WINDOW_MS,
                                )
                                .await;
                            } else {
                                status.set_text("Bluetooth disabled");
                                device_list::populate_device_list(
                                    &bt_list_box,
                                    &[],
                                    no_op_remove(),
                                    no_op_menu_active(),
                                );
                                stop_bt_background_tasks(&state);
                                let _ = bt.stop_discovery().await;
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

/// Run a manual Bluetooth scan (spinner visible, list hidden).
pub(super) fn run_manual_scan(
    state: Rc<RefCell<AppState>>,
    bt_tab: gtk4::ToggleButton,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
    scan_btn: gtk4::Button,
    spinner: gtk4::Spinner,
    scroll: gtk4::ScrolledWindow,
) {
    scan_btn.set_sensitive(false);
    spinner.set_visible(true);
    spinner.set_spinning(true);
    scroll.set_visible(false);

    glib::spawn_future_local(async move {
        run_bt_scan_burst(
            state,
            list_box,
            status,
            bt_tab,
            Some(ManualBtScanUi {
                scan_btn,
                spinner,
                scroll,
            }),
            BT_MANUAL_SCAN_WINDOW_MS,
        )
        .await;
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
    if state.borrow().bt_menu_open {
        log::debug!("BT menu open — skipping refresh");
        return;
    }
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

            let on_remove = build_remove_callback(state, list_box, status, &bt);
            let on_menu_active = build_menu_active_callback(state);
            device_list::populate_device_list(list_box, &devices, on_remove, on_menu_active);
            log::info!("BT device list refreshed: {} devices", devices.len());
            state.borrow_mut().bt_devices = devices;
        }
        Err(e) => {
            log::error!("Failed to get BT devices: {e}");
            status.set_text("Failed to load devices");
        }
    }
}

struct ManualBtScanUi {
    scan_btn: gtk4::Button,
    spinner: gtk4::Spinner,
    scroll: gtk4::ScrolledWindow,
}

fn start_bt_background_tasks(
    state: Rc<RefCell<AppState>>,
    bt_tab: gtk4::ToggleButton,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
) {
    if state.borrow().bt_auto_scan_active {
        return;
    }
    state.borrow_mut().bt_auto_scan_active = true;

    // Immediate background scan burst when tab activates
    glib::spawn_future_local({
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let bt_tab = bt_tab.clone();
        async move {
            run_bt_scan_burst(
                Rc::clone(&state),
                list_box.clone(),
                status.clone(),
                bt_tab.clone(),
                None,
                BT_AUTO_SCAN_WINDOW_MS,
            )
            .await;
            if !bt_tab.is_active() || !state.borrow().bt_auto_scan_active {
                return;
            }
            schedule_bt_auto_scan(state, bt_tab, list_box, status);
        }
    });

    let live_refresh_id = glib::timeout_add_local(
        std::time::Duration::from_millis(BT_LIVE_REFRESH_INTERVAL_MS),
        {
            let state = Rc::clone(&state);
            let list_box = list_box.clone();
            let status = status.clone();
            let bt_tab = bt_tab.clone();
            move || {
                if !bt_tab.is_active() {
                    state.borrow_mut().bt_live_refresh_source = None;
                    return glib::ControlFlow::Break;
                }
                if state.borrow().bt_scan_in_progress {
                    return glib::ControlFlow::Continue;
                }
                glib::spawn_future_local({
                    let state = Rc::clone(&state);
                    let list_box = list_box.clone();
                    let status = status.clone();
                    async move {
                        refresh_bt_list(&state, &list_box, &status).await;
                    }
                });
                glib::ControlFlow::Continue
            }
        },
    );

    let mut st = state.borrow_mut();
    st.bt_live_refresh_source = Some(live_refresh_id);
}

fn stop_bt_background_tasks(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    if let Some(id) = st.bt_auto_scan_source.take() {
        id.remove();
    }
    if let Some(id) = st.bt_live_refresh_source.take() {
        id.remove();
    }
    st.bt_auto_scan_active = false;
}

fn schedule_bt_auto_scan(
    state: Rc<RefCell<AppState>>,
    bt_tab: gtk4::ToggleButton,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
) {
    let state_for_cb = Rc::clone(&state);
    let source_id = glib::timeout_add_local_once(
        std::time::Duration::from_millis(BT_AUTO_SCAN_COOLDOWN_MS),
        move || {
            state_for_cb.borrow_mut().bt_auto_scan_source = None;
            if !bt_tab.is_active() || !state_for_cb.borrow().bt_auto_scan_active {
                return;
            }
            glib::spawn_future_local({
                let state = Rc::clone(&state_for_cb);
                let list_box = list_box.clone();
                let status = status.clone();
                let bt_tab = bt_tab.clone();
                async move {
                    run_bt_scan_burst(
                        Rc::clone(&state),
                        list_box.clone(),
                        status.clone(),
                        bt_tab.clone(),
                        None,
                        BT_AUTO_SCAN_WINDOW_MS,
                    )
                    .await;
                    if bt_tab.is_active() && state.borrow().bt_auto_scan_active {
                        schedule_bt_auto_scan(state, bt_tab, list_box, status);
                    }
                }
            });
        },
    );
    state.borrow_mut().bt_auto_scan_source = Some(source_id);
}

fn no_op_remove() -> std::rc::Rc<dyn Fn(String)> {
    std::rc::Rc::new(|_path| {})
}

fn no_op_menu_active() -> std::rc::Rc<dyn Fn(bool)> {
    std::rc::Rc::new(|_active| {})
}

fn build_remove_callback(
    state: &Rc<RefCell<AppState>>,
    list_box: &gtk4::ListBox,
    status: &gtk4::Label,
    bt: &BluetoothManager,
) -> std::rc::Rc<dyn Fn(String)> {
    let state = Rc::clone(state);
    let list_box = list_box.clone();
    let status = status.clone();
    let bt = bt.clone();
    std::rc::Rc::new(move |device_path| {
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let bt = bt.clone();
        glib::spawn_future_local(async move {
            status.set_text("Unpairing device...");
            match bt.remove_device(&device_path).await {
                Ok(_) => {
                    status.set_text("Device unpaired");
                    refresh_bt_list(&state, &list_box, &status).await;
                }
                Err(e) => {
                    log::error!("Remove failed: {e}");
                    status.set_text(&format!("Failed to unpair: {}", e));
                }
            }
        });
    })
}

fn build_menu_active_callback(state: &Rc<RefCell<AppState>>) -> std::rc::Rc<dyn Fn(bool)> {
    let state = Rc::clone(state);
    std::rc::Rc::new(move |active| {
        state.borrow_mut().bt_menu_open = active;
    })
}

async fn run_bt_scan_burst(
    state: Rc<RefCell<AppState>>,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
    bt_tab: gtk4::ToggleButton,
    manual_ui: Option<ManualBtScanUi>,
    scan_window_ms: u64,
) {
    fn finish_manual_ui(ui: ManualBtScanUi) {
        ui.spinner.set_spinning(false);
        ui.spinner.set_visible(false);
        ui.scroll.set_visible(true);
        ui.scan_btn.set_sensitive(true);
    }

    struct ScanGuard(Rc<RefCell<AppState>>);
    impl Drop for ScanGuard {
        fn drop(&mut self) {
            self.0.borrow_mut().bt_scan_in_progress = false;
        }
    }

    if !bt_tab.is_active() {
        if let Some(ui) = manual_ui {
            finish_manual_ui(ui);
        }
        return;
    }

    {
        let mut st = state.borrow_mut();
        if st.bt_scan_in_progress {
            if let Some(ui) = manual_ui {
                status.set_text("Scan already running");
                finish_manual_ui(ui);
            }
            return;
        }
        st.bt_scan_in_progress = true;
    }
    let _guard = ScanGuard(Rc::clone(&state));

    let bt = match get_bt(&state) {
        Some(bt) => bt,
        None => {
            if let Some(ui) = manual_ui {
                finish_manual_ui(ui);
            }
            return;
        }
    };

    let powered = match bt.is_powered().await {
        Ok(p) => p,
        Err(e) => {
            log::error!("Failed to get BT power state: {e}");
            true
        }
    };

    if !powered {
        status.set_text("Bluetooth disabled");
        if let Some(ui) = manual_ui {
            finish_manual_ui(ui);
        }
        return;
    }

    if let Some(ref ui) = manual_ui {
        ui.spinner.set_visible(true);
        ui.spinner.set_spinning(true);
        ui.scroll.set_visible(false);
    }

    let discovering = bt.is_discovering().await.unwrap_or(false);
    let mut started_discovery = false;
    if !discovering {
        match bt.start_discovery().await {
            Ok(()) => {
                started_discovery = true;
            }
            Err(e) => {
                log::warn!("BT discovery failed: {e}");
            }
        }
    }

    glib::timeout_future(std::time::Duration::from_millis(scan_window_ms)).await;

    if bt_tab.is_active() {
        refresh_bt_list(&state, &list_box, &status).await;
    }

    if started_discovery {
        if let Err(e) = bt.stop_discovery().await {
            log::warn!("BT discovery stop failed: {e}");
        }
    }

    if let Some(ui) = manual_ui {
        finish_manual_ui(ui);
    }
}
