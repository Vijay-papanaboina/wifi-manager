//! Audio detail page widgets.
//!
//! Discovery, action dispatch, and snapshot lifetime belong to
//! `app::audio`; these handles keep the GTK tree typed.  The layout is kept
//! compact so device selection and volume actions remain usable at the same
//! width as the Wi-Fi and Bluetooth detail pages.

use gtk4::{
    Align, Box as GtkBox, Button, Label, ListBox, Orientation, Scale, ScrolledWindow,
    SelectionMode, Stack, StackTransitionType, ToggleButton, prelude::*,
};

#[derive(Clone)]
pub(crate) struct AudioWidgets {
    pub(crate) container: GtkBox,
    pub(crate) status: Label,
    pub(crate) output_current: Label,
    pub(crate) output_mute: Button,
    pub(crate) output_scale: Scale,
    pub(crate) output_list: ListBox,
    pub(crate) output_empty: Label,
    pub(crate) input_current: Label,
    pub(crate) input_mute: Button,
    pub(crate) input_scale: Scale,
    pub(crate) input_list: ListBox,
    pub(crate) input_empty: Label,
}

impl AudioWidgets {
    pub(crate) fn new() -> Self {
        let container = GtkBox::new(Orientation::Vertical, 8);
        container.add_css_class("cc-audio-content");
        container.set_vexpand(true);
        container.set_valign(Align::Fill);

        let subtab_bar = GtkBox::new(Orientation::Horizontal, 0);
        subtab_bar.add_css_class("subtab-bar");
        subtab_bar.set_margin_top(2);
        subtab_bar.set_margin_bottom(6);

        let output_tab = ToggleButton::with_label("Output");
        output_tab.add_css_class("subtab-button");
        output_tab.set_active(true);
        output_tab.set_hexpand(true);
        output_tab.set_tooltip_text(Some("Show output devices"));
        set_pointer_cursor(&output_tab);

        let input_tab = ToggleButton::with_label("Input");
        input_tab.add_css_class("subtab-button");
        input_tab.set_hexpand(true);
        input_tab.set_tooltip_text(Some("Show microphone devices"));
        set_pointer_cursor(&input_tab);
        output_tab.set_group(Some(&input_tab));
        subtab_bar.append(&output_tab);
        subtab_bar.append(&input_tab);
        container.append(&subtab_bar);

        let device_stack = Stack::new();
        device_stack.set_transition_type(StackTransitionType::Crossfade);
        device_stack.set_transition_duration(120);
        device_stack.set_vexpand(true);
        device_stack.set_valign(Align::Fill);
        device_stack.set_hexpand(true);
        device_stack.add_css_class("cc-audio-device-stack");

        // Each tab contains only its selected-device row and selectable list.
        let output_section = GtkBox::new(Orientation::Vertical, 4);
        output_section.add_css_class("cc-audio-section");
        output_section.set_vexpand(true);
        output_section.set_valign(Align::Fill);
        let (output_current_row, output_current) =
            current_device_row("audio-volume-high-symbolic", "No output selected");
        output_section.append(&output_current_row);
        let (output_scroll, output_list, output_empty) = device_list("No output devices found");
        output_section.append(&output_scroll);
        output_section.append(&output_empty);
        device_stack.add_named(&output_section, Some("output"));

        let input_section = GtkBox::new(Orientation::Vertical, 4);
        input_section.add_css_class("cc-audio-section");
        input_section.set_vexpand(true);
        input_section.set_valign(Align::Fill);
        let (input_current_row, input_current) =
            current_device_row("audio-input-microphone-symbolic", "No microphone selected");
        input_section.append(&input_current_row);
        let (input_scroll, input_list, input_empty) = device_list("No microphone devices found");
        input_section.append(&input_scroll);
        input_section.append(&input_empty);
        device_stack.add_named(&input_section, Some("input"));
        container.append(&device_stack);

        let stack_for_output = device_stack.clone();
        output_tab.connect_toggled(move |tab| {
            if tab.is_active() {
                stack_for_output.set_visible_child_name("output");
            }
        });
        let stack_for_input = device_stack.clone();
        input_tab.connect_toggled(move |tab| {
            if tab.is_active() {
                stack_for_input.set_visible_child_name("input");
            }
        });

        let volume_section = GtkBox::new(Orientation::Vertical, 4);
        volume_section.add_css_class("cc-audio-volume-section");
        volume_section.append(&section_heading("Volume & mute"));
        let (output_controls, output_mute, output_scale) =
            volume_control_row("Output", "Mute output", "Output volume");
        let (input_controls, input_mute, input_scale) =
            volume_control_row("Input", "Mute microphone", "Microphone input volume");
        volume_section.append(&output_controls);
        volume_section.append(&input_controls);
        container.append(&volume_section);

        let status = Label::new(Some("Connecting to audio server…"));
        status.set_halign(Align::Start);
        status.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        status.add_css_class("cc-detail-status");

        Self {
            container,
            status,
            output_current,
            output_mute,
            output_scale,
            output_list,
            output_empty,
            input_current,
            input_mute,
            input_scale,
            input_list,
            input_empty,
        }
    }
}

