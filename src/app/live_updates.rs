//! Live updates — D-Bus signal subscriptions for real-time network changes.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::network_manager::WifiManager;
use crate::ui::{
    home::{HomeWidgets, TileState},
    window::PanelWidgets,
};

use super::refresh_queue::{RefreshKind, RefreshQueue, drain_signals};
use super::{AppState, get_wifi, refresh_list};

/// Refresh only the Home Wi-Fi tile from the current NetworkManager snapshot.
///
/// The compatibility status sink belongs to whichever detail page is active,
/// so Home updates use their own typed handle and never affect Wi-Fi or
/// Bluetooth detail status text.
async fn refresh_home_wifi_snapshot(wifi: &WifiManager, home: &HomeWidgets) {
    let enabled = match wifi.is_wifi_enabled().await {
        Ok(enabled) => enabled,
        Err(error) => {
            log::warn!("Failed to read Wi-Fi state for Home: {error}");
            home.set_wifi_enabled(false);
            home.wifi.set_state(TileState::Unavailable);
            return;
        }
    };

    home.set_wifi_enabled(enabled);
    if !enabled {
        home.set_wifi_status(Some("Wi-Fi disabled"));
        return;
    }

    match wifi.get_networks().await {
        Ok(networks) => {
            if let Some(network) = networks.iter().find(|network| network.is_connected) {
                home.set_wifi_status(Some(&format!("Connected to {}", network.ssid)));
            } else {
                home.set_wifi_status(Some("Wi-Fi enabled"));
            }
        }
        Err(error) => {
            log::warn!("Failed to read Wi-Fi networks for Home: {error}");
            home.wifi.set_state(TileState::Unavailable);
        }
    }
}

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
    let home = widgets.home.clone();
    let refreshes = Rc::new(RefreshQueue::default());

    // D-Bus signal consumers must keep polling even while a refresh is waiting
    // for a reply on the same connection. Share one worker across all three
    // signals, with at most one pending follow-up rather than one task/event.
    glib::spawn_future_local({
        let refreshes = Rc::clone(&refreshes);
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let switch = switch.clone();
        let wifi_tab = wifi_tab.clone();
        async move {
            refreshes
                .run(std::time::Duration::from_millis(300), |kind| {
                    let state = Rc::clone(&state);
                    let list_box = list_box.clone();
                    let status = status.clone();
                    let switch = switch.clone();
                    let wifi_tab = wifi_tab.clone();
                    let home = home.clone();
                    async move {
                        let wifi = get_wifi(&state);
                        if kind == RefreshKind::Device {
                            refresh_home_wifi_snapshot(&wifi, &home).await;
                            if wifi_tab.is_active() {
                                match wifi.is_wifi_enabled().await {
                                    Ok(enabled) if wifi_tab.is_active() => {
                                        switch.set_active(enabled)
                                    }
                                    Err(e) => log::error!("Failed to check WiFi state: {e}"),
                                    _ => {}
                                }
                            }
                        }
                        if wifi_tab.is_active() {
                            refresh_list(&state, &list_box, &status).await;
                        }
                    }
                })
                .await;
        }
    });

    // Subscribe to Device.StateChanged signal
    {
        let state = Rc::clone(&state);
        let refreshes = Rc::clone(&refreshes);

        glib::spawn_future_local(async move {
            let wifi = get_wifi(&state);
            let conn = wifi.connection();
            let device_path = wifi.wifi_device_path();

            // Build a DeviceProxy for the WiFi device
            let device_proxy = match crate::dbus::proxies::DeviceProxy::builder(conn)
                .path(device_path.to_owned())
            {
                Ok(builder) => match builder.build().await {
                    Ok(proxy) => proxy,
                    Err(error) => {
                        log::error!("Failed to create device proxy for live updates: {error}");
                        return;
                    }
                },
                Err(error) => {
                    log::error!("Invalid Wi-Fi device path for live updates: {error}");
                    return;
                }
            };

            // Listen for state changes
            let stream = match device_proxy.receive_state_changed().await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Failed to subscribe to device StateChanged: {e}");
                    return;
                }
            };

            log::info!("Live updates: subscribed to device StateChanged signal");

            drain_signals(stream, |signal| {
                let args = match signal.args() {
                    Ok(a) => a,
                    Err(_) => return,
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
                    log::info!(
                        "StateChanged: disconnected while hidden — bg reconnect loop started"
                    );
                }

                refreshes.request(RefreshKind::Device);
            })
            .await;
        });
    }

    // Subscribe and start draining each stream immediately; do not leave an
    // existing stream idle while awaiting another subscription on the same bus.
    for added in [true, false] {
        let state = Rc::clone(&state);
        let wifi_tab = wifi_tab.clone();
        let refreshes = Rc::clone(&refreshes);

        glib::spawn_future_local(async move {
            let wifi = get_wifi(&state);
            let conn = wifi.connection();
            let device_path = wifi.wifi_device_path();

            let wireless_proxy = match crate::dbus::proxies::WirelessProxy::builder(conn)
                .path(device_path.to_owned())
            {
                Ok(builder) => match builder.build().await {
                    Ok(proxy) => proxy,
                    Err(error) => {
                        log::error!("Failed to create wireless proxy for live updates: {error}");
                        return;
                    }
                },
                Err(error) => {
                    log::error!("Invalid Wi-Fi device path for live updates: {error}");
                    return;
                }
            };

            let request_refresh = || {
                if wifi_tab.is_active() {
                    refreshes.request(RefreshKind::Networks);
                }
            };
            if added {
                match wireless_proxy.receive_access_point_added().await {
                    Ok(stream) => {
                        log::info!("Live updates: subscribed to AccessPointAdded");
                        drain_signals(stream, |_| request_refresh()).await;
                    }
                    Err(e) => log::error!("Failed to subscribe to AccessPointAdded: {e}"),
                }
            } else {
                match wireless_proxy.receive_access_point_removed().await {
                    Ok(stream) => {
                        log::info!("Live updates: subscribed to AccessPointRemoved");
                        drain_signals(stream, |_| request_refresh()).await;
                    }
                    Err(e) => log::error!("Failed to subscribe to AccessPointRemoved: {e}"),
                }
            }
        });
    }
}
