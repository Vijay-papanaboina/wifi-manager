//! Audio manager and Audio detail-page integration.
//!
//! This controller owns the one PulseAudio-compatible connection used by the
//! panel.  The legacy Display volume row and the Audio detail page both route
//! through it, so a missing server produces one honest unavailable state and
//! never creates competing subscriptions.

use gtk4::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::controls::audio::AudioManager;
use crate::domain::audio::{AudioDevice, AudioSnapshot, device_status_label};
use crate::ui::{audio::AudioWidgets, window::PanelWidgets};

const MUTED_VOLUME_ICON: &str = "audio-volume-muted-symbolic";

#[derive(Default)]
struct AudioUiState {
    snapshot: AudioSnapshot,
    output_ids: Vec<String>,
    input_ids: Vec<String>,
    available: bool,
    terminal_unavailable: bool,
}

#[derive(Clone)]
struct AudioUpdateGuards {
    list: Rc<Cell<bool>>,
    output_scale: Rc<Cell<bool>>,
    input_scale: Rc<Cell<bool>>,
    display_scale: Rc<Cell<bool>>,
}

impl AudioUpdateGuards {
    fn new() -> Self {
        Self {
            list: Rc::new(Cell::new(false)),
            output_scale: Rc::new(Cell::new(false)),
            input_scale: Rc::new(Cell::new(false)),
            display_scale: Rc::new(Cell::new(false)),
        }
    }
}

#[derive(Clone)]
struct DisplayVolumeControls {
    scale: gtk4::Scale,
    button: gtk4::Button,
    icon: gtk4::Image,
}

impl DisplayVolumeControls {
    fn from_panel(widgets: &PanelWidgets) -> Self {
        Self {
            scale: widgets.controls.volume_scale().clone(),
            button: widgets.controls.volume_btn().clone(),
            icon: widgets.controls.volume_icon().clone(),
        }
    }
}

