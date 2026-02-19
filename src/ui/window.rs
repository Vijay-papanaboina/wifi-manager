//! Main floating panel window with layer-shell support.
//!
//! Composes the header, network list, Bluetooth device list, and password
//! dialog into the panel. Uses a GtkStack to switch between Wi-Fi and
//! Bluetooth views based on the header tab selection.

use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, CssProvider, ListBox, Orientation, Stack,
    StackTransitionType, gdk,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::{device_list, header, hotspot_row, network_list, password_dialog};
use crate::config::{Config, Position};

/// All the UI handles needed by the app controller.
pub struct PanelWidgets {
    pub window: ApplicationWindow,
    pub wifi_switch: gtk4::Switch,
    pub title_label: gtk4::Label,
    pub status_label: gtk4::Label,
    pub scan_button: gtk4::Button,
    pub wifi_tab: gtk4::ToggleButton,
    pub bt_tab: gtk4::ToggleButton,
    // Wi-Fi page
    pub network_list_box: ListBox,
    pub network_scroll: gtk4::ScrolledWindow,
    pub spinner: gtk4::Spinner,
    pub password_revealer: gtk4::Revealer,
    pub password_entry: gtk4::Entry,
    pub connect_button: gtk4::Button,
    pub cancel_button: gtk4::Button,
    pub error_label: gtk4::Label,
    pub password_title: gtk4::Label,
    // Hotspot row
    pub hotspot_toggle: gtk4::Switch,
    pub hotspot_status: gtk4::Label,
    pub hotspot_revealer: gtk4::Revealer,
    pub hotspot_ssid: gtk4::Label,
    pub hotspot_container: gtk4::Box,
    pub hotspot_menu_btn: gtk4::MenuButton,
    // Bluetooth page
    pub bt_list_box: ListBox,
    pub bt_scroll: gtk4::ScrolledWindow,
    pub bt_spinner: gtk4::Spinner,
    // Content stack
    pub content_stack: Stack,
}

/// Build the main floating panel window with all UI components.
pub fn build_window(app: &Application) -> PanelWidgets {
    let config = Config::load();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("WiFi Manager")
        .default_width(340)
        .default_height(400)
        .build();

    // Initialize layer shell
    window.init_layer_shell();
    window.set_namespace(Some("wifi-manager"));
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    // Apply position from config
    apply_position(&window, &config);

    // Don't push other windows
    window.set_exclusive_zone(-1);

    // Main container
    let main_box = GtkBox::new(Orientation::Vertical, 0);
    main_box.add_css_class("wifi-panel");

    // Header
    let header = header::build_header();
    main_box.append(&header.container);

    // Separator
    let sep = gtk4::Separator::new(Orientation::Horizontal);
    sep.add_css_class("header-separator");
    main_box.append(&sep);

    // ── Content Stack (switches between Wi-Fi and Bluetooth pages) ──
    let content_stack = Stack::new();
    content_stack.set_transition_type(StackTransitionType::Crossfade);
    content_stack.set_transition_duration(150);
    content_stack.add_css_class("content-stack");

    // ── Wi-Fi page ──────────────────────────────────────────────────
    let wifi_page = GtkBox::new(Orientation::Vertical, 0);

    let (scrolled, list_box) = network_list::build_network_list();

    let spinner = gtk4::Spinner::new();
    spinner.set_spinning(true);
    spinner.add_css_class("loading-spinner");
    spinner.set_size_request(32, 32);
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_valign(gtk4::Align::Center);
    spinner.set_margin_top(20);
    spinner.set_margin_bottom(20);

    let hotspot = hotspot_row::build_hotspot_row();
    wifi_page.append(&hotspot.container);

    wifi_page.append(&spinner);
    wifi_page.append(&scrolled);
    scrolled.set_visible(false);

    let (revealer, entry, connect_btn, cancel_btn, error_label, password_title) =
        password_dialog::build_password_section();
    wifi_page.append(&revealer);

    content_stack.add_named(&wifi_page, Some("wifi"));

    // ── Bluetooth page ─────────────────────────────────────────────
    let bt_page = GtkBox::new(Orientation::Vertical, 0);

    let (bt_scrolled, bt_list_box) = device_list::build_device_list();

    let bt_spinner = gtk4::Spinner::new();
    bt_spinner.set_spinning(true);
    bt_spinner.add_css_class("loading-spinner");
    bt_spinner.set_size_request(32, 32);
    bt_spinner.set_halign(gtk4::Align::Center);
    bt_spinner.set_valign(gtk4::Align::Center);
    bt_spinner.set_margin_top(20);
    bt_spinner.set_margin_bottom(20);

    bt_page.append(&bt_spinner);
    bt_page.append(&bt_scrolled);
    bt_scrolled.set_visible(false);

    content_stack.add_named(&bt_page, Some("bluetooth"));

    // Start on Wi-Fi page
    content_stack.set_visible_child_name("wifi");
    main_box.append(&content_stack);

    // ── Tab switching — only manages content stack page ──────────────
    // Title, status, and switch sync is handled by app controllers
    // which can do async D-Bus calls to query actual power state.
    {
        let stack = content_stack.clone();
        header.wifi_tab.connect_toggled(move |btn| {
            if btn.is_active() {
                stack.set_visible_child_name("wifi");
            }
        });
    }
    {
        let stack = content_stack.clone();
        header.bt_tab.connect_toggled(move |btn| {
            if btn.is_active() {
                stack.set_visible_child_name("bluetooth");
            }
        });
    }

    window.set_child(Some(&main_box));

    // Load CSS theme
    load_css();

    log::info!("Layer-shell panel built (hidden)");

    PanelWidgets {
        window,
        wifi_switch: header.toggle_switch,
        title_label: header.title_label,
        status_label: header.status_label,
        scan_button: header.scan_button,
        wifi_tab: header.wifi_tab,
        bt_tab: header.bt_tab,
        network_list_box: list_box,
        network_scroll: scrolled,
        spinner,
        password_revealer: revealer,
        password_entry: entry,
        connect_button: connect_btn,
        cancel_button: cancel_btn,
        error_label,
        password_title,
        // Hotspot
        hotspot_toggle: hotspot.toggle,
        hotspot_status: hotspot.status_label,
        hotspot_revealer: hotspot.detail_revealer,
        hotspot_ssid: hotspot.ssid_value,
        hotspot_container: hotspot.container,
        hotspot_menu_btn: hotspot.menu_btn,
        bt_list_box,
        bt_scroll: bt_scrolled,
        bt_spinner,
        content_stack,
    }
}

