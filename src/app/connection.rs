//! Connection — WiFi toggle, network click, and password dialog handlers.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::access_point::SecurityType;
use crate::ui::network_list;
use crate::ui::window::PanelWidgets;

use super::{AppState, get_wifi, refresh_list};

/// Wire the WiFi toggle switch (only when WiFi tab is active).
pub(super) fn setup_wifi_toggle(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();
    let wifi_tab = widgets.wifi_tab.clone();

    widgets
        .wifi_switch
        .connect_state_set(move |_switch, enabled| {
            // Only handle WiFi toggle when WiFi tab is active
            if !wifi_tab.is_active() {
                return glib::Propagation::Proceed;
            }

            let state = Rc::clone(&state);
            let list_box = list_box.clone();
            let status = status.clone();

            glib::spawn_future_local(async move {
                let wifi = get_wifi(&state);
                let result = wifi.set_wifi_enabled(enabled).await;

                match result {
                    Ok(_) => {
                        if enabled {
                            status.set_text("WiFi enabled");
                            glib::timeout_future(std::time::Duration::from_millis(2000)).await;
                            let _ = wifi.request_scan().await;
                            glib::timeout_future(std::time::Duration::from_millis(1500)).await;
                            refresh_list(&state, &list_box, &status).await;
                        } else {
                            status.set_text("WiFi disabled");
                            let config = crate::config::Config::load();
                            let wifi = get_wifi(&state);
                            let empty_pending = std::collections::HashMap::new();
                            let on_forget = std::rc::Rc::new(|_ssid: String| {});
                            let row_ssids = network_list::populate_network_list(
                                &list_box,
                                &[],
                                &config,
                                &wifi,
                                &status,
                                &empty_pending,
                                on_forget,
                            );
                            state.borrow_mut().wifi_row_ssids = row_ssids;
                        }
                    }
                    Err(e) => {
                        log::error!("WiFi toggle failed: {e}");
                        status.set_text("Toggle failed");
                    }
                }
            });

            glib::Propagation::Proceed
        });
}

/// Wire network row clicks to connect or show password dialog.
pub(super) fn setup_network_click(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let revealer = widgets.password_revealer.clone();
    let entry = widgets.password_entry.clone();
    let error_label = widgets.error_label.clone();
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();

    widgets
        .network_list_box
        .connect_row_activated(move |_list, row| {
            let index = row.index() as usize;
            let state = Rc::clone(&state);
            let revealer = revealer.clone();
            let entry = entry.clone();
            let error_label = error_label.clone();
            let list_box = list_box.clone();
            let status = status.clone();

            glib::spawn_future_local(async move {
                let network = {
                    let st = state.borrow();
                    let ssid = st
                        .wifi_row_ssids
                        .get(index)
                        .and_then(|v| v.clone());
                    ssid.and_then(|ssid| {
                        st.networks.iter().find(|n| n.ssid == ssid).cloned()
                    })
                };

                let Some(network) = network else {
                    return;
                };

                let wifi = get_wifi(&state);

                let set_pending = |state: &Rc<RefCell<AppState>>,
                                   status: &gtk4::Label,
                                   list_box: &gtk4::ListBox,
                                   ssid: &str,
                                   pending_label: &str,
                                   status_prefix: &str| {
                    {
                        let mut st = state.borrow_mut();
                        st.wifi_pending
                            .insert(ssid.to_string(), pending_label.to_string());
                    }
                    status.set_text(&format!("{} {}...", status_prefix, ssid));
                    glib::spawn_future_local({
                        let state = Rc::clone(state);
                        let list_box = list_box.clone();
                        let status = status.clone();
                        async move {
                            refresh_list(&state, &list_box, &status).await;
                        }
                    });
                };

                let clear_pending = |state: &Rc<RefCell<AppState>>,
                                     list_box: &gtk4::ListBox,
                                     status: &gtk4::Label,
                                     ssid: &str| {
                    let mut st = state.borrow_mut();
                    st.wifi_pending.remove(ssid);
                    glib::spawn_future_local({
                        let state = Rc::clone(state);
                        let list_box = list_box.clone();
                        let status = status.clone();
                        async move {
                            refresh_list(&state, &list_box, &status).await;
                        }
                    });
                };

                if network.is_connected {
                    // Disconnect
                    set_pending(
                        &state,
                        &status,
                        &list_box,
                        &network.ssid,
                        "Disconnecting",
                        "Disconnecting from",
                    );
                    match wifi.disconnect().await {
                        Ok(_) => {
                            glib::timeout_future(std::time::Duration::from_millis(500)).await;
                            clear_pending(&state, &list_box, &status, &network.ssid);
                            refresh_list(&state, &list_box, &status).await;
                        }
                        Err(e) => {
                            log::error!("Disconnect failed: {e}");
                            status.set_text("Disconnect failed");
                            clear_pending(&state, &list_box, &status, &network.ssid);
                        }
                    }
                } else if network.is_saved || network.security == SecurityType::Open {
                    // Connect directly (no password needed)
                    set_pending(
                        &state,
                        &status,
                        &list_box,
                        &network.ssid,
                        "Connecting",
                        "Connecting to",
                    );
                    match wifi.connect_to_network(&network, None).await {
                        Ok(_) => {
                            glib::timeout_future(std::time::Duration::from_millis(2000)).await;
                            clear_pending(&state, &list_box, &status, &network.ssid);
                            refresh_list(&state, &list_box, &status).await;
                        }
                        Err(e) => {
                            log::error!("Connect failed: {e}");
                            status.set_text(&format!("Failed: {}", e));
                            clear_pending(&state, &list_box, &status, &network.ssid);
                        }
                    }
                } else {
                    // Show password dialog
                    state.borrow_mut().selected_ssid = Some(network.ssid.clone());
                    error_label.set_visible(false);
                    entry.set_text("");
                    revealer.set_reveal_child(true);
                    entry.grab_focus();
                }
            });
        });
}

