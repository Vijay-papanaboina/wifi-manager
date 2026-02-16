//! Header bar widget — WiFi toggle switch, status label, and scan button.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, Switch};

/// Build the header bar containing:
/// - WiFi toggle switch (left)
/// - Status label (center, expands)
/// - Scan/refresh button (right)
///
/// Returns `(header_box, wifi_switch, status_label, scan_button)`.
pub fn build_header() -> (GtkBox, Switch, Label, Button) {
    let header = GtkBox::new(Orientation::Horizontal, 12);
    header.add_css_class("header");

    // WiFi toggle switch
    let wifi_switch = Switch::new();
    wifi_switch.set_active(true);
    wifi_switch.add_css_class("wifi-toggle");
    wifi_switch.set_valign(gtk4::Align::Center);
    wifi_switch.set_tooltip_text(Some("Enable/Disable WiFi"));

    // Header info (Title + Status)
    let info_box = GtkBox::new(Orientation::Vertical, 2);
    info_box.add_css_class("header-info");
    info_box.set_hexpand(true);

    let title_label = Label::new(Some("Wi-Fi"));
    title_label.add_css_class("header-title");
    title_label.set_halign(gtk4::Align::Start);

    let status_label = Label::new(Some("Checking status..."));
    status_label.add_css_class("status-label");
    status_label.set_halign(gtk4::Align::Start);
    status_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    info_box.append(&title_label);
    info_box.append(&status_label);

    // Scan button
    let scan_button = Button::from_icon_name("view-refresh-symbolic");
    scan_button.add_css_class("scan-button");
    scan_button.set_tooltip_text(Some("Scan for networks"));
    scan_button.set_valign(gtk4::Align::Center);

    header.append(&wifi_switch);
    header.append(&info_box);
    header.append(&scan_button);

    (header, wifi_switch, status_label, scan_button)
}
