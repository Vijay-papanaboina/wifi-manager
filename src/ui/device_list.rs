//! Scrollable list of Bluetooth devices.

use crate::ui::window::{MAX_LIST_HEIGHT, MIN_LIST_HEIGHT};
use gtk4::prelude::*;
use gtk4::{Align, Label, ListBox, ListBoxRow, PolicyType, ScrolledWindow, SelectionMode};
use std::collections::HashMap;

use super::device_row;
use crate::domain::bluetooth::BluetoothDevice;

/// Build a scrollable device list.
///
/// Returns `(scrolled_window, list_box)`.
pub(crate) fn build_device_list() -> (ScrolledWindow, ListBox) {
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
pub(crate) fn populate_device_list(
    list_box: &ListBox,
    devices: &[BluetoothDevice],
    pending: &HashMap<String, String>,
    on_remove: std::rc::Rc<dyn Fn(String)>,
    on_menu_active: std::rc::Rc<dyn Fn(bool)>,
) -> Vec<Option<String>> {
    // Remove all existing rows
    while let Some(row) = list_box.first_child() {
        list_box.remove(&row);
    }

    if devices.is_empty() {
        let empty = Label::new(Some("No devices found"));
        empty.add_css_class("empty-label");
        list_box.append(&empty);
        return Vec::new();
    }

    let mut row_paths: Vec<Option<String>> = Vec::new();
    let mut paired = Vec::new();
    let mut available = Vec::new();

    // The domain order is connected, paired, name, path. Partitioning here
    // keeps the visual sections correct even when a connected device is not
    // paired (for example, a newly discovered device).
    for device in devices {
        if device.paired {
            paired.push(device);
        } else {
            available.push(device);
        }
    }

    let append_device = |device: &BluetoothDevice, row_paths: &mut Vec<Option<String>>| {
        let on_remove = on_remove.clone();
        let on_menu_active = on_menu_active.clone();

        let pending_label = pending.get(&device.device_path).cloned();
        let row = device_row::build_device_row(
            device,
            pending_label,
            move |device_path| {
                on_remove(device_path);
            },
            move |active| {
                on_menu_active(active);
            },
        );
        list_box.append(&row);
        row_paths.push(Some(device.device_path.clone()));
    };

    let has_paired = !paired.is_empty();
    let has_available = !available.is_empty();
    for device in &paired {
        append_device(device, &mut row_paths);
    }
    if has_paired && has_available {
        list_box.append(&build_separator_row("Available devices"));
        row_paths.push(None);
    }
    for device in &available {
        append_device(device, &mut row_paths);
    }

    row_paths
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
