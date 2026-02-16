//! Single network row widget — shows SSID, signal, security, band, and state.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, ListBoxRow, Orientation};

use crate::dbus::access_point::{Band, Network, SecurityType};

/// Signal strength thresholds for icon selection.
fn signal_icon<'a>(strength: u8, icons: &'a [String; 4]) -> (&'a str, &'static str) {
    let icon = match strength {
        75..=100 => &icons[3],  // strong
        50..=74 => &icons[2],   // good
        25..=49 => &icons[1],   // fair
        _ => &icons[0],         // weak
    };
    let class = match strength {
        75..=100 => "signal-strong",
        50..=74 => "signal-good",
        25..=49 => "signal-fair",
        _ => "signal-weak",
    };
    (icon.as_str(), class)
}

/// Build a `ListBoxRow` for a single network.
///
/// The row displays: signal bars | [SSID / Subtitle] | menu.
pub fn build_network_row(
    network: &Network,
    config: &crate::config::Config,
    on_forget: impl Fn(String) + 'static,
) -> ListBoxRow {
    let row = ListBoxRow::new();
    row.add_css_class("network-row");

    if network.is_connected {
        row.add_css_class("connected");
    } else if network.is_saved {
        row.add_css_class("saved");
    }

    let hbox = GtkBox::new(Orientation::Horizontal, 12);
    hbox.add_css_class("network-row-content");
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);

    // Signal strength icon
    let (icon_text, signal_class) = signal_icon(network.strength, &config.signal_icons);
    let signal_label = Label::new(Some(icon_text));
    signal_label.add_css_class("signal-icon");
    signal_label.add_css_class(signal_class);
    signal_label.set_valign(gtk4::Align::Center);

    // Info VBox (SSID + Subtitle)
    let info_vbox = GtkBox::new(Orientation::Vertical, 2);
    info_vbox.add_css_class("network-row-info");
    info_vbox.set_hexpand(true);
    info_vbox.set_valign(gtk4::Align::Center);

    // SSID name
    let ssid_label = Label::new(Some(&network.ssid));
    ssid_label.add_css_class("ssid-label");
    ssid_label.set_halign(gtk4::Align::Start);
    ssid_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    // Subtitle line (Band · Connectivity)
    let mut subtitle_parts = Vec::new();
    
    if network.band == Band::FiveGhz {
        subtitle_parts.push("5G".to_string());
    }

    if network.is_connected {
        subtitle_parts.push("Connected".to_string());
    }

    let subtitle_text = subtitle_parts.join(" · ");
    let subtitle_label = Label::new(Some(&subtitle_text));
    subtitle_label.add_css_class("network-subtitle");
    subtitle_label.set_halign(gtk4::Align::Start);
    subtitle_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    info_vbox.append(&ssid_label);
    info_vbox.append(&subtitle_label);

    hbox.append(&signal_label);
    hbox.append(&info_vbox);

    // Lock icon (if secured)
    if network.security != SecurityType::Open {
        let lock_label = Label::new(Some(&config.lock_icon));
        lock_label.add_css_class("security-icon");
        lock_label.set_valign(gtk4::Align::Center);
        hbox.append(&lock_label);
    }

    // Saved icon
    if network.is_saved && !network.is_connected {
        let saved_label = Label::new(Some(&config.saved_icon));
        saved_label.add_css_class("saved-icon");
        saved_label.set_valign(gtk4::Align::Center);
        hbox.append(&saved_label);
    }

    // Connected checkmark (optional, but user might want it replaced by something too)
    // Removed because we have "Connected" in subtitle and maybe user wants it cleaner.

    // Menu button (only for saved networks)
    if network.is_saved || network.is_connected {
        use gtk4::{gio, MenuButton, PopoverMenu};

        let menu = gio::Menu::new();
        menu.append(Some("Forget"), Some("row.forget"));

        let popover = PopoverMenu::from_model(Some(&menu));
        popover.add_css_class("network-popover");

        let menu_btn = MenuButton::new();
        menu_btn.set_icon_name("view-more-symbolic");
        menu_btn.add_css_class("network-menu-btn");
        menu_btn.add_css_class("flat");
        menu_btn.set_has_frame(false);
        menu_btn.set_direction(gtk4::ArrowType::None);
        menu_btn.set_popover(Some(&popover));
        menu_btn.set_halign(gtk4::Align::End);
        menu_btn.set_valign(gtk4::Align::Center);

        // Add action to the row
        let action = gio::SimpleAction::new("forget", None);
        let ssid = network.ssid.clone();
        action.connect_activate(move |_, _| {
            on_forget(ssid.clone());
        });

        let action_group = gio::SimpleActionGroup::new();
        action_group.add_action(&action);
        row.insert_action_group("row", Some(&action_group));

        hbox.append(&menu_btn);
    }

    row.set_child(Some(&hbox));
    row
}
