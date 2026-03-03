//! Scanning — initial state, scan-on-show polling, and manual scan button.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::ui::window::PanelWidgets;

use super::{AppState, get_wifi, refresh_list};

const WIFI_SCAN_RESULT_WAIT_MS: u64 = 2500;
const WIFI_AUTO_SCAN_INTERVAL_MS: u64 = 15000;

/// Polls a shared `scan_requested` flag and, when set, updates the WiFi enabled state,
/// requests a scan, waits for results, and refreshes the network list UI.
///
/// This schedules a GLib timeout on the GTK main thread that checks the flag periodically
/// (every 200 ms) and spawns an async task to perform the state update, scan request,
/// wait for scan results, and UI refresh.
///
/// # Examples
///
/// ```
/// // Assume `widgets`, `state`, and `scan_requested` have been created earlier.
/// // `scan_requested` can be set from other code to trigger a scan-on-show cycle:
/// // scan_requested.store(true, std::sync::atomic::Ordering::Relaxed);
/// setup_scan_on_show(&widgets, state.clone(), scan_requested.clone());
/// ```
pub(super) fn setup_scan_on_show(
    widgets: &PanelWidgets,
    state: Rc<RefCell<AppState>>,
    scan_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();
    let switch = widgets.wifi_switch.clone();

    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if scan_requested.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let state = Rc::clone(&state);
            let list_box = list_box.clone();
            let status = status.clone();
            let switch = switch.clone();

            glib::spawn_future_local(async move {
                let wifi = get_wifi(&state);

                // Update WiFi switch state
                match wifi.is_wifi_enabled().await {
                    Ok(enabled) => switch.set_active(enabled),
                    Err(e) => log::error!("Failed to get WiFi state: {e}"),
                }

                // Scan and refresh
                if let Err(e) = wifi.request_scan().await {
                    log::warn!("Scan-on-show scan failed: {e}");
                }
                glib::timeout_future(std::time::Duration::from_millis(
                    WIFI_SCAN_RESULT_WAIT_MS,
                ))
                .await;
                refresh_list(&state, &list_box, &status).await;
            });
        }
        glib::ControlFlow::Continue
    });
}

/// Initialize Wi‑Fi UI state and perform the initial network scan.
///
/// This sets the Wi‑Fi toggle to the current adapter state, requests a first scan,
/// waits briefly for results, refreshes the displayed network list, and hides the
/// startup spinner (showing the list).
///
/// # Examples
///
/// ```
/// // Create or obtain `widgets` and `state` appropriate for the panel, then:
/// // setup_initial_state(&widgets, state.clone());
/// ```
pub(super) fn setup_initial_state(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let switch = widgets.wifi_switch.clone();
    let status = widgets.status_label.clone();
    let list_box = widgets.network_list_box.clone();
    let spinner = widgets.spinner.clone();
    let scrolled = widgets.network_scroll.clone();

    glib::spawn_future_local(async move {
        let wifi = get_wifi(&state);

        // Set WiFi switch to current state
        match wifi.is_wifi_enabled().await {
            Ok(enabled) => switch.set_active(enabled),
            Err(e) => log::error!("Failed to get WiFi state: {e}"),
        }

        // Trigger initial scan
        if let Err(e) = wifi.request_scan().await {
            log::warn!("Initial scan failed: {e}");
        }

        // Brief delay to let NM populate APs after scan
        glib::timeout_future(std::time::Duration::from_millis(WIFI_SCAN_RESULT_WAIT_MS)).await;
        refresh_list(&state, &list_box, &status).await;

        // Hide spinner, show network list
        spinner.set_spinning(false);
        spinner.set_visible(false);
        scrolled.set_visible(true);
    });
}

/// Initiates a manual Wi‑Fi scan and updates the UI to reflect scan progress.
///
/// Disables the provided scan button, spawns a local task that performs the scan,
/// and uses the provided spinner and scrolled window to show a loading state (spinner visible, list hidden)
/// while the scan runs.
pub(super) fn run_manual_scan(
    state: Rc<RefCell<AppState>>,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
    scan_btn: gtk4::Button,
    spinner: gtk4::Spinner,
    scrolled: gtk4::ScrolledWindow,
) {
    // Set scan button as default focus to avoid accidental WiFi toggle
    scan_btn.grab_focus();
    scan_btn.set_sensitive(false);

    glib::spawn_future_local(async move {
        run_wifi_scan(
            state,
            list_box,
            status,
            Some(ManualWifiScanUi {
                scan_btn,
                spinner,
                scrolled,
            }),
        )
        .await;
    });
}

