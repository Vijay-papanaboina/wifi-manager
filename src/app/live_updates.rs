//! Live updates — D-Bus signal subscriptions for real-time network changes.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::ui::window::PanelWidgets;

use super::{AppState, get_wifi, refresh_list};

/// Subscribe to NM D-Bus signals for live state updates.
///
/// Watches:
/// - Device StateChanged — fires when connection state changes (connected/disconnected/etc)
/// - Wireless AccessPointAdded/Removed — fires when APs appear/disappear
///
/// On any change, the network list is auto-refreshed after a brief debounce.
pub(super) fn setup_live_updates(
    widgets: &PanelWidgets,
    state: Rc<RefCell<AppState>>,
    panel_visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();
    let switch = widgets.wifi_switch.clone();
    let wifi_tab = widgets.wifi_tab.clone();

    // Subscribe to Device.StateChanged signal
    {
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let switch = switch.clone();
        let wifi_tab = wifi_tab.clone();

        glib::spawn_future_local(async move {
            let wifi = get_wifi(&state);
            let conn = wifi.connection();
            let device_path = wifi.wifi_device_path();

            // Build a DeviceProxy for the WiFi device
            let device_proxy = match crate::dbus::proxies::DeviceProxy::builder(conn)
                .path(device_path.to_owned())
                .unwrap()
                .build()
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    log::error!("Failed to create device proxy for live updates: {e}");
                    return;
                }
            };

            // Listen for state changes
            let mut stream = match device_proxy.receive_state_changed().await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Failed to subscribe to device StateChanged: {e}");
                    return;
                }
            };

            log::info!("Live updates: subscribed to device StateChanged signal");

            use futures_util::StreamExt;
            while let Some(signal) = stream.next().await {
                let args = match signal.args() {
                    Ok(a) => a,
                    Err(_) => continue,
                };
                log::info!(
                    "Device state changed: {} -> {} (reason: {})",
                    args.old_state,
                    args.new_state,
                    args.reason
                );

                // NM device state constants:
                //   20  = Unavailable  (e.g. Wi-Fi radio off)
                //   30  = Disconnected
                //   100 = Activated (connected)
                let new_state = args.new_state;

                let is_hidden = !panel_visible.load(std::sync::atomic::Ordering::Relaxed);

                if new_state == 100 {
                    // Connected — stop the background reconnect loop immediately.
                    super::scanning::stop_wifi_bg_reconnect(&state);
                    log::info!("StateChanged: connected — bg reconnect loop stopped");
                } else if (new_state == 30 || new_state == 20) && is_hidden {
                    // Disconnected while panel is hidden — start the loop.
                    super::scanning::start_wifi_bg_reconnect(Rc::clone(&state));
                    log::info!("StateChanged: disconnected while hidden — bg reconnect loop started");
                }

                // Update WiFi switch state
                if wifi_tab.is_active() {
                    match wifi.is_wifi_enabled().await {
                        Ok(enabled) if wifi_tab.is_active() => switch.set_active(enabled),
                        Err(e) => log::error!("Failed to check WiFi state: {e}"),
                        _ => {}
                    }
                }

                // Brief debounce then refresh UI
                glib::timeout_future(std::time::Duration::from_millis(500)).await;
                if wifi_tab.is_active() {
                    refresh_list(&state, &list_box, &status).await;
                }
            }
        });
    }

    // Subscribe to Wireless AccessPointAdded/Removed signals
    {
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let wifi_tab = wifi_tab.clone();

        glib::spawn_future_local(async move {
            let wifi = get_wifi(&state);
            let conn = wifi.connection();
            let device_path = wifi.wifi_device_path();

            let wireless_proxy = match crate::dbus::proxies::WirelessProxy::builder(conn)
                .path(device_path.to_owned())
                .unwrap()
                .build()
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    log::error!("Failed to create wireless proxy for live updates: {e}");
                    return;
                }
            };

            // Listen for AP changes
            let ap_added = match wireless_proxy.receive_access_point_added().await {
                Ok(s) => Some(s),
                Err(e) => {
                    log::error!("Failed to subscribe to AccessPointAdded: {e}");
                    None
                }
            };
            let ap_removed = match wireless_proxy.receive_access_point_removed().await {
                Ok(s) => Some(s),
                Err(e) => {
                    log::error!("Failed to subscribe to AccessPointRemoved: {e}");
                    None
                }
            };

            if ap_added.is_none() && ap_removed.is_none() {
                log::error!("Live updates: failed to subscribe to AccessPointAdded/Removed signals");
                return;
            }

            log::info!("Live updates: subscribed to AccessPointAdded/Removed signals");

            use futures_util::StreamExt;
            if let Some(mut ap_added) = ap_added {
                let state_added = Rc::clone(&state);
                let list_box_added = list_box.clone();
                let status_added = status.clone();
                let wifi_tab_added = wifi_tab.clone();
                glib::spawn_future_local(async move {
                    while (ap_added.next().await).is_some() {
                        if !wifi_tab_added.is_active() {
                            continue;
                        }
                        log::debug!("AccessPoint added, refreshing list");
                        glib::timeout_future(std::time::Duration::from_millis(300)).await;
                        if wifi_tab_added.is_active() {
                            refresh_list(&state_added, &list_box_added, &status_added).await;
                        }
                    }
                });
            }

            if let Some(mut ap_removed) = ap_removed {
                let state_removed = Rc::clone(&state);
                let list_box_removed = list_box.clone();
                let status_removed = status.clone();
                let wifi_tab_removed = wifi_tab.clone();
                glib::spawn_future_local(async move {
                    while (ap_removed.next().await).is_some() {
                        if !wifi_tab_removed.is_active() {
                            continue;
                        }
                        log::debug!("AccessPoint removed, refreshing list");
                        glib::timeout_future(std::time::Duration::from_millis(300)).await;
                        if wifi_tab_removed.is_active() {
                            refresh_list(&state_removed, &list_box_removed, &status_removed).await;
                        }
                    }
                });
            }
        });
    }
}
