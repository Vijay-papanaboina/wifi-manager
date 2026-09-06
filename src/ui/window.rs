//! Main floating Control Center window with layer-shell support.
//!
//! The root stack owns the reusable home page and the Wi-Fi, Bluetooth,
//! and Power detail pages.  Legacy Wi-Fi/Bluetooth toggle handles
//! remain in `PanelWidgets` so existing async controllers keep their active
//! feature guards while navigation is handled by the stack.

use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Label, ListBox,
    Orientation, Stack, StackTransitionType, ToggleButton, gdk,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::cell::RefCell;

use super::{
    audio, controls_panel, device_list, header, home, media, network_list, password_dialog, power,
    system, vpn_list,
};
use crate::config::{Config, Position};

thread_local! {
    /// The installed user provider, replaced on every reload.
    static USER_CSS_PROVIDER: RefCell<Option<CssProvider>> = const { RefCell::new(None) };
}

/// Minimum pixel height for list boxes (shows ~3 items).
pub(crate) const MIN_LIST_HEIGHT: i32 = 220;
/// Maximum pixel height for list boxes before scrolling (shows ~4–5 items).
pub(crate) const MAX_LIST_HEIGHT: i32 = 360;

/// Default width of the main panel window.
pub(crate) const WINDOW_WIDTH: i32 = 400;

/// Pages in the root Control Center stack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelPage {
    Home,
    Wifi,
    Bluetooth,
    Audio,
    Power,
    System,
}

impl PanelPage {
    fn name(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Wifi => "wifi",
            Self::Bluetooth => "bluetooth",
            Self::Audio => "audio",
            Self::Power => "power",
            Self::System => "system",
        }
    }
}

/// All UI handles needed by the app controller.
#[derive(Clone)]
pub(crate) struct PanelWidgets {
    pub window: ApplicationWindow,
    pub wifi_switch: gtk4::Switch,
    pub title_label: gtk4::Label,
    /// Compatibility status sink used by existing async controllers.  It is
    /// not rendered; app/mod.rs mirrors it into page-local labels.
    pub status_label: gtk4::Label,
    pub wifi_status_label: gtk4::Label,
    pub bluetooth_status_label: gtk4::Label,
    pub scan_button: gtk4::Button,
    /// Hidden feature selectors retained for existing controller wiring.
    pub wifi_tab: gtk4::ToggleButton,
    pub bt_tab: gtk4::ToggleButton,
    // Wi-Fi page
    pub wifi_networks_tab: ToggleButton,
    pub wifi_vpn_tab: ToggleButton,
    pub network_list_box: ListBox,
    pub network_scroll: gtk4::ScrolledWindow,
    pub spinner: gtk4::Spinner,
    pub password_revealer: gtk4::Revealer,
    pub password_entry: gtk4::Entry,
    pub connect_button: gtk4::Button,
    pub cancel_button: gtk4::Button,
    pub error_label: gtk4::Label,
    // VPN page (inside Wi-Fi detail)
    pub vpn_import_button: gtk4::Button,
    pub vpn_open_button: gtk4::Button,
    pub vpn_list_box: ListBox,
    pub vpn_scroll: gtk4::ScrolledWindow,
    pub vpn_spinner: gtk4::Spinner,
    // Bluetooth page
    pub bt_list_box: ListBox,
    pub bt_scroll: gtk4::ScrolledWindow,
    pub bt_spinner: gtk4::Spinner,
    // Root stack and home page
    pub content_stack: Stack,
    pub home: home::HomeWidgets,
    pub audio: audio::AudioWidgets,
    pub power: power::PowerWidgets,
    pub media: media::MediaWidgets,
    pub system: system::SystemWidgets,
    // Display and Power detail controls
    pub controls: controls_panel::ControlsPanel,
}

