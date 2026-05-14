//! VPN UI — lists NetworkManager VPN/WireGuard profiles and allows toggle connect/disconnect.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::glib;
use gtk4::prelude::*;

use crate::ui::vpn_list;
use crate::ui::window::PanelWidgets;

use super::{AppState, PendingVpnAction};

const VPN_REFRESH_INTERVAL_MS: u64 = 1500;
const VPN_PENDING_TIMEOUT_MS: u64 = 20_000;

pub(super) fn setup_vpn(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let wifi_tab = widgets.wifi_tab.clone();
    let networks_tab = widgets.wifi_networks_tab.clone();
    let vpn_tab = widgets.wifi_vpn_tab.clone();
    let scan_btn = widgets.scan_button.clone();
    let status = widgets.status_label.clone();
    let wifi_list_box = widgets.network_list_box.clone();

    let vpn_list_box = widgets.vpn_list_box.clone();
    let vpn_spinner = widgets.vpn_spinner.clone();
    let vpn_scroll = widgets.vpn_scroll.clone();

    // When VPN sub-tab becomes active: disable scan and start VPN refresh.
    vpn_tab.connect_toggled({
        let state = Rc::clone(&state);
        let wifi_tab = wifi_tab.clone();
        let vpn_tab = vpn_tab.clone();
        let scan_btn = scan_btn.clone();
        let status = status.clone();
        let vpn_list_box = vpn_list_box.clone();
        let vpn_spinner = vpn_spinner.clone();
        let vpn_scroll = vpn_scroll.clone();
        move |btn| {
            if !btn.is_active() {
                return;
            }

            // Only do work if Wi-Fi top tab is active.
            if !wifi_tab.is_active() {
                return;
            }

            scan_btn.set_sensitive(false);
            scan_btn.set_tooltip_text(Some("Scan is disabled in VPN view"));

            // Stop Wi-Fi auto scan while user is in VPN view.
            super::scanning::stop_wifi_auto_scan(&state);

            start_vpn_refresh(
                Rc::clone(&state),
                wifi_tab.clone(),
                vpn_tab.clone(),
                vpn_list_box.clone(),
                status.clone(),
                vpn_spinner.clone(),
                vpn_scroll.clone(),
            );
        }
    });

    // When Networks sub-tab becomes active: stop VPN refresh and restore scan.
    networks_tab.connect_toggled({
        let state = Rc::clone(&state);
        let wifi_tab = wifi_tab.clone();
        let wifi_list_box = wifi_list_box.clone();
        let scan_btn = scan_btn.clone();
        let status = status.clone();
        move |btn| {
            if !btn.is_active() {
                return;
            }

            stop_vpn_refresh(&state);

            scan_btn.set_sensitive(true);
            scan_btn.set_tooltip_text(Some("Scan for networks"));

            if wifi_tab.is_active() {
                super::scanning::start_wifi_auto_scan(
                    Rc::clone(&state),
                    wifi_tab.clone(),
                    wifi_list_box.clone(),
                    status.clone(),
                );
            }
        }
    });
}

pub(super) fn start_vpn_refresh(
    state: Rc<RefCell<AppState>>,
    wifi_tab: gtk4::ToggleButton,
    vpn_tab: gtk4::ToggleButton,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
    spinner: gtk4::Spinner,
    scrolled: gtk4::ScrolledWindow,
) {
    if state.borrow().vpn_refresh_source.is_some() {
        return;
    }

    // Refresh immediately.
    glib::spawn_future_local({
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let spinner = spinner.clone();
        let scrolled = scrolled.clone();
        async move {
            refresh_vpn_list(state, list_box, status, spinner, scrolled).await;
        }
    });

    let id = glib::timeout_add_local(
        std::time::Duration::from_millis(VPN_REFRESH_INTERVAL_MS),
        {
            let state = Rc::clone(&state);
            move || {
                if !wifi_tab.is_active() || !vpn_tab.is_active() {
                    state.borrow_mut().vpn_refresh_source = None;
                    return glib::ControlFlow::Break;
                }

                glib::spawn_future_local({
                    let state = Rc::clone(&state);
                    let list_box = list_box.clone();
                    let status = status.clone();
                    let spinner = spinner.clone();
                    let scrolled = scrolled.clone();
                    async move {
                        refresh_vpn_list(state, list_box, status, spinner, scrolled).await;
                    }
                });
                glib::ControlFlow::Continue
            }
        },
    );

    state.borrow_mut().vpn_refresh_source = Some(id);
    log::info!("VPN refresh loop started (interval: {} ms)", VPN_REFRESH_INTERVAL_MS);
}

pub(super) fn stop_vpn_refresh(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    if let Some(id) = st.vpn_refresh_source.take() {
        id.remove();
        log::info!("VPN refresh loop stopped");
    }
}