/// Load the default CSS theme and optional user overrides.
fn load_css() {
    let display = gdk::Display::default().expect("Could not get default display");

    // Load bundled default theme
    let default_css = include_str!("../../resources/style.css");
    let provider = CssProvider::new();
    provider.load_from_string(default_css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    // Load hotspot theme (modular)
    let hotspot_css = include_str!("../../resources/hotspot.css");
    let hotspot_provider = CssProvider::new();
    hotspot_provider.load_from_string(hotspot_css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &hotspot_provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    log::info!("Default CSS themes loaded");

    // Load optional user theme override
    if let Some(config_dir) = dirs_config_path() {
        let user_css_path = config_dir.join("style.css");
        if user_css_path.exists() {
            let user_provider = CssProvider::new();
            user_provider.load_from_path(user_css_path.to_str().unwrap_or_default());
            gtk4::style_context_add_provider_for_display(
                &display,
                &user_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_USER,
            );
            log::info!("User CSS theme loaded from {:?}", user_css_path);
        }
    }
}

/// Reload user CSS (for --reload flag).
pub fn reload_css() {
    let display = gdk::Display::default().expect("Could not get default display");

    // Reload optional user theme override
    if let Some(config_dir) = dirs_config_path() {
        let user_css_path = config_dir.join("style.css");
        if user_css_path.exists() {
            let user_provider = CssProvider::new();
            user_provider.load_from_path(user_css_path.to_str().unwrap_or_default());
            gtk4::style_context_add_provider_for_display(
                &display,
                &user_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_USER,
            );
            log::info!("User CSS reloaded from {:?}", user_css_path);
        }
    }
}

/// Get the config directory: ~/.config/wifi-manager/
fn dirs_config_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        std::path::PathBuf::from(home)
            .join(".config")
            .join("wifi-manager"),
    )
}

/// Apply window position and margins from config to a layer-shell window.
fn apply_position(window: &ApplicationWindow, config: &Config) {
    // Set anchors based on position
    let (top, bottom, left, right) = match config.position {
        Position::Center => (false, false, false, false),
        Position::TopCenter => (true, false, false, false),
        Position::TopRight => (true, false, false, true),
        Position::TopLeft => (true, false, true, false),
        Position::BottomCenter => (false, true, false, false),
        Position::BottomRight => (false, true, false, true),
        Position::BottomLeft => (false, true, true, false),
        Position::CenterRight => (false, false, false, true),
        Position::CenterLeft => (false, false, true, false),
    };

    window.set_anchor(Edge::Top, top);
    window.set_anchor(Edge::Bottom, bottom);
    window.set_anchor(Edge::Left, left);
    window.set_anchor(Edge::Right, right);

    // Apply margins
    window.set_margin(Edge::Top, config.margin_top);
    window.set_margin(Edge::Right, config.margin_right);
    window.set_margin(Edge::Bottom, config.margin_bottom);
    window.set_margin(Edge::Left, config.margin_left);

    log::info!("Window position: {:?}, margins: t={} r={} b={} l={}",
        config.position, config.margin_top, config.margin_right,
        config.margin_bottom, config.margin_left);
}