fn section_heading(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_halign(Align::Start);
    label.add_css_class("cc-section-heading");
    label
}

fn current_device_row(icon_name: &str, initial_name: &str) -> (GtkBox, Label) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("cc-current-device-row");
    row.set_valign(Align::Start);

    let icon = gtk4::Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    icon.add_css_class("cc-audio-device-icon");

    let current = Label::new(Some(initial_name));
    current.set_halign(Align::Start);
    current.set_hexpand(true);
    current.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    current.add_css_class("cc-current-device");

    row.append(&icon);
    row.append(&current);
    (row, current)
}

fn volume_control_row(
    name: &str,
    mute_tooltip: &str,
    scale_tooltip: &str,
) -> (GtkBox, Button, Scale) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("cc-volume-row");

    let label = Label::new(Some(name));
    label.set_halign(Align::Start);
    label.set_width_chars(6);
    label.add_css_class("cc-volume-label");

    let mute = Button::from_icon_name("audio-volume-high-symbolic");
    mute.add_css_class("flat");
    mute.add_css_class("circular");
    mute.add_css_class("cc-audio-action");
    mute.set_tooltip_text(Some(mute_tooltip));
    set_pointer_cursor(&mute);

    let scale = Scale::builder()
        .orientation(Orientation::Horizontal)
        .hexpand(true)
        .draw_value(true)
        .value_pos(gtk4::PositionType::Right)
        .adjustment(&gtk4::Adjustment::new(0.0, 0.0, 100.0, 1.0, 10.0, 0.0))
        .tooltip_text(scale_tooltip)
        .build();
    scale.set_hexpand(true);
    set_pointer_cursor(&scale);

    row.append(&label);
    row.append(&mute);
    row.append(&scale);
    (row, mute, scale)
}

fn device_list(empty_text: &str) -> (ScrolledWindow, ListBox, Label) {
    let list = ListBox::new();
    list.set_selection_mode(SelectionMode::Single);
    list.add_css_class("cc-device-list");
    list.set_vexpand(true);

    let scroll = ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_valign(Align::Fill);
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_child(Some(&list));
    scroll.add_css_class("cc-device-scroll");

    let empty = Label::new(Some(empty_text));
    empty.set_halign(Align::Start);
    empty.add_css_class("cc-empty-state");
    empty.set_visible(false);

    (scroll, list, empty)
}

fn set_pointer_cursor<W: IsA<gtk4::Widget>>(widget: &W) {
    if let Some(cursor) = gtk4::gdk::Cursor::from_name("pointer", None) {
        widget.set_cursor(Some(&cursor));
    }
}
