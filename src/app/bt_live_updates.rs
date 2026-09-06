//! Bluetooth live updates — D-Bus signal subscriptions for real-time device changes.
//!
//! Mirrors `live_updates.rs` for WiFi, using BlueZ ObjectManager signals.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::bluetooth_manager::BluetoothManager;
use crate::dbus::bluez_proxies::{Adapter1Proxy, BluezObjectManagerProxy};
use crate::ui::{
    home::{HomeWidgets, TileState},
    window::PanelWidgets,
};

use super::AppState;
use super::bt_helpers::{clear_bt_list, refresh_bt_list};
use super::bt_scanning::{start_bt_background_tasks, stop_bt_background_tasks};

/// Refresh only the Home Bluetooth tile from the current BlueZ snapshot.
///
/// Detail-page status remains owned by the active Bluetooth controller. This
/// helper intentionally writes only the typed Home tile handles.
async fn refresh_home_bluetooth_snapshot(bt: &BluetoothManager, home: &HomeWidgets) {
    let powered = match bt.is_powered().await {
        Ok(powered) => powered,
        Err(error) => {
            log::warn!("Failed to read Bluetooth power for Home: {error}");
            home.set_bluetooth_enabled(false);
            home.bluetooth.set_state(TileState::Unavailable);
            return;
        }
    };

    home.set_bluetooth_enabled(powered);
    if !powered {
        home.set_bluetooth_status(Some("Bluetooth disabled"));
        return;
    }

    match bt.get_devices().await {
        Ok(devices) => {
            if let Some(device) = devices.iter().find(|device| device.connected) {
                home.set_bluetooth_status(Some(&format!("Connected to {}", device.display_name)));
            } else {
                home.set_bluetooth_status(Some("Bluetooth enabled"));
            }
        }
        Err(error) => {
            log::warn!("Failed to read Bluetooth devices for Home: {error}");
            home.bluetooth.set_state(TileState::Unavailable);
        }
    }
}

