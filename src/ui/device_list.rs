//! Scrollable list of Bluetooth devices.

use gtk4::prelude::*;
use gtk4::{Label, ListBox, PolicyType, ScrolledWindow, SelectionMode};

use super::device_row;
use crate::dbus::bluetooth_device::BluetoothDevice;
use crate::dbus::bluetooth_manager::BluetoothManager;

/// Build a scrollable device list.
///
/// Returns `(scrolled_window, list_box)`.
pub fn build_device_list() -> (ScrolledWindow, ListBox) {
    let list_box = ListBox::new();
    list_box.add_css_class("device-list");
    list_box.set_selection_mode(SelectionMode::None);
    list_box.set_activate_on_single_click(true);

    let scrolled = ScrolledWindow::new();
    scrolled.add_css_class("device-scroll");
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_min_content_height(100);
    scrolled.set_max_content_height(420);
    scrolled.set_child(Some(&list_box));

    (scrolled, list_box)
}

/// Clear the list and repopulate with the given Bluetooth devices.
pub fn populate_device_list(
    list_box: &ListBox,
    devices: &[BluetoothDevice],
    bt: &BluetoothManager,
    status: &gtk4::Label,
) {
    use gtk4::glib;

    // Remove all existing rows
    while let Some(row) = list_box.first_child() {
        list_box.remove(&row);
    }

    if devices.is_empty() {
        let empty = Label::new(Some("No devices found"));
        empty.add_css_class("empty-label");
        list_box.append(&empty);
        return;
    }

    let bt = bt.clone();
    let status_clone = status.clone();

    for device in devices {
        let bt_clone = bt.clone();
        let status_clone2 = status_clone.clone();

        let row = device_row::build_device_row(device, move |device_path| {
            let bt = bt_clone.clone();
            let status = status_clone2.clone();

            glib::spawn_future_local(async move {
                status.set_text("Removing device...");
                match bt.remove_device(&device_path).await {
                    Ok(_) => {
                        status.set_text("Device removed");
                    }
                    Err(e) => {
                        log::error!("Remove failed: {e}");
                        status.set_text(&format!("Failed to remove: {}", e));
                    }
                }
            });
        });
        list_box.append(&row);
    }
}
