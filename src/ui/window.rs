//! Main floating panel window with layer-shell support.
//!
//! Composes the header, network list, and password dialog into the panel,
//! loads the CSS theme, and sets up layer-shell positioning.

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, CssProvider, ListBox, Orientation, gdk};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::{header, network_list, password_dialog};
use crate::config::{Config, Position};

/// All the UI handles needed by the app controller.
pub struct PanelWidgets {
    pub window: ApplicationWindow,
    pub wifi_switch: gtk4::Switch,
    pub status_label: gtk4::Label,
    pub scan_button: gtk4::Button,
    pub network_list_box: ListBox,
    pub network_scroll: gtk4::ScrolledWindow,
    pub spinner: gtk4::Spinner,
    pub password_revealer: gtk4::Revealer,
    pub password_entry: gtk4::Entry,
    pub connect_button: gtk4::Button,
    pub cancel_button: gtk4::Button,
    pub error_label: gtk4::Label,
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
    let (header_box, wifi_switch, status_label, scan_button) = header::build_header();
    main_box.append(&header_box);

    // Separator
    let sep = gtk4::Separator::new(Orientation::Horizontal);
    sep.add_css_class("header-separator");
    main_box.append(&sep);

    // Network list
    let (scrolled, list_box) = network_list::build_network_list();

    // Loading spinner (shown while scanning)
    let spinner = gtk4::Spinner::new();
    spinner.set_spinning(true);
    spinner.add_css_class("loading-spinner");
    spinner.set_size_request(32, 32);
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_valign(gtk4::Align::Center);
    spinner.set_margin_top(20);
    spinner.set_margin_bottom(20);

    // Stack to switch between spinner and list
    main_box.append(&spinner);
    main_box.append(&scrolled);
    scrolled.set_visible(false); // Hide list until loaded

    // Password entry section (hidden by default)
    let (revealer, entry, connect_btn, cancel_btn, error_label) =
        password_dialog::build_password_section();
    main_box.append(&revealer);

    window.set_child(Some(&main_box));

    // Load CSS theme
    load_css();

    // Window starts hidden — daemon controls visibility via Toggle/Show
    // window.present() is NOT called here; the daemon will show it on demand.
    log::info!("Layer-shell panel built (hidden)");

    PanelWidgets {
        window,
        wifi_switch,
        status_label,
        scan_button,
        network_list_box: list_box,
        network_scroll: scrolled,
        spinner,
        password_revealer: revealer,
        password_entry: entry,
        connect_button: connect_btn,
        cancel_button: cancel_btn,
        error_label,
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
    log::info!("Default CSS theme loaded");

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
