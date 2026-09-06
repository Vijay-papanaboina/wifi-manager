use futures_util::future::LocalBoxFuture;
use gtk4::{Box, Button, Image, Label, Orientation, Scale, Window, glib, prelude::*};

use crate::config::Config;
use crate::error::AppResult;

fn set_pointer_cursor<W: IsA<gtk4::Widget>>(widget: &W) {
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        widget.set_cursor(Some(&cursor));
    }
}

fn make_glyph_button(icon: &str, tooltip: &str) -> Button {
    let label = Label::builder()
        .label(icon)
        .halign(gtk4::Align::Center)
        .valign(gtk4::Align::Center)
        .build();
    label.add_css_class("glyph-button-label");

    let button = Button::builder().child(&label).tooltip_text(tooltip).build();
    set_pointer_cursor(&button);
    button.add_css_class("flat");
    button.add_css_class("circular");
    button.add_css_class("glyph-button");
    button
}

pub(crate) fn set_button_glyph(button: &Button, glyph: &str) {
    if let Some(label) = button.child().and_downcast::<Label>() {
        label.set_label(glyph);
    } else {
        let label = Label::builder()
            .label(glyph)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .build();
        label.add_css_class("glyph-button-label");
        button.set_child(Some(&label));
    }
}

fn show_confirm_dialog(
    window: &Window,
    title: &str,
    message: &str,
    action: impl FnOnce() + 'static,
) {
    let dialog = gtk4::AlertDialog::builder()
        .modal(true)
        .message(title)
        .detail(message)
        .buttons(["Cancel", "Ok"])
        .cancel_button(0)
        .default_button(1)
        .build();

    let window_clone = window.clone();
    glib::spawn_future_local(async move {
        // choose_future returns the index of the clicked button
        if dialog.choose_future(Some(&window_clone)).await == Ok(1) {
            action();
        }
    });
}

fn show_error_dialog(window: Option<&Window>, message: impl Into<String>) {
    let message = message.into();
    let dialog = gtk4::AlertDialog::builder()
        .modal(true)
        .message("Error")
        .detail(&message)
        .buttons(["Ok"])
        .default_button(0)
        .build();

    if let Some(win) = window {
        let win_clone = win.clone();
        glib::spawn_future_local(async move {
            let _ = dialog.choose_future(Some(&win_clone)).await;
        });
    } else {
        // If no window is available, just print to log (already done by caller usually)
        log::error!("Error dialog (no window context): {}", message);
    }
}

fn connect_power_button(btn: &Button, title: &str, message: &str, action: PowerAction) {
    let title = title.to_string();
    let message = message.to_string();
    btn.connect_clicked(move |button| {
        let Some(window) = button.root().and_downcast::<Window>() else {
            log::warn!("Power button '{}' was clicked but has no window attachment", title);
            return;
        };
        let title_clone = title.clone();
        let window_clone = window.clone();
        show_confirm_dialog(&window, &title, &message, move || {
            glib::spawn_future_local(async move {
                if let Err(error) = action().await {
                    log::error!("{}: {}", title_clone, error);
                    show_error_dialog(Some(&window_clone), error.to_string());
                }
            });
        });
    });
}

pub(crate) type PowerAction = fn() -> LocalBoxFuture<'static, AppResult<()>>;

/// Backend actions supplied by the application layer to the controls view.
pub(crate) struct PowerActions {
    pub(crate) poweroff: PowerAction,
    pub(crate) reboot: PowerAction,
    pub(crate) suspend: PowerAction,
    pub(crate) logout: PowerAction,
}

/// The unified panel for Brightness, Volume, and Night Mode controls.
#[derive(Clone)]
pub(crate) struct ControlsPanel {
    display_page: Box,
    power_page: Box,
    brightness_scale: Scale,
    brightness_btn: Button,
    volume_scale: Scale,
    volume_icon: Image,
    volume_btn: Button,
    night_mode_scale: Scale,
    night_mode_btn: Button,
    poweroff_btn: Button,
    reboot_btn: Button,
    suspend_btn: Button,
    logout_btn: Button,
}

