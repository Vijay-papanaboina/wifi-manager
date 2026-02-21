use gtk4::{prelude::*, Box, Orientation, Scale, Image, Revealer, ToggleButton, RevealerTransitionType, Button, MessageDialog, MessageType, ButtonsType, ResponseType, Window};
use std::rc::Rc;
use std::cell::RefCell;
use crate::controls::power;

/// Duration of the slider reveal animation in milliseconds
pub const SLIDE_TRANSITION_MS: u32 = 250;

#[allow(deprecated)]
fn show_confirm_dialog(btn: &Button, title: &str, message: &str, action: impl FnOnce() + 'static) {
    let window = btn.root().and_downcast::<Window>();
    let dialog = MessageDialog::builder()
        .modal(true)
        .message_type(MessageType::Question)
        .buttons(ButtonsType::OkCancel)
        .text(title)
        .secondary_text(message)
        .build();
    
    if let Some(win) = window {
        dialog.set_transient_for(Some(&win));
    }
    
    let action_cell = Rc::new(RefCell::new(Some(action)));
    
    dialog.connect_response(move |dlg, response| {
        if response == ResponseType::Ok {
            if let Some(act) = action_cell.borrow_mut().take() {
                act();
            }
        }
        dlg.destroy();
    });
    dialog.present();
}

#[allow(deprecated)]
fn show_error_dialog(window: Option<&Window>, message: &str) {
    let dialog = MessageDialog::builder()
        .modal(true)
        .message_type(MessageType::Error)
        .buttons(ButtonsType::Ok)
        .text("Error")
        .secondary_text(message)
        .build();
    
    if let Some(win) = window {
        dialog.set_transient_for(Some(win));
    }
    
    dialog.connect_response(|dlg, _| dlg.destroy());
    dialog.present();
}

/// The unified panel for Brightness, Volume, and Night Mode controls.
#[allow(dead_code)]
pub struct ControlsPanel {
    container: Box,
    brightness_scale: Scale,
    volume_scale: Scale,
    volume_icon: Image,
    night_mode_scale: Scale,
    revealer: Revealer,
    toggle_button: ToggleButton,
}

impl Default for ControlsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlsPanel {
    pub fn container(&self) -> &Box { &self.container }
    pub fn brightness_scale(&self) -> &Scale { &self.brightness_scale }
    pub fn volume_scale(&self) -> &Scale { &self.volume_scale }
    pub fn volume_icon(&self) -> &Image { &self.volume_icon }
    pub fn night_mode_scale(&self) -> &Scale { &self.night_mode_scale }
    pub fn revealer(&self) -> &Revealer { &self.revealer }
    pub fn toggle_button(&self) -> &ToggleButton { &self.toggle_button }

