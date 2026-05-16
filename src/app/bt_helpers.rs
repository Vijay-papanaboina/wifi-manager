//! Bluetooth helper utilities — shared by `bluetooth.rs` and `bt_scanning.rs`.
//!
//! Kept separate because none of these functions set up GTK signal handlers;
//! they are pure helpers or async D-Bus fetch routines.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;

use crate::dbus::bluetooth_manager::BluetoothManager;
use crate::ui::device_list;

use super::AppState;

/// Extract BluetoothManager from AppState.
pub(super) fn get_bt(state: &Rc<RefCell<AppState>>) -> Option<BluetoothManager> {
    state.borrow().bluetooth.clone()
}

/// No-op remove callback (used when BT is off / list is empty).
pub(super) fn no_op_remove() -> std::rc::Rc<dyn Fn(String)> {
    std::rc::Rc::new(|_path| {})
}

/// No-op menu-active callback (used when BT is off / list is empty).
pub(super) fn no_op_menu_active() -> std::rc::Rc<dyn Fn(bool)> {
    std::rc::Rc::new(|_active| {})
}

/// Build the callback that handles "Unpair device" from the row context menu.
pub(super) fn build_remove_callback(
    state: &Rc<RefCell<AppState>>,
    list_box: &gtk4::ListBox,
    status: &gtk4::Label,
    bt: &BluetoothManager,
) -> std::rc::Rc<dyn Fn(String)> {
    let state = Rc::clone(state);
    let list_box = list_box.clone();
    let status = status.clone();
    let bt = bt.clone();
    std::rc::Rc::new(move |device_path| {
        let state = Rc::clone(&state);
        let list_box = list_box.clone();
        let status = status.clone();
        let bt = bt.clone();
        glib::spawn_future_local(async move {
            status.set_text("Unpairing device...");
            {
                let mut st = state.borrow_mut();
                st.bt_pending
                    .insert(device_path.clone(), "Unpairing".to_string());
            }
            refresh_bt_list(&state, &list_box, &status).await;
            match bt.remove_device(&device_path).await {
                Ok(_) => {
                    status.set_text("Device unpaired");
                    {
                        let mut st = state.borrow_mut();
                        st.bt_pending.remove(&device_path);
                    }
                    refresh_bt_list(&state, &list_box, &status).await;
                }
                Err(e) => {
                    log::error!("Remove failed: {e}");
                    status.set_text(&format!("Failed to unpair: {}", e));
                    {
                        let mut st = state.borrow_mut();
                        st.bt_pending.remove(&device_path);
                    }
                }
            }
        });
    })
}

/// Build the callback that tracks whether a row context menu is open.
///
/// While open, list refreshes are suppressed to avoid the popover closing.
pub(super) fn build_menu_active_callback(state: &Rc<RefCell<AppState>>) -> std::rc::Rc<dyn Fn(bool)> {
    let state = Rc::clone(state);
    std::rc::Rc::new(move |active| {
        state.borrow_mut().bt_menu_open = active;
    })
}

/// Refresh the Bluetooth device list from D-Bus and update the UI.
///
/// Skips the refresh if a context menu is currently open.
pub(super) async fn refresh_bt_list(
    state: &Rc<RefCell<AppState>>,
    list_box: &gtk4::ListBox,
    status: &gtk4::Label,
) {
    if state.borrow().bt_menu_open {
        log::debug!("BT menu open — skipping refresh");
        return;
    }
    let bt = match get_bt(state) {
        Some(bt) => bt,
        None => return,
    };

    match bt.get_devices().await {
        Ok(devices) => {
            let connected = devices.iter().find(|d| d.connected);
            match connected {
                Some(d) => status.set_text(&format!("Connected to {}", d.display_name)),
                None => status.set_text("Not connected"),
            }

            let on_remove = build_remove_callback(state, list_box, status, &bt);
            let on_menu_active = build_menu_active_callback(state);
            let row_paths = device_list::populate_device_list(
                list_box,
                &devices,
                &state.borrow().bt_pending,
                on_remove,
                on_menu_active,
            );
            state.borrow_mut().bt_row_paths = row_paths;
            log::info!("BT device list refreshed: {} devices", devices.len());
            state.borrow_mut().bt_devices = devices;
        }
        Err(e) => {
            log::error!("Failed to get BT devices: {e}");
            status.set_text("Failed to load devices");
        }
    }
}
