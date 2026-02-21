use gtk4::{prelude::*, Box, Orientation, Scale, Image, Revealer, ToggleButton, RevealerTransitionType, Button};
use crate::controls::power;

/// Duration of the slider reveal animation in milliseconds
pub const SLIDE_TRANSITION_MS: u32 = 250;

/// The unified panel for Brightness, Volume, and Night Mode controls.
pub struct ControlsPanel {
    pub container: Box,
    pub brightness_scale: Scale,
    pub volume_scale: Scale,
    pub volume_icon: Image,
    pub night_mode_scale: Scale,
    pub revealer: Revealer,
    pub toggle_button: ToggleButton,
}

impl Default for ControlsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlsPanel {
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

        let btn_poweroff = Button::builder().icon_name("system-shutdown-symbolic").build();
        btn_poweroff.add_css_class("flat");
        btn_poweroff.add_css_class("circular");
        btn_poweroff.connect_clicked(|_| power::poweroff());
        
        let btn_reboot = Button::builder().icon_name("system-reboot-symbolic").build();
        btn_reboot.add_css_class("flat");
        btn_reboot.add_css_class("circular");
        btn_reboot.connect_clicked(|_| power::reboot());
        
        let btn_suspend = Button::builder().icon_name("media-playback-pause-symbolic").build();
        btn_suspend.add_css_class("flat");
        btn_suspend.add_css_class("circular");
        btn_suspend.connect_clicked(|_| power::suspend());
        
        let btn_logout = Button::builder().icon_name("system-log-out-symbolic").build();
        btn_logout.add_css_class("flat");
        btn_logout.add_css_class("circular");
        btn_logout.connect_clicked(|_| power::logout());

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
