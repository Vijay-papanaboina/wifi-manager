//! Bluetooth scanning and background task management.
//!
//! Handles scan bursts (both manual and automatic), the periodic live-refresh
//! timer, and the cooldown-based auto-scan scheduler.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use super::AppState;
use super::bt_helpers::{get_bt, refresh_bt_list};

const BT_MANUAL_SCAN_WINDOW_MS: u64 = 5000;
pub(super) const BT_AUTO_SCAN_WINDOW_MS: u64 = 10000;
const BT_AUTO_SCAN_COOLDOWN_MS: u64 = 10000;
const BT_LIVE_REFRESH_INTERVAL_MS: u64 = 2000;

/// Widget handles passed to `run_bt_scan_burst` for a manually triggered scan.
pub(super) struct ManualBtScanUi {
    pub(super) scan_btn: gtk4::Button,
    pub(super) spinner: gtk4::Spinner,
    pub(super) scroll: gtk4::ScrolledWindow,
}

/// Run a manual Bluetooth scan: show spinner, hide list, scan, then restore.
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

/// Start the background auto-scan + live-refresh timers.
///
/// Guards against double-start with `bt_auto_scan_active`.
pub(super) fn start_bt_background_tasks(
    state: Rc<RefCell<AppState>>,
    bt_tab: gtk4::ToggleButton,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
) {
    if state.borrow().bt_auto_scan_active {
        return;
    }
    state.borrow_mut().bt_auto_scan_active = true;

    // Kick off an immediate scan burst when the tab activates.
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

    // Periodic live-refresh (shows newly visible devices without a full scan).
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

    state.borrow_mut().bt_live_refresh_source = Some(live_refresh_id);
}

/// Cancel all background tasks and clear their source IDs.
pub(super) fn stop_bt_background_tasks(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    if let Some(id) = st.bt_auto_scan_source.take() {
        id.remove();
    }
    if let Some(id) = st.bt_live_refresh_source.take() {
        id.remove();
    }
    st.bt_auto_scan_active = false;
}

/// Resume background tasks when the panel becomes visible again.
///
/// Only starts if the BT tab is active and the adapter is powered.
pub(super) async fn resume_bt_background_tasks(
    state: Rc<RefCell<AppState>>,
    bt_tab: gtk4::ToggleButton,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
) {
    if !bt_tab.is_active() {
        return;
    }
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
    if !powered {
        return;
    }
    start_bt_background_tasks(state, bt_tab, list_box, status);
}

/// Tell BlueZ to stop discovery (best-effort, logs a warning on failure).
pub(super) async fn stop_bt_discovery(state: Rc<RefCell<AppState>>) {
    if let Some(bt) = get_bt(&state) {
        if let Err(e) = bt.stop_discovery().await {
            log::warn!("BT discovery stop failed: {e}");
        }
    }
}

/// Schedule the next automatic scan burst after the cooldown period.
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

/// Run a single Bluetooth discovery window.
///
/// Handles the scan guard, power check, D-Bus start/stop, and optional
/// manual-scan UI (spinner/scroll visibility).
pub(super) async fn run_bt_scan_burst(
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

    // RAII guard that clears bt_scan_in_progress even if we return early.
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
