//! Scrollable list of available WiFi networks.

use crate::ui::window::{MAX_LIST_HEIGHT, MIN_LIST_HEIGHT};
use gtk4::prelude::*;
use gtk4::{Align, Label, ListBox, ListBoxRow, PolicyType, ScrolledWindow, SelectionMode};

use super::network_row;
use crate::domain::network::{Network, sort_networks};

/// Build a scrollable network list.
///
/// Returns `(scrolled_window, list_box)` — the list_box is needed to populate
/// rows and handle selection events.
pub(crate) fn build_network_list() -> (ScrolledWindow, ListBox) {
    let list_box = ListBox::new();
    list_box.add_css_class("network-list");
    list_box.set_selection_mode(SelectionMode::None);
    list_box.set_activate_on_single_click(true);

    let scrolled = ScrolledWindow::new();
    scrolled.add_css_class("network-scroll");
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_has_frame(false);
    scrolled.set_propagate_natural_height(true);
    scrolled.set_min_content_height(MIN_LIST_HEIGHT);
    scrolled.set_max_content_height(MAX_LIST_HEIGHT);
    scrolled.set_child(Some(&list_box));

    (scrolled, list_box)
}

/// Clear the list and repopulate with the given networks.
pub(crate) fn populate_network_list(
    list_box: &ListBox,
    networks: &[Network],
    config: &crate::config::Config,
    pending: &std::collections::HashMap<String, String>,
    on_forget: std::rc::Rc<dyn Fn(String)>,
) -> Vec<Option<String>> {
    use gtk4::prelude::*;

    // Remove all existing rows
    while let Some(row) = list_box.first_child() {
        list_box.remove(&row);
    }

    if networks.is_empty() {
        let empty = Label::new(Some("No networks found"));
        empty.add_css_class("empty-label");
        list_box.append(&empty);
        return Vec::new();
    }

    // Keep one ordering policy at the domain boundary. The backend and this
    // defensive copy both use the same comparator; sectioning below must not
    // replace signal ordering with an unrelated alphabetical sort.
    let mut ordered = networks.to_vec();
    sort_networks(&mut ordered);

    let mut connected: Vec<&Network> = Vec::new();
    let mut saved: Vec<&Network> = Vec::new();
    let mut available: Vec<&Network> = Vec::new();

    for net in &ordered {
        if net.is_connected {
            connected.push(net);
        } else if net.is_saved {
            saved.push(net);
        } else {
            available.push(net);
        }
    }

    let mut row_ssids: Vec<Option<String>> = Vec::new();
    let mut rendered_any = false;

    let append_network = |net: &Network, row_ssids: &mut Vec<Option<String>>| {
        let pending_label = pending.get(&net.ssid).map(String::as_str);
        let on_forget = on_forget.clone();
        let row = network_row::build_network_row(net, config, pending_label, move |ssid| {
            on_forget(ssid);
        });
        list_box.append(&row);
        row_ssids.push(Some(net.ssid.clone()));
    };

    for net in &connected {
        append_network(net, &mut row_ssids);
        rendered_any = true;
    }
    for net in &saved {
        append_network(net, &mut row_ssids);
        rendered_any = true;
    }

    if !available.is_empty() && rendered_any {
        list_box.append(&build_separator_row("Available networks"));
        row_ssids.push(None);
    }

    for net in &available {
        append_network(net, &mut row_ssids);
    }

    row_ssids
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
