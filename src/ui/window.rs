//! Main floating panel window with layer-shell support.
//!
//! Composes the header, network list, Bluetooth device list, and password
//! dialog into the panel. Uses a GtkStack to switch between Wi-Fi and
//! Bluetooth views based on the header tab selection.

use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, CssProvider, ListBox, Orientation, Stack,
    StackTransitionType, ToggleButton, gdk,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::{controls_panel, device_list, header, network_list, password_dialog, vpn_list};
use crate::config::{Config, Position};

/// Minimum pixel height for list boxes (shows ~3 items)
pub const MIN_LIST_HEIGHT: i32 = 220;
/// Maximum pixel height for list boxes before scrolling (shows ~4–5 items)
pub const MAX_LIST_HEIGHT: i32 = 360;

/// Default width of the main panel window
pub const WINDOW_WIDTH: i32 = 340;

/// All UI handles needed by the app controller.
#[allow(dead_code)]
pub struct PanelWidgets {
    pub window: ApplicationWindow,
    pub wifi_switch: gtk4::Switch,
    pub title_label: gtk4::Label,
    pub status_label: gtk4::Label,
    pub scan_button: gtk4::Button,
    pub wifi_tab: gtk4::ToggleButton,
    pub bt_tab: gtk4::ToggleButton,
    // Wi-Fi page
    pub wifi_networks_tab: ToggleButton,
    pub wifi_vpn_tab: ToggleButton,
    pub wifi_sub_stack: Stack,
    pub network_list_box: ListBox,
    pub network_scroll: gtk4::ScrolledWindow,
    pub spinner: gtk4::Spinner,
    pub password_revealer: gtk4::Revealer,
    pub password_entry: gtk4::Entry,
    pub connect_button: gtk4::Button,
    pub cancel_button: gtk4::Button,
    pub error_label: gtk4::Label,
    // VPN page (inside Wi-Fi tab)
    pub vpn_add_button: gtk4::Button,
    pub vpn_open_button: gtk4::Button,
    pub vpn_list_box: ListBox,
    pub vpn_scroll: gtk4::ScrolledWindow,
    pub vpn_spinner: gtk4::Spinner,
    // Bluetooth page
    pub bt_list_box: ListBox,
    pub bt_scroll: gtk4::ScrolledWindow,
    pub bt_spinner: gtk4::Spinner,
    // Content stack
    pub content_stack: Stack,
    // Controls panel
    pub controls: controls_panel::ControlsPanel,
}

