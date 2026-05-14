//! Scrollable list of VPN profiles.

use gtk4::prelude::*;
use gtk4::{Label, ListBox, PolicyType, ScrolledWindow, SelectionMode};

use crate::ui::window::{MAX_LIST_HEIGHT, MIN_LIST_HEIGHT};

use super::vpn_row;
use crate::dbus::vpn_manager::{VpnActive, VpnProfile};

pub fn build_vpn_list() -> (ScrolledWindow, ListBox) {
    let list_box = ListBox::new();
    list_box.add_css_class("vpn-list");
    list_box.set_selection_mode(SelectionMode::None);

    let scrolled = ScrolledWindow::new();
    scrolled.add_css_class("vpn-scroll");
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_has_frame(false);
    scrolled.set_propagate_natural_height(true);
    scrolled.set_min_content_height(MIN_LIST_HEIGHT);
    scrolled.set_max_content_height(MAX_LIST_HEIGHT);
    scrolled.set_child(Some(&list_box));

    (scrolled, list_box)
}

pub fn populate_vpn_list(
    list_box: &ListBox,
    profiles: &[VpnProfile],
    active_by_conn: &std::collections::HashMap<String, VpnActive>,
    pending: &std::collections::HashMap<String, String>,
    on_toggle: std::rc::Rc<dyn Fn(String, bool)>,
) -> Vec<String> {
    while let Some(row) = list_box.first_child() {
        list_box.remove(&row);
    }

    if profiles.is_empty() {
        let empty = Label::new(Some("No VPN profiles found"));
        empty.add_css_class("empty-label");
        list_box.append(&empty);
        return Vec::new();
    }

    let mut row_paths = Vec::new();
    for p in profiles {
        let pending_label = pending.get(&p.connection_path).cloned();
        let active = active_by_conn.get(&p.connection_path);
        let row = vpn_row::build_vpn_row(p, active, pending_label.as_deref(), {
            let on_toggle = on_toggle.clone();
            let conn_path = p.connection_path.clone();
            move |enabled| {
                on_toggle(conn_path.clone(), enabled);
            }
        });
        list_box.append(&row);
        row_paths.push(p.connection_path.clone());
    }

    row_paths
}