/// Start periodic background Wi‑Fi scans while the Wi‑Fi tab is active.
///
/// Schedules an immediate background scan and then starts a repeating timeout that spawns
/// additional background scans every WIFI_AUTO_SCAN_INTERVAL_MS while `wifi_tab` remains active.
/// The timeout source id is stored in `state.wifi_auto_scan_source`. If an auto-scan source
/// already exists, the function returns without scheduling another one. When the tab becomes
/// inactive the timer stops and the stored source is cleared.
///
/// Parameters:
/// - `state`: shared application state where the auto-scan timer id is stored and scan tasks use
///   Wi‑Fi resources from.
/// - `wifi_tab`: toggle button representing the Wi‑Fi tab; its active state controls whether
///   auto-scanning continues.
/// - `list_box`, `status`: UI elements forwarded to each scan so results and status text can be
///   refreshed.
///
/// # Examples
///
/// ```no_run
/// // Called when the Wi‑Fi tab is shown/activated:
/// start_wifi_auto_scan(state.clone(), wifi_tab.clone(), list_box.clone(), status.clone());
/// ```
pub(super) fn start_wifi_auto_scan(
    state: Rc<RefCell<AppState>>,
    wifi_tab: gtk4::ToggleButton,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
) {
    if state.borrow().wifi_auto_scan_source.is_some() {
        return;
    }

    // Immediate background scan when tab activates
    glib::spawn_future_local({
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        async move {
            run_wifi_scan(state, list_box, status, None).await;
        }
    });

    let scan_id = glib::timeout_add_local(
        std::time::Duration::from_millis(WIFI_AUTO_SCAN_INTERVAL_MS),
        {
            let state = Rc::clone(&state);
            let list_box = list_box.clone();
            let status = status.clone();
            let wifi_tab = wifi_tab.clone();
            move || {
                if !wifi_tab.is_active() {
                    state.borrow_mut().wifi_auto_scan_source = None;
                    return glib::ControlFlow::Break;
                }
                glib::spawn_future_local({
                    let state = Rc::clone(&state);
                    let list_box = list_box.clone();
                    let status = status.clone();
                    async move {
                        run_wifi_scan(state, list_box, status, None).await;
                    }
                });
                glib::ControlFlow::Continue
            }
        },
    );

    state.borrow_mut().wifi_auto_scan_source = Some(scan_id);
}

/// Stops and cancels the active automatic Wi‑Fi scan timer, if one is set on the application state.
///
/// If an automatic scan source is present in the state's `wifi_auto_scan_source`, this function
/// removes it so periodic background scans stop.
///
/// # Examples
///
/// ```ignore
/// // Assuming `state` is an `Rc<RefCell<AppState>>` previously passed to start_wifi_auto_scan:
/// stop_wifi_auto_scan(&state);
/// ```
pub(super) fn stop_wifi_auto_scan(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    if let Some(id) = st.wifi_auto_scan_source.take() {
        id.remove();
    }
}

struct ManualWifiScanUi {
    scan_btn: gtk4::Button,
    spinner: gtk4::Spinner,
    scrolled: gtk4::ScrolledWindow,
}

/// Perform a WiFi scan, update the app state and the network list, and manage optional manual-scan UI.
///
/// This function ensures only one scan runs at a time; if a scan is already in progress it returns immediately
/// (and re-enables the manual scan button if provided). If WiFi is disabled it restores any provided manual UI
/// and returns. On success it requests a system WiFi scan, waits for results, then refreshes the displayed
/// network list and restores UI state. Errors are logged and, on scan failure, the status label is updated to
/// "Scan failed" and any provided manual UI is restored.
///
/// # Parameters
///
/// - `manual_ui`: When `Some`, the supplied controls are shown/hidden and enabled/disabled to reflect scan progress.
///
/// # Examples
///
/// ```
/// // Example usage (pseudo-code: construct real AppState and GTK widgets in application code)
/// # use std::rc::Rc;
/// # use std::cell::RefCell;
/// # async fn run_example() {
/// // let state = Rc::new(RefCell::new(AppState::new()));
/// // let list_box = gtk4::ListBox::new();
/// // let status = gtk4::Label::new(None);
/// // run_wifi_scan(state, list_box, status, None).await;
/// # }
/// ```
async fn run_wifi_scan(
    state: Rc<RefCell<AppState>>,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
    manual_ui: Option<ManualWifiScanUi>,
) {
    struct ScanGuard(Rc<RefCell<AppState>>);
    impl Drop for ScanGuard {
        fn drop(&mut self) {
            self.0.borrow_mut().wifi_scan_in_progress = false;
        }
    }

    {
        let mut st = state.borrow_mut();
        if st.wifi_scan_in_progress {
            if let Some(ui) = manual_ui {
                ui.scan_btn.set_sensitive(true);
            }
            return;
        }
        st.wifi_scan_in_progress = true;
    }
    let _guard = ScanGuard(Rc::clone(&state));

    let wifi = get_wifi(&state);
    match wifi.is_wifi_enabled().await {
        Ok(false) => {
            if let Some(ui) = manual_ui {
                ui.spinner.set_spinning(false);
                ui.spinner.set_visible(false);
                ui.scrolled.set_visible(true);
                ui.scan_btn.set_sensitive(true);
            }
            return;
        }
        Err(e) => {
            log::error!("Failed to get WiFi state: {e}");
        }
        _ => {}
    }

    if let Some(ref ui) = manual_ui {
        ui.spinner.set_visible(true);
        ui.spinner.set_spinning(true);
        ui.scrolled.set_visible(false);
    }

    if let Err(e) = wifi.request_scan().await {
        log::error!("Scan failed: {e}");
        status.set_text("Scan failed");
        if let Some(ui) = manual_ui {
            ui.spinner.set_spinning(false);
            ui.spinner.set_visible(false);
            ui.scrolled.set_visible(true);
            ui.scan_btn.set_sensitive(true);
        }
        return;
    }

    glib::timeout_future(std::time::Duration::from_millis(WIFI_SCAN_RESULT_WAIT_MS)).await;
    refresh_list(&state, &list_box, &status).await;

    if let Some(ui) = manual_ui {
        ui.spinner.set_spinning(false);
        ui.spinner.set_visible(false);
        ui.scrolled.set_visible(true);
        ui.scan_btn.set_sensitive(true);
    }
}
