//! Hotspot controller — handles toggle events and state synchronization.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::glib;

use crate::ui::window::PanelWidgets;
use super::AppState;

/// Set up hotspot UI event handlers.
pub(super) fn setup_hotspot(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let toggle = widgets.hotspot_toggle.clone();
    let status = widgets.hotspot_status.clone();
    let revealer = widgets.hotspot_revealer.clone();
    let ssid_label = widgets.hotspot_ssid.clone();
    let wifi_list = widgets.network_list_box.clone();
    let wifi_status = widgets.status_label.clone();
    let is_updating = Rc::new(RefCell::new(false));

    // ── Initial state ──
    {
        let state = Rc::clone(&state);
        let toggle = toggle.clone();
        let status = status.clone();
        let revealer = revealer.clone();
        let ssid_label = ssid_label.clone();
        let is_updating = is_updating.clone();

        glib::spawn_future_local(async move {
            let (hotspot, config) = {
                let st = state.borrow();
                (st.hotspot.clone(), crate::config::Config::load())
            };

            if hotspot.is_hotspot_active().await {
                *is_updating.borrow_mut() = true;
                toggle.set_active(true);
                *is_updating.borrow_mut() = false;
                status.set_text("Active");
                revealer.set_reveal_child(true);
                ssid_label.set_text(&config.hotspot_ssid);
            }
        });
    }

    // ── Toggle action ──
    let state_c = Rc::clone(&state);
    let toggle_c = toggle.clone();
    let status_c = status.clone();
    let revealer_c = revealer.clone();
    let ssid_label_c = ssid_label.clone();
    let wifi_list_c = wifi_list.clone();
    let wifi_status_c = wifi_status.clone();
    let is_updating_c = is_updating.clone();

    toggle.connect_state_set(move |_switch, enabled| {
        // Guard against recursive signal triggers from set_active()
        if *is_updating_c.borrow() {
            return glib::Propagation::Proceed;
        }

        let state = Rc::clone(&state_c);
        let toggle = toggle_c.clone();
        let status = status_c.clone();
        let revealer = revealer_c.clone();
        let ssid_label = ssid_label_c.clone();
        let wifi_list = wifi_list_c.clone();
        let wifi_status = wifi_status_c.clone();
        let is_updating = is_updating_c.clone();

        glib::spawn_future_local(async move {
            let (hotspot, config) = {
                let st = state.borrow();
                (st.hotspot.clone(), crate::config::Config::load())
            };

            if enabled {
                status.set_text("Starting...");

                match hotspot.start_hotspot(
                    &config.hotspot_ssid,
                    if config.hotspot_password.is_empty() { None } else { Some(&config.hotspot_password) },
                ).await {
                    Ok(_) => {
                        status.set_text("Active");
                        revealer.set_reveal_child(true);
                        ssid_label.set_text(&config.hotspot_ssid);
                        wifi_status.set_text("Hotspot active — WiFi paused");
                    }
                    Err(e) => {
                        log::error!("Failed to start hotspot: {e}");
                        status.set_text("Error");
                        revealer.set_reveal_child(false);
                        *is_updating.borrow_mut() = true;
                        toggle.set_active(false);
                        *is_updating.borrow_mut() = false;
                    }
                }
            } else {
                status.set_text("Stopping...");
                if hotspot.is_hotspot_active().await {
                    match hotspot.stop_hotspot().await {
                        Ok(_) => {
                            status.set_text("Off");
                            revealer.set_reveal_child(false);
                            wifi_status.set_text("Wi-Fi");
                            // Trigger scan to restore network list
                            glib::timeout_future(std::time::Duration::from_millis(500)).await;
                            let wifi = state.borrow().wifi.clone();
                            let _ = wifi.request_scan().await;
                            super::refresh_list(&state, &wifi_list, &wifi_status).await;
                        }
                        Err(e) => {
                            log::error!("Failed to stop hotspot: {e}");
                            status.set_text("Error");
                            *is_updating.borrow_mut() = true;
                            toggle.set_active(true);
                            *is_updating.borrow_mut() = false;
                        }
                    }
                } else {
                    status.set_text("Off");
                    revealer.set_reveal_child(false);
                    wifi_status.set_text("Wi-Fi");
                }
            }
        });

        glib::Propagation::Proceed
    });

    // ── Edit Actions ──
    {
        use gtk4::gio;
        use super::HotspotConfigMode;

        let action_group = gio::SimpleActionGroup::new();

        // 1. Change SSID
        let action_ssid = gio::SimpleAction::new("change_ssid", None);
        {
            let widgets_revealer = widgets.password_revealer.clone();
            let widgets_entry = widgets.password_entry.clone();
            let widgets_error = widgets.error_label.clone();
            let widgets_connect = widgets.connect_button.clone();
            let widgets_title = widgets.password_title.clone();
            let state = Rc::clone(&state);

            action_ssid.connect_activate(move |_, _| {
                log::info!("Change Hotspot SSID requested");
                let mut st = state.borrow_mut();
                st.is_configuring_hotspot = true;
                st.hotspot_config_mode = Some(HotspotConfigMode::Name);
                
                widgets_error.set_visible(false);
                widgets_entry.set_text("");
                widgets_entry.set_placeholder_text(Some("New Hotspot Name"));
                widgets_entry.set_visibility(true); // show text for SSID
                widgets_entry.set_input_purpose(gtk4::InputPurpose::FreeForm);
                widgets_entry.set_secondary_icon_name(None); // Hide "eye" icon for SSID
                
                widgets_title.set_text("Change Hotspot Name");
                widgets_connect.set_label("Save");
                widgets_revealer.set_reveal_child(true);
                widgets_entry.grab_focus();
            });
        }
        action_group.add_action(&action_ssid);

        // 2. Change Password
        let action_pass = gio::SimpleAction::new("change_password", None);
        {
            let widgets_revealer = widgets.password_revealer.clone();
            let widgets_entry = widgets.password_entry.clone();
            let widgets_error = widgets.error_label.clone();
            let widgets_connect = widgets.connect_button.clone();
            let widgets_title = widgets.password_title.clone();
            let state = Rc::clone(&state);

            action_pass.connect_activate(move |_, _| {
                log::info!("Change Hotspot Password requested");
                let mut st = state.borrow_mut();
                st.is_configuring_hotspot = true;
                st.hotspot_config_mode = Some(HotspotConfigMode::Password);
                
                widgets_error.set_visible(false);
                widgets_entry.set_text("");
                widgets_entry.set_placeholder_text(Some("New Hotspot Password"));
                widgets_entry.set_visibility(false); // mask characters for password
                widgets_entry.set_input_purpose(gtk4::InputPurpose::Password);
                widgets_entry.set_secondary_icon_name(Some("view-reveal-symbolic")); // Show "eye" icon
                
                widgets_title.set_text("Change Hotspot Password");
                widgets_connect.set_label("Save");
                widgets_revealer.set_reveal_child(true);
                widgets_entry.grab_focus();
            });
        }
        action_group.add_action(&action_pass);

        // Attach action group to the button directly to ensure popover finds it
        widgets.hotspot_menu_btn.insert_action_group("hotspot", Some(&action_group));
    }
}