impl PanelWidgets {
    /// Navigate to a root-stack page and keep legacy feature guards in sync.
    pub(crate) fn navigate_to(&self, page: PanelPage) {
        self.content_stack.set_visible_child_name(page.name());

        match page {
            PanelPage::Home => {
                self.title_label.set_text("Control Center");
                self.title_label.set_visible(true);
                self.wifi_switch.set_visible(false);
                self.scan_button.set_visible(false);
                self.wifi_tab.set_active(false);
                self.bt_tab.set_active(false);
            }
            PanelPage::Wifi => {
                self.title_label.set_text("Wi-Fi");
                self.title_label.set_visible(false);
                self.wifi_switch.set_visible(true);
                self.scan_button.set_visible(true);
                self.scan_button.set_sensitive(true);
                self.scan_button.set_tooltip_text(Some("Scan for networks"));
                self.wifi_tab.set_active(true);
            }
            PanelPage::Bluetooth => {
                self.title_label.set_text("Bluetooth");
                self.title_label.set_visible(false);
                self.wifi_switch.set_visible(true);
                self.scan_button.set_visible(true);
                self.scan_button.set_sensitive(true);
                self.scan_button.set_tooltip_text(Some("Scan for devices"));
                self.bt_tab.set_active(true);
            }
            PanelPage::Audio => {
                self.title_label.set_text("Audio");
                self.title_label.set_visible(false);
                self.wifi_switch.set_visible(false);
                self.scan_button.set_visible(false);
                self.wifi_tab.set_active(false);
                self.bt_tab.set_active(false);
            }
            PanelPage::Power => {
                self.title_label.set_text("Power");
                self.title_label.set_visible(false);
                self.wifi_switch.set_visible(false);
                self.scan_button.set_visible(false);
                self.wifi_tab.set_active(false);
                self.bt_tab.set_active(false);
            }
            PanelPage::System => {
                self.title_label.set_text("System");
                self.title_label.set_visible(false);
                self.wifi_switch.set_visible(false);
                self.scan_button.set_visible(false);
                self.wifi_tab.set_active(false);
                self.bt_tab.set_active(false);
            }
        }
    }

    pub(crate) fn show_home(&self) {
        self.navigate_to(PanelPage::Home);
    }
}