/// Build the main floating panel window with all UI components.
pub fn build_window(app: &Application) -> PanelWidgets {
    let config = Config::load();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("WiFi Manager")
        .default_width(WINDOW_WIDTH)
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
    content_stack.set_vexpand(true); // Pushes the controls panel to the absolute bottom statically
    content_stack.add_css_class("content-stack");

    // ── Wi-Fi page ──────────────────────────────────────────────────
    let wifi_page = GtkBox::new(Orientation::Vertical, 0);

    // Sub-tabs inside Wi-Fi: Networks / VPN
    let wifi_subtab_bar = GtkBox::new(Orientation::Horizontal, 0);
    wifi_subtab_bar.add_css_class("subtab-bar");
    wifi_subtab_bar.set_margin_top(6);
    wifi_subtab_bar.set_margin_bottom(6);

    let wifi_networks_tab = ToggleButton::with_label("Networks");
    wifi_networks_tab.add_css_class("subtab-button");
    wifi_networks_tab.add_css_class("tab-active");
    wifi_networks_tab.set_active(true);
    wifi_networks_tab.set_hexpand(true);
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        wifi_networks_tab.set_cursor(Some(&cursor));
    }

    let wifi_vpn_tab = ToggleButton::with_label("VPN");
    wifi_vpn_tab.add_css_class("subtab-button");
    wifi_vpn_tab.set_hexpand(true);
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        wifi_vpn_tab.set_cursor(Some(&cursor));
    }

    wifi_networks_tab.set_group(Some(&wifi_vpn_tab));

    wifi_subtab_bar.append(&wifi_networks_tab);
    wifi_subtab_bar.append(&wifi_vpn_tab);
    wifi_page.append(&wifi_subtab_bar);

    let wifi_sub_stack = Stack::new();
    wifi_sub_stack.set_transition_type(StackTransitionType::Crossfade);
    wifi_sub_stack.set_transition_duration(150);
    wifi_sub_stack.set_vexpand(true);
    wifi_sub_stack.add_css_class("wifi-sub-stack");

    // Networks view
    let wifi_networks_view = GtkBox::new(Orientation::Vertical, 0);

    let (scrolled, list_box) = network_list::build_network_list();

    let spinner = gtk4::Spinner::new();
    spinner.set_spinning(true);
    spinner.add_css_class("loading-spinner");
    spinner.set_size_request(32, MIN_LIST_HEIGHT); // Width 32, Height matches min_content_height of list
    spinner.set_halign(gtk4::Align::Center);
    spinner.set_valign(gtk4::Align::Center);
    spinner.set_margin_top(20);
    spinner.set_margin_bottom(20);

    wifi_networks_view.append(&spinner);
    wifi_networks_view.append(&scrolled);
    scrolled.set_visible(false);

    let (revealer, entry, connect_btn, cancel_btn, error_label) =
        password_dialog::build_password_section();
    wifi_networks_view.append(&revealer);

    wifi_sub_stack.add_named(&wifi_networks_view, Some("networks"));

    // VPN view
    let vpn_view = GtkBox::new(Orientation::Vertical, 0);
    let vpn_actions = GtkBox::new(Orientation::Horizontal, 8);
    vpn_actions.add_css_class("vpn-actions-row");
    vpn_actions.set_margin_start(20);
    vpn_actions.set_margin_end(20);
    vpn_actions.set_margin_bottom(6);

    let vpn_add_button = gtk4::Button::with_label("Add Profile");
    vpn_add_button.add_css_class("vpn-action-btn");
    vpn_add_button.set_hexpand(true);
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        vpn_add_button.set_cursor(Some(&cursor));
    }

    let vpn_open_button = gtk4::Button::with_label("Open Settings");
    vpn_open_button.add_css_class("vpn-action-btn");
    vpn_open_button.set_hexpand(true);
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        vpn_open_button.set_cursor(Some(&cursor));
    }

    vpn_actions.append(&vpn_add_button);
    vpn_actions.append(&vpn_open_button);
    vpn_view.append(&vpn_actions);

    let (vpn_scrolled, vpn_list_box) = vpn_list::build_vpn_list();

    let vpn_spinner = gtk4::Spinner::new();
    vpn_spinner.set_spinning(true);
    vpn_spinner.add_css_class("loading-spinner");
    vpn_spinner.set_size_request(32, MIN_LIST_HEIGHT);
    vpn_spinner.set_halign(gtk4::Align::Center);
    vpn_spinner.set_valign(gtk4::Align::Center);
    vpn_spinner.set_margin_top(20);
    vpn_spinner.set_margin_bottom(20);

    vpn_view.append(&vpn_spinner);
    vpn_view.append(&vpn_scrolled);
    vpn_scrolled.set_visible(false);

    wifi_sub_stack.add_named(&vpn_view, Some("vpn"));
    wifi_sub_stack.set_visible_child_name("networks");
    wifi_page.append(&wifi_sub_stack);

    content_stack.add_named(&wifi_page, Some("wifi"));

    // ── Bluetooth page ─────────────────────────────────────────────
    let bt_page = GtkBox::new(Orientation::Vertical, 0);

    let (bt_scrolled, bt_list_box) = device_list::build_device_list();

    let bt_spinner = gtk4::Spinner::new();
    bt_spinner.set_spinning(true);
    bt_spinner.add_css_class("loading-spinner");
    bt_spinner.set_size_request(32, MIN_LIST_HEIGHT); // Width 32, Height matches min_content_height of list
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

    // ── Controls Panel (Bottom Footer) ─────────────────────────────
    let controls = controls_panel::ControlsPanel::new();
    main_box.append(controls.container());

    // Smoothly shrink window when controls are hidden
    let window_clone = window.clone();
    controls.toggle_button().connect_toggled(move |btn: &gtk4::ToggleButton| {
        if !btn.is_active() { // Slider section is collapsing
            let win_ref = window_clone.clone();
            let btn_ref = btn.clone();
            // Wait slightly longer than the slide transition before recalibrating
            let delay = std::time::Duration::from_millis(controls_panel::SLIDE_TRANSITION_MS as u64 + 10);
            gtk4::glib::timeout_add_local(delay, move || {
                // Only resize if still collapsed
                if !btn_ref.is_active() {
                    win_ref.set_default_size(WINDOW_WIDTH, -1); // Keep width fixed, shrink height
                }
                gtk4::glib::ControlFlow::Break
            });
        }
    });

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

    // ── Wi-Fi sub-tabs (Networks / VPN) ─────────────────────────────
    {
        let sub_stack = wifi_sub_stack.clone();
        wifi_networks_tab.connect_toggled(move |btn| {
            if btn.is_active() {
                sub_stack.set_visible_child_name("networks");
            }
        });
    }
    {
        let sub_stack = wifi_sub_stack.clone();
        wifi_vpn_tab.connect_toggled(move |btn| {
            if btn.is_active() {
                sub_stack.set_visible_child_name("vpn");
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
        wifi_networks_tab,
        wifi_vpn_tab,
        wifi_sub_stack,
        network_list_box: list_box,
        network_scroll: scrolled,
        spinner,
        password_revealer: revealer,
        password_entry: entry,
        connect_button: connect_btn,
        cancel_button: cancel_btn,
        error_label,
        vpn_add_button,
        vpn_open_button,
        vpn_list_box,
        vpn_scroll: vpn_scrolled,
        vpn_spinner,
        bt_list_box,
        bt_scroll: bt_scrolled,
        bt_spinner,
        content_stack,
        controls,
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
