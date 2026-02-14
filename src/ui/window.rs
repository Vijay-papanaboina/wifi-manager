//! Main floating panel window with layer-shell support.
//!
//! Composes the header, network list, and password dialog into the panel,
//! loads the CSS theme, and sets up layer-shell positioning.

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, CssProvider, ListBox, Orientation, gdk};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::{header, network_list, password_dialog};

/// All the UI handles needed by the app controller.
pub struct PanelWidgets {
    pub window: ApplicationWindow,
    pub wifi_switch: gtk4::Switch,
    pub status_label: gtk4::Label,
    pub scan_button: gtk4::Button,
    pub network_list_box: ListBox,
    pub password_revealer: gtk4::Revealer,
    pub password_entry: gtk4::Entry,
    pub connect_button: gtk4::Button,
    pub cancel_button: gtk4::Button,
    pub error_label: gtk4::Label,
}

/// Build the main floating panel window with all UI components.
pub fn build_window(app: &Application) -> PanelWidgets {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("WiFi Manager")
        .default_width(380)
        .default_height(400)
        .build();

    // Initialize layer shell
    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    // Anchor to top-right with margin
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_margin(Edge::Top, 10);
    window.set_margin(Edge::Right, 10);

    // Don't push other windows
    window.set_exclusive_zone(-1);

    // Main container
    let main_box = GtkBox::new(Orientation::Vertical, 0);
    main_box.add_css_class("wifi-panel");

    // Header
    let (header_box, wifi_switch, status_label, scan_button) = header::build_header();
    main_box.append(&header_box);

    // Network list
    let (scrolled, list_box) = network_list::build_network_list();
    main_box.append(&scrolled);

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

/// Get the config directory: ~/.config/wifi-manager/
fn dirs_config_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        std::path::PathBuf::from(home)
            .join(".config")
            .join("wifi-manager"),
    )
}