/// Debounce Home-only refreshes shared by the ObjectManager added/removed
/// streams. Discovery commonly emits a burst, and a single pending source
/// keeps those signals from serializing full BlueZ snapshots.
fn schedule_home_bluetooth_refresh(
    pending: &Rc<RefCell<Option<glib::SourceId>>>,
    bt: BluetoothManager,
    home: HomeWidgets,
) {
    if pending.borrow().is_some() {
        return;
    }

    let pending_for_callback = Rc::clone(pending);
    let source_id =
        glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
            pending_for_callback.borrow_mut().take();
            glib::spawn_future_local(async move {
                refresh_home_bluetooth_snapshot(&bt, &home).await;
            });
        });
    *pending.borrow_mut() = Some(source_id);
}

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
    let home = widgets.home.clone();

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
        let home_power = home.clone();
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
                        home_power.set_bluetooth_enabled(false);
                        home_power.bluetooth.set_state(TileState::Unavailable);
                        continue;
                    }
                };

                // Powered changes must update Home independently of which
                // detail page currently owns the legacy controls.
                refresh_home_bluetooth_snapshot(&bt_for_power, &home_power).await;

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

        // BlueZ reports Device1.Connected changes through the generic
        // PropertiesChanged signal. Scope the match to this adapter so
        // another adapter cannot perturb this panel's Home tile.
        let device_rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.bluez")
            .and_then(|rule| rule.interface("org.freedesktop.DBus.Properties"))
            .and_then(|rule| rule.member("PropertiesChanged"))
            .and_then(|rule| rule.arg(0, "org.bluez.Device1"))
            .and_then(|rule| rule.path_namespace(bt.adapter_path()))
            .map(|rule| rule.build());

        match device_rule {
            Ok(device_rule) => {
                match zbus::MessageStream::for_match_rule(device_rule, conn, Some(32)).await {
                    Ok(mut device_stream) => {
                        let bt_for_device = bt.clone();
                        let home_for_device = home.clone();
                        glib::spawn_future_local(async move {
                            use futures_util::StreamExt;
                            while let Some(message) = device_stream.next().await {
                                let message = match message {
                                    Ok(message) => message,
                                    Err(error) => {
                                        log::debug!(
                                            "Ignoring BlueZ PropertiesChanged stream error: {error}"
                                        );
                                        continue;
                                    }
                                };
                                let (interface, changed, invalidated) = match message
                                    .body()
                                    .deserialize::<(
                                        String,
                                        HashMap<String, zbus::zvariant::OwnedValue>,
                                        Vec<String>,
                                    )>() {
                                    Ok(body) => body,
                                    Err(error) => {
                                        log::debug!(
                                            "Ignoring malformed BlueZ PropertiesChanged signal: {error}"
                                        );
                                        continue;
                                    }
                                };

                                if interface != "org.bluez.Device1"
                                    || (!changed.contains_key("Connected")
                                        && !invalidated.iter().any(|name| name == "Connected"))
                                {
                                    continue;
                                }

                                refresh_home_bluetooth_snapshot(&bt_for_device, &home_for_device)
                                    .await;
                            }
                        });
                    }
                    Err(error) => {
                        log::error!(
                            "Failed to subscribe to BlueZ Device1 PropertiesChanged: {error}"
                        );
                    }
                }
            }
            Err(error) => {
                log::error!("Failed to build BlueZ Device1 match rule: {error}");
            }
        }

        // Battery1 is a sibling interface on the same BlueZ device object.
        // Keep its optional Percentage property live in the detail list while
        // leaving Home/status ownership to the existing Device1 path.
        let battery_rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.bluez")
            .and_then(|rule| rule.interface("org.freedesktop.DBus.Properties"))
            .and_then(|rule| rule.member("PropertiesChanged"))
            .and_then(|rule| rule.arg(0, "org.bluez.Battery1"))
            .and_then(|rule| rule.path_namespace(bt.adapter_path()))
            .map(|rule| rule.build());

        match battery_rule {
            Ok(battery_rule) => {
                match zbus::MessageStream::for_match_rule(battery_rule, conn, Some(32)).await {
                    Ok(mut battery_stream) => {
                        let state_for_battery = Rc::clone(&state);
                        let bt_tab_for_battery = bt_tab.clone();
                        let bt_list_box_for_battery = bt_list_box.clone();
                        let status_for_battery = status.clone();
                        glib::spawn_future_local(async move {
                            use futures_util::StreamExt;
                            while let Some(message) = battery_stream.next().await {
                                let message = match message {
                                    Ok(message) => message,
                                    Err(error) => {
                                        log::debug!(
                                            "Ignoring BlueZ Battery1 PropertiesChanged stream error: {error}"
                                        );
                                        continue;
                                    }
                                };
                                let (_interface, changed, invalidated) = match message
                                    .body()
                                    .deserialize::<(
                                        String,
                                        HashMap<String, zbus::zvariant::OwnedValue>,
                                        Vec<String>,
                                    )>() {
                                    Ok(body) => body,
                                    Err(error) => {
                                        log::debug!(
                                            "Ignoring malformed BlueZ Battery1 PropertiesChanged signal: {error}"
                                        );
                                        continue;
                                    }
                                };
                                if !changed.contains_key("Percentage")
                                    && !invalidated.iter().any(|name| name == "Percentage")
                                {
                                    continue;
                                }
                                if bt_tab_for_battery.is_active() {
                                    refresh_bt_list(
                                        &state_for_battery,
                                        &bt_list_box_for_battery,
                                        &status_for_battery,
                                    )
                                    .await;
                                }
                            }
                        });
                    }
                    Err(error) => {
                        log::error!(
                            "Failed to subscribe to BlueZ Battery1 PropertiesChanged: {error}"
                        );
                    }
                }
            }
            Err(error) => {
                log::error!("Failed to build BlueZ Battery1 match rule: {error}");
            }
        }

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
        let home_refresh_source = Rc::new(RefCell::new(None));
        let bt_tab_added = bt_tab.clone();
        let bt_list_box_added = bt_list_box.clone();
        let status_added = status.clone();
        let state_added = Rc::clone(&state);
        let bt_added = bt.clone();
        let home_added = home.clone();
        let home_refresh_added = Rc::clone(&home_refresh_source);
        if let Some(mut added_stream) = added_stream {
            glib::spawn_future_local(async move {
                while (added_stream.next().await).is_some() {
                    schedule_home_bluetooth_refresh(
                        &home_refresh_added,
                        bt_added.clone(),
                        home_added.clone(),
                    );
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
        let bt_removed = bt.clone();
        let home_removed = home.clone();
        let home_refresh_removed = Rc::clone(&home_refresh_source);
        if let Some(mut removed_stream) = removed_stream {
            glib::spawn_future_local(async move {
                while (removed_stream.next().await).is_some() {
                    schedule_home_bluetooth_refresh(
                        &home_refresh_removed,
                        bt_removed.clone(),
                        home_removed.clone(),
                    );
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
