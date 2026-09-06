use gtk4::glib;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::introspect::{SinkInfo, SourceInfo};
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet};
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet, State};
use libpulse_binding::proplist::Proplist;
use libpulse_binding::volume::Volume;
use libpulse_glib_binding::Mainloop;
use log::{error, warn};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::domain::audio::{
    AudioDevice, AudioDeviceKind, AudioSnapshot, clamp_percent, description_or_id,
    sort_audio_devices, source_is_eligible,
};

const CONNECTION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CONNECTION_POLL_LIMIT: u32 = 50;

struct RefreshState {
    epoch: u64,
    server_done: bool,
    sinks_done: bool,
    sources_done: bool,
    default_output_id: Option<String>,
    default_input_id: Option<String>,
    outputs: Vec<AudioDevice>,
    inputs: Vec<AudioDevice>,
    failed: bool,
    error_reported: bool,
    emitted: bool,
}

impl RefreshState {
    fn new(epoch: u64) -> Self {
        Self {
            epoch,
            server_done: false,
            sinks_done: false,
            sources_done: false,
            default_output_id: None,
            default_input_id: None,
            outputs: Vec::new(),
            inputs: Vec::new(),
            failed: false,
            error_reported: false,
            emitted: false,
        }
    }
}

/// Manages PulseAudio's native protocol through the GLib main loop.
///
/// PulseAudio and PipeWire-Pulse expose the same protocol used here; no hardware-specific
/// facilities are queried. The main loop and context are retained by this manager so asynchronous
/// callbacks remain valid for its lifetime.
pub(crate) struct AudioManager {
    _mainloop: Rc<RefCell<Mainloop>>,
    context: Rc<RefCell<Context>>,
    on_snapshot: Rc<dyn Fn(AudioSnapshot)>,
    on_result: Rc<dyn Fn(Result<(), String>)>,
    on_unavailable: Rc<dyn Fn(String)>,
    refresh_epoch: Cell<u64>,
    refresh_scheduled: Cell<bool>,
    connection_finished: Cell<bool>,
    unavailable_reported: Cell<bool>,
    ready: Cell<bool>,
}

impl AudioManager {
    /// Construct and connect an audio manager.
    ///
    /// The result callback receives asynchronous action and refresh errors. A successful action
    /// is reported before a fresh server snapshot is fetched. Terminal context failures use the
    /// separate unavailable callback so they cannot be mistaken for ordinary page errors.
    pub(crate) fn new<F, C, U>(
        on_snapshot: F,
        on_result: C,
        on_unavailable: U,
    ) -> Result<Rc<Self>, String>
    where
        F: Fn(AudioSnapshot) + 'static,
        C: Fn(Result<(), String>) + 'static,
        U: Fn(String) + 'static,
    {
        let mut proplist = Proplist::new().ok_or("Failed to create PulseAudio proplist")?;
        proplist
            .set_str(libpulse_binding::proplist::properties::APPLICATION_NAME, "wifi-manager")
            .map_err(|_| "Failed to set application name in proplist")?;

        let mainloop = Mainloop::new(None).ok_or("Failed to create PulseAudio GLib mainloop")?;
        let context =
            Context::new_with_proplist(&mainloop, "wifi-manager-audio-context", &proplist)
                .ok_or("Failed to create PulseAudio context")?;

        let manager = Rc::new(Self {
            _mainloop: Rc::new(RefCell::new(mainloop)),
            context: Rc::new(RefCell::new(context)),
            on_snapshot: Rc::new(on_snapshot),
            on_result: Rc::new(on_result),
            on_unavailable: Rc::new(on_unavailable),
            refresh_epoch: Cell::new(0),
            refresh_scheduled: Cell::new(false),
            connection_finished: Cell::new(false),
            unavailable_reported: Cell::new(false),
            ready: Cell::new(false),
        });

        {
            let manager_weak = Rc::downgrade(&manager);
            manager.context.borrow_mut().set_state_callback(Some(Box::new(move || {
                if let Some(manager) = manager_weak.upgrade() {
                    manager.handle_context_state();
                }
            })));
        }

        if let Err(error) =
            manager.context.borrow_mut().connect(None, ContextFlagSet::NOFLAGS, None)
        {
            let message = format!("PulseAudio connect error: {error}");
            (manager.on_result)(Err(message.clone()));
            return Err(message);
        }

        // The state callback is normally enough, but a finite watchdog also handles a server that
        // never completes the connection handshake. The weak reference lets the timer terminate
        // on its own when the manager is dropped.
        let manager_weak = Rc::downgrade(&manager);
        let polls = Rc::new(Cell::new(0_u32));
        glib::timeout_add_local(CONNECTION_POLL_INTERVAL, move || {
            let Some(manager) = manager_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };

            if manager.connection_finished.get() {
                return glib::ControlFlow::Break;
            }

            manager.handle_context_state();
            if manager.connection_finished.get() {
                return glib::ControlFlow::Break;
            }

            let poll = polls.get().saturating_add(1);
            polls.set(poll);
            if poll >= CONNECTION_POLL_LIMIT {
                manager.finish_connection("PulseAudio context connection timed out".to_string());
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });

        Ok(manager)
    }

