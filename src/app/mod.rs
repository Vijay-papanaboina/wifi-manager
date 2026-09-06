//! Application controller — bridges the GTK4 UI and the D-Bus backend.
//!
//! Split into sub-modules:
//! - `scanning` — scan-on-show, initial scan, scan button
//! - `connection` — WiFi toggle, network click, password dialog
//! - `live_updates` — D-Bus signal subscriptions for real-time changes
//! - `shortcuts` — Escape key, reload polling

mod audio;
mod bluetooth;
mod bt_helpers;
mod bt_live_updates;
mod bt_scanning;
mod commands;
mod connection;
mod controls;
mod live_updates;
mod scanning;
mod shortcuts;
mod state;
mod system_power;
mod vpn;
mod vpn_import;
mod vpn_utils;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::network_manager::WifiManager;
use crate::dbus::vpn_manager::VpnManager;
use crate::ui::network_list;
use crate::ui::window::PanelWidgets;

use self::commands::ScanTarget;
pub(super) use self::state::PendingVpnAction;
use self::state::{ActiveTab, AppState};

/// Set up all event handlers, kick off the initial scan, start live updates,
/// and wire scan-on-show polling.
pub(crate) fn setup(
    widgets: &PanelWidgets,
    wifi: WifiManager,
    scan_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
    panel_state: crate::daemon::PanelState,
) {
    let vpn = VpnManager::new(wifi.connection());
    let state = Rc::new(RefCell::new(AppState {
        wifi,
        vpn,
        // The shell opens on Home; no feature page owns the compatibility
        // status sink until navigation activates its legacy selector.
        active_tab: ActiveTab::None,
        networks: Vec::new(),
        selected_ssid: None,
        bluetooth: None,
        bt_devices: Vec::new(),
        bt_row_paths: Vec::new(),
        bt_pending: HashMap::new(),
        bt_scan_in_progress: false,
        bt_auto_scan_source: None,
        bt_live_refresh_source: None,
        bt_auto_scan_active: false,
        bt_task_generation: 0,
        bt_power_transition_in_progress: false,
        bt_menu_open: false,
        wifi_scan_in_progress: false,
        wifi_auto_scan_source: None,
        wifi_bg_reconnect_source: None,
        wifi_row_ssids: Vec::new(),
        wifi_pending: HashMap::new(),
        vpn_pending: HashMap::new(),
        vpn_active_by_conn: HashMap::new(),
        vpn_refresh_source: None,
        vpn_view_active: false,
        vpn_view_generation: 0,
        vpn_busy_count: 0,
        vpn_normalizing: false,
    }));

    setup_status_ownership(widgets, Rc::clone(&state));
    connection::setup_wifi_toggle(widgets, Rc::clone(&state));
    connection::setup_network_click(widgets, Rc::clone(&state));
    connection::setup_password_actions(widgets, Rc::clone(&state));
    live_updates::setup_live_updates(widgets, Rc::clone(&state), panel_state.visible.clone());
    scanning::setup_scan_on_show(widgets, Rc::clone(&state), scan_requested);
    bluetooth::setup_bluetooth(widgets, Rc::clone(&state));
    bt_live_updates::setup_bt_live_updates(widgets, Rc::clone(&state), panel_state.visible.clone());
    setup_scan_button_dispatch(widgets, Rc::clone(&state));
    setup_wifi_tab_sync(widgets, Rc::clone(&state));
    vpn::setup_vpn(widgets, Rc::clone(&state), panel_state.clone());
    if widgets.wifi_tab.is_active() {
        scanning::start_wifi_auto_scan(
            Rc::clone(&state),
            widgets.wifi_tab.clone(),
            widgets.network_list_box.clone(),
            widgets.status_label.clone(),
        );
    }
    let reload_requested = panel_state.reload_requested.clone();
    shortcuts::setup_escape_key(widgets, panel_state.clone());
    shortcuts::setup_reload_on_request(widgets, Rc::clone(&state), reload_requested);
    scanning::setup_initial_state(widgets, Rc::clone(&state));
    setup_visibility_pause(widgets, Rc::clone(&state), panel_state);
}

