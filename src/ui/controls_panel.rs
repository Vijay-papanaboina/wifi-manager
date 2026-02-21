use gtk4::{prelude::*, Box, Orientation, Scale, Image, Revealer, ToggleButton, RevealerTransitionType};

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
            .has_frame(false)
            .margin_bottom(8) // Add some breathing room below the button itself
            .build();
            
        // The container holding all the sliders
        let sliders_box = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .build();

        // Revealer to animate the sliders box
        let revealer = Revealer::builder()
            .transition_type(RevealerTransitionType::SlideUp)
            .transition_duration(250)
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
            .adjustment(&gtk4::Adjustment::new(50.0, 0.0, 100.0, 1.0, 10.0, 0.0))
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

        // Assemble sliders into the inner box
        sliders_box.append(&brightness_row);
        sliders_box.append(&volume_row);
        sliders_box.append(&night_mode_row);
        
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
