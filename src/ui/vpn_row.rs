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
