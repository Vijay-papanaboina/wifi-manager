//! Single VPN profile row widget — shows name, status, and a connect toggle.

use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, ListBoxRow, Orientation, Switch};

use crate::dbus::vpn_manager::{VpnActive, VpnProfile};

/// Build a `ListBoxRow` for a VPN profile.
///
/// Layout: [Name / Subtitle] [toggle]
pub fn build_vpn_row(
    profile: &VpnProfile,
    active: Option<&VpnActive>,
    pending_label: Option<&str>,
    on_toggle: impl Fn(bool) + 'static,
    on_edit: impl Fn() + 'static,
    on_delete: impl Fn() + 'static,
) -> ListBoxRow {
    let row = ListBoxRow::new();
    row.add_css_class("vpn-row");

    let is_active = active.map(|a| a.state == 2).unwrap_or(false);
    if is_active {
        row.add_css_class("connected");
    }
    if pending_label.is_some()
        || active
            .map(|a| a.state == 1 || a.state == 3)
            .unwrap_or(false)
    {
        row.add_css_class("pending");
    }

    let hbox = GtkBox::new(Orientation::Horizontal, 12);
    hbox.add_css_class("vpn-row-content");
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);

    let info_vbox = GtkBox::new(Orientation::Vertical, 2);
    info_vbox.add_css_class("vpn-row-info");
    info_vbox.set_hexpand(true);
    info_vbox.set_valign(gtk4::Align::Center);

    let name_label = Label::new(Some(&profile.name));
    name_label.add_css_class("vpn-name");
    name_label.set_halign(gtk4::Align::Start);
    name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    let subtitle_text = vpn_subtitle(active, pending_label);
    let subtitle_label = Label::new(Some(&subtitle_text));
    subtitle_label.add_css_class("vpn-subtitle");
    if pending_label.is_some() {
        subtitle_label.add_css_class("vpn-pending");
    }
    subtitle_label.set_halign(gtk4::Align::Start);
    subtitle_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    info_vbox.append(&name_label);
    info_vbox.append(&subtitle_label);

    hbox.append(&info_vbox);

    let toggle = Switch::new();
    toggle.add_css_class("vpn-toggle");
    toggle.set_active(is_active);
    toggle.set_valign(gtk4::Align::Center);
    toggle.set_halign(gtk4::Align::End);
    toggle.set_sensitive(pending_label.is_none());
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        toggle.set_cursor(Some(&cursor));
    }

    toggle.connect_state_set(move |_sw, state| {
        on_toggle(state);
        Propagation::Proceed
    });

    hbox.append(&toggle);

    use gtk4::{gio, MenuButton, PopoverMenu};

    let menu = gio::Menu::new();
    menu.append(Some("Edit Profile"), Some("row.edit"));
    menu.append(Some("Delete Profile"), Some("row.delete"));

    let popover = PopoverMenu::from_model(Some(&menu));
    popover.add_css_class("vpn-popover");

    let menu_btn = MenuButton::new();
    menu_btn.set_icon_name("view-more-symbolic");
    menu_btn.add_css_class("vpn-menu-btn");
    menu_btn.add_css_class("flat");
    menu_btn.set_has_frame(false);
    menu_btn.set_direction(gtk4::ArrowType::None);
    menu_btn.set_popover(Some(&popover));
    menu_btn.set_halign(gtk4::Align::End);
    menu_btn.set_valign(gtk4::Align::Center);
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        menu_btn.set_cursor(Some(&cursor));
    }

    let edit_action = gio::SimpleAction::new("edit", None);
    edit_action.connect_activate(move |_, _| on_edit());
    let delete_action = gio::SimpleAction::new("delete", None);
    delete_action.connect_activate(move |_, _| on_delete());
    let action_group = gio::SimpleActionGroup::new();
    action_group.add_action(&edit_action);
    action_group.add_action(&delete_action);
    row.insert_action_group("row", Some(&action_group));

    hbox.append(&menu_btn);
    row.set_child(Some(&hbox));
    row
}

fn vpn_subtitle(active: Option<&VpnActive>, pending: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(a) = active {
        let state = match a.state {
            1 => "Connecting",
            2 => "Connected",
            3 => "Disconnecting",
            4 => "Disconnected",
            _ => "Unknown",
        };
        parts.push(state.to_string());
    }

    if let Some(p) = pending {
        parts.push(p.to_string());
    }

    if parts.is_empty() {
        "Disconnected".to_string()
    } else {
        parts.join(" · ")
    }
}