async fn refresh_vpn_list(
    state: Rc<RefCell<AppState>>,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
    spinner: gtk4::Spinner,
    scrolled: gtk4::ScrolledWindow,
) {
    let vpn = state.borrow().vpn.clone();

    let profiles = match vpn.list_profiles().await {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to list VPN profiles: {e}");
            status.set_text("Failed to load VPN profiles");
            spinner.set_spinning(false);
            spinner.set_visible(false);
            scrolled.set_visible(true);
            return;
        }
    };

    let active_by_conn = match vpn.active_by_connection_path().await {
        Ok(m) => m,
        Err(e) => {
            log::warn!("Failed to get active VPN state: {e}");
            std::collections::HashMap::new()
        }
    };

    {
        let mut st = state.borrow_mut();
        st.vpn_active_by_conn = active_by_conn.clone();

        // Clear pending labels once the active state stabilizes and
        // force-unlock rows if NM keeps them in pending too long.
        let now = Instant::now();
        let timeout = Duration::from_millis(VPN_PENDING_TIMEOUT_MS);
        st.vpn_pending.retain(|conn_path, pending| {
            if now.duration_since(pending.started_at) > timeout {
                return false;
            }
            if let Some(active) = active_by_conn.get(conn_path) {
                active.state == 1 || active.state == 3
            } else {
                false
            }
        });
    }

    // Keep header status in sync with current VPN state instead of sticking
    // to the last manual action text.
    update_vpn_header_status(&status, &profiles, &active_by_conn);

    let on_toggle: Rc<dyn Fn(String, bool)> = {
        let state = Rc::clone(&state);
        let status = status.clone();
        Rc::new(move |conn_path: String, enabled: bool| {
            let state = Rc::clone(&state);
            let status = status.clone();
            glib::spawn_future_local(async move {
                let vpn = state.borrow().vpn.clone();

                if enabled {
                    let blocking_active_path = {
                        let st = state.borrow();
                        find_blocking_active_path_for_connect(&st, &conn_path)
                    };

                    {
                        let mut st = state.borrow_mut();
                        st.vpn_pending.insert(
                            conn_path.clone(),
                            PendingVpnAction {
                                label: "Connecting".to_string(),
                                started_at: Instant::now(),
                            },
                        );
                    }
                    status.set_text("Switching VPN...");

                    if let Some(active_path) = blocking_active_path {
                        if let Err(e) = vpn.disconnect(&active_path).await {
                            log::error!("VPN switch disconnect failed: {e}");
                            status.set_text(&format!("VPN switch failed: {e}"));
                            state.borrow_mut().vpn_pending.remove(&conn_path);
                            return;
                        }
                    }

                    match vpn.connect(&conn_path).await {
                        Ok(_) => {}
                        Err(e) => {
                            log::error!("VPN connect failed: {e}");
                            status.set_text(&format!("VPN connect failed: {e}"));
                            state.borrow_mut().vpn_pending.remove(&conn_path);
                        }
                    }
                } else {
                    let active_path = {
                        let st = state.borrow();
                        st.vpn_active_by_conn
                            .get(&conn_path)
                            .map(|a| a.active_path.clone())
                    };

                    let Some(active_path) = active_path else {
                        return;
                    };

                    {
                        let mut st = state.borrow_mut();
                        st.vpn_pending.insert(
                            conn_path.clone(),
                            PendingVpnAction {
                                label: "Disconnecting".to_string(),
                                started_at: Instant::now(),
                            },
                        );
                    }
                    match vpn.disconnect(&active_path).await {
                        Ok(_) => {}
                        Err(e) => {
                            log::error!("VPN disconnect failed: {e}");
                            status.set_text(&format!("VPN disconnect failed: {e}"));
                            state.borrow_mut().vpn_pending.remove(&conn_path);
                        }
                    }
                }
            });
        })
    };

    let pending = {
        state
            .borrow()
            .vpn_pending
            .iter()
            .map(|(k, v)| (k.clone(), v.label.clone()))
            .collect::<std::collections::HashMap<String, String>>()
    };
    let _row_paths =
        vpn_list::populate_vpn_list(&list_box, &profiles, &active_by_conn, &pending, on_toggle);

    spinner.set_spinning(false);
    spinner.set_visible(false);
    scrolled.set_visible(true);
}

fn find_blocking_active_path_for_connect(
    st: &AppState,
    target_conn_path: &str,
) -> Option<String> {
    for net in st.vpn_active_by_conn.values() {
        if net.connection_path == target_conn_path {
            continue;
        }
        if net.state == 1 || net.state == 2 {
            return Some(net.active_path.clone());
        }
    }
    None
}

fn update_vpn_header_status(
    status: &gtk4::Label,
    profiles: &[crate::dbus::vpn_manager::VpnProfile],
    active_by_conn: &std::collections::HashMap<String, crate::dbus::vpn_manager::VpnActive>,
) {
    let mut connected_name: Option<&str> = None;
    let mut connecting_name: Option<&str> = None;
    let mut disconnecting_name: Option<&str> = None;

    for profile in profiles {
        if let Some(active) = active_by_conn.get(&profile.connection_path) {
            match active.state {
                2 => connected_name = Some(&profile.name),
                1 => connecting_name = Some(&profile.name),
                3 => disconnecting_name = Some(&profile.name),
                _ => {}
            }
        }
    }

    if let Some(name) = connected_name {
        status.set_text(&format!("VPN connected: {name}"));
    } else if let Some(name) = connecting_name {
        status.set_text(&format!("VPN connecting: {name}"));
    } else if let Some(name) = disconnecting_name {
        status.set_text(&format!("VPN disconnecting: {name}"));
    } else {
        status.set_text("VPN disconnected");
    }
}
