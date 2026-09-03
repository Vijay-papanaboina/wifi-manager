//! Bluetooth live updates — D-Bus signal subscriptions for real-time device changes.
//!
//! Mirrors `live_updates.rs` for WiFi, using BlueZ ObjectManager signals.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::bluez_proxies::{Adapter1Proxy, BluezObjectManagerProxy};
use crate::ui::window::PanelWidgets;

use super::bt_helpers::{clear_bt_list, refresh_bt_list};
use super::bt_scanning::{start_bt_background_tasks, stop_bt_background_tasks};
use super::AppState;

/// Subscribe to BlueZ ObjectManager signals for live BT updates.
///
/// Watches `InterfacesAdded` — fires when a new device is discovered or
/// a device's interface changes (e.g. Connected property change).
///
/// This refreshes the BT device list automatically, but only when the
/// Bluetooth tab is active.
pub(super) fn setup_bt_live_updates(
    widgets: &PanelWidgets,
    state: Rc<RefCell<AppState>>,
    panel_visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let bt_list_box = widgets.bt_list_box.clone();
    let status = widgets.status_label.clone();
    let bt_tab = widgets.bt_tab.clone();
    let switch = widgets.wifi_switch.clone();

    glib::spawn_future_local(async move {
        // Wait until the BT manager is initialized
        // (setup_bluetooth runs concurrently)
        let mut attempts = 0_u8;
        let bt = loop {
            {
                let st = state.borrow();
                if let Some(ref bt) = st.bluetooth {
                    break bt.clone();
                }
            }
            glib::timeout_future(std::time::Duration::from_millis(500)).await;
            attempts = attempts.saturating_add(1);
            // Check a few times then give up (no BT adapter).
            // Keep this counter local so a later retry is not poisoned by a
            // previous setup attempt.
            if attempts > 10 {
                log::debug!("BT live updates: no adapter after 5s, giving up");
                return;
            }
        };

        // Adapter1.Powered is a property change, not an ObjectManager
        // interface add/remove. Keep the shared switch and BT list in sync
        // when another process (for example bluetoothctl) changes power.
        let bt_for_power = bt.clone();
        let state_power = Rc::clone(&state);
        let bt_tab_power = bt_tab.clone();
        let bt_list_box_power = bt_list_box.clone();
        let status_power = status.clone();
        let switch_power = switch.clone();
        let panel_visible_power = panel_visible.clone();
        glib::spawn_future_local(async move {
            let adapter = match Adapter1Proxy::builder(bt_for_power.connection())
                .path(bt_for_power.adapter_path().to_owned())
            {
                Ok(builder) => match builder.build().await {
                    Ok(adapter) => adapter,
                    Err(e) => {
                        log::error!("Failed to create BlueZ Adapter1 for power updates: {e}");
                        return;
                    }
                },
                Err(e) => {
                    log::error!("Failed to set BlueZ Adapter1 path for power updates: {e}");
                    return;
                }
            };

            let mut powered_stream = adapter.receive_powered_changed().await;
            use futures_util::StreamExt;
            while powered_stream.next().await.is_some() {
                let powered = match adapter.powered().await {
                    Ok(powered) => powered,
                    Err(e) => {
                        log::warn!("Failed to read Bluetooth power after change: {e}");
                        continue;
                    }
                };

                // The switch is shared by Wi-Fi and Bluetooth. Do not let a
                // BT event overwrite the Wi-Fi view while that tab is active.
                if !bt_tab_power.is_active() {
                    if !powered {
                        stop_bt_background_tasks(&state_power);
                        let _ = bt_for_power.stop_discovery().await;
                    }
                    continue;
                }

                switch_power.set_active(powered);
                if !powered {
                    stop_bt_background_tasks(&state_power);
                    clear_bt_list(
                        &state_power,
                        &bt_list_box_power,
                        &status_power,
                        "Bluetooth disabled",
                    );
                    let _ = bt_for_power.stop_discovery().await;
                } else if !state_power.borrow().bt_power_transition_in_progress
                    && panel_visible_power.load(std::sync::atomic::Ordering::Relaxed)
                {
                    refresh_bt_list(&state_power, &bt_list_box_power, &status_power).await;
                    if bt_tab_power.is_active() {
                        start_bt_background_tasks(
                            Rc::clone(&state_power),
                            bt_tab_power.clone(),
                            bt_list_box_power.clone(),
                            status_power.clone(),
                        );
                    }
                }
            }
        });

        let conn = bt.connection();

        let obj_manager = match BluezObjectManagerProxy::new(conn).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("Failed to create BlueZ ObjectManager for live updates: {e}");
                return;
            }
        };

        // InterfacesAdded — new devices discovered
        let added_stream = match obj_manager.receive_interfaces_added().await {
            Ok(s) => Some(s),
            Err(e) => {
                log::error!("Failed to subscribe to InterfacesAdded: {e}");
                None
            }
        };

        // InterfacesRemoved — devices disappeared
        let removed_stream = match obj_manager.receive_interfaces_removed().await {
            Ok(s) => Some(s),
            Err(e) => {
                log::error!("Failed to subscribe to InterfacesRemoved: {e}");
                None
            }
        };

        if added_stream.is_none() && removed_stream.is_none() {
            log::error!("BT live updates: failed to subscribe to InterfacesAdded/Removed");
            return;
        }

        log::info!("BT live updates: subscribed to InterfacesAdded/Removed signals");

        use futures_util::StreamExt;
        let bt_tab_added = bt_tab.clone();
        let bt_list_box_added = bt_list_box.clone();
        let status_added = status.clone();
        let state_added = Rc::clone(&state);
        if let Some(mut added_stream) = added_stream {
            glib::spawn_future_local(async move {
                while (added_stream.next().await).is_some() {
                    if !bt_tab_added.is_active() {
                        continue;
                    }
                    log::debug!("BT InterfacesAdded — refreshing device list");
                    glib::timeout_future(std::time::Duration::from_millis(300)).await;
                    if !bt_tab_added.is_active() {
                        continue;
                    }
                    refresh_bt_list(&state_added, &bt_list_box_added, &status_added).await;
                }
            });
        }

        let bt_tab_removed = bt_tab.clone();
        let bt_list_box_removed = bt_list_box.clone();
        let status_removed = status.clone();
        let state_removed = Rc::clone(&state);
        if let Some(mut removed_stream) = removed_stream {
            glib::spawn_future_local(async move {
                while (removed_stream.next().await).is_some() {
                    if !bt_tab_removed.is_active() {
                        continue;
                    }
                    log::debug!("BT InterfacesRemoved — refreshing device list");
                    glib::timeout_future(std::time::Duration::from_millis(300)).await;
                    if !bt_tab_removed.is_active() {
                        continue;
                    }
                    refresh_bt_list(&state_removed, &bt_list_box_removed, &status_removed).await;
                }
            });
        }
    });
}
