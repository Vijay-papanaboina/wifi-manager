//! UPower and power-profiles-daemon integration for the Power detail page.
//!
//! Both services are optional on desktops.  The controllers therefore keep
//! battery and profile state independent: an absent profile daemon never hides
//! a real battery snapshot, and a late action error cannot replace a newer
//! property update.

#![allow(deprecated)]

use gtk4::{glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use crate::config::Config;
use crate::dbus::power::{BatteryManager, PowerProfileManager};
use crate::domain::power::{
    BatterySnapshot, PowerProfileSnapshot, battery_status_label, profile_status_label,
};
use crate::ui::{
    power::{CHARGE_LIMIT_PRESETS, ChargeLimitController, ChargeLimitStatus, PowerWidgets},
    window::PanelWidgets,
};

#[derive(Default)]
struct PowerUiState {
    battery: BatterySnapshot,
    profiles: PowerProfileSnapshot,
    profile_generation: u64,
}

/// Start independent UPower and power-profile watchers.
pub(crate) fn setup(widgets: &PanelWidgets) {
    let view = widgets.power.clone();
    let home = widgets.home.clone();
    let state = Rc::new(RefCell::new(PowerUiState::default()));
    let combo_update = Rc::new(Cell::new(false));
    let charge_limit = ChargeLimitController::new("/sys/class/power_supply");
    let saved_charge_limit = Config::load().saved_charge_limit();

    render_battery(&view, &home, BatterySnapshot::unavailable(), false);
    render_profiles(&view, &state, PowerProfileSnapshot::unavailable(), &combo_update);
    let charge_limit_status = charge_limit.status();
    render_charge_limit(
        &view,
        &charge_limit_status,
        saved_charge_limit,
        saved_charge_limit,
        &combo_update,
    );
    wire_charge_limit_action(
        &view,
        charge_limit,
        charge_limit_status,
        saved_charge_limit,
        combo_update.clone(),
    );

    let battery_view = view.clone();
    let battery_home = home.clone();
    let battery_state = Rc::clone(&state);
    glib::spawn_future_local(async move {
        match BatteryManager::connect().await {
            Ok(manager) => {
                let manager = Rc::new(manager);
                let manager_for_watch = Rc::clone(&manager);
                glib::spawn_future_local(async move {
                    let result = manager_for_watch
                        .watch_changes({
                            let battery_view = battery_view.clone();
                            let battery_home = battery_home.clone();
                            let battery_state = Rc::clone(&battery_state);
                            move |snapshot| {
                                battery_state.borrow_mut().battery = snapshot.clone();
                                render_battery(&battery_view, &battery_home, snapshot, true);
                            }
                        })
                        .await;
                    if let Err(error) = result {
                        log::warn!("UPower watcher stopped: {error}");
                        battery_view
                            .status
                            .set_text(&format!("Battery service unavailable: {error}"));
                        battery_home.set_power_battery_status(None);
                    }
                });
            }
            Err(error) => {
                log::info!("UPower is unavailable: {error}");
                battery_view.status.set_text("Battery service unavailable");
                battery_home.set_power_battery_status(None);
            }
        }
    });

    let profile_view = view.clone();
    let profile_state = Rc::clone(&state);
    let profile_combo_update = Rc::clone(&combo_update);
    glib::spawn_future_local(async move {
        match PowerProfileManager::connect().await {
            Ok(manager) => {
                let manager = Rc::new(manager);
                wire_profile_action(
                    &profile_view,
                    Rc::clone(&profile_state),
                    Rc::clone(&profile_combo_update),
                    Rc::clone(&manager),
                );

                let manager_for_watch = Rc::clone(&manager);
                glib::spawn_future_local(async move {
                    let result = manager_for_watch
                        .watch_changes({
                            let profile_view = profile_view.clone();
                            let profile_state = Rc::clone(&profile_state);
                            let profile_combo_update = Rc::clone(&profile_combo_update);
                            move |snapshot| {
                                render_profiles(
                                    &profile_view,
                                    &profile_state,
                                    snapshot,
                                    &profile_combo_update,
                                );
                            }
                        })
                        .await;
                    if let Err(error) = result {
                        log::warn!("Power profile watcher stopped: {error}");
                        profile_view.profile_section.set_visible(false);
                        profile_view.profile_status.set_text("Power profiles unavailable");
                    }
                });
            }
            Err(error) => {
                log::info!("Power-profiles-daemon is unavailable: {error}");
                profile_view.profile_section.set_visible(false);
                profile_view.profile_status.set_text("Power profiles unavailable");
            }
        }
    });
}

fn render_battery(
    view: &PowerWidgets,
    home: &crate::ui::home::HomeWidgets,
    snapshot: BatterySnapshot,
    service_connected: bool,
) {
    if !snapshot.present {
        view.battery_icon.set_icon_name(Some("battery-missing-symbolic"));
        view.battery_summary.set_text("No battery detected");
        view.battery_details.set_text("A desktop power supply may not expose a battery.");
        view.status.set_text(if service_connected {
            "Battery service connected · no battery present"
        } else {
            "Battery unavailable"
        });
        home.set_power_battery_status(Some("No battery detected"));
        return;
    }

    let icon =
        snapshot.icon_name.as_deref().filter(|icon| !icon.is_empty()).unwrap_or("battery-symbolic");
    view.battery_icon.set_icon_name(Some(icon));
    view.battery_summary.set_text(&battery_status_label(&snapshot));
    view.battery_details.set_text(&battery_details(&snapshot));
    view.status.set_text("Battery status updated");
    home.set_power_battery_status(Some(&battery_status_label(&snapshot)));
}

fn battery_details(snapshot: &BatterySnapshot) -> String {
    let mut details = vec![snapshot.state.label().to_string()];
    if let Some(seconds) = snapshot.time_to_empty.map(|duration| duration.as_secs()) {
        details.push(format!("{} remaining", format_duration(seconds)));
    }
    if let Some(seconds) = snapshot.time_to_full.map(|duration| duration.as_secs()) {
        details.push(format!("{} until full", format_duration(seconds)));
    }
    if let Some(rate) = snapshot.energy_rate {
        details.push(format!("{rate:.1} W"));
    }
    if snapshot.warning_level > 0 {
        details.push(format!("Warning level {}", snapshot.warning_level));
    }
    details.join(" · ")
}

fn format_duration(seconds: u64) -> String {
    let total_minutes = seconds.div_ceil(60);
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours > 0 { format!("{hours}h {minutes}m") } else { format!("{minutes}m") }
}

fn render_profiles(
    view: &PowerWidgets,
    state: &Rc<RefCell<PowerUiState>>,
    snapshot: PowerProfileSnapshot,
    combo_update: &Cell<bool>,
) {
    {
        let mut state = state.borrow_mut();
        state.profiles = snapshot.clone();
        state.profile_generation = state.profile_generation.wrapping_add(1);
    }

    if !snapshot.available {
        view.profile_section.set_visible(false);
        view.profile_combo.set_sensitive(false);
        view.profile_combo.remove_all();
        view.profile_status.set_text(&profile_status_label(&snapshot));
        return;
    }

    combo_update.set(true);
    view.profile_combo.remove_all();
    for profile in &snapshot.profiles {
        // IDs are the daemon's exact action identifiers and labels.  No
        // built-in list is substituted when a daemon advertises a new one.
        view.profile_combo.append(Some(&profile.id), &profile.id);
    }
    if let Some(active) = snapshot.active_profile.as_deref() {
        if !view.profile_combo.set_active_id(Some(active)) {
            view.profile_combo.set_active(None);
        }
    } else {
        view.profile_combo.set_active(None);
    }
    view.profile_combo.set_sensitive(!snapshot.profiles.is_empty());
    view.profile_section.set_visible(true);
    view.profile_status.set_text(&profile_status_label(&snapshot));
    combo_update.set(false);
}

fn render_charge_limit(
    view: &PowerWidgets,
    status: &ChargeLimitStatus,
    saved_limit: Option<u8>,
    preferred_limit: Option<u8>,
    combo_update: &Cell<bool>,
) {
    combo_update.set(true);
    match status {
        ChargeLimitStatus::Supported(current) => {
            if !view.charge_limit_combo.set_active_id(Some(&current.to_string())) {
                view.charge_limit_combo.set_active(None);
            }
            view.charge_limit_combo.set_sensitive(true);
            if *current == 100 {
                view.charge_limit_status.set_text("Current limit: 100% (no limit)");
            } else {
                view.charge_limit_status.set_text(&format!("Current limit: {current}%"));
            }
        }
        ChargeLimitStatus::Mixed => {
            view.charge_limit_combo.set_active(None);
            view.charge_limit_combo.set_sensitive(false);
            view.charge_limit_status
                .set_text("Mixed limits across power supplies; selection disabled");
        }
        ChargeLimitStatus::NotSupported => {
            view.charge_limit_combo.set_active(None);
            view.charge_limit_combo.set_sensitive(false);
            view.charge_limit_status.set_text("Not supported by the available power supplies");
        }
        ChargeLimitStatus::PermissionRequired => {
            let display_limit = preferred_limit
                .or(saved_limit)
                .filter(|value| CHARGE_LIMIT_PRESETS.contains(value));
            if let Some(display_limit) = display_limit {
                view.charge_limit_combo.set_active_id(Some(&display_limit.to_string()));
            } else {
                view.charge_limit_combo.set_active(None);
            }
            view.charge_limit_combo.set_sensitive(true);
            if saved_limit == display_limit {
                if let Some(display_limit) = display_limit {
                    view.charge_limit_status.set_text(&format!(
                        "Saved limit: {display_limit}%; current value unavailable this session. Choose a preset to request authorization"
                    ));
                } else {
                    view.charge_limit_status.set_text(
                        "Current limit unavailable; choose a preset to request administrator authorization",
                    );
                }
            } else {
                view.charge_limit_status.set_text(
                    "Choose a preset to request administrator authorization; current value is unavailable this session",
                );
            }
        }
        ChargeLimitStatus::Error(error) => {
            view.charge_limit_combo.set_active(None);
            view.charge_limit_combo.set_sensitive(false);
            view.charge_limit_status.set_text(&format!("Charge limit unavailable: {error}"));
        }
    }
    combo_update.set(false);
}

fn wire_charge_limit_action(
    view: &PowerWidgets,
    controller: ChargeLimitController,
    initial_status: ChargeLimitStatus,
    saved_limit: Option<u8>,
    combo_update: Rc<Cell<bool>>,
) {
    let view = view.clone();
    let confirmed = Rc::new(RefCell::new(initial_status));
    let selected = Rc::new(RefCell::new(
        view.charge_limit_combo
            .active_id()
            .and_then(|id| id.parse::<u8>().ok())
            .filter(|value| CHARGE_LIMIT_PRESETS.contains(value))
            .or(saved_limit),
    ));
    let combo = view.charge_limit_combo.clone();

    combo.connect_changed(move |combo| {
        if combo_update.get() {
            return;
        }
        let Some(requested) = combo.active_id().and_then(|id| id.parse::<u8>().ok()) else {
            return;
        };
        if !CHARGE_LIMIT_PRESETS.contains(&requested) {
            return;
        }

        let previous = *selected.borrow();
        let current_status = confirmed.borrow().clone();
        if matches!(current_status, ChargeLimitStatus::PermissionRequired) {
            *selected.borrow_mut() = Some(requested);
            view.charge_limit_combo.set_sensitive(false);
            view.charge_limit_status.set_text(&format!(
                "Requesting administrator authorization to apply {requested}%…"
            ));
            request_charge_limit_authorization(
                &view,
                controller.clone(),
                Rc::clone(&confirmed),
                Rc::clone(&selected),
                Rc::clone(&combo_update),
                requested,
                previous,
                saved_limit,
            );
            return;
        }

        // Restore the last reread value until the write and verification
        // complete; the control never claims success optimistically.
        render_charge_limit(&view, &current_status, saved_limit, previous, &combo_update);
        view.charge_limit_combo.set_sensitive(false);
        view.charge_limit_status.set_text(&format!("Applying charge limit {requested}%…"));

        match controller.set_limit(requested) {
            Ok(status) => {
                *confirmed.borrow_mut() = status.clone();
                let display_limit = match &status {
                    ChargeLimitStatus::Supported(current) => {
                        *selected.borrow_mut() = Some(*current);
                        Some(*current)
                    }
                    _ => {
                        *selected.borrow_mut() = previous;
                        previous
                    }
                };
                render_charge_limit(&view, &status, saved_limit, display_limit, &combo_update);
                if let ChargeLimitStatus::Supported(current) = status {
                    if current == requested {
                        view.charge_limit_status.set_text(&match Config::persist_charge_limit(requested) {
                            Ok(()) => format!("Charge limit set to {requested}%"),
                            Err(error) => format!(
                                "Charge limit set to {requested}%, but could not save preference: {error}"
                            ),
                        });
                    } else {
                        view.charge_limit_status.set_text(&format!(
                            "Requested {requested}%, but the power supply reports {current}%"
                        ));
                    }
                }
            }
            Err(error) => {
                let status = controller.status();
                *confirmed.borrow_mut() = status.clone();
                restore_charge_limit_selection(&view, previous, &combo_update);
                render_charge_limit(&view, &status, saved_limit, previous, &combo_update);
                view.charge_limit_status.set_text(&format!("Could not set charge limit: {error}"));
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn request_charge_limit_authorization(
    view: &PowerWidgets,
    controller: ChargeLimitController,
    confirmed: Rc<RefCell<ChargeLimitStatus>>,
    selected: Rc<RefCell<Option<u8>>>,
    combo_update: Rc<Cell<bool>>,
    requested: u8,
    previous: Option<u8>,
    saved_limit: Option<u8>,
) {
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            view.charge_limit_status
                .set_text(&format!("Could not locate the wifi-manager executable: {error}"));
            *selected.borrow_mut() = previous;
            restore_charge_limit_selection(view, previous, &combo_update);
            let status = controller.status();
            *confirmed.borrow_mut() = status.clone();
            render_charge_limit(view, &status, saved_limit, previous, &combo_update);
            return;
        }
    };

    view.charge_limit_combo.set_sensitive(false);
    view.charge_limit_status
        .set_text(&format!("Requesting administrator authorization to apply {requested}%…"));

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = Command::new("pkexec")
            .arg(executable)
            .arg("--charge-limit-helper")
            .arg(requested.to_string())
            .status()
            .map(|status| status.success())
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });

    let view = view.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || match receiver.try_recv() {
        Ok(Ok(true)) => {
            let status = controller.status();
            *confirmed.borrow_mut() = status.clone();
            match &status {
                ChargeLimitStatus::PermissionRequired => {
                    *selected.borrow_mut() = Some(requested);
                    restore_charge_limit_selection(&view, Some(requested), &combo_update);
                    render_charge_limit(
                        &view,
                        &status,
                        saved_limit,
                        Some(requested),
                        &combo_update,
                    );
                    view.charge_limit_combo.set_sensitive(true);
                    view.charge_limit_status.set_text(&match Config::persist_charge_limit(requested) {
                        Ok(()) => format!(
                            "Verified at {requested}%; choose another limit to request authorization"
                        ),
                        Err(error) => format!(
                            "Verified at {requested}%, but could not save preference: {error}"
                        ),
                    });
                }
                ChargeLimitStatus::Supported(current) => {
                    *selected.borrow_mut() = Some(*current);
                    restore_charge_limit_selection(&view, Some(*current), &combo_update);
                    render_charge_limit(&view, &status, saved_limit, Some(*current), &combo_update);
                    if *current == requested {
                        view.charge_limit_status.set_text(&match Config::persist_charge_limit(requested) {
                            Ok(()) => format!("Authorized; charge limit verified at {current}%"),
                            Err(error) => format!(
                                "Authorized; charge limit verified at {current}%, but could not save preference: {error}"
                            ),
                        });
                    } else {
                        view.charge_limit_status.set_text(&format!(
                            "Authorized, but the power supply reports {current}% instead of {requested}%"
                        ));
                    }
                }
                _ => {
                    *selected.borrow_mut() = None;
                    render_charge_limit(&view, &status, saved_limit, None, &combo_update);
                }
            }
            glib::ControlFlow::Break
        }
        Ok(Ok(false)) => {
            let status = controller.status();
            *confirmed.borrow_mut() = status.clone();
            *selected.borrow_mut() = previous;
            restore_charge_limit_selection(&view, previous, &combo_update);
            render_charge_limit(&view, &status, saved_limit, previous, &combo_update);
            view.charge_limit_status.set_text("Authorization was cancelled or the helper failed");
            glib::ControlFlow::Break
        }
        Ok(Err(error)) => {
            let status = controller.status();
            *confirmed.borrow_mut() = status.clone();
            *selected.borrow_mut() = previous;
            restore_charge_limit_selection(&view, previous, &combo_update);
            render_charge_limit(&view, &status, saved_limit, previous, &combo_update);
            view.charge_limit_status
                .set_text(&format!("Could not request administrator authorization: {error}"));
            glib::ControlFlow::Break
        }
        Err(TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(TryRecvError::Disconnected) => {
            let status = controller.status();
            *confirmed.borrow_mut() = status.clone();
            *selected.borrow_mut() = previous;
            restore_charge_limit_selection(&view, previous, &combo_update);
            render_charge_limit(&view, &status, saved_limit, previous, &combo_update);
            view.charge_limit_status.set_text("Authorization helper exited unexpectedly");
            glib::ControlFlow::Break
        }
    });
}

fn restore_charge_limit_selection(
    view: &PowerWidgets,
    selected: Option<u8>,
    combo_update: &Cell<bool>,
) {
    combo_update.set(true);
    if let Some(selected) = selected {
        view.charge_limit_combo.set_active_id(Some(&selected.to_string()));
    } else {
        view.charge_limit_combo.set_active(None);
    }
    combo_update.set(false);
}

fn wire_profile_action(
    view: &PowerWidgets,
    state: Rc<RefCell<PowerUiState>>,
    combo_update: Rc<Cell<bool>>,
    manager: Rc<PowerProfileManager>,
) {
    let view = view.clone();
    let profile_combo = view.profile_combo.clone();
    profile_combo.connect_changed(move |combo| {
        if combo_update.get() {
            return;
        }
        let Some(requested) = combo.active_id().map(|id| id.to_string()) else {
            return;
        };
        let (previous, generation) = {
            let state = state.borrow();
            (state.profiles.active_profile.clone(), state.profile_generation)
        };
        if previous.as_deref() == Some(requested.as_str()) {
            return;
        }

        // Keep the visual selection on the daemon-confirmed profile until a
        // property update says otherwise.  A failed request therefore never
        // lies about the active profile.
        combo_update.set(true);
        if let Some(previous) = previous.as_deref() {
            if !combo.set_active_id(Some(previous)) {
                combo.set_active(None);
            }
        } else {
            combo.set_active(None);
        }
        combo_update.set(false);
        view.profile_status.set_text(&format!("Setting profile {requested}…"));

        let manager = Rc::clone(&manager);
        let state = Rc::clone(&state);
        let view = view.clone();
        let combo_update_for_future = Rc::clone(&combo_update);
        glib::spawn_future_local(async move {
            match manager.set_active_profile(&requested).await {
                Ok(()) => {
                    // The profile watcher owns truth; this message is only a
                    // request acknowledgement until ActiveProfile changes.
                    if state.borrow().profile_generation == generation {
                        view.profile_status
                            .set_text(&format!("Profile change requested: {requested}"));
                    }
                }
                Err(error) => {
                    if state.borrow().profile_generation == generation {
                        view.profile_status.set_text(&format!("Could not set profile: {error}"));
                        restore_profile_selection(&view, &state, &combo_update_for_future);
                    }
                }
            }
        });
    });
}

fn restore_profile_selection(
    view: &PowerWidgets,
    state: &Rc<RefCell<PowerUiState>>,
    combo_update: &Cell<bool>,
) {
    let active = state.borrow().profiles.active_profile.clone();
    combo_update.set(true);
    if let Some(active) = active.as_deref() {
        if !view.profile_combo.set_active_id(Some(active)) {
            view.profile_combo.set_active(None);
        }
    } else {
        view.profile_combo.set_active(None);
    }
    combo_update.set(false);
}

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn rounds_total_duration_before_splitting_hours() {
        assert_eq!(format_duration(59), "1m");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(61), "2m");
        assert_eq!(format_duration(3599), "1h 0m");
        assert_eq!(format_duration(7199), "2h 0m");
    }
}