/// Wire password dialog connect/cancel buttons and Enter key.
pub(super) fn setup_password_actions(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let revealer = widgets.password_revealer.clone();
    let entry = widgets.password_entry.clone();
    let error_label = widgets.error_label.clone();
    let list_box = widgets.network_list_box.clone();
    let status_label = widgets.status_label.clone();

    // Cancel button — hide the password section
    {
        let revealer = revealer.clone();
        widgets.cancel_button.connect_clicked(move |_| {
            revealer.set_reveal_child(false);
        });
    }

    // Connect button
    {
        let state = Rc::clone(&state);
        let revealer = revealer.clone();
        let entry = entry.clone();
        let error_label = error_label.clone();
        let list_box = list_box.clone();
        let status = status_label.clone();

        widgets.connect_button.connect_clicked(move |btn| {
            let password = entry.text().to_string();
            if password.is_empty() {
                error_label.set_text("Password cannot be empty");
                error_label.set_visible(true);
                return;
            }

            btn.set_sensitive(false);
            let state = Rc::clone(&state);
            let revealer = revealer.clone();
            let error_label = error_label.clone();
            let list_box = list_box.clone();
            let status = status.clone();
            let btn = btn.clone();

            glib::spawn_future_local(async move {
                let (network, wifi) = {
                    let st = state.borrow();
                    let net = st.selected_ssid.as_ref().and_then(|ssid| {
                        st.networks.iter().find(|n| n.ssid == *ssid).cloned()
                    });
                    (net, st.wifi.clone())
                };

                let Some(network) = network else {
                    btn.set_sensitive(true);
                    return;
                };

                {
                    let mut st = state.borrow_mut();
                    st.wifi_pending
                        .insert(network.ssid.clone(), "Connecting".to_string());
                }
                status.set_text(&format!("Connecting to {}...", network.ssid));
                refresh_list(&state, &list_box, &status).await;

                match wifi.connect_to_network(&network, Some(&password)).await {
                    Ok(_) => {
                        revealer.set_reveal_child(false);
                        glib::timeout_future(std::time::Duration::from_millis(2000)).await;
                        state.borrow_mut().wifi_pending.remove(&network.ssid);
                        refresh_list(&state, &list_box, &status).await;
                    }
                    Err(e) => {
                        log::error!("Connect with password failed: {e}");
                        error_label.set_text("Connection failed — check password");
                        error_label.set_visible(true);
                        state.borrow_mut().wifi_pending.remove(&network.ssid);
                    }
                }
                btn.set_sensitive(true);
            });
        });
    }

    // Enter key in password entry triggers connect
    {
        let connect_btn = widgets.connect_button.clone();
        widgets.password_entry.connect_activate(move |_| {
            connect_btn.emit_clicked();
        });
    }
}