/// Start controls whose services do not depend on NetworkManager.  This is
/// called while the window is being activated so Audio/Power navigation and
/// their honest unavailable states still work when Wi-Fi setup is unavailable.
pub(crate) fn setup_system_controls(widgets: &PanelWidgets) {
    controls::setup_controls(widgets);
    audio::setup(widgets);
    system_power::setup(widgets);
}

/// Clone the WifiManager out of the RefCell (avoids holding borrow across await).
fn get_wifi(state: &Rc<RefCell<AppState>>) -> WifiManager {
    state.borrow().wifi.clone()
}

/// Keep compatibility status writes isolated to the visible detail page.
///
/// Existing controllers all receive `status_label`; that handle is now a
/// hidden sink. Mirroring its changes into page-local labels preserves those
/// controller APIs without allowing a Wi-Fi refresh to overwrite Bluetooth's
/// rendered status after navigation.
fn setup_status_ownership(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let status = widgets.status_label.clone();
    status.set_visible(false);

    let wifi_status = widgets.wifi_status_label.clone();
    let bt_status = widgets.bluetooth_status_label.clone();
    let home = widgets.home.clone();
    let wifi_tab_for_status = widgets.wifi_tab.clone();
    let bt_tab_for_status = widgets.bt_tab.clone();
    let state_for_status = Rc::clone(&state);
    status.connect_notify_local(Some("label"), move |label, _| {
        let text = label.text();
        let active_tab = state_for_status.borrow().active_tab;
        if wifi_tab_for_status.is_active() && active_tab == ActiveTab::Wifi {
            wifi_status.set_text(text.as_str());
            home.set_wifi_status(Some(text.as_str()));
        } else if bt_tab_for_status.is_active() && active_tab == ActiveTab::Bluetooth {
            bt_status.set_text(text.as_str());
            home.set_bluetooth_status(Some(text.as_str()));
        }
    });

    {
        let state = Rc::clone(&state);
        let status = status.clone();
        widgets.wifi_tab.connect_toggled(move |tab| {
            if tab.is_active() {
                state.borrow_mut().active_tab = ActiveTab::Wifi;
                status.set_text("Checking Wi-Fi…");
            } else if state.borrow().active_tab == ActiveTab::Wifi {
                state.borrow_mut().active_tab = ActiveTab::None;
            }
        });
    }

    {
        let state = Rc::clone(&state);
        let status = status.clone();
        widgets.bt_tab.connect_toggled(move |tab| {
            if tab.is_active() {
                state.borrow_mut().active_tab = ActiveTab::Bluetooth;
                status.set_text("Checking Bluetooth…");
            } else if state.borrow().active_tab == ActiveTab::Bluetooth {
                state.borrow_mut().active_tab = ActiveTab::None;
            }
        });
    }

    // The panel can be shown and interacted with while the initial D-Bus
    // setup is still running. Reconcile the current selection in case a
    // feature toggle happened before these handlers were installed.
    if widgets.wifi_tab.is_active() {
        state.borrow_mut().active_tab = ActiveTab::Wifi;
        status.set_text("Checking Wi-Fi…");
    }
    if widgets.bt_tab.is_active() {
        state.borrow_mut().active_tab = ActiveTab::Bluetooth;
        status.set_text("Checking Bluetooth…");
    }

    // The switch is shared by the legacy Wi-Fi/Bluetooth controllers.  Its
    // value is supplied by their real snapshots/toggle results, so mirror it
    // into only the corresponding Home tile whenever one of those features
    // owns the switch.
    let home = widgets.home.clone();
    let wifi_tab_for_switch = widgets.wifi_tab.clone();
    let bt_tab_for_switch = widgets.bt_tab.clone();
    widgets.wifi_switch.connect_notify_local(Some("active"), move |switch, _| {
        if wifi_tab_for_switch.is_active() {
            home.set_wifi_enabled(switch.is_active());
        } else if bt_tab_for_switch.is_active() {
            home.set_bluetooth_enabled(switch.is_active());
        }
    });

    // Bluetooth hides its legacy selector when no adapter exists. Reflect
    // that concrete availability in the home tile without probing hardware
    // from the UI layer.
    let home = widgets.home.clone();
    widgets.bt_tab.connect_notify_local(Some("visible"), move |tab, _| {
        if !tab.is_visible() {
            home.bluetooth.set_state(crate::ui::home::TileState::Unavailable);
        }
    });
}

