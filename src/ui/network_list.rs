//! Scrollable list of available WiFi networks.

use gtk4::prelude::*;
use gtk4::{Label, ListBox, PolicyType, ScrolledWindow, SelectionMode};

use super::network_row;
use crate::dbus::access_point::Network;

/// Build a scrollable network list.
///
/// Returns `(scrolled_window, list_box)` — the list_box is needed to populate
/// rows and handle selection events.
pub fn build_network_list() -> (ScrolledWindow, ListBox) {
    let list_box = ListBox::new();
    list_box.add_css_class("network-list");
    list_box.set_selection_mode(SelectionMode::None);
    list_box.set_activate_on_single_click(true);

    let scrolled = ScrolledWindow::new();
    scrolled.add_css_class("network-scroll");
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_min_content_height(100);
    scrolled.set_max_content_height(420);
    scrolled.set_child(Some(&list_box));

    (scrolled, list_box)
}

/// Clear the list and repopulate with the given networks.
pub fn populate_network_list(list_box: &ListBox, networks: &[Network]) {
    // Remove all existing rows
    while let Some(row) = list_box.first_child() {
        list_box.remove(&row);
    }

    if networks.is_empty() {
        let empty = Label::new(Some("No networks found"));
        empty.add_css_class("empty-label");
        list_box.append(&empty);
        return;
    }

    for net in networks {
        let row = network_row::build_network_row(net);
        list_box.append(&row);
    }
}
