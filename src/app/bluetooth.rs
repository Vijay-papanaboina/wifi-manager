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

/// Initialize Bluetooth UI handlers and wire Bluetooth manager lifecycle to the UI.
///
/// Sets up event handlers for the Bluetooth tab, power switch, scan button and device row
/// actions. If no Bluetooth adapter is available the Bluetooth tab and spinner are hidden
/// and no further handlers are attached. When a Bluetooth adapter is present this function:
/// - stores the Bluetooth manager in app state,
/// - synchronizes UI state when the BT tab is activated,
/// - starts/stops background scanning and live refresh tasks as the tab and power state change,
/// - initiates manual scan bursts when enabling Bluetooth or pressing Scan,
/// - handles connect / disconnect / pair flows when device rows are activated.
///
/// # Examples
///
/// ```
/// // Construct widgets and state appropriately in your application and then:
/// // setup_bluetooth(&widgets, state.clone());
/// //
/// // This example is a usage sketch — real widgets and AppState must be provided by the app.
/// ```
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

/// Initiates a manual Bluetooth scan and updates the UI for the scan burst.
///
/// Shows the spinner, hides the device list, and disables the scan button, then
/// spawns an asynchronous task that runs a scan burst using a ManualBtScanUi
/// wrapper. The UI will be restored by the scan task when the burst completes.
///
/// # Examples
///
/// ```no_run
/// // `state`, `bt_tab`, `list_box`, `status`, `scan_btn`, `spinner`, `scroll`
/// // are GTK objects created elsewhere in the application.
/// run_manual_scan(state, bt_tab, list_box, status, scan_btn, spinner, scroll);
/// ```
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

/// Retrieve the BluetoothManager stored in the application state.
///
/// # Returns
///
/// `Some(BluetoothManager)` if a manager is currently stored in `AppState`, `None` otherwise.
///
/// # Examples
///
/// ```
/// // Given `state: Rc<RefCell<AppState>>`
/// let maybe_bt = get_bt(&state);
/// if let Some(bt) = maybe_bt {
///     // use bt
/// }
/// ```
fn get_bt(state: &Rc<RefCell<AppState>>) -> Option<BluetoothManager> {
    state.borrow().bluetooth.clone()
}

/// Refreshes the Bluetooth device list and updates the UI and AppState.
///
/// This function does nothing if the Bluetooth menu is currently open. It obtains the
/// Bluetooth manager from the shared application state, requests the current devices,
/// updates the provided status label to show the connected device (or "Not connected"),
/// populates the given GTK list with devices, and stores the refreshed device list in
/// AppState. On failure it logs the error and sets the status label to "Failed to load devices".
///
/// # Parameters
///
/// - `state`: Shared application state containing the Bluetooth manager and device list.
/// - `list_box`: GTK ListBox to populate with device rows.
/// - `status`: GTK Label used to show connection/status text.
///
/// # Examples
///
/// ```
/// // within an async context where `state`, `list_box`, and `status` are available:
/// refresh_bt_list(&state, &list_box, &status).await;
/// ```
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

/// Starts Bluetooth background activity for the BT tab.
///
/// Marks automatic scanning as active, launches an immediate auto-scan burst, schedules subsequent
/// auto-scans after a cooldown, and starts a periodic live-refresh timer that refreshes the device
/// list while the BT tab remains active and no scan is in progress.
///
/// # Parameters
///
/// - `state`: shared application state containing Bluetooth manager and background-task flags.
/// - `bt_tab`: the BT tab toggle button; tasks stop when this tab is no longer active.
/// - `list_box`: UI list to be refreshed with discovered devices.
/// - `status`: status label updated by refresh and scan operations.
///
/// # Examples
///
/// ```no_run
/// # use std::rc::Rc;
/// # use std::cell::RefCell;
/// # use gtk4::prelude::*;
/// // Assume `state`, `bt_tab`, `list_box`, and `status` are previously initialized.
/// start_bt_background_tasks(Rc::clone(&state), bt_tab, list_box, status);
/// ```
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

/// Stops any scheduled Bluetooth background timers and disables auto-scan.
///
/// Cancels and removes the stored auto-scan and live-refresh timer sources from the
/// application state and marks Bluetooth auto-scan as inactive.
///
/// # Examples
///
/// ```no_run
/// let state = Rc::new(RefCell::new(AppState::default()));
/// stop_bt_background_tasks(&state);
/// ```
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

/// Schedule a delayed Bluetooth auto-scan and store its timer ID in application state.
///
/// After a cooldown (BT_AUTO_SCAN_COOLDOWN_MS), this function will clear the stored
/// cooldown source, and if the Bluetooth tab is still active and auto-scan remains enabled,
/// launch a background scan burst. When that scan completes, it will reschedule another
/// auto-scan under the same conditions.
///
/// The function stores the GLib timeout source id in `AppState.bt_auto_scan_source` so it can
/// be cancelled or inspected later.
///
/// # Parameters
///
/// - `state`: shared application state; this function mutates `bt_auto_scan_source`.
/// - `bt_tab`: the Bluetooth tab toggle; used to check whether the tab is currently active.
/// - `list_box`: the devices list UI used by the scheduled scan burst.
/// - `status`: status label updated during the scan burst.
///
/// # Examples
///
/// ```no_run
/// // Given initialized `state`, `bt_tab`, `list_box`, and `status`:
/// schedule_bt_auto_scan(state.clone(), bt_tab.clone(), list_box.clone(), status.clone());
/// ```
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