fn wifi_owns_status(state: &Rc<RefCell<AppState>>) -> bool {
    state.borrow().active_tab == ActiveTab::Wifi
}

pub(super) fn set_wifi_status(state: &Rc<RefCell<AppState>>, status: &gtk4::Label, text: &str) {
    if wifi_owns_status(state) {
        status.set_text(text);
    }
}

pub(super) fn bluetooth_owns_status(state: &Rc<RefCell<AppState>>) -> bool {
    state.borrow().active_tab == ActiveTab::Bluetooth
}

pub(super) fn set_bluetooth_status(
    state: &Rc<RefCell<AppState>>,
    status: &gtk4::Label,
    text: &str,
) {
    if bluetooth_owns_status(state) {
        status.set_text(text);
    }
}

/// Refresh the network list from D-Bus and update the UI.
async fn refresh_list(
    state: &Rc<RefCell<AppState>>,
    list_box: &gtk4::ListBox,
    status: &gtk4::Label,
) {
    if !wifi_owns_status(state) {
        return;
    }
    let wifi = get_wifi(state);
    let networks = wifi.get_networks().await;

    if !wifi_owns_status(state) {
        return;
    }

    match networks {
        Ok(nets) => {
            // Update status with connected network
            let connected = nets.iter().find(|n| n.is_connected);
            match connected {
                Some(n) => set_wifi_status(state, status, &format!("Connected to {}", n.ssid)),
                None => set_wifi_status(state, status, "Not connected"),
            }

            let config = crate::config::Config::load();
            let on_forget = {
                let state = Rc::clone(state);
                let list_box = list_box.clone();
                let status = status.clone();
                std::rc::Rc::new(move |ssid: String| {
                    let state = Rc::clone(&state);
                    let list_box = list_box.clone();
                    let status = status.clone();
                    glib::spawn_future_local(async move {
                        let wifi = get_wifi(&state);
                        set_wifi_status(&state, &status, &format!("Forgetting {}...", ssid));
                        match wifi.forget_network(&ssid).await {
                            Ok(_) => {
                                set_wifi_status(&state, &status, &format!("Forgot {}", ssid));
                                refresh_list(&state, &list_box, &status).await;
                            }
                            Err(e) => {
                                log::error!("Forget failed: {e}");
                                set_wifi_status(
                                    &state,
                                    &status,
                                    &format!("Failed to forget: {}", e),
                                );
                            }
                        }
                    });
                })
            };
            let row_ssids = network_list::populate_network_list(
                list_box,
                &nets,
                &config,
                &state.borrow().wifi_pending,
                on_forget,
            );
            log::info!("Network list refreshed: {} networks", nets.len());
            let mut st = state.borrow_mut();
            st.networks = nets;
            st.wifi_row_ssids = row_ssids;
        }
        Err(e) => {
            log::error!("Failed to get networks: {e}");
            set_wifi_status(state, status, "Failed to load networks");
        }
    }
}

