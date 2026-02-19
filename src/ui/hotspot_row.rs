//! Hotspot toggle row — sits at the top of the WiFi page.
//!
//! Shows a hotspot icon, label, status text, and a toggle switch.
//! When active, reveals the SSID and password below.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation, Revealer, RevealerTransitionType, Switch};

/// Widgets produced by the hotspot row builder.
pub struct HotspotRowWidgets {
    pub container: GtkBox,
    pub toggle: Switch,
    pub status_label: Label,
    pub detail_revealer: Revealer,
    pub ssid_value: Label,
    pub menu_btn: gtk4::MenuButton,
}

/// Build the hotspot quick-action row.
pub fn build_hotspot_row() -> HotspotRowWidgets {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.add_css_class("hotspot-row");

    // ── Main row: icon + label + status + toggle ─────────────
    let main_row = GtkBox::new(Orientation::Horizontal, 12);
    main_row.add_css_class("hotspot-row-content");
    main_row.set_margin_top(6);
    main_row.set_margin_bottom(6);
    main_row.set_margin_start(12);
    main_row.set_margin_end(12);

    let icon = Label::new(Some("󰑩"));
    icon.add_css_class("hotspot-icon");
    icon.set_valign(gtk4::Align::Center);

    let info = GtkBox::new(Orientation::Vertical, 2);
    info.set_hexpand(true);
    info.set_valign(gtk4::Align::Center);

    let title = Label::new(Some("Hotspot"));
    title.add_css_class("hotspot-title");
    title.set_halign(gtk4::Align::Start);

    let status_label = Label::new(Some("Off"));
    status_label.add_css_class("hotspot-status");
    status_label.set_halign(gtk4::Align::Start);

    info.append(&title);
    info.append(&status_label);

    let toggle = Switch::new();
    toggle.add_css_class("hotspot-toggle");
    toggle.set_valign(gtk4::Align::Center);

    main_row.append(&icon);
    main_row.append(&info);

    // ── Menu button ──────────────────────────────────────────
    use gtk4::{gio, MenuButton, PopoverMenu};

    let menu = gio::Menu::new();
    menu.append(Some("Change Name"), Some("hotspot.change_ssid"));
    menu.append(Some("Change Password"), Some("hotspot.change_password"));

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

    main_row.append(&menu_btn);
    main_row.append(&toggle);

    // ── Detail section (revealed when active) ────────────────
    let detail_revealer = Revealer::new();
    detail_revealer.set_transition_type(RevealerTransitionType::SlideDown);
    detail_revealer.set_transition_duration(200);
    detail_revealer.set_reveal_child(false);

    let detail_box = GtkBox::new(Orientation::Vertical, 4);
    detail_box.add_css_class("hotspot-detail");
    detail_box.set_margin_start(44); // align with text (past icon)
    detail_box.set_margin_end(12);
    detail_box.set_margin_bottom(8);

    let ssid_row = build_detail_line("SSID");
    // Password line removed as requested

    detail_box.append(&ssid_row.0);

    detail_revealer.set_child(Some(&detail_box));

    container.append(&main_row);
    container.append(&detail_revealer);

    HotspotRowWidgets {
        container,
        toggle,
        status_label,
        detail_revealer,
        ssid_value: ssid_row.1,
        menu_btn,
    }
}

/// Helper: build a "Label:  Value" detail line.
fn build_detail_line(label_text: &str) -> (GtkBox, Label) {
    let row = GtkBox::new(Orientation::Horizontal, 8);

    let label = Label::new(Some(label_text));
    label.add_css_class("hotspot-detail-label");
    label.set_halign(gtk4::Align::Start);

    let value = Label::new(Some("—"));
    value.add_css_class("hotspot-detail-value");
    value.set_halign(gtk4::Align::Start);
    value.set_hexpand(true);
    value.set_selectable(true);

    row.append(&label);
    row.append(&value);
    (row, value)
}