/// Build the main floating panel window with all UI components.
pub(crate) fn build_window(app: &Application) -> PanelWidgets {
    let config = Config::load();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("WiFi Manager")
        .default_width(WINDOW_WIDTH)
        .build();

    // Initialize layer shell.
    window.init_layer_shell();
    window.set_namespace(Some("wifi-manager"));
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    // Apply position from config.
    apply_position(&window, &config);

    // Don't push other windows.
    window.set_exclusive_zone(-1);

    let main_box = GtkBox::new(Orientation::Vertical, 0);
    main_box.add_css_class("wifi-panel");

    let header = header::build_header();
    main_box.append(&header.container);

    let sep = gtk4::Separator::new(Orientation::Horizontal);
    sep.add_css_class("header-separator");
    main_box.append(&sep);

    let content_stack = Stack::new();
    content_stack.set_transition_type(StackTransitionType::Crossfade);
    content_stack.set_transition_duration(150);
    content_stack.set_vexpand(true);
    content_stack.add_css_class("content-stack");

    // ── Home page ────────────────────────────────────────────────────
    let home = home::HomeWidgets::new(&config);
    let home_page = GtkBox::new(Orientation::Vertical, 0);
    home_page.add_css_class("cc-page");
    home_page.append(&home.container);
    let media = media::MediaWidgets::new(&config.media_icon);
    home_page.append(&media.container);
    content_stack.add_named(&home_page, Some(PanelPage::Home.name()));

    // ── Wi-Fi detail page ────────────────────────────────────────────
    let wifi_page = GtkBox::new(Orientation::Vertical, 0);
    wifi_page.add_css_class("cc-page");
    let wifi_status_label = Label::new(Some("Loading…"));
    let (wifi_detail_header, wifi_back_button) = make_detail_header("Wi-Fi", &wifi_status_label);
    wifi_page.append(&wifi_detail_header);

    // Sub-tabs inside Wi-Fi: Networks / VPN.
    let wifi_subtab_bar = GtkBox::new(Orientation::Horizontal, 0);
    wifi_subtab_bar.add_css_class("subtab-bar");
    wifi_subtab_bar.set_margin_top(6);
    wifi_subtab_bar.set_margin_bottom(6);

    let wifi_networks_tab = ToggleButton::with_label("Networks");
    wifi_networks_tab.add_css_class("subtab-button");
    wifi_networks_tab.add_css_class("tab-active");
    wifi_networks_tab.set_active(true);
    wifi_networks_tab.set_hexpand(true);
    set_pointer_cursor(&wifi_networks_tab);

    let wifi_vpn_tab = ToggleButton::with_label("VPN");
    wifi_vpn_tab.add_css_class("subtab-button");
    wifi_vpn_tab.set_hexpand(true);
    set_pointer_cursor(&wifi_vpn_tab);

    wifi_networks_tab.set_group(Some(&wifi_vpn_tab));
    wifi_subtab_bar.append(&wifi_networks_tab);
    wifi_subtab_bar.append(&wifi_vpn_tab);
    wifi_page.append(&wifi_subtab_bar);

    let wifi_sub_stack = Stack::new();
    wifi_sub_stack.set_transition_type(StackTransitionType::Crossfade);
    wifi_sub_stack.set_transition_duration(150);
    wifi_sub_stack.set_vexpand(true);
    wifi_sub_stack.add_css_class("wifi-sub-stack");

    // Networks view.
    let wifi_networks_view = GtkBox::new(Orientation::Vertical, 0);
    let (scrolled, list_box) = network_list::build_network_list();

    let spinner = gtk4::Spinner::new();
    spinner.set_spinning(true);
    spinner.add_css_class("loading-spinner");
    spinner.set_size_request(32, MIN_LIST_HEIGHT);
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

    // VPN view.
    let vpn_view = GtkBox::new(Orientation::Vertical, 0);
    let vpn_actions = GtkBox::new(Orientation::Horizontal, 8);
    vpn_actions.add_css_class("vpn-actions-row");
    vpn_actions.set_margin_start(20);
    vpn_actions.set_margin_end(20);
    vpn_actions.set_margin_bottom(6);

    let vpn_import_button = Button::with_label("Import Profile");
    vpn_import_button.add_css_class("vpn-action-btn");
    vpn_import_button.set_hexpand(true);
    set_pointer_cursor(&vpn_import_button);

    let vpn_open_button = Button::with_label("Open Settings");
    vpn_open_button.add_css_class("vpn-action-btn");
    vpn_open_button.set_hexpand(true);
    set_pointer_cursor(&vpn_open_button);

    vpn_actions.append(&vpn_import_button);
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
    content_stack.add_named(&wifi_page, Some(PanelPage::Wifi.name()));

    // ── Bluetooth detail page ────────────────────────────────────────
    let bt_page = GtkBox::new(Orientation::Vertical, 0);
    bt_page.add_css_class("cc-page");
    let bluetooth_status_label = Label::new(Some("Loading…"));
    let (bt_detail_header, bt_back_button) =
        make_detail_header("Bluetooth", &bluetooth_status_label);
    bt_page.append(&bt_detail_header);

    let (bt_scrolled, bt_list_box) = device_list::build_device_list();
    let bt_spinner = gtk4::Spinner::new();
    bt_spinner.set_spinning(true);
    bt_spinner.add_css_class("loading-spinner");
    bt_spinner.set_size_request(32, MIN_LIST_HEIGHT);
    bt_spinner.set_halign(gtk4::Align::Center);
    bt_spinner.set_valign(gtk4::Align::Center);
    bt_spinner.set_margin_top(20);
    bt_spinner.set_margin_bottom(20);

    bt_page.append(&bt_spinner);
    bt_page.append(&bt_scrolled);
    bt_scrolled.set_visible(false);
    content_stack.add_named(&bt_page, Some(PanelPage::Bluetooth.name()));

    // ── Audio detail page ────────────────────────────────────────────
    let audio = audio::AudioWidgets::new();
    let audio_page = GtkBox::new(Orientation::Vertical, 0);
    audio_page.add_css_class("cc-page");
    let (audio_header, audio_back_button) = make_detail_header("Audio", &audio.status);
    audio_page.append(&audio_header);
    audio_page.append(&audio.container);
    content_stack.add_named(&audio_page, Some(PanelPage::Audio.name()));

    // ── Persistent quick controls and Power detail page ──────────────
    let controls = controls_panel::ControlsPanel::new();
    // Keep the existing high-frequency controls on Home.  These are the
    // original widget instances, so their existing manager bindings and
    // session power actions remain single-owner and directly reachable.
    let quick_controls = GtkBox::new(Orientation::Vertical, 0);
    quick_controls.add_css_class("cc-quick-controls");
    quick_controls.set_margin_start(12);
    quick_controls.set_margin_end(12);
    quick_controls.set_margin_bottom(8);
    quick_controls.append(controls.display_page());
    quick_controls.append(controls.power_page());
    home_page.append(&quick_controls);

    let power = power::PowerWidgets::new();
    let power_page = GtkBox::new(Orientation::Vertical, 0);
    power_page.add_css_class("cc-page");
    let (power_header, power_back_button) = make_detail_header("Power", &power.status);
    power_page.append(&power_header);
    power_page.append(&power.container);
    let power_hint = Label::new(Some("Session actions are available on Home."));
    power_hint.add_css_class("cc-detail-hint");
    power_hint.set_margin_start(16);
    power_hint.set_margin_end(16);
    power_page.append(&power_hint);
    content_stack.add_named(&power_page, Some(PanelPage::Power.name()));

    // ── System detail page ──────────────────────────────────────────
    let system = system::SystemWidgets::new();
    let system_page = GtkBox::new(Orientation::Vertical, 0);
    system_page.add_css_class("cc-page");
    let (system_header, system_back_button) = make_detail_header("System", &system.status);
    system_page.append(&system_header);
    system_page.append(&system.container);
    content_stack.add_named(&system_page, Some(PanelPage::System.name()));

    // Start on Home, where the persistent quick controls are immediately
    // usable without opening a detail page.
    content_stack.set_visible_child_name(PanelPage::Home.name());
    main_box.append(&content_stack);
    window.set_child(Some(&main_box));

    load_css();

    let widgets = PanelWidgets {
        window,
        wifi_switch: header.toggle_switch,
        title_label: header.title_label,
        status_label: header.status_label,
        wifi_status_label,
        bluetooth_status_label,
        scan_button: header.scan_button,
        wifi_tab: header.wifi_tab,
        bt_tab: header.bt_tab,
        wifi_networks_tab,
        wifi_vpn_tab,
        network_list_box: list_box,
        network_scroll: scrolled,
        spinner,
        password_revealer: revealer,
        password_entry: entry,
        connect_button: connect_btn,
        cancel_button: cancel_btn,
        error_label,
        vpn_import_button,
        vpn_open_button,
        vpn_list_box,
        vpn_scroll: vpn_scrolled,
        vpn_spinner,
        bt_list_box,
        bt_scroll: bt_scrolled,
        bt_spinner,
        content_stack,
        home,
        audio,
        power,
        media,
        system,
        controls,
    };

    wire_navigation(
        &widgets,
        wifi_back_button,
        bt_back_button,
        audio_back_button,
        power_back_button,
        system_back_button,
    );
    log::info!("Layer-shell Control Center built (hidden)");
    widgets
}