    fn handle_context_state(self: &Rc<Self>) {
        let state = self.context.borrow().get_state();
        match state {
            State::Ready => self.handle_ready(),
            State::Failed => self.finish_connection("PulseAudio context failed".to_string()),
            State::Terminated => {
                self.finish_connection("PulseAudio context terminated".to_string())
            }
            _ => {}
        }
    }

    fn finish_connection(&self, error: String) {
        self.connection_finished.set(true);
        if self.unavailable_reported.replace(true) {
            return;
        }
        error!("{error}");
        (self.on_unavailable)(error);
    }

    fn handle_ready(self: &Rc<Self>) {
        if self.ready.replace(true) {
            return;
        }

        self.connection_finished.set(true);
        self.setup_subscription();
        (self.on_result)(Ok(()));
        self.request_refresh();
    }

    fn setup_subscription(self: &Rc<Self>) {
        let manager_weak = Rc::downgrade(self);
        let mut context = self.context.borrow_mut();
        context.set_subscribe_callback(Some(Box::new(move |facility, _operation, _index| {
            if let Some(manager) = manager_weak.upgrade()
                && matches!(facility, Some(Facility::Sink | Facility::Source | Facility::Server))
            {
                manager.request_refresh();
            }
        })));

        let manager_weak = Rc::downgrade(self);
        drop(context.subscribe(
            InterestMaskSet::SINK | InterestMaskSet::SOURCE | InterestMaskSet::SERVER,
            move |success| {
                if !success && let Some(manager) = manager_weak.upgrade() {
                    manager.report_error("Failed to subscribe to PulseAudio audio events");
                }
            },
        ));
    }

    fn request_refresh(self: &Rc<Self>) {
        self.refresh_epoch.set(self.refresh_epoch.get().wrapping_add(1));
        if self.refresh_scheduled.replace(true) {
            return;
        }

        let manager_weak = Rc::downgrade(self);
        glib::idle_add_local(move || {
            let Some(manager) = manager_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            manager.refresh_scheduled.set(false);
            let epoch = manager.refresh_epoch.get();
            manager.start_refresh(epoch);
            glib::ControlFlow::Break
        });
    }

