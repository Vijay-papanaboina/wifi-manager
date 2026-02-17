//! Header bar widget — toggle switch, status label, scan button, and tab bar.
//!
//! The header now includes a tab bar for switching between Wi-Fi and Bluetooth.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, Switch, ToggleButton};

/// All widgets produced by the header builder.
pub struct HeaderWidgets {
    pub container: GtkBox,
    pub toggle_switch: Switch,
    pub title_label: Label,
    pub status_label: Label,
    pub scan_button: Button,
    pub wifi_tab: ToggleButton,
    pub bt_tab: ToggleButton,
}

/// Build the header containing:
/// - Top row: toggle switch (left) + title/status (center) + scan button (right)
/// - Tab bar: Wi-Fi / Bluetooth toggle buttons
pub fn build_header() -> HeaderWidgets {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.add_css_class("header");

    // ── Top row ──────────────────────────────────────────────────────
    let top_row = GtkBox::new(Orientation::Horizontal, 12);
    top_row.add_css_class("header-top");

    // Toggle switch (controls WiFi or BT power depending on active tab)
    let toggle_switch = Switch::new();
    toggle_switch.set_active(true);
    toggle_switch.add_css_class("wifi-toggle");
    toggle_switch.set_valign(gtk4::Align::Center);
    toggle_switch.set_tooltip_text(Some("Enable/Disable"));

    // Title + Status
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
    scan_button.set_tooltip_text(Some("Scan"));
    scan_button.set_valign(gtk4::Align::Center);

    top_row.append(&toggle_switch);
    top_row.append(&info_box);
    top_row.append(&scan_button);

    // ── Tab bar ──────────────────────────────────────────────────────
    let tab_bar = GtkBox::new(Orientation::Horizontal, 0);
    tab_bar.add_css_class("tab-bar");

    let wifi_tab = ToggleButton::with_label("󰖩  Wi-Fi");
    wifi_tab.add_css_class("tab-button");
    wifi_tab.add_css_class("tab-active");
    wifi_tab.set_active(true);
    wifi_tab.set_hexpand(true);

    let bt_tab = ToggleButton::with_label("󰂯  Bluetooth");
    bt_tab.add_css_class("tab-button");
    bt_tab.set_hexpand(true);

    // Mutual exclusion: clicking one deactivates the other
    wifi_tab.set_group(Some(&bt_tab));

    tab_bar.append(&wifi_tab);
    tab_bar.append(&bt_tab);

    container.append(&top_row);
    container.append(&tab_bar);

    HeaderWidgets {
        container,
        toggle_switch,
        title_label,
        status_label,
        scan_button,
        wifi_tab,
        bt_tab,
    }
}