/// A no-op device removal callback.
///
/// This returns an `Rc`-wrapped closure that accepts a device path `String` and does nothing.
/// Useful as a default placeholder when no removal action is required.
///
/// # Examples
///
/// ```
/// let cb = no_op_remove();
/// cb("device/path".to_string()); // no-op
/// ```
fn no_op_remove() -> std::rc::Rc<dyn Fn(String)> {
    std::rc::Rc::new(|_path| {})
}

/// Returns a shared no-op callback that accepts a boolean `active` flag and does nothing.
///
/// The returned value is an `Rc<dyn Fn(bool)>` suitable as a placeholder when a menu-active callback is required.
///
/// # Examples
///
/// ```
/// let cb = no_op_menu_active();
/// cb(true);  // no-op
/// cb(false); // still a no-op
/// ```
fn no_op_menu_active() -> std::rc::Rc<dyn Fn(bool)> {
    std::rc::Rc::new(|_active| {})
}

/// Creates a callback that unpairs a Bluetooth device by its object path and updates the UI.
///
/// The returned closure takes a device object path (`String`), attempts to remove/unpair that
/// device via the provided `BluetoothManager`, updates `status` with the outcome, and refreshes
/// the device list on success. On failure the closure logs the error and sets `status` to an
/// error message.
///
/// # Examples
///
/// ```
/// // Given `state`, `list_box`, `status`, and `bt` already initialized:
/// let remove_cb = build_remove_callback(&state, &list_box, &status, &bt);
/// remove_cb("/org/bluez/hci0/dev_XX_XX_XX_XX_XX_XX".into());
/// ```
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

/// Creates a shared callback that updates the app state's Bluetooth menu active flag.
///
/// The returned `Rc<dyn Fn(bool)>` accepts `active` and sets `AppState.bt_menu_open` to that value.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use std::cell::RefCell;
///
/// struct AppState { bt_menu_open: bool }
///
/// fn build_menu_active_callback(state: &Rc<RefCell<AppState>>) -> std::rc::Rc<dyn Fn(bool)> {
///     let state = Rc::clone(state);
///     std::rc::Rc::new(move |active| {
///         state.borrow_mut().bt_menu_open = active;
///     })
/// }
///
/// let state = Rc::new(RefCell::new(AppState { bt_menu_open: false }));
/// let cb = build_menu_active_callback(&state);
/// cb(true);
/// assert!(state.borrow().bt_menu_open);
/// ```
fn build_menu_active_callback(state: &Rc<RefCell<AppState>>) -> std::rc::Rc<dyn Fn(bool)> {
    let state = Rc::clone(state);
    std::rc::Rc::new(move |active| {
        state.borrow_mut().bt_menu_open = active;
    })
}

/// Performs a single Bluetooth scan burst and updates UI/state accordingly.
///
/// This starts a discovery window for the specified duration, refreshes the device
/// list when complete (if the Bluetooth tab is still active), and will stop discovery
/// if it started it. If a manual scan UI is provided, the spinner/scroll/button are
/// updated before and after the burst. The function returns immediately if the BT tab
/// is not active, if a scan is already in progress, if no Bluetooth manager is available,
/// or if Bluetooth is powered off.
///
/// Observable effects:
/// - Sets and clears the global "scan in progress" flag to prevent concurrent bursts.
/// - Starts discovery if not already discovering and stops it when appropriate.
/// - Calls refresh_bt_list after the scan window when the tab remains active.
/// - Updates provided ManualBtScanUi (spinner/scroll/button) to reflect scan state.
/// - Updates `status` label with short messages when relevant (e.g., "Bluetooth disabled",
///   "Scan already running").
///
/// # Examples
///
/// ```no_run
/// use std::rc::Rc;
/// use std::cell::RefCell;
/// // Assume `AppState`, `ManualBtScanUi` and GTK widgets are created elsewhere.
/// # async fn example_call() {
/// let state = Rc::new(RefCell::new(/* AppState */ unimplemented!()));
/// let list_box = /* gtk4::ListBox */ unimplemented!();
/// let status = /* gtk4::Label */ unimplemented!();
/// let bt_tab = /* gtk4::ToggleButton */ unimplemented!();
/// let manual_ui = None::<ManualBtScanUi>;
/// // Run a 5 second scan burst
/// run_bt_scan_burst(state, list_box, status, bt_tab, manual_ui, 5000).await;
/// # }
/// ```
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