    fn start_refresh(self: &Rc<Self>, epoch: u64) {
        if self.context.borrow().get_state() != State::Ready {
            return;
        }

        let pending = Rc::new(RefCell::new(RefreshState::new(epoch)));
        let context = self.context.borrow();
        let introspector = context.introspect();

        let manager_weak = Rc::downgrade(self);
        let pending_server = Rc::clone(&pending);
        introspector.get_server_info(move |info| {
            let Some(manager) = manager_weak.upgrade() else {
                return;
            };
            if !manager.refresh_is_current(&pending_server) {
                return;
            }

            {
                let mut pending = pending_server.borrow_mut();
                pending.default_output_id = info
                    .default_sink_name
                    .as_ref()
                    .map(ToString::to_string)
                    .filter(|id| !id.is_empty());
                pending.default_input_id = info
                    .default_source_name
                    .as_ref()
                    .map(ToString::to_string)
                    .filter(|id| !id.is_empty());
                pending.server_done = true;
            }
            manager.complete_refresh(&pending_server);
        });

        let manager_weak = Rc::downgrade(self);
        let pending_sinks = Rc::clone(&pending);
        introspector.get_sink_info_list(move |result| {
            let Some(manager) = manager_weak.upgrade() else {
                return;
            };
            if !manager.refresh_is_current(&pending_sinks) {
                return;
            }

            match result {
                ListResult::Item(info) => {
                    if let Some(device) = sink_to_device(info) {
                        pending_sinks.borrow_mut().outputs.push(device);
                    }
                }
                ListResult::End => {
                    pending_sinks.borrow_mut().sinks_done = true;
                    manager.complete_refresh(&pending_sinks);
                }
                ListResult::Error => manager
                    .fail_refresh(&pending_sinks, "Failed to fetch PulseAudio output devices"),
            }
        });

        let manager_weak = Rc::downgrade(self);
        let pending_sources = Rc::clone(&pending);
        introspector.get_source_info_list(move |result| {
            let Some(manager) = manager_weak.upgrade() else {
                return;
            };
            if !manager.refresh_is_current(&pending_sources) {
                return;
            }

            match result {
                ListResult::Item(info) => {
                    if let Some(device) = source_to_device(info) {
                        pending_sources.borrow_mut().inputs.push(device);
                    }
                }
                ListResult::End => {
                    pending_sources.borrow_mut().sources_done = true;
                    manager.complete_refresh(&pending_sources);
                }
                ListResult::Error => manager
                    .fail_refresh(&pending_sources, "Failed to fetch PulseAudio input devices"),
            }
        });
    }

    fn refresh_is_current(&self, pending: &Rc<RefCell<RefreshState>>) -> bool {
        self.refresh_epoch.get() == pending.borrow().epoch
    }

    fn fail_refresh(self: &Rc<Self>, pending: &Rc<RefCell<RefreshState>>, message: &str) {
        let should_report = {
            let mut pending = pending.borrow_mut();
            if pending.failed || self.refresh_epoch.get() != pending.epoch {
                false
            } else {
                pending.failed = true;
                if pending.error_reported {
                    false
                } else {
                    pending.error_reported = true;
                    true
                }
            }
        };

        if should_report {
            self.report_error(message);
        }
    }

    fn complete_refresh(self: &Rc<Self>, pending: &Rc<RefCell<RefreshState>>) {
        let snapshot = {
            let mut pending = pending.borrow_mut();
            if pending.emitted
                || pending.failed
                || pending.epoch != self.refresh_epoch.get()
                || !pending.server_done
                || !pending.sinks_done
                || !pending.sources_done
            {
                return;
            }

            pending.emitted = true;
            let mut outputs = std::mem::take(&mut pending.outputs);
            let mut inputs = std::mem::take(&mut pending.inputs);
            for device in &mut outputs {
                device.is_default =
                    pending.default_output_id.as_deref() == Some(device.id.as_str());
            }
            for device in &mut inputs {
                device.is_default = pending.default_input_id.as_deref() == Some(device.id.as_str());
            }
            sort_audio_devices(&mut outputs);
            sort_audio_devices(&mut inputs);
            AudioSnapshot {
                outputs,
                inputs,
                default_output_id: pending.default_output_id.clone(),
                default_input_id: pending.default_input_id.clone(),
            }
        };

        (self.on_snapshot)(snapshot);
    }

    fn report_error(&self, message: &str) {
        warn!("{message}");
        (self.on_result)(Err(message.to_string()));
    }

    fn validate_action(self: &Rc<Self>, id: &str) -> Result<(), String> {
        if id.is_empty() {
            return self.reject_action("Audio device id must not be empty".to_string());
        }
        if id.as_bytes().contains(&0) {
            return self.reject_action("Audio device id must not contain NUL".to_string());
        }
        if self.context.borrow().get_state() != State::Ready {
            return self.reject_action("PulseAudio context is not ready".to_string());
        }
        Ok(())
    }

    fn reject_action(self: &Rc<Self>, message: String) -> Result<(), String> {
        self.report_error(&message);
        Err(message)
    }

