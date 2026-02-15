//! Scanning — initial state, scan-on-show polling, and manual scan button.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::ui::window::PanelWidgets;

use super::{AppState, get_wifi, refresh_list};

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
                glib::timeout_future(std::time::Duration::from_millis(1500)).await;
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
        glib::timeout_future(std::time::Duration::from_millis(1500)).await;
        refresh_list(&state, &list_box, &status).await;

        // Hide spinner, show network list
        spinner.set_spinning(false);
        spinner.set_visible(false);
        scrolled.set_visible(true);
    });
}

/// Wire the scan button to trigger a scan and refresh the list.
pub(super) fn setup_scan_button(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();
    let scan_btn = widgets.scan_button.clone();
    let spinner = widgets.spinner.clone();
    let scrolled = widgets.network_scroll.clone();

    // Set scan button as default focus to avoid accidental WiFi toggle
    scan_btn.grab_focus();

    scan_btn.connect_clicked(move |btn| {
        btn.set_sensitive(false);
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let btn = btn.clone();
        let spinner = spinner.clone();
        let scrolled = scrolled.clone();

        // Show spinner, hide list
        spinner.set_visible(true);
        spinner.set_spinning(true);
        scrolled.set_visible(false);

        glib::spawn_future_local(async move {
            let wifi = get_wifi(&state);
            if let Err(e) = wifi.request_scan().await {
                log::error!("Scan failed: {e}");
                status.set_text("Scan failed");
                spinner.set_spinning(false);
                spinner.set_visible(false);
                scrolled.set_visible(true);
                btn.set_sensitive(true);
                return;
            }

            // Wait for scan results
            glib::timeout_future(std::time::Duration::from_millis(1500)).await;
            refresh_list(&state, &list_box, &status).await;

            // Hide spinner, show list
            spinner.set_spinning(false);
            spinner.set_visible(false);
            scrolled.set_visible(true);
            btn.set_sensitive(true);
        });
    });
}
