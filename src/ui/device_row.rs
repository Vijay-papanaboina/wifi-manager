//! Single Bluetooth device row widget — shows device icon, name, status, and actions.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, ListBoxRow, Orientation};

use crate::dbus::bluetooth_device::BluetoothDevice;

/// Create a ListBoxRow that represents a Bluetooth device.
///
/// The row displays the device category icon, display name, a status subtitle
/// (e.g., "Connected" or "Paired"), and optional indicators and actions:
/// - a nearby dot when the device is in range and not connected,
/// - a trusted icon when the device is trusted and not connected,
/// - a menu button (with an "Unpair" action) when the device is paired or connected.
/// The provided `on_remove` callback is invoked with the device path when the Unpair action is activated.
/// The provided `on_menu_active` callback is called with the menu button's active state when it changes.
///
/// # Examples
///
/// ```
/// // Assume `device` is a BluetoothDevice instance available in this scope.
/// let row = build_device_row(&device, |_device_path| {}, |_active| {});
/// assert!(row.child().is_some());
/// ```
pub fn build_device_row(
    device: &BluetoothDevice,
    on_remove: impl Fn(String) + 'static,
    on_menu_active: impl Fn(bool) + 'static,
) -> ListBoxRow {
    let row = ListBoxRow::new();
    row.add_css_class("device-row");

    if device.connected {
        row.add_css_class("connected");
    } else if device.paired {
        row.add_css_class("paired");
    }

    let hbox = GtkBox::new(Orientation::Horizontal, 12);
    hbox.add_css_class("device-row-content");
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);

    // Device category icon
    let icon_text = device.category.default_icon();
    let icon_label = Label::new(Some(icon_text));
    icon_label.add_css_class("device-icon");
    icon_label.set_valign(gtk4::Align::Center);

    // Info VBox (Name + Subtitle)
    let info_vbox = GtkBox::new(Orientation::Vertical, 2);
    info_vbox.add_css_class("device-row-info");
    info_vbox.set_hexpand(true);
    info_vbox.set_valign(gtk4::Align::Center);

    // Device name row (name + nearby indicator)
    let name_row = GtkBox::new(Orientation::Horizontal, 6);
    name_row.set_halign(gtk4::Align::Start);
    name_row.set_hexpand(true);

    let name_label = Label::new(Some(&device.display_name));
    name_label.add_css_class("device-name");
    name_label.set_halign(gtk4::Align::Start);
    name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    name_row.append(&name_label);

    if device.is_in_range() && !device.connected {
        let nearby = Label::new(Some("•"));
        nearby.add_css_class("device-nearby");
        nearby.set_valign(gtk4::Align::Center);
        nearby.set_tooltip_text(Some("Nearby"));
        name_row.append(&nearby);
    }

    // Subtitle line (status)
    let subtitle_text = device_subtitle(device);
    let subtitle_label = Label::new(Some(&subtitle_text));
    subtitle_label.add_css_class("device-subtitle");
    subtitle_label.set_halign(gtk4::Align::Start);
    subtitle_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    info_vbox.append(&name_row);
    info_vbox.append(&subtitle_label);

    hbox.append(&icon_label);
    hbox.append(&info_vbox);

    // Trusted icon (if trusted but not connected)
    if device.trusted && !device.connected {
        let trusted_label = Label::new(Some("󰄬"));
        trusted_label.add_css_class("trusted-icon");
        trusted_label.set_valign(gtk4::Align::Center);
        trusted_label.set_tooltip_text(Some("Trusted"));
        hbox.append(&trusted_label);
    }

    // Menu button (for paired or connected devices)
    if device.paired || device.connected {
        use gtk4::{gio, MenuButton, PopoverMenu};

        let menu = gio::Menu::new();
        menu.append(Some("Unpair"), Some("row.remove"));

        let popover = PopoverMenu::from_model(Some(&menu));
        popover.add_css_class("device-popover");

        let menu_btn = MenuButton::new();
        menu_btn.set_icon_name("view-more-symbolic");
        menu_btn.add_css_class("device-menu-btn");
        menu_btn.add_css_class("flat");
        menu_btn.set_has_frame(false);
        menu_btn.set_direction(gtk4::ArrowType::None);
        menu_btn.set_popover(Some(&popover));
        menu_btn.set_halign(gtk4::Align::End);
        menu_btn.set_valign(gtk4::Align::Center);
        menu_btn.connect_active_notify(move |btn| {
            on_menu_active(btn.is_active());
        });

        let action = gio::SimpleAction::new("remove", None);
        let device_path = device.device_path.clone();
        action.connect_activate(move |_, _| {
            on_remove(device_path.clone());
        });

        let action_group = gio::SimpleActionGroup::new();
        action_group.add_action(&action);
        row.insert_action_group("row", Some(&action_group));

        hbox.append(&menu_btn);
    }

    row.set_child(Some(&hbox));
    row
}

/// Build the subtitle text for a Bluetooth device.
fn device_subtitle(device: &BluetoothDevice) -> String {
    let mut parts = Vec::new();

    parts.push(device.category.to_string());

    if device.connected {
        parts.push("Connected".to_string());
    } else if device.paired {
        parts.push("Paired".to_string());
    }

    parts.join(" · ")
}