fn make_detail_header(title: &str, status: &Label) -> (GtkBox, Button) {
    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.add_css_class("cc-detail-header");
    row.set_margin_start(16);
    row.set_margin_end(16);
    row.set_margin_top(12);
    row.set_margin_bottom(4);

    let back = Button::from_icon_name("go-previous-symbolic");
    back.add_css_class("cc-back-button");
    back.set_tooltip_text(Some("Back to Control Center"));
    set_pointer_cursor(&back);

    let info = GtkBox::new(Orientation::Vertical, 2);
    info.set_hexpand(true);

    let title_label = Label::new(Some(title));
    title_label.add_css_class("cc-detail-title");
    title_label.set_halign(gtk4::Align::Start);

    status.add_css_class("cc-detail-status");
    status.set_halign(gtk4::Align::Start);
    status.set_ellipsize(gtk4::pango::EllipsizeMode::End);

    info.append(&title_label);
    info.append(status);
    row.append(&back);
    row.append(&info);
    (row, back)
}

fn wire_navigation(
    widgets: &PanelWidgets,
    wifi_back: Button,
    bt_back: Button,
    audio_back: Button,
    power_back: Button,
    system_back: Button,
) {
    let navigation = [
        (widgets.home.wifi.button().clone(), PanelPage::Wifi),
        (widgets.home.bluetooth.button().clone(), PanelPage::Bluetooth),
        (widgets.home.audio.button().clone(), PanelPage::Audio),
        (widgets.home.power_battery.button().clone(), PanelPage::Power),
        (widgets.home.system.button().clone(), PanelPage::System),
    ];
    for (button, page) in navigation {
        let widgets = widgets.clone();
        button.connect_clicked(move |_| widgets.navigate_to(page));
    }

    for back in [wifi_back, bt_back, audio_back, power_back, system_back] {
        let widgets = widgets.clone();
        back.connect_clicked(move |_| widgets.show_home());
    }
}

