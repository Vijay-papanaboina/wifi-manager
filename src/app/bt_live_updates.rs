//! Bluetooth live updates — D-Bus signal subscriptions for real-time device changes.
//!
//! Mirrors `live_updates.rs` for WiFi, using BlueZ ObjectManager signals.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::bluez_proxies::BluezObjectManagerProxy;
use crate::ui::window::PanelWidgets;

use super::bluetooth::refresh_bt_list;
use super::AppState;

/// Subscribes to BlueZ ObjectManager InterfacesAdded and InterfacesRemoved signals to keep the Bluetooth device list updated when the Bluetooth tab is active.
///
/// Waits for a Bluetooth manager to become available, subscribes to the ObjectManager signals, and triggers a debounced (300 ms) refresh of the BT list whenever devices are added or removed. Subscriptions are aborted on error; refreshes run only while the Bluetooth tab is active.
///
/// # Examples
///
/// ```no_run
/// // assuming `widgets` and `state` are already initialized
/// setup_bt_live_updates(&widgets, state.clone());
/// ```
pub(super) fn setup_bt_live_updates(widgets: &PanelWidgets, state: Rc<RefCell<AppState>>) {
    let bt_list_box = widgets.bt_list_box.clone();
    let status = widgets.status_label.clone();
    let bt_tab = widgets.bt_tab.clone();

    glib::spawn_future_local(async move {
        // Wait until the BT manager is initialized
        // (setup_bluetooth runs concurrently)
        let bt = loop {
            {
                let st = state.borrow();
                if let Some(ref bt) = st.bluetooth {
                    break bt.clone();
                }
            }
            glib::timeout_future(std::time::Duration::from_millis(500)).await;
            // Check a few times then give up (no BT adapter)
            static ATTEMPTS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
            if ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 10 {
                log::debug!("BT live updates: no adapter after 5s, giving up");
                return;
            }
        };

        let conn = bt.connection();

        let obj_manager = match BluezObjectManagerProxy::new(conn).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("Failed to create BlueZ ObjectManager for live updates: {e}");
                return;
            }
        };

        // InterfacesAdded — new devices discovered
        let mut added_stream = match obj_manager.receive_interfaces_added().await {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to subscribe to InterfacesAdded: {e}");
                return;
            }
        };

        // InterfacesRemoved — devices disappeared
        let mut removed_stream = match obj_manager.receive_interfaces_removed().await {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to subscribe to InterfacesRemoved: {e}");
                return;
            }
        };

        log::info!("BT live updates: subscribed to InterfacesAdded/Removed signals");

        use futures_util::StreamExt;
        let bt_tab_added = bt_tab.clone();
        let bt_list_box_added = bt_list_box.clone();
        let status_added = status.clone();
        let state_added = Rc::clone(&state);
        glib::spawn_future_local(async move {
            while (added_stream.next().await).is_some() {
                if !bt_tab_added.is_active() {
                    continue;
                }
                log::debug!("BT InterfacesAdded — refreshing device list");
                glib::timeout_future(std::time::Duration::from_millis(300)).await;
                refresh_bt_list(&state_added, &bt_list_box_added, &status_added).await;
            }
        });

        let bt_tab_removed = bt_tab.clone();
        let bt_list_box_removed = bt_list_box.clone();
        let status_removed = status.clone();
        let state_removed = Rc::clone(&state);
        glib::spawn_future_local(async move {
            while (removed_stream.next().await).is_some() {
                if !bt_tab_removed.is_active() {
                    continue;
                }
                log::debug!("BT InterfacesRemoved — refreshing device list");
                glib::timeout_future(std::time::Duration::from_millis(300)).await;
                refresh_bt_list(&state_removed, &bt_list_box_removed, &status_removed).await;
            }
        });
    });
}