    fn action_completed(self: &Rc<Self>, action: &'static str, success: bool) {
        if success {
            (self.on_result)(Ok(()));
            self.request_refresh();
        } else {
            self.report_error(&format!("PulseAudio rejected {action}"));
        }
    }

    /// Set the server default output. This changes the default for new apps; it does not
    /// force-move active streams.
    pub(crate) fn set_default_output(self: &Rc<Self>, id: &str) -> Result<(), String> {
        self.validate_action(id)?;
        let manager_weak = Rc::downgrade(self);
        let name = id.to_string();
        let mut context = self.context.borrow_mut();
        drop(context.set_default_sink(&name, move |success| {
            if let Some(manager) = manager_weak.upgrade() {
                manager.action_completed("setting the default output", success);
            }
        }));
        Ok(())
    }

    /// Set the server default input. This changes the default for new apps; it does not
    /// force-move active streams.
    pub(crate) fn set_default_input(self: &Rc<Self>, id: &str) -> Result<(), String> {
        self.validate_action(id)?;
        let manager_weak = Rc::downgrade(self);
        let name = id.to_string();
        let mut context = self.context.borrow_mut();
        drop(context.set_default_source(&name, move |success| {
            if let Some(manager) = manager_weak.upgrade() {
                manager.action_completed("setting the default input", success);
            }
        }));
        Ok(())
    }

    pub(crate) fn set_output_volume(self: &Rc<Self>, id: &str, percent: f64) -> Result<(), String> {
        self.set_volume(id, percent, AudioDeviceKind::Output)
    }

    pub(crate) fn set_input_volume(self: &Rc<Self>, id: &str, percent: f64) -> Result<(), String> {
        self.set_volume(id, percent, AudioDeviceKind::Input)
    }

    fn set_volume(
        self: &Rc<Self>,
        id: &str,
        percent: f64,
        kind: AudioDeviceKind,
    ) -> Result<(), String> {
        self.validate_action(id)?;
        let name = id.to_string();
        let target = pulse_volume(percent);
        let seen = Rc::new(Cell::new(false));
        let manager_weak = Rc::downgrade(self);
        let context = self.context.borrow();
        let introspector = context.introspect();

        match kind {
            AudioDeviceKind::Output => {
                let seen_end = Rc::clone(&seen);
                let manager_weak = manager_weak.clone();
                let name_for_lookup = name.clone();
                introspector.get_sink_info_by_name(&name, move |result| match result {
                    ListResult::Item(info) => {
                        if seen.replace(true) {
                            return;
                        }
                        let channels = info.volume.len().max(info.channel_map.len());
                        let Some(manager) = manager_weak.upgrade() else {
                            return;
                        };
                        if channels == 0 {
                            manager.report_error("PulseAudio output has no channels");
                            return;
                        }

                        let mut volume = info.volume;
                        volume.set(channels, target);
                        let manager_weak = Rc::downgrade(&manager);
                        let context = manager.context.borrow();
                        let mut introspector = context.introspect();
                        drop(introspector.set_sink_volume_by_name(
                            &name_for_lookup,
                            &volume,
                            Some(Box::new(move |success| {
                                if let Some(manager) = manager_weak.upgrade() {
                                    manager.action_completed("setting output volume", success);
                                }
                            })),
                        ));
                    }
                    ListResult::End => {
                        if !seen_end.get()
                            && let Some(manager) = manager_weak.upgrade()
                        {
                            manager.report_error("PulseAudio output device was not found");
                        }
                    }
                    ListResult::Error => {
                        if let Some(manager) = manager_weak.upgrade() {
                            manager.report_error("Failed to inspect PulseAudio output device");
                        }
                    }
                });
            }
            AudioDeviceKind::Input => {
                let seen_end = Rc::clone(&seen);
                let manager_weak = manager_weak.clone();
                let name_for_lookup = name.clone();
                introspector.get_source_info_by_name(&name, move |result| match result {
                    ListResult::Item(info) => {
                        if seen.replace(true) {
                            return;
                        }
                        let channels = info.volume.len().max(info.channel_map.len());
                        let Some(manager) = manager_weak.upgrade() else {
                            return;
                        };
                        if channels == 0 {
                            manager.report_error("PulseAudio input has no channels");
                            return;
                        }

                        let mut volume = info.volume;
                        volume.set(channels, target);
                        let manager_weak = Rc::downgrade(&manager);
                        let context = manager.context.borrow();
                        let mut introspector = context.introspect();
                        drop(introspector.set_source_volume_by_name(
                            &name_for_lookup,
                            &volume,
                            Some(Box::new(move |success| {
                                if let Some(manager) = manager_weak.upgrade() {
                                    manager.action_completed("setting input volume", success);
                                }
                            })),
                        ));
                    }
                    ListResult::End => {
                        if !seen_end.get()
                            && let Some(manager) = manager_weak.upgrade()
                        {
                            manager.report_error("PulseAudio input device was not found");
                        }
                    }
                    ListResult::Error => {
                        if let Some(manager) = manager_weak.upgrade() {
                            manager.report_error("Failed to inspect PulseAudio input device");
                        }
                    }
                });
            }
        }

        Ok(())
    }

