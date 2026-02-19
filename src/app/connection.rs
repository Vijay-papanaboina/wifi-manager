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
                            network_list::populate_network_list(&list_box, &[], &config, &wifi, &status);
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
                    st.networks.get(index).cloned()
                };

                let Some(network) = network else {
                    return;
                };

                let wifi = get_wifi(&state);

                if network.is_connected {
                    // Disconnect
                    status.set_text(&format!("Disconnecting from {}...", network.ssid));
                    match wifi.disconnect().await {
                        Ok(_) => {
                            glib::timeout_future(std::time::Duration::from_millis(500)).await;
                            refresh_list(&state, &list_box, &status).await;
                        }
                        Err(e) => {
                            log::error!("Disconnect failed: {e}");
                            status.set_text("Disconnect failed");
                        }
                    }
                } else if network.is_saved || network.security == SecurityType::Open {
                    // Connect directly (no password needed)
                    status.set_text(&format!("Connecting to {}...", network.ssid));
                    match wifi.connect_to_network(&network, None).await {
                        Ok(_) => {
                            glib::timeout_future(std::time::Duration::from_millis(2000)).await;
                            refresh_list(&state, &list_box, &status).await;
                        }
                        Err(e) => {
                            log::error!("Connect failed: {e}");
                            status.set_text(&format!("Failed: {}", e));
                        }
                    }
                } else {
                    // Show password dialog
                    state.borrow_mut().selected_index = Some(index);
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
        let state = Rc::clone(&state);
        let revealer = revealer.clone();
        widgets.cancel_button.connect_clicked(move |_| {
            state.borrow_mut().is_configuring_hotspot = false;
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
            let state_c = Rc::clone(&state);
            let mode = state_c.borrow().hotspot_config_mode;

            // ── Validation ──
            match mode {
                Some(crate::app::HotspotConfigMode::Name) => {
                    if password.is_empty() {
                        error_label.set_text("Hotspot name cannot be empty");
                        error_label.set_visible(true);
                        return;
                    }
                }
                Some(crate::app::HotspotConfigMode::Password) => {
                    // WPA-PSK requires 8-63 characters, or empty for OPEN
                    if !password.is_empty() && password.len() < 8 {
                        error_label.set_text("Password must be at least 8 characters");
                        error_label.set_visible(true);
                        return;
                    }
                }
                None => {}
            }

            btn.set_sensitive(false);
            let state = Rc::clone(&state);
            let revealer = revealer.clone();
            let entry = entry.clone();
            let error_label = error_label.clone();
            let list_box = list_box.clone();
            let status = status.clone();
            let btn = btn.clone();

            glib::spawn_future_local(async move {
                let is_config = state.borrow().is_configuring_hotspot;

                if is_config {
                    // ── Handle Hotspot Settings Save ──
                    let mut config = crate::config::Config::load();
                    let mode = state.borrow().hotspot_config_mode;

                    match mode {
                        Some(crate::app::HotspotConfigMode::Name) => {
                            config.hotspot_ssid = password.clone();
                        }
                        Some(crate::app::HotspotConfigMode::Password) => {
                            config.hotspot_password = password.clone();
                        }
                        None => {
                            log::warn!("Hotspot config saved with no mode set");
                        }
                    }

                    if let Err(e) = config.save() {
                        log::error!("Failed to save config: {e}");
                        error_label.set_text("Failed to save settings");
                        error_label.set_visible(true);
                        btn.set_sensitive(true);
                        return;
                    }

                    status.set_text("Hotspot settings saved");
                    
                    // ── Sync UI labels immediately ──
                    let ssid_label = state.borrow().hotspot_ssid_label.clone();
                    if let Some(l) = ssid_label {
                        l.set_text(&config.hotspot_ssid);
                    }

                    let hotspot = state.borrow().hotspot.clone();
                    if hotspot.is_hotspot_active().await {
                        status.set_text("Restarting hotspot...");
                        let ssid = config.hotspot_ssid.clone();
                        let pass = if config.hotspot_password.is_empty() { None } else { Some(config.hotspot_password.as_str()) };
                        
                        // We use a small delay or just wait for stop to finish
                        let _ = hotspot.stop_hotspot().await;
                        glib::timeout_future(std::time::Duration::from_millis(500)).await;
                        let _ = hotspot.start_hotspot(&ssid, pass).await;
                    }

                    {
                        let mut st = state.borrow_mut();
                        st.is_configuring_hotspot = false;
                        st.hotspot_config_mode = None;
                    }
                    revealer.set_reveal_child(false);
                    
                    // Reset dialog UI for next time
                    btn.set_label("Connect");
                    entry.set_placeholder_text(Some("Enter password"));
                } else {
                    // ── Handle WiFi Connection ──
                    if password.is_empty() {
                        error_label.set_text("Password cannot be empty");
                        error_label.set_visible(true);
                        btn.set_sensitive(true);
                        return;
                    }
                    let (network, wifi) = {
                        let st = state.borrow();
                        let net = st.selected_index.and_then(|i| st.networks.get(i).cloned());
                        (net, st.wifi.clone())
                    };

                    let Some(network) = network else {
                        btn.set_sensitive(true);
                        return;
                    };

                    status.set_text(&format!("Connecting to {}...", network.ssid));

                    match wifi.connect_to_network(&network, Some(&password)).await {
                        Ok(_) => {
                            revealer.set_reveal_child(false);
                            glib::timeout_future(std::time::Duration::from_millis(2000)).await;
                            refresh_list(&state, &list_box, &status).await;
                        }
                        Err(e) => {
                            log::error!("Connect with password failed: {e}");
                            error_label.set_text("Connection failed — check password");
                            error_label.set_visible(true);
                        }
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