    pub fn new() -> Self {
        let container = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(8)
            .margin_bottom(0) // Let inner elements dictate bottom spacing
            .margin_start(16)
            .margin_end(16)
            .css_classes(["controls-panel"])
            .build();
            
        // Toggle Button for collapsing/expanding
        let toggle_button = ToggleButton::builder()
            .icon_name("pan-down-symbolic")
            .halign(gtk4::Align::Center)
            .margin_bottom(8) // Add some breathing room below the button itself
            .tooltip_text("Show/Hide Controls")
            .build();
        toggle_button.add_css_class("flat");
        toggle_button.add_css_class("circular");
            
        // The container holding all the sliders
        let sliders_box = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .build();

        // Revealer to animate the sliders box
        let revealer = Revealer::builder()
            .transition_type(RevealerTransitionType::SlideDown)
            .transition_duration(SLIDE_TRANSITION_MS)
            .child(&sliders_box)
            .reveal_child(false) // Start collapsed
            .build();

        // ── Connect toggle button to revealer ──
        let r_clone = revealer.clone();
        toggle_button.connect_toggled(move |btn| {
            let active = btn.is_active();
            r_clone.set_reveal_child(active);
            if active {
                btn.set_icon_name("pan-up-symbolic");
            } else {
                btn.set_icon_name("pan-down-symbolic");
            }
        });

        // Brightness Row
        let brightness_row = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();
            
        let brightness_icon = Image::builder()
            .icon_name("display-brightness-symbolic")
            .pixel_size(16)
            .build();
            
        let brightness_scale = Scale::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .draw_value(true)
            .value_pos(gtk4::PositionType::Right)
            .tooltip_text("Brightness")
            .adjustment(&gtk4::Adjustment::new(100.0, 5.0, 100.0, 1.0, 10.0, 0.0))
            .build();

        brightness_row.append(&brightness_icon);
        brightness_row.append(&brightness_scale);

        // Volume Row
        let volume_row = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();
            
        let volume_icon = Image::builder()
            .icon_name("audio-volume-high-symbolic")
            .pixel_size(16)
            .build();
            
        let volume_scale = Scale::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .draw_value(true)
            .value_pos(gtk4::PositionType::Right)
            .tooltip_text("Volume")
            .adjustment(&gtk4::Adjustment::new(0.0, 0.0, 100.0, 1.0, 10.0, 0.0))
            .build();

        volume_row.append(&volume_icon);
        volume_row.append(&volume_scale);

        // Night Mode Row
        let night_mode_row = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .build();
            
        let night_mode_icon = Image::builder()
            .icon_name("weather-clear-night-symbolic")
            .pixel_size(16)
            .build();
            
        // Map 0 -> 6500K (coolest/no effect), 4000 -> 2500K (warmest)
        let night_mode_scale = Scale::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .draw_value(true)
            .value_pos(gtk4::PositionType::Right)
            .tooltip_text("Night Mode (Color Temperature)")
            .adjustment(&gtk4::Adjustment::new(0.0, 0.0, 4000.0, 100.0, 500.0, 0.0))
            .build();

        night_mode_row.append(&night_mode_icon);
        night_mode_row.append(&night_mode_scale);

        // Power Controls Row
        let power_row = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(36) // Larger gap between buttons
            .halign(gtk4::Align::Center)
            .margin_top(12)
            .margin_bottom(12) // Gap from the bottom window edge
            .build();

        fn connect_power_button(
            btn: &Button,
            title: &str,
            message: &str,
            action: impl Fn() -> Result<(), String> + Clone + 'static,
        ) {
            let title = title.to_string();
            let message = message.to_string();
            btn.connect_clicked(move |b| {
                let win = b.root().and_downcast::<Window>();
                let action = action.clone();
                let title_clone = title.clone();
                show_confirm_dialog(b, &title, &message, move || {
                    if let Err(e) = action() {
                        log::error!("{}: {}", title_clone, e);
                        show_error_dialog(win.as_ref(), &e);
                    }
                });
            });
        }

        let btn_poweroff = Button::builder()
            .icon_name("system-shutdown-symbolic")
            .tooltip_text("Power Off")
            .build();
        btn_poweroff.add_css_class("flat");
        btn_poweroff.add_css_class("circular");
        connect_power_button(&btn_poweroff, "Power Off", "Are you sure you want to power off the system?", power::poweroff);
        
        let btn_reboot = Button::builder()
            .icon_name("system-reboot-symbolic")
            .tooltip_text("Reboot")
            .build();
        btn_reboot.add_css_class("flat");
        btn_reboot.add_css_class("circular");
        connect_power_button(&btn_reboot, "Reboot", "Are you sure you want to reboot the system?", power::reboot);
        
        let btn_suspend = Button::builder()
            .icon_name("weather-clear-night-symbolic")
            .tooltip_text("Suspend / Sleep")
            .build();
        btn_suspend.add_css_class("flat");
        btn_suspend.add_css_class("circular");
        connect_power_button(&btn_suspend, "Suspend", "Are you sure you want to suspend the system?", power::suspend);
        
        let btn_logout = Button::builder()
            .icon_name("system-log-out-symbolic")
            .tooltip_text("Log Out")
            .build();
        btn_logout.add_css_class("flat");
        btn_logout.add_css_class("circular");
        connect_power_button(&btn_logout, "Logout", "Are you sure you want to log out?", power::logout);

        power_row.append(&btn_logout);
        power_row.append(&btn_suspend);
        power_row.append(&btn_reboot);
        power_row.append(&btn_poweroff);

        // Assemble sliders into the inner box
        sliders_box.append(&brightness_row);
        sliders_box.append(&volume_row);
        sliders_box.append(&night_mode_row);
        sliders_box.append(&power_row);
        
        // Assemble main container logic
        container.append(&toggle_button); // Pin button above
        container.append(&revealer);      // Let sliders drop below

        Self {
            container,
            brightness_scale,
            volume_scale,
            volume_icon,
            night_mode_scale,
            revealer,
            toggle_button,
        }
    }
}
