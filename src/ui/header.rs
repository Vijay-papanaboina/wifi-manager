//! Shared shell header and compatibility feature controls.
//!
//! The visible navigation lives in the Control Center stack.  The Wi-Fi and
//! Bluetooth toggle buttons remain in a hidden compatibility box because the
//! existing application controllers use them as their feature-activation
//! signals.  The status label is likewise retained as a non-rendered bridge;
//! page-local status labels are owned by `window.rs` and updated by the app
//! controller.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, Switch, ToggleButton};

/// All widgets produced by the header builder.
pub(crate) struct HeaderWidgets {
    pub container: GtkBox,
    pub toggle_switch: Switch,
    pub title_label: Label,
    /// Compatibility status sink used by existing async controllers.
    pub status_label: Label,
    pub scan_button: Button,
    /// Hidden feature selectors retained for existing controller wiring.
    pub wifi_tab: ToggleButton,
    pub bt_tab: ToggleButton,
}

/// Build the shell header containing the global title and feature controls.
///
/// The switch and scan button are shown only on Wi-Fi/Bluetooth detail pages
/// by the window navigation helper.  The old feature tabs and status sink are
/// kept out of the visible layout so asynchronous Wi-Fi and Bluetooth work
/// cannot overwrite a shared rendered status label.
pub(crate) fn build_header() -> HeaderWidgets {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.add_css_class("header");

    let top_row = GtkBox::new(Orientation::Horizontal, 12);
    top_row.add_css_class("header-top");

    let toggle_switch = Switch::new();
    toggle_switch.set_active(true);
    toggle_switch.add_css_class("wifi-toggle");
    toggle_switch.set_valign(gtk4::Align::Center);
    toggle_switch.set_tooltip_text(Some("Enable or disable the active feature"));
    toggle_switch.set_visible(false);
    set_pointer_cursor(&toggle_switch);

    let info_box = GtkBox::new(Orientation::Vertical, 2);
    info_box.add_css_class("header-info");
    info_box.set_hexpand(true);

    let title_label = Label::new(Some("Control Center"));
    title_label.add_css_class("header-title");
    title_label.set_halign(gtk4::Align::Start);

    // This label is a compatibility sink for the existing controller APIs.
    // It is intentionally not appended to the visible info box.
    let status_label = Label::new(Some("Loading…"));
    status_label.add_css_class("compat-status-label");
    status_label.set_visible(false);

    info_box.append(&title_label);

    let scan_button = Button::from_icon_name("view-refresh-symbolic");
    scan_button.add_css_class("scan-button");
    scan_button.set_tooltip_text(Some("Scan"));
    scan_button.set_valign(gtk4::Align::Center);
    scan_button.set_visible(false);
    set_pointer_cursor(&scan_button);

    top_row.append(&toggle_switch);
    top_row.append(&info_box);
    top_row.append(&scan_button);
    container.append(&top_row);

    // Keep the legacy tabs and status sink alive without presenting them as a
    // second navigation system.  Their active state is the source of truth
    // for existing Wi-Fi/Bluetooth async controller guards.
    let compatibility_box = GtkBox::new(Orientation::Vertical, 0);
    compatibility_box.add_css_class("cc-compatibility-controls");
    compatibility_box.set_visible(false);

    let wifi_tab = ToggleButton::with_label("Wi-Fi");
    wifi_tab.add_css_class("tab-button");
    wifi_tab.set_active(false);
    let bt_tab = ToggleButton::with_label("Bluetooth");
    bt_tab.add_css_class("tab-button");
    bt_tab.set_active(false);
    wifi_tab.set_group(Some(&bt_tab));
    compatibility_box.append(&wifi_tab);
    compatibility_box.append(&bt_tab);
    compatibility_box.append(&status_label);
    container.append(&compatibility_box);

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

fn set_pointer_cursor<W: IsA<gtk4::Widget>>(widget: &W) {
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        widget.set_cursor(Some(&cursor));
    }
}