/// Start audio discovery and bind all Audio/Display controls.
pub(crate) fn setup(widgets: &PanelWidgets) {
    let view = widgets.audio.clone();
    let home = widgets.home.clone();
    let state = Rc::new(RefCell::new(AudioUiState::default()));
    let updates = AudioUpdateGuards::new();
    let display = DisplayVolumeControls::from_panel(widgets);

    render_unavailable(
        &view,
        &home,
        &display,
        &updates.display_scale,
        "Connecting to audio server…",
    );

    let state_for_snapshot = Rc::clone(&state);
    let view_for_snapshot = view.clone();
    let home_for_snapshot = home.clone();
    let updates_for_snapshot = updates.clone();
    let display_for_snapshot = display.clone();

    let state_for_result = Rc::clone(&state);
    let view_for_result = view.clone();
    let updates_for_result = updates.clone();
    let state_for_unavailable = Rc::clone(&state);
    let view_for_unavailable = view.clone();
    let home_for_unavailable = home.clone();
    let display_for_unavailable = display.clone();
    let updates_for_unavailable = updates.clone();

    let manager = match AudioManager::new(
        move |snapshot| {
            render_snapshot(
                &view_for_snapshot,
                &home_for_snapshot,
                &state_for_snapshot,
                snapshot,
                &updates_for_snapshot,
                &display_for_snapshot,
            );
        },
        move |result| match result {
            Ok(()) => {
                if state_for_result.borrow().available {
                    view_for_result.status.set_text("Audio server connected");
                }
            }
            Err(error) => {
                log::warn!("Audio manager: {error}");
                view_for_result.status.set_text(&format!("Audio error: {error}"));
                restore_list_selection(
                    &view_for_result,
                    &state_for_result,
                    &updates_for_result.list,
                );
            }
        },
        move |error| {
            {
                let mut state = state_for_unavailable.borrow_mut();
                state.available = false;
                state.terminal_unavailable = true;
            }
            render_unavailable(
                &view_for_unavailable,
                &home_for_unavailable,
                &display_for_unavailable,
                &updates_for_unavailable.display_scale,
                &format!("Audio unavailable: {error}"),
            );
        },
    ) {
        Ok(manager) => manager,
        Err(error) => {
            render_unavailable(
                &view,
                &home,
                &display,
                &updates.display_scale,
                &format!("Audio unavailable: {error}"),
            );
            return;
        }
    };

    if !state.borrow().terminal_unavailable {
        state.borrow_mut().available = true;
    }
    wire_actions(&view, &display, Rc::clone(&state), &updates, Rc::clone(&manager));

    // The manager is retained by the action closures.  This weak watchdog is
    // useful when a panel is rebuilt during a GTK application reload: it does
    // not introduce another audio connection or subscription.
    let manager_weak = Rc::downgrade(&manager);
    glib::timeout_add_local(std::time::Duration::from_secs(30), move || {
        if manager_weak.upgrade().is_some() {
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    });
}

fn wire_actions(
    view: &AudioWidgets,
    display: &DisplayVolumeControls,
    state: Rc<RefCell<AudioUiState>>,
    updates: &AudioUpdateGuards,
    manager: Rc<AudioManager>,
) {
    view.output_scale.set_format_value_func(|_, value| format!("{}%", value.round() as i32));
    view.input_scale.set_format_value_func(|_, value| format!("{}%", value.round() as i32));
    display.scale.set_format_value_func(|_, value| format!("{}%", value.round() as i32));

    let manager_for_output_scale = Rc::clone(&manager);
    let state_for_output_scale = Rc::clone(&state);
    let view_for_output_scale = view.clone();
    let output_guard = Rc::clone(&updates.output_scale);
    view.output_scale.connect_value_changed(move |scale| {
        if output_guard.get() {
            return;
        }
        let Some(id) = state_for_output_scale.borrow().snapshot.default_output_id.clone() else {
            view_for_output_scale.status.set_text("Audio output unavailable");
            return;
        };
        view_for_output_scale.status.set_text("Setting output volume…");
        if let Err(error) = manager_for_output_scale.set_output_volume(&id, scale.value()) {
            view_for_output_scale.status.set_text(&format!("Audio error: {error}"));
        }
    });

    let manager_for_input_scale = Rc::clone(&manager);
    let state_for_input_scale = Rc::clone(&state);
    let view_for_input_scale = view.clone();
    let input_guard = Rc::clone(&updates.input_scale);
    view.input_scale.connect_value_changed(move |scale| {
        if input_guard.get() {
            return;
        }
        let Some(id) = state_for_input_scale.borrow().snapshot.default_input_id.clone() else {
            view_for_input_scale.status.set_text("Microphone input unavailable");
            return;
        };
        view_for_input_scale.status.set_text("Setting microphone volume…");
        if let Err(error) = manager_for_input_scale.set_input_volume(&id, scale.value()) {
            view_for_input_scale.status.set_text(&format!("Audio error: {error}"));
        }
    });

    let manager_for_display_scale = Rc::clone(&manager);
    let state_for_display_scale = Rc::clone(&state);
    let view_for_display_scale = view.clone();
    let display_guard = Rc::clone(&updates.display_scale);
    let display_scale = display.scale.clone();
    display_scale.connect_value_changed(move |scale| {
        if display_guard.get() {
            return;
        }
        let Some(id) = state_for_display_scale.borrow().snapshot.default_output_id.clone() else {
            view_for_display_scale.status.set_text("Audio output unavailable");
            return;
        };
        view_for_display_scale.status.set_text("Setting output volume…");
        if let Err(error) = manager_for_display_scale.set_output_volume(&id, scale.value()) {
            view_for_display_scale.status.set_text(&format!("Audio error: {error}"));
        }
    });

    let manager_for_output_mute = Rc::clone(&manager);
    let state_for_output_mute = Rc::clone(&state);
    let view_for_output_mute = view.clone();
    view.output_mute.connect_clicked(move |_| {
        let snapshot = state_for_output_mute.borrow();
        let Some(id) = snapshot.snapshot.default_output_id.clone() else {
            view_for_output_mute.status.set_text("Audio output unavailable");
            return;
        };
        let Some(device) = snapshot.snapshot.outputs.iter().find(|device| device.id == id) else {
            view_for_output_mute.status.set_text("Audio output unavailable");
            return;
        };
        let muted = !device.muted;
        drop(snapshot);
        view_for_output_mute.status.set_text("Setting output mute…");
        if let Err(error) = manager_for_output_mute.set_output_mute(&id, muted) {
            view_for_output_mute.status.set_text(&format!("Audio error: {error}"));
        }
    });

    let manager_for_input_mute = Rc::clone(&manager);
    let state_for_input_mute = Rc::clone(&state);
    let view_for_input_mute = view.clone();
    view.input_mute.connect_clicked(move |_| {
        let snapshot = state_for_input_mute.borrow();
        let Some(id) = snapshot.snapshot.default_input_id.clone() else {
            view_for_input_mute.status.set_text("Microphone input unavailable");
            return;
        };
        let Some(device) = snapshot.snapshot.inputs.iter().find(|device| device.id == id) else {
            view_for_input_mute.status.set_text("Microphone input unavailable");
            return;
        };
        let muted = !device.muted;
        drop(snapshot);
        view_for_input_mute.status.set_text("Setting microphone mute…");
        if let Err(error) = manager_for_input_mute.set_input_mute(&id, muted) {
            view_for_input_mute.status.set_text(&format!("Audio error: {error}"));
        }
    });

    let manager_for_display_mute = Rc::clone(&manager);
    let state_for_display_mute = Rc::clone(&state);
    let view_for_display_mute = view.clone();
    let display_button = display.button.clone();
    display_button.connect_clicked(move |_| {
        let snapshot = state_for_display_mute.borrow();
        let Some(id) = snapshot.snapshot.default_output_id.clone() else {
            view_for_display_mute.status.set_text("Audio output unavailable");
            return;
        };
        let Some(device) = snapshot.snapshot.outputs.iter().find(|device| device.id == id) else {
            view_for_display_mute.status.set_text("Audio output unavailable");
            return;
        };
        let muted = !device.muted;
        drop(snapshot);
        view_for_display_mute.status.set_text("Setting output mute…");
        if let Err(error) = manager_for_display_mute.set_output_mute(&id, muted) {
            view_for_display_mute.status.set_text(&format!("Audio error: {error}"));
        }
    });

    let manager_for_output_list = Rc::clone(&manager);
    let state_for_output_list = Rc::clone(&state);
    let view_for_output_list = view.clone();
    let list_guard = Rc::clone(&updates.list);
    view.output_list.connect_row_selected(move |_, row| {
        if list_guard.get() {
            return;
        }
        let Some(row) = row else { return };
        let index = row.index();
        if index < 0 {
            return;
        }
        let snapshot = state_for_output_list.borrow();
        let Some(id) = snapshot.output_ids.get(index as usize).cloned() else {
            return;
        };
        if snapshot.snapshot.default_output_id.as_deref() == Some(id.as_str()) {
            return;
        }
        drop(snapshot);
        view_for_output_list.status.set_text("Setting default output…");
        if let Err(error) = manager_for_output_list.set_default_output(&id) {
            view_for_output_list.status.set_text(&format!("Audio error: {error}"));
        }
    });

    let manager_for_input_list = Rc::clone(&manager);
    let state_for_input_list = Rc::clone(&state);
    let view_for_input_list = view.clone();
    let list_guard = Rc::clone(&updates.list);
    view.input_list.connect_row_selected(move |_, row| {
        if list_guard.get() {
            return;
        }
        let Some(row) = row else { return };
        let index = row.index();
        if index < 0 {
            return;
        }
        let snapshot = state_for_input_list.borrow();
        let Some(id) = snapshot.input_ids.get(index as usize).cloned() else {
            return;
        };
        if snapshot.snapshot.default_input_id.as_deref() == Some(id.as_str()) {
            return;
        }
        drop(snapshot);
        view_for_input_list.status.set_text("Setting default microphone…");
        if let Err(error) = manager_for_input_list.set_default_input(&id) {
            view_for_input_list.status.set_text(&format!("Audio error: {error}"));
        }
    });
}

fn render_snapshot(
    view: &AudioWidgets,
    home: &crate::ui::home::HomeWidgets,
    state: &Rc<RefCell<AudioUiState>>,
    snapshot: AudioSnapshot,
    updates: &AudioUpdateGuards,
    display: &DisplayVolumeControls,
) {
    let output = default_device(&snapshot.outputs, snapshot.default_output_id.as_deref());
    let input = default_device(&snapshot.inputs, snapshot.default_input_id.as_deref());

    {
        let mut ui = state.borrow_mut();
        if ui.terminal_unavailable {
            return;
        }
        ui.snapshot = snapshot.clone();
        ui.output_ids = snapshot.outputs.iter().map(|device| device.id.clone()).collect();
        ui.input_ids = snapshot.inputs.iter().map(|device| device.id.clone()).collect();
        ui.available = true;
    }

    updates.list.set(true);
    populate_device_list(&view.output_list, &view.output_empty, &snapshot.outputs, "output");
    populate_device_list(&view.input_list, &view.input_empty, &snapshot.inputs, "microphone");
    select_default(&view.output_list, &snapshot.outputs, snapshot.default_output_id.as_deref());
    select_default(&view.input_list, &snapshot.inputs, snapshot.default_input_id.as_deref());
    updates.list.set(false);

    set_current_device(
        &view.output_current,
        &view.output_mute,
        &view.output_scale,
        output,
        &updates.output_scale,
    );
    set_current_device(
        &view.input_current,
        &view.input_mute,
        &view.input_scale,
        input,
        &updates.input_scale,
    );
    render_display_controls(display, output, &updates.display_scale);
    view.output_list.set_sensitive(!snapshot.outputs.is_empty());
    view.input_list.set_sensitive(!snapshot.inputs.is_empty());
    view.output_mute.set_sensitive(output.is_some());
    view.input_mute.set_sensitive(input.is_some());
    view.output_scale.set_sensitive(output.is_some());
    view.input_scale.set_sensitive(input.is_some());
    view.status.set_text("Audio server connected");

    home.set_audio_output_status(output.map(device_status_label).as_deref());
    home.set_microphone_input_status(input.map(device_status_label).as_deref());
}

fn render_unavailable(
    view: &AudioWidgets,
    home: &crate::ui::home::HomeWidgets,
    display: &DisplayVolumeControls,
    display_scale_update: &Cell<bool>,
    message: &str,
) {
    view.status.set_text(message);
    view.output_current.set_text("No output available");
    view.input_current.set_text("No microphone available");
    view.output_mute.set_icon_name(MUTED_VOLUME_ICON);
    view.input_mute.set_icon_name(MUTED_VOLUME_ICON);
    view.output_mute.set_sensitive(false);
    view.input_mute.set_sensitive(false);
    view.output_scale.set_sensitive(false);
    view.input_scale.set_sensitive(false);
    view.output_list.set_sensitive(false);
    view.input_list.set_sensitive(false);
    view.output_empty.set_text("Audio server unavailable");
    view.input_empty.set_text("Audio server unavailable");
    view.output_empty.set_visible(true);
    view.input_empty.set_visible(true);
    view.output_list.set_visible(false);
    view.input_list.set_visible(false);
    render_display_controls(display, None, display_scale_update);
    home.set_audio_output_status(None);
    home.set_microphone_input_status(None);
}

fn render_display_controls(
    display: &DisplayVolumeControls,
    output: Option<&AudioDevice>,
    scale_update: &Cell<bool>,
) {
    let (volume, icon, available) = match output {
        Some(output) => (f64::from(output.volume_percent), volume_icon(output), true),
        None => (0.0, MUTED_VOLUME_ICON, false),
    };

    display.button.set_sensitive(available);
    display.scale.set_sensitive(available);
    scale_update.set(true);
    display.scale.set_value(volume);
    scale_update.set(false);
    display.button.set_icon_name(icon);
    display.icon.set_icon_name(Some(icon));
}

fn restore_list_selection(
    view: &AudioWidgets,
    state: &Rc<RefCell<AudioUiState>>,
    list_update: &Cell<bool>,
) {
    let snapshot = state.borrow();
    list_update.set(true);
    select_default(
        &view.output_list,
        &snapshot.snapshot.outputs,
        snapshot.snapshot.default_output_id.as_deref(),
    );
    select_default(
        &view.input_list,
        &snapshot.snapshot.inputs,
        snapshot.snapshot.default_input_id.as_deref(),
    );
    list_update.set(false);
}

fn default_device<'a>(devices: &'a [AudioDevice], id: Option<&str>) -> Option<&'a AudioDevice> {
    id.and_then(|id| devices.iter().find(|device| device.id == id))
        .or_else(|| devices.iter().find(|device| device.is_default))
}

