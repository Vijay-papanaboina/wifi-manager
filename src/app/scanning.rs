//! Scanning — initial state, scan-on-show polling, and manual scan button.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::ui::window::PanelWidgets;

use super::{AppState, get_wifi, refresh_list};

const WIFI_SCAN_RESULT_WAIT_MS: u64 = 2500;
const WIFI_AUTO_SCAN_INTERVAL_MS: u64 = 15000;

/// Poll the scan_requested flag and trigger scan+refresh when set.
/// This runs on the GTK main thread via glib::timeout_add_local.
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

/// Initial state: check WiFi status and trigger first scan.
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

/// Run a manual Wi-Fi scan (spinner visible, list hidden).
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
            status.set_text("Wi-Fi is disabled");
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