impl Default for ControlsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlsPanel {
    /// Detail-page container for brightness, volume, and Night Mode.
    pub(crate) fn display_page(&self) -> &Box {
        &self.display_page
    }
    /// Detail-page container for poweroff, reboot, suspend, and logout.
    pub(crate) fn power_page(&self) -> &Box {
        &self.power_page
    }
    pub(crate) fn brightness_scale(&self) -> &Scale {
        &self.brightness_scale
    }
    pub(crate) fn brightness_btn(&self) -> &Button {
        &self.brightness_btn
    }
    pub(crate) fn volume_scale(&self) -> &Scale {
        &self.volume_scale
    }
    pub(crate) fn volume_icon(&self) -> &Image {
        &self.volume_icon
    }
    pub(crate) fn volume_btn(&self) -> &Button {
        &self.volume_btn
    }
    pub(crate) fn night_mode_scale(&self) -> &Scale {
        &self.night_mode_scale
    }
    pub(crate) fn night_mode_btn(&self) -> &Button {
        &self.night_mode_btn
    }
    /// Apply configuration values that can change without rebuilding widgets.
    pub(crate) fn apply_config(&self, config: &Config) {
        self.brightness_btn.set_icon_name(&config.brightness_icon);
        self.volume_btn.set_icon_name(&config.volume_icon);
        let night_mode_icon = if self.night_mode_scale.is_sensitive() {
            &config.night_mode_on_icon
        } else {
            &config.night_mode_off_icon
        };
        set_button_glyph(&self.night_mode_btn, night_mode_icon);
        set_button_glyph(&self.poweroff_btn, &config.poweroff_icon);
        set_button_glyph(&self.reboot_btn, &config.reboot_icon);
        set_button_glyph(&self.suspend_btn, &config.suspend_icon);
        set_button_glyph(&self.logout_btn, &config.logout_icon);
    }

    /// Attach application-provided power actions to the power buttons.
    pub(crate) fn bind_power_actions(&self, actions: PowerActions) {
        connect_power_button(
            &self.poweroff_btn,
            "Power Off",
            "Are you sure you want to power off the system?",
            actions.poweroff,
        );
        connect_power_button(
            &self.reboot_btn,
            "Reboot",
            "Are you sure you want to reboot the system?",
            actions.reboot,
        );
        connect_power_button(
            &self.suspend_btn,
            "Suspend",
            "Are you sure you want to suspend the system?",
            actions.suspend,
        );
        connect_power_button(
            &self.logout_btn,
            "Logout",
            "Are you sure you want to log out?",
            actions.logout,
        );
    }