fn set_current_device(
    name: &gtk4::Label,
    mute: &gtk4::Button,
    scale: &gtk4::Scale,
    device: Option<&AudioDevice>,
    scale_update: &Cell<bool>,
) {
    let Some(device) = device else {
        name.set_text("No device selected");
        mute.set_icon_name("audio-volume-muted-symbolic");
        mute.set_sensitive(false);
        scale_update.set(true);
        scale.set_value(0.0);
        scale_update.set(false);
        scale.set_sensitive(false);
        return;
    };

    name.set_text(&device_status_label(device));
    mute.set_icon_name(volume_icon(device));
    mute.set_sensitive(true);
    scale_update.set(true);
    scale.set_value(f64::from(device.volume_percent));
    scale_update.set(false);
    scale.set_sensitive(true);
}

fn volume_icon(device: &AudioDevice) -> &'static str {
    if device.muted {
        "audio-volume-muted-symbolic"
    } else if device.volume_percent < 33 {
        "audio-volume-low-symbolic"
    } else if device.volume_percent < 66 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

fn populate_device_list(
    list: &gtk4::ListBox,
    empty: &gtk4::Label,
    devices: &[AudioDevice],
    kind: &str,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    empty.set_visible(devices.is_empty());
    list.set_visible(!devices.is_empty());
    for device in devices {
        let row = gtk4::ListBoxRow::new();
        row.set_selectable(true);
        row.set_activatable(true);
        row.set_tooltip_text(Some(&format!("Select {} as default", device.label)));

        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        content.set_margin_top(6);
        content.set_margin_bottom(6);
        content.set_margin_start(8);
        content.set_margin_end(8);
        let label = gtk4::Label::new(Some(&device.label));
        label.set_halign(gtk4::Align::Start);
        label.set_hexpand(true);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        label.set_tooltip_text(Some(&format!("{} ({})", device.label, device.id)));
        content.append(&label);
        if device.is_default {
            let default_label = gtk4::Label::new(Some("Default"));
            default_label.add_css_class("cc-default-device");
            default_label.set_tooltip_text(Some(&format!("Current default {kind}")));
            content.append(&default_label);
        }
        row.set_child(Some(&content));
        list.append(&row);
    }
}

fn select_default(list: &gtk4::ListBox, devices: &[AudioDevice], default_id: Option<&str>) {
    let index = default_id
        .and_then(|id| devices.iter().position(|device| device.id == id))
        .or_else(|| devices.iter().position(|device| device.is_default));
    if let Some(index) = index {
        if let Some(row) = list.row_at_index(index as i32) {
            list.select_row(Some(&row));
        }
    } else {
        list.unselect_all();
    }
}
