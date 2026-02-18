//! Hotspot controller — handles toggle events and state synchronization.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::ui::window::PanelWidgets;
use super::AppState;

/// Set up hotspot UI event handlers.
pub(super) fn setup_hotspot(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let toggle = widgets.hotspot_toggle.clone();
    let status = widgets.hotspot_status.clone();
    let revealer = widgets.hotspot_revealer.clone();
    let ssid_label = widgets.hotspot_ssid.clone();
    let pass_label = widgets.hotspot_password.clone();
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
        let pass_label = pass_label.clone();
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
                pass_label.set_text(&config.hotspot_password);
            }
        });
    }

    // ── Toggle action ──
    let state_c = Rc::clone(&state);
    let toggle_c = toggle.clone();
    let status_c = status.clone();
    let revealer_c = revealer.clone();
    let ssid_label_c = ssid_label.clone();
    let pass_label_c = pass_label.clone();
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
        let pass_label = pass_label_c.clone();
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

                // Detect current WiFi channel for concurrent mode.
                // Hardware requires AP on same channel as STA (#channels <= 1).
                let wifi = { state.borrow().wifi.clone() };
                let (band, channel) = if let Some(freq) = wifi.get_active_frequency().await {
                    let ch = freq_to_channel(freq);
                    let band = if freq >= 4900 { "a" } else { "bg" };
                    log::info!("Concurrent mode: freq={freq}MHz, ch={ch:?}, band={band}");
                    (band.to_string(), ch)
                } else {
                    (config.hotspot_band.clone(), None)
                };

                match hotspot.start_hotspot(
                    &config.hotspot_ssid,
                    &config.hotspot_password,
                    &band,
                    channel,
                ).await {
                    Ok(_) => {
                        status.set_text("Active");
                        revealer.set_reveal_child(true);
                        ssid_label.set_text(&config.hotspot_ssid);
                        pass_label.set_text(&config.hotspot_password);
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
                match hotspot.stop_hotspot().await {
                    Ok(_) => {
                        status.set_text("Off");
                        revealer.set_reveal_child(false);
                        wifi_status.set_text("Wi-Fi");
                        // Trigger scan to restore network list
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
            }
        });

        glib::Propagation::Proceed
    });
}

/// Convert WiFi frequency (MHz) to channel number.
fn freq_to_channel(freq: u32) -> Option<u32> {
    match freq {
        2412 => Some(1),
        2417 => Some(2),
        2422 => Some(3),
        2427 => Some(4),
        2432 => Some(5),
        2437 => Some(6),
        2442 => Some(7),
        2447 => Some(8),
        2452 => Some(9),
        2457 => Some(10),
        2462 => Some(11),
        2467 => Some(12),
        2472 => Some(13),
        2484 => Some(14),
        f if f >= 5170 && f <= 5925 => Some((f - 5000) / 5),
        _ => None,
    }
}
