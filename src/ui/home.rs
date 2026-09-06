//! Compact, reusable Control Center home tiles.
//!
//! The home view stays a passive renderer.  Controllers can replace neutral
//! states with values from their existing snapshots without rebuilding the
//! grid or reaching through an untyped widget tree.

use gtk4::{Align, Box as GtkBox, Button, Grid, Image, Label, Orientation, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Neutral states a controller may use before it has a backend value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TileState {
    Loading,
    Unavailable,
}

impl TileState {
    fn label(self) -> &'static str {
        match self {
            Self::Loading => "Loading…",
            Self::Unavailable => "Unavailable",
        }
    }
}

/// Typed handles for one home tile.
#[derive(Clone)]
pub(crate) struct HomeTileWidgets {
    button: Button,
    status: Label,
}

impl HomeTileWidgets {
    pub(crate) fn button(&self) -> &Button {
        &self.button
    }

    /// Reflect a controller-provided radio/power snapshot in the tile.
    pub(crate) fn set_active(&self, active: bool) {
        if active {
            self.button.add_css_class("cc-tile-active");
        } else {
            self.button.remove_css_class("cc-tile-active");
        }
    }

    /// Render a controller-provided status. `None` deliberately renders an
    /// honest unavailable state while keeping the detail page reachable.
    pub(crate) fn set_status(&self, status: Option<&str>) {
        self.status.set_text(status.unwrap_or("Unavailable"));
        self.button.set_sensitive(true);
    }

    /// Render a neutral state without inventing a hardware value.
    pub(crate) fn set_state(&self, state: TileState) {
        self.status.set_text(state.label());
        self.button.set_sensitive(true);
    }
}

#[derive(Default)]
struct AudioSummary {
    output: Option<String>,
    input: Option<String>,
    output_updated: bool,
    input_updated: bool,
}

/// Home-page handles exposed to application controllers.
#[derive(Clone)]
pub(crate) struct HomeWidgets {
    /// A deterministic two-column grid at the fixed panel width.
    pub(crate) container: Grid,
    pub(crate) wifi: HomeTileWidgets,
    pub(crate) bluetooth: HomeTileWidgets,
    /// The merged Audio tile.
    pub(crate) audio: HomeTileWidgets,
    pub(crate) power_battery: HomeTileWidgets,
    audio_summary: Rc<RefCell<AudioSummary>>,
}

impl HomeWidgets {
    pub(crate) fn new() -> Self {
        let container = Grid::new();
        container.set_column_homogeneous(true);
        container.set_row_spacing(8);
        container.set_column_spacing(8);
        container.set_margin_top(12);
        container.set_margin_bottom(10);
        container.set_margin_start(12);
        container.set_margin_end(12);
        container.set_hexpand(true);
        container.set_vexpand(false);
        container.add_css_class("cc-home-grid");

        let wifi = make_tile("network-wireless-symbolic", "Wi-Fi", "Open Wi-Fi details");
        let bluetooth =
            make_tile("bluetooth-active-symbolic", "Bluetooth", "Open Bluetooth details");
        let audio = make_tile("audio-volume-high-symbolic", "Audio", "Open Audio details");
        let power_battery = make_tile("battery-symbolic", "Power / Battery", "Open Power details");

        // The quick controls below this grid own display actions directly.
        container.attach(wifi.button(), 0, 0, 1, 1);
        container.attach(bluetooth.button(), 1, 0, 1, 1);
        container.attach(audio.button(), 0, 1, 1, 1);
        container.attach(power_battery.button(), 1, 1, 1, 1);

        let audio_summary = Rc::new(RefCell::new(AudioSummary::default()));
        let widgets = Self { container, wifi, bluetooth, audio, power_battery, audio_summary };
        widgets.render_audio_summary();
        widgets
    }

    pub(crate) fn set_wifi_status(&self, status: Option<&str>) {
        self.wifi.set_status(status);
    }

    pub(crate) fn set_wifi_enabled(&self, enabled: bool) {
        self.wifi.set_active(enabled);
    }

    pub(crate) fn set_bluetooth_status(&self, status: Option<&str>) {
        self.bluetooth.set_status(status);
    }

    pub(crate) fn set_bluetooth_enabled(&self, enabled: bool) {
        self.bluetooth.set_active(enabled);
    }

    /// Update the merged Audio subtitle. Existing audio controller callbacks
    /// call the output and input methods independently; the shared state
    /// keeps both values visible in one ellipsized label.
    pub(crate) fn set_audio_output_status(&self, status: Option<&str>) {
        {
            let mut summary = self.audio_summary.borrow_mut();
            summary.output = status.map(str::to_owned);
            summary.output_updated = true;
        }
        self.render_audio_summary();
    }

    pub(crate) fn set_microphone_input_status(&self, status: Option<&str>) {
        {
            let mut summary = self.audio_summary.borrow_mut();
            summary.input = status.map(str::to_owned);
            summary.input_updated = true;
        }
        self.render_audio_summary();
    }

    pub(crate) fn set_power_battery_status(&self, status: Option<&str>) {
        self.power_battery.set_status(status);
    }

    fn render_audio_summary(&self) {
        let summary = self.audio_summary.borrow();
        let output = summary.output.as_deref().unwrap_or("Unavailable");
        let input = summary.input.as_deref().unwrap_or("Unavailable");
        let text = if !summary.output_updated && !summary.input_updated {
            "Output: Loading… · Input: Loading…".to_string()
        } else {
            format!("Output: {output} · Input: {input}")
        };
        self.audio.status.set_text(&text);
    }
}

fn make_tile(icon_name: &str, title: &str, tooltip: &str) -> HomeTileWidgets {
    let button = Button::new();
    button.add_css_class("cc-tile");
    button.set_tooltip_text(Some(tooltip));
    button.set_hexpand(true);
    button.set_vexpand(false);
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        button.set_cursor(Some(&cursor));
    }

    // Keep the icon beside the title, with the status spanning the full
    // content width on the row below it.
    let content = GtkBox::new(Orientation::Vertical, 4);
    content.set_halign(Align::Fill);
    content.set_valign(Align::Center);
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(10);
    content.set_margin_end(10);

    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(22);
    icon.set_valign(Align::Center);
    icon.add_css_class("cc-tile-icon");

    let title_row = GtkBox::new(Orientation::Horizontal, 10);
    title_row.set_hexpand(true);
    title_row.set_halign(Align::Fill);
    title_row.set_valign(Align::Center);

    let title_label = Label::new(Some(title));
    title_label.set_halign(Align::Start);
    title_label.set_hexpand(true);
    title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title_label.add_css_class("cc-tile-title");

    let status = Label::new(Some(TileState::Loading.label()));
    status.set_halign(Align::Start);
    status.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    status.set_hexpand(true);
    status.add_css_class("cc-tile-status");

    title_row.append(&icon);
    title_row.append(&title_label);
    content.append(&title_row);
    content.append(&status);
    button.set_child(Some(&content));

    HomeTileWidgets { button, status }
}
