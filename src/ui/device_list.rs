//! Scrollable list of Bluetooth devices.

use gtk4::prelude::*;
use gtk4::{Align, Label, ListBox, ListBoxRow, PolicyType, ScrolledWindow, SelectionMode};
use crate::ui::window::{MIN_LIST_HEIGHT, MAX_LIST_HEIGHT};

use super::device_row;
use crate::dbus::bluetooth_device::BluetoothDevice;

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
    scrolled.set_has_frame(false);
    scrolled.set_propagate_natural_height(true);
    scrolled.set_min_content_height(MIN_LIST_HEIGHT);
    scrolled.set_max_content_height(MAX_LIST_HEIGHT);
    scrolled.set_child(Some(&list_box));

    (scrolled, list_box)
}

/// Clear the list and repopulate with the given Bluetooth devices.
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

    let has_paired = devices.iter().any(|d| d.paired);
    let mut inserted_separator = false;

    for device in devices {
        if has_paired && !device.paired && !inserted_separator {
            list_box.append(&build_separator_row("Available devices"));
            inserted_separator = true;
        }

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

fn build_separator_row(label: &str) -> ListBoxRow {
    let row = ListBoxRow::new();
    row.add_css_class("list-separator-row");
    row.set_selectable(false);
    row.set_activatable(false);

    let title = Label::new(Some(label));
    title.add_css_class("list-separator");
    title.set_halign(Align::Start);
    title.set_margin_start(16);
    title.set_margin_end(16);
    title.set_margin_top(6);
    title.set_margin_bottom(4);
    row.set_child(Some(&title));
    row
}