    pub(crate) fn set_output_mute(self: &Rc<Self>, id: &str, muted: bool) -> Result<(), String> {
        self.set_mute(id, muted, AudioDeviceKind::Output)
    }

    pub(crate) fn set_input_mute(self: &Rc<Self>, id: &str, muted: bool) -> Result<(), String> {
        self.set_mute(id, muted, AudioDeviceKind::Input)
    }

    fn set_mute(
        self: &Rc<Self>,
        id: &str,
        muted: bool,
        kind: AudioDeviceKind,
    ) -> Result<(), String> {
        self.validate_action(id)?;
        let manager_weak = Rc::downgrade(self);
        let name = id.to_string();
        let context = self.context.borrow();
        let mut introspector = context.introspect();
        match kind {
            AudioDeviceKind::Output => {
                drop(introspector.set_sink_mute_by_name(
                    &name,
                    muted,
                    Some(Box::new(move |success| {
                        if let Some(manager) = manager_weak.upgrade() {
                            manager.action_completed("setting output mute", success);
                        }
                    })),
                ));
            }
            AudioDeviceKind::Input => {
                drop(introspector.set_source_mute_by_name(
                    &name,
                    muted,
                    Some(Box::new(move |success| {
                        if let Some(manager) = manager_weak.upgrade() {
                            manager.action_completed("setting input mute", success);
                        }
                    })),
                ));
            }
        }
        Ok(())
    }
}

fn pulse_volume(percent: f64) -> Volume {
    let percent = u64::from(clamp_percent(percent));
    Volume(((percent * u64::from(Volume::NORMAL.0)) / 100) as u32)
}

fn sink_to_device(info: &SinkInfo<'_>) -> Option<AudioDevice> {
    let id = info.name.as_ref()?.to_string();
    if id.is_empty() {
        return None;
    }
    Some(AudioDevice {
        label: description_or_id(info.description.as_deref(), &id),
        kind: AudioDeviceKind::Output,
        is_default: false,
        volume_percent: clamp_percent(
            (info.volume.avg().0 as f64 * 100.0) / Volume::NORMAL.0 as f64,
        ),
        muted: info.mute,
        id,
    })
}

fn source_to_device(info: &SourceInfo<'_>) -> Option<AudioDevice> {
    if !source_is_eligible(info.monitor_of_sink) {
        return None;
    }
    let id = info.name.as_ref()?.to_string();
    if id.is_empty() {
        return None;
    }
    Some(AudioDevice {
        label: description_or_id(info.description.as_deref(), &id),
        kind: AudioDeviceKind::Input,
        is_default: false,
        volume_percent: clamp_percent(
            (info.volume.avg().0 as f64 * 100.0) / Volume::NORMAL.0 as f64,
        ),
        muted: info.mute,
        id,
    })
}

impl Drop for AudioManager {
    fn drop(&mut self) {
        if let Ok(mut context) = self.context.try_borrow_mut() {
            match context.get_state() {
                State::Failed | State::Terminated => {}
                _ => context.disconnect(),
            }
        }
    }
}