/// Dispatch scan button clicks to Wi-Fi or Bluetooth based on active tab.
fn setup_scan_button_dispatch(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let scan_btn = widgets.scan_button.clone();
    let bt_tab = widgets.bt_tab.clone();
    let vpn_tab = widgets.wifi_vpn_tab.clone();
    let bt_list_box = widgets.bt_list_box.clone();
    let bt_spinner = widgets.bt_spinner.clone();
    let bt_scroll = widgets.bt_scroll.clone();
    let wifi_list_box = widgets.network_list_box.clone();
    let wifi_spinner = widgets.spinner.clone();
    let wifi_scroll = widgets.network_scroll.clone();
    let status = widgets.status_label.clone();

    let scan_btn_cb = scan_btn.clone();
    scan_btn.connect_clicked(move |_btn| {
        match ScanTarget::from_tabs(bt_tab.is_active(), vpn_tab.is_active()) {
            ScanTarget::Bluetooth => bluetooth::run_manual_scan(
                Rc::clone(&state),
                bt_tab.clone(),
                bt_list_box.clone(),
                status.clone(),
                scan_btn_cb.clone(),
                bt_spinner.clone(),
                bt_scroll.clone(),
            ),
            ScanTarget::Vpn => {
                set_wifi_status(&state, &status, "VPN view updates automatically");
            }
            ScanTarget::Wifi => scanning::run_manual_scan(
                Rc::clone(&state),
                wifi_list_box.clone(),
                status.clone(),
                scan_btn_cb.clone(),
                wifi_spinner.clone(),
                wifi_scroll.clone(),
            ),
        }
    });
}

/// Sync the toggle switch to WiFi power state when WiFi tab is activated.
fn setup_wifi_tab_sync(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let wifi_tab = widgets.wifi_tab.clone();
    let vpn_tab = widgets.wifi_vpn_tab.clone();
    let switch = widgets.wifi_switch.clone();
    let title = widgets.title_label.clone();
    let status = widgets.status_label.clone();
    let list_box = widgets.network_list_box.clone();
    let scan_btn = widgets.scan_button.clone();
    let vpn_list_box = widgets.vpn_list_box.clone();
    let vpn_spinner = widgets.vpn_spinner.clone();
    let vpn_scroll = widgets.vpn_scroll.clone();
    let vpn_import_btn = widgets.vpn_import_button.clone();
    let vpn_open_btn = widgets.vpn_open_button.clone();
    let window = widgets.window.clone();

    wifi_tab.connect_toggled(move |btn| {
        if !btn.is_active() {
            scanning::stop_wifi_auto_scan(&state);
            vpn::deactivate_vpn_view(&state);
            vpn::stop_vpn_refresh(&state);
            return;
        }

        title.set_text("Wi-Fi");
        switch.set_tooltip_text(Some("Enable/Disable Wi-Fi"));
        if vpn_tab.is_active() {
            scan_btn.set_sensitive(false);
            scan_btn.set_tooltip_text(Some("Scan is disabled in VPN view"));
        } else {
            scan_btn.set_sensitive(true);
            scan_btn.set_tooltip_text(Some("Scan for networks"));
        }

        let state_for_refresh = Rc::clone(&state);
        let switch = switch.clone();
        let status = status.clone();
        let list_box = list_box.clone();
        let status_for_refresh = status.clone();
        let list_box_for_refresh = list_box.clone();
        let wifi_tab_for_refresh = btn.clone();

        gtk4::glib::spawn_future_local(async move {
            let wifi = get_wifi(&state_for_refresh);

            // Sync switch to actual WiFi power state
            match wifi.is_wifi_enabled().await {
                Ok(enabled) if wifi_tab_for_refresh.is_active() => switch.set_active(enabled),
                Err(e) => log::error!("Failed to get WiFi state on tab switch: {e}"),
                _ => {}
            }

            // Refresh network list
            if wifi_tab_for_refresh.is_active() {
                refresh_list(&state_for_refresh, &list_box_for_refresh, &status_for_refresh).await;
            }
        });

        if vpn_tab.is_active() {
            vpn::start_vpn_refresh(
                Rc::clone(&state),
                btn.clone(),
                vpn_tab.clone(),
                vpn::VpnView {
                    window: window.clone(),
                    list_box: vpn_list_box.clone(),
                    status: status.clone(),
                    spinner: vpn_spinner.clone(),
                    scrolled: vpn_scroll.clone(),
                    import_btn: vpn_import_btn.clone(),
                    open_btn: vpn_open_btn.clone(),
                },
            );
        } else {
            scanning::start_wifi_auto_scan(
                Rc::clone(&state),
                btn.clone(),
                list_box.clone(),
                status.clone(),
            );
        }
    });
}