/// Load the default CSS theme and optional user overrides.
fn load_css() {
    let Some(display) = gdk::Display::default() else {
        log::error!("Could not get default display; CSS theme was not loaded");
        return;
    };

    let default_css = include_str!("../../resources/style.css");
    let provider = CssProvider::new();
    provider.load_from_string(default_css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    log::info!("Default CSS theme loaded");

    reload_user_css(&display);
}

/// Reload user CSS (for --reload flag).
pub(crate) fn reload_css() {
    let Some(display) = gdk::Display::default() else {
        log::error!("Could not get default display; CSS reload skipped");
        return;
    };

    reload_user_css(&display);
}

fn reload_user_css(display: &gdk::Display) {
    USER_CSS_PROVIDER.with(|provider_cell| {
        let previous = provider_cell.borrow_mut().take();
        if let Some(previous) = previous {
            gtk4::style_context_remove_provider_for_display(display, &previous);
        }

        let Some(config_dir) = dirs_config_path() else {
            return;
        };
        let user_css_path = config_dir.join("style.css");
        if !user_css_path.exists() {
            return;
        }

        let user_provider = CssProvider::new();
        user_provider.load_from_path(user_css_path.to_str().unwrap_or_default());
        gtk4::style_context_add_provider_for_display(
            display,
            &user_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
        *provider_cell.borrow_mut() = Some(user_provider);
        log::info!("User CSS theme loaded from {:?}", user_css_path);
    });
}

/// Get the config directory: ~/.config/wifi-manager/.
fn dirs_config_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(std::path::PathBuf::from(home).join(".config").join("wifi-manager"))
}

/// Apply window position and margins from config to a layer-shell window.
fn apply_position(window: &ApplicationWindow, config: &Config) {
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

    window.set_margin(Edge::Top, config.margin_top);
    window.set_margin(Edge::Right, config.margin_right);
    window.set_margin(Edge::Bottom, config.margin_bottom);
    window.set_margin(Edge::Left, config.margin_left);

    log::info!(
        "Window position: {:?}, margins: t={} r={} b={} l={}",
        config.position,
        config.margin_top,
        config.margin_right,
        config.margin_bottom,
        config.margin_left
    );
}

/// Reapply configuration values that are safe to change while the panel is
/// running. Static widget structure remains intact; only placement and
/// configurable glyphs are updated.
pub(crate) fn apply_runtime_config(
    window: &ApplicationWindow,
    controls: &controls_panel::ControlsPanel,
    home: &home::HomeWidgets,
    media: &media::MediaWidgets,
    config: &Config,
) {
    apply_position(window, config);
    controls.apply_config(config);
    home.apply_config(config);
    media.apply_config(&config.media_icon);
}

fn set_pointer_cursor<W: IsA<gtk4::Widget>>(widget: &W) {
    if let Some(cursor) = gdk::Cursor::from_name("pointer", None) {
        widget.set_cursor(Some(&cursor));
    }
}
