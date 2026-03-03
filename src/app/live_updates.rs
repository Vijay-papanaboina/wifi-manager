//! Live updates — D-Bus signal subscriptions for real-time network changes.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;

use crate::ui::window::PanelWidgets;

use super::{AppState, get_wifi, refresh_list};

/// Register D-Bus signal handlers to keep the network UI in sync with system changes.
///
/// Subscribes to NetworkManager device StateChanged and wireless AccessPointAdded/AccessPointRemoved
/// signals; updates the Wi-Fi toggle and refreshes the network list after a short debounce when
/// relevant events are received.
///
/// # Examples
///
/// ```no_run
/// // given `widgets: PanelWidgets` and `state: Rc<RefCell<AppState>>`
/// setup_live_updates(&widgets, state.clone());
/// ```
pub(super) fn setup_live_updates(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();
    let switch = widgets.wifi_switch.clone();

    // Subscribe to Device.StateChanged signal
    {
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let switch = switch.clone();

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

                // Update WiFi switch state
                match wifi.is_wifi_enabled().await {
                    Ok(enabled) => switch.set_active(enabled),
                    Err(e) => log::error!("Failed to check WiFi state: {e}"),
                }

                // Brief debounce then refresh
                glib::timeout_future(std::time::Duration::from_millis(500)).await;
                refresh_list(&state, &list_box, &status).await;
            }
        });
    }

    // Subscribe to Wireless AccessPointAdded/Removed signals
    {
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();

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
            let mut ap_added = match wireless_proxy.receive_access_point_added().await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Failed to subscribe to AccessPointAdded: {e}");
                    return;
                }
            };
            let mut ap_removed = match wireless_proxy.receive_access_point_removed().await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Failed to subscribe to AccessPointRemoved: {e}");
                    return;
                }
            };

            log::info!("Live updates: subscribed to AccessPointAdded/Removed signals");

            use futures_util::StreamExt;
            let state_added = Rc::clone(&state);
            let list_box_added = list_box.clone();
            let status_added = status.clone();
            glib::spawn_future_local(async move {
                while (ap_added.next().await).is_some() {
                    log::debug!("AccessPoint added, refreshing list");
                    glib::timeout_future(std::time::Duration::from_millis(300)).await;
                    refresh_list(&state_added, &list_box_added, &status_added).await;
                }
            });

            let state_removed = Rc::clone(&state);
            let list_box_removed = list_box.clone();
            let status_removed = status.clone();
            glib::spawn_future_local(async move {
                while (ap_removed.next().await).is_some() {
                    log::debug!("AccessPoint removed, refreshing list");
                    glib::timeout_future(std::time::Duration::from_millis(300)).await;
                    refresh_list(&state_removed, &list_box_removed, &status_removed).await;
                }
            });
        });
    }
}