/// Stop background scans when the panel is hidden; resume when shown.
fn setup_visibility_pause(
    widgets: &PanelWidgets,
    state: Rc<RefCell<AppState>>,
    panel_state: crate::daemon::PanelState,
) {
    use std::sync::atomic::Ordering;

    let last_visible = Rc::new(RefCell::new(panel_state.visible.load(Ordering::Relaxed)));
    let wifi_tab = widgets.wifi_tab.clone();
    let bt_tab = widgets.bt_tab.clone();
    let wifi_list_box = widgets.network_list_box.clone();
    let bt_list_box = widgets.bt_list_box.clone();
    let status = widgets.status_label.clone();

    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        let visible = panel_state.visible.load(Ordering::Relaxed);
        let mut last = last_visible.borrow_mut();
        if *last != visible {
            *last = visible;
            if !visible {
                scanning::stop_wifi_auto_scan(&state);
                // Stop bg reconnect too — panel is opening so fast loop takes over.
                scanning::stop_wifi_bg_reconnect(&state);
                vpn::stop_vpn_refresh(&state);
                bluetooth::stop_bt_background_tasks(&state);
                let state_bt = Rc::clone(&state);
                glib::spawn_future_local(async move {
                    bluetooth::stop_bt_discovery(state_bt).await;
                });

                // If Wi-Fi is currently disconnected, start the slow background
                // reconnect loop so NM can find and join a saved network.
                let state_bg = Rc::clone(&state);
                glib::spawn_future_local(async move {
                    let wifi = get_wifi(&state_bg);
                    // NM device state 100 = Activated (connected).
                    // We check by asking for the active connection path;
                    // a path of "/" means no active connection.
                    let is_connected = match wifi
                        .connection()
                        .call_method(
                            Some("org.freedesktop.NetworkManager"),
                            wifi.wifi_device_path(),
                            Some("org.freedesktop.DBus.Properties"),
                            "Get",
                            &("org.freedesktop.NetworkManager.Device", "ActiveConnection"),
                        )
                        .await
                    {
                        Ok(msg) => {
                            match msg.body().deserialize::<zbus::zvariant::OwnedObjectPath>() {
                                Ok(path) => path.as_str() != "/",
                                Err(error) => {
                                    log::warn!(
                                        "Could not decode NetworkManager ActiveConnection: {error}"
                                    );
                                    false
                                }
                            }
                        }
                        Err(_) => false,
                    };

                    if !is_connected {
                        log::info!("Panel hidden while disconnected — starting bg reconnect loop");
                        scanning::start_wifi_bg_reconnect(state_bg);
                    } else {
                        log::info!("Panel hidden while connected — no bg reconnect needed");
                    }
                });
            } else {
                if wifi_tab.is_active() {
                    scanning::start_wifi_auto_scan(
                        Rc::clone(&state),
                        wifi_tab.clone(),
                        wifi_list_box.clone(),
                        status.clone(),
                    );
                }
                if bt_tab.is_active() {
                    let state_bt = Rc::clone(&state);
                    let bt_tab = bt_tab.clone();
                    let bt_list_box = bt_list_box.clone();
                    let status = status.clone();
                    glib::spawn_future_local(async move {
                        bluetooth::resume_bt_background_tasks(
                            state_bt,
                            bt_tab,
                            bt_list_box,
                            status,
                        )
                        .await;
                    });
                }
            }
        }
        glib::ControlFlow::Continue
    });
}
