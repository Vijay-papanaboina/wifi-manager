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

    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.add_css_class("network-row-content");
    hbox.set_margin_top(2);
    hbox.set_margin_bottom(2);

    // Signal strength icon
    let (icon_text, signal_class) = signal_icon(network.strength, &config.signal_icons);
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

    // Menu button (only for saved networks)
    if network.is_saved || network.is_connected {
        use gtk4::{MenuButton, PopoverMenu, gio};
        
        let menu = gio::Menu::new();
        menu.append(Some("Forget"), Some("row.forget"));
        
        let popover = PopoverMenu::from_model(Some(&menu));
        popover.add_css_class("network-popover");

        let menu_btn = MenuButton::new();
        menu_btn.set_label("⋮");
        menu_btn.add_css_class("network-menu-btn");
        menu_btn.add_css_class("flat");  // Remove button background
        menu_btn.set_has_frame(false);   // Remove button frame
        menu_btn.set_direction(gtk4::ArrowType::None);  // Remove arrow
        menu_btn.set_popover(Some(&popover));
        menu_btn.set_halign(gtk4::Align::End);
        
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