    pub(crate) fn new() -> Self {
        let config = Config::load();
        let display_page = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(4)
            .margin_bottom(0) // Let inner elements dictate bottom spacing
            .margin_start(0)
            .margin_end(0)
            .css_classes(["controls-panel"])
            .build();

        let power_page = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(4)
            .margin_bottom(0)
            .margin_start(0)
            .margin_end(0)
            .css_classes(["controls-panel", "cc-power-controls"])
            .build();

        // The container holding all always-visible quick controls.
        let sliders_box = Box::builder().orientation(Orientation::Vertical).spacing(12).build();

        // Brightness Row
        let brightness_row =
            Box::builder().orientation(Orientation::Horizontal).spacing(12).build();

        // Clickable brightness icon: click to toggle between 1% dim and last custom level
        let brightness_btn = Button::builder()
            .icon_name(&config.brightness_icon)
            .tooltip_text("Click to toggle minimum brightness")
            .build();
        set_pointer_cursor(&brightness_btn);
        brightness_btn.add_css_class("flat");
        brightness_btn.add_css_class("circular");

        let brightness_scale = Scale::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .draw_value(true)
            .value_pos(gtk4::PositionType::Right)
            .tooltip_text("Brightness")
            .adjustment(&gtk4::Adjustment::new(100.0, 1.0, 100.0, 1.0, 10.0, 0.0))
            .build();
        set_pointer_cursor(&brightness_scale);

        brightness_row.append(&brightness_btn);
        brightness_row.append(&brightness_scale);

        // Volume Row
        let volume_row = Box::builder().orientation(Orientation::Horizontal).spacing(12).build();

        // Clickable volume icon: click to toggle mute
        let volume_btn = Button::builder()
            .icon_name(&config.volume_icon)
            .tooltip_text("Click to toggle mute")
            .build();
        set_pointer_cursor(&volume_btn);
        volume_btn.add_css_class("flat");
        volume_btn.add_css_class("circular");

        // Hidden Image widget kept for dynamic icon updates from volume callbacks
        let volume_icon =
            Image::builder().icon_name(&config.volume_icon).pixel_size(16).visible(false).build();

        let volume_scale = Scale::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .draw_value(true)
            .value_pos(gtk4::PositionType::Right)
            .tooltip_text("Volume")
            .adjustment(&gtk4::Adjustment::new(0.0, 0.0, 100.0, 1.0, 10.0, 0.0))
            .build();
        set_pointer_cursor(&volume_scale);

        volume_row.append(&volume_btn);
        volume_row.append(&volume_scale);

        // Night Mode Row
        let night_mode_row =
            Box::builder().orientation(Orientation::Horizontal).spacing(12).build();

        // Clickable moon icon: click to toggle Night Mode on/off
        let night_mode_btn =
            make_glyph_button(&config.night_mode_off_icon, "Click to toggle Night Mode");

        // Map 0 -> 6500K (coolest/no effect), 3500 -> 3000K (warmest)
        let night_mode_scale = Scale::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .draw_value(true)
            .value_pos(gtk4::PositionType::Right)
            .tooltip_text("Night Mode (Color Temperature)")
            .adjustment(&gtk4::Adjustment::new(0.0, 0.0, 3500.0, 100.0, 500.0, 0.0))
            .build();
        set_pointer_cursor(&night_mode_scale);
        night_mode_scale.set_sensitive(false); // Disabled until toggled On

        night_mode_row.append(&night_mode_btn);
        night_mode_row.append(&night_mode_scale);

        // Power Controls Row
        let power_row = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(0)
            .hexpand(true)
            .homogeneous(true)
            .margin_top(6)
            .margin_bottom(6)
            .css_classes(["power-row"])
            .build();

        let btn_poweroff = make_glyph_button(&config.poweroff_icon, "Power Off");

        let btn_reboot = make_glyph_button(&config.reboot_icon, "Reboot");

        let btn_suspend = make_glyph_button(&config.suspend_icon, "Suspend / Sleep");

        let btn_logout = make_glyph_button(&config.logout_icon, "Log Out");

        for button in [&btn_logout, &btn_suspend, &btn_reboot, &btn_poweroff] {
            button.set_hexpand(true);
            button.set_halign(gtk4::Align::Fill);
        }

        power_row.append(&btn_logout);
        power_row.append(&btn_suspend);
        power_row.append(&btn_reboot);
        power_row.append(&btn_poweroff);

        // Assemble sliders into the inner box
        sliders_box.append(&brightness_row);
        sliders_box.append(&volume_row);
        sliders_box.append(&night_mode_row);

        // Keep one ControlsPanel handle for existing app wiring while making
        // all high-frequency controls permanently reachable from Home.
        display_page.append(&sliders_box);
        power_page.append(&power_row);

        Self {
            display_page,
            power_page,
            brightness_scale,
            brightness_btn,
            volume_scale,
            volume_icon,
            volume_btn,
            night_mode_scale,
            night_mode_btn,
            poweroff_btn: btn_poweroff,
            reboot_btn: btn_reboot,
            suspend_btn: btn_suspend,
            logout_btn: btn_logout,
        }
    }
}
