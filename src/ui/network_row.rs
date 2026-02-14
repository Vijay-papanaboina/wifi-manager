//! Single network row widget — shows SSID, signal, security, band, and state.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, ListBoxRow, Orientation};

use crate::dbus::access_point::{Band, Network, SecurityType};

/// Signal strength thresholds for icon selection.
fn signal_icon(strength: u8) -> (&'static str, &'static str) {
    match strength {
        75..=100 => ("▂▄▆█", "signal-strong"),
        50..=74 => ("▂▄▆_", "signal-good"),
        25..=49 => ("▂▄__", "signal-fair"),
        _ => ("▂___", "signal-weak"),
    }
}

/// Security display icon.
fn security_icon(security: &SecurityType) -> &'static str {
    match security {
        SecurityType::Open => "🔓",
        _ => "🔒",
    }
}

/// Build a `ListBoxRow` for a single network.
///
/// The row displays: signal bars | SSID | band badge | security icon | status.
pub fn build_network_row(network: &Network) -> ListBoxRow {
    let row = ListBoxRow::new();
    row.add_css_class("network-row");

    if network.is_connected {
        row.add_css_class("connected");
    } else if network.is_saved {
        row.add_css_class("saved");
    }

    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.set_margin_top(2);
    hbox.set_margin_bottom(2);

    // Signal strength icon
    let (icon_text, signal_class) = signal_icon(network.strength);
    let signal_label = Label::new(Some(icon_text));
    signal_label.add_css_class("signal-icon");
    signal_label.add_css_class(signal_class);
    signal_label.set_width_chars(4);

    // SSID name
    let ssid_label = Label::new(Some(&network.ssid));
    ssid_label.add_css_class("ssid-label");
    ssid_label.set_hexpand(true);
    ssid_label.set_halign(gtk4::Align::Start);
    ssid_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    ssid_label.set_max_width_chars(24);

    hbox.append(&signal_label);
    hbox.append(&ssid_label);

    // Band badge (only show for 5 GHz)
    if network.band == Band::FiveGhz {
        let band_label = Label::new(Some("5G"));
        band_label.add_css_class("band-badge");
        hbox.append(&band_label);
    }

    // Security icon
    let sec_label = Label::new(Some(security_icon(&network.security)));
    sec_label.add_css_class("security-icon");
    hbox.append(&sec_label);

    // Connected checkmark or Saved label
    if network.is_connected {
        let check = Label::new(Some("✓"));
        check.add_css_class("connected-icon");
        hbox.append(&check);
    } else if network.is_saved {
        let saved = Label::new(Some("saved"));
        saved.add_css_class("saved-label");
        hbox.append(&saved);
    }

    row.set_child(Some(&hbox));
    row
}
