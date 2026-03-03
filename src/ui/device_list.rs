//! Scrollable list of Bluetooth devices.

use gtk4::prelude::*;
use gtk4::{Label, ListBox, PolicyType, ScrolledWindow, SelectionMode};
use crate::ui::window::{MIN_LIST_HEIGHT, MAX_LIST_HEIGHT};

use super::device_row;
use crate::dbus::bluetooth_device::BluetoothDevice;

/// Create a styled, vertically scrollable ListBox wrapped in a ScrolledWindow.
///
/// The returned tuple contains a `ScrolledWindow` whose child is the configured `ListBox`.
/// The `ListBox` is styled with the "device-list" CSS class, has selection disabled,
/// and activates on single click. The `ScrolledWindow` is styled with "device-scroll",
/// constrains content height between the defined minimum and maximum, hides its frame,
/// and uses automatic vertical scrolling.
///
/// # Examples
///
/// ```
/// let (scrolled, list) = build_device_list();
/// // The ListBox is set as the scrolled window's child.
/// assert!(scrolled.child().is_some());
/// // ListBox should be configured for single-click activation.
/// ```
pub fn build_device_list() -> (ScrolledWindow, ListBox) {
    let list_box = ListBox::new();
    list_box.add_css_class("device-list");
    list_box.set_selection_mode(SelectionMode::None);
    list_box.set_activate_on_single_click(true);

    let scrolled = ScrolledWindow::new();
    scrolled.add_css_class("device-scroll");
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_has_frame(false);
    scrolled.set_propagate_natural_height(true);
    scrolled.set_min_content_height(MIN_LIST_HEIGHT);
    scrolled.set_max_content_height(MAX_LIST_HEIGHT);
    scrolled.set_child(Some(&list_box));

    (scrolled, list_box)
}

/// Replace the contents of a ListBox with rows for the provided Bluetooth devices.
///
/// If `devices` is empty, a label reading "No devices found" is appended. Otherwise a row is
/// created for each device and appended to `list_box`. The `on_remove` callback is invoked with
/// the device path when a row requests removal; the `on_menu_active` callback is invoked with the
/// menu's active state when a row's menu becomes active or inactive.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use gtk4::ListBox;
///
/// let list_box = ListBox::new();
/// let devices: Vec<BluetoothDevice> = Vec::new();
///
/// let on_remove = Rc::new(|path: String| {
///     eprintln!("remove requested for: {}", path);
/// });
/// let on_menu_active = Rc::new(|active: bool| {
///     eprintln!("menu active: {}", active);
/// });
///
/// populate_device_list(&list_box, &devices, on_remove, on_menu_active);
/// ```
pub fn populate_device_list(
    list_box: &ListBox,
    devices: &[BluetoothDevice],
    on_remove: std::rc::Rc<dyn Fn(String)>,
    on_menu_active: std::rc::Rc<dyn Fn(bool)>,
) {
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

    for device in devices {
        let on_remove = on_remove.clone();
        let on_menu_active = on_menu_active.clone();

        let row = device_row::build_device_row(
            device,
            move |device_path| {
                on_remove(device_path);
            },
            move |active| {
                on_menu_active(active);
            },
        );
        list_box.append(&row);
    }
}
