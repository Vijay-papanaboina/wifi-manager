use gtk4::{prelude::*, Box, Orientation, Scale, Image};

/// The unified panel for Brightness, Volume, and Night Mode controls.
pub struct ControlsPanel {
    pub container: Box,
    pub brightness_scale: Scale,
    pub volume_scale: Scale,
    pub volume_icon: Image,
    pub night_mode_scale: Scale,
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
            .spacing(8)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(16)
            .margin_end(16)
            .css_classes(["controls-panel"])
            .build();

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
            .draw_value(false)
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
            .draw_value(false)
            .tooltip_text("Volume")
            .adjustment(&gtk4::Adjustment::new(100.0, 0.0, 100.0, 1.0, 10.0, 0.0))
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
            
        let night_mode_scale = Scale::builder()
            .orientation(Orientation::Horizontal)
            .hexpand(true)
            .draw_value(false)
            .tooltip_text("Night Mode (Color Temperature)")
            .adjustment(&gtk4::Adjustment::new(6500.0, 2500.0, 6500.0, 100.0, 500.0, 0.0))
            // Inverted so slider right = warmer (2500K), left = cooler (6500K)
            .inverted(true)
            .build();

        night_mode_row.append(&night_mode_icon);
        night_mode_row.append(&night_mode_scale);

        // Assemble
        container.append(&brightness_row);
        container.append(&volume_row);
        container.append(&night_mode_row);

        Self {
            container,
            brightness_scale,
            volume_scale,
            volume_icon,
            night_mode_scale,
        }
    }
}
