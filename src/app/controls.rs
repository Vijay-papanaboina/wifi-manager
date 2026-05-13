use std::cell::{Cell, RefCell};
use std::rc::Rc;
use gtk4::prelude::*;
use gtk4::{glib, Scale};

use crate::controls::brightness::BrightnessManager;
use crate::controls::volume::VolumeManager;
use crate::controls::night_mode::NightModeManager;
use crate::state::AppStateStore;
use crate::ui::window::PanelWidgets;

const NEUTRAL_TEMP_KELVIN: f64 = 6500.0;
const MIN_TEMP_KELVIN: f64 = 3000.0;

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn inverse_smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    let mut lo = 0.0;
    let mut hi = 1.0;
    for _ in 0..20 {
        let mid = (lo + hi) * 0.5;
        let y = smoothstep(mid);
        if y < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

fn slider_to_kelvin(val: f64, max: f64) -> f64 {
    let t = (val / max).clamp(0.0, 1.0);
    let t_smooth = smoothstep(t);
    let range = NEUTRAL_TEMP_KELVIN - MIN_TEMP_KELVIN;
    NEUTRAL_TEMP_KELVIN - (t_smooth * range)
}

fn kelvin_to_slider(kelvin: f64, max: f64) -> f64 {
    let kelvin = kelvin.clamp(MIN_TEMP_KELVIN, NEUTRAL_TEMP_KELVIN);
    let range = NEUTRAL_TEMP_KELVIN - MIN_TEMP_KELVIN;
    let t_smooth = (NEUTRAL_TEMP_KELVIN - kelvin) / range;
    let t = inverse_smoothstep(t_smooth);
    t * max
}

pub fn setup_controls(widgets: &PanelWidgets) {
    let brightness_scale = widgets.controls.brightness_scale().clone();
    let brightness_btn = widgets.controls.brightness_btn().clone();
    let volume_scale = widgets.controls.volume_scale().clone();
    let volume_icon = widgets.controls.volume_icon().clone();
    let volume_btn = widgets.controls.volume_btn().clone();
    let night_mode_scale = widgets.controls.night_mode_scale().clone();
    let night_mode_btn = widgets.controls.night_mode_btn().clone();

    // Load persisted dynamic state
    let state_store = Rc::new(RefCell::new(AppStateStore::load()));

    // Formatter for brightness and volume
    let percent_formatter = |_: &Scale, val: f64| -> String {
        format!("{}%", val.round() as i32)
    };

    // ── Brightness ───────────────────────────────────────────────
    let b_scale = brightness_scale.clone();
    b_scale.set_format_value_func(percent_formatter);

    glib::spawn_future_local(async move {
        match BrightnessManager::new().await {
            Ok(manager) => {
                let manager = Rc::new(manager);
                let is_updating_ui = Rc::new(Cell::new(false));

                // Set initial value
                if let Some(pct) = manager.get_brightness_percent() {
                    is_updating_ui.set(true);
                    b_scale.set_value(pct);
                    is_updating_ui.set(false);
                }

                // Start watching for external brightness changes
                let is_updating_ui_watcher = Rc::clone(&is_updating_ui);
                let b_scale_watcher = b_scale.clone();
                manager.watch_changes(250, move |val| {
                    is_updating_ui_watcher.set(true);
                    b_scale_watcher.set_value(val);
                    is_updating_ui_watcher.set(false);
                });

                // Listen for UI slider changes -> tell backend (debounced)
                let mgr_clone = Rc::clone(&manager);
                let is_updating_ui_slider = Rc::clone(&is_updating_ui);
                let pending_source: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

                b_scale.connect_value_changed(move |scale: &gtk4::Scale| {
                    if is_updating_ui_slider.get() {
                        return;
                    }
                    let val = scale.value();

                    if let Some(source_id) = pending_source.borrow_mut().take() {
                        source_id.remove();
                    }

                    let mgr = Rc::clone(&mgr_clone);
                    let pending_clone = Rc::clone(&pending_source);

                    let new_source = glib::timeout_add_local(
                        std::time::Duration::from_millis(50),
                        move || {
                            let mgr_inner = Rc::clone(&mgr);
                            glib::spawn_future_local(async move {
                                if let Err(e) = mgr_inner.set_brightness_percent(val).await {
                                    log::warn!("Failed to set brightness: {}", e);
                                }
                            });
                            pending_clone.borrow_mut().take();
                            glib::ControlFlow::Break
                        }
                    );

                    *pending_source.borrow_mut() = Some(new_source);
                });

                // ── Brightness icon button: set to 1% ──────
                let mgr_btn = Rc::clone(&manager);
                let b_scale_ref = brightness_scale.clone();

                brightness_btn.connect_clicked(move |_btn| {
                    b_scale_ref.set_value(1.0);
                    let mgr_inner = Rc::clone(&mgr_btn);
                    glib::spawn_future_local(async move {
                        let _ = mgr_inner.set_brightness_percent(1.0).await;
                    });
                });
            }
            Err(e) => log::error!("Failed to initialize BrightnessManager: {}", e),
        }
    });

    // ── Volume ───────────────────────────────────────────────────
    let v_scale = volume_scale.clone();
    v_scale.set_format_value_func(percent_formatter);
    let v_icon = volume_icon.clone();
    let v_btn = volume_btn.clone();
    let handler_id: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
    let handler_id_cb = handler_id.clone();

    let is_muted = Rc::new(Cell::new(false));
    let is_muted_cb = Rc::clone(&is_muted);

    match VolumeManager::new(
        move |state| {
            if let Some(id) = handler_id_cb.borrow().as_ref() {
                v_scale.block_signal(id);
            }
            v_scale.set_value(state.percent);
            if let Some(id) = handler_id_cb.borrow().as_ref() {
                v_scale.unblock_signal(id);
            }

            is_muted_cb.set(state.muted);

            let icon_name = if state.muted {
                "audio-volume-muted-symbolic"
            } else if state.percent < 33.0 {
                "audio-volume-low-symbolic"
            } else if state.percent < 66.0 {
                "audio-volume-medium-symbolic"
            } else {
                "audio-volume-high-symbolic"
            };
            v_icon.set_icon_name(Some(icon_name));
            v_btn.set_icon_name(icon_name);
        },
        move |result| match result {
            Ok(_) => log::info!("Volume control connected successfully"),
            Err(e) => log::error!("Failed to connect Volume controls: {}", e),
        },
    ) {
        Ok(manager) => {
            let mgr = Rc::clone(&manager);

            // ── Volume icon button: toggle mute ──────────────────────
            volume_btn.connect_clicked(move |_| {
                let new_mute = !is_muted.get();
                mgr.set_mute(new_mute);
            });

            // Listen for UI slider changes -> tell backend
            let id = volume_scale.connect_value_changed(move |scale: &gtk4::Scale| {
                let val = scale.value();
                manager.set_volume_percent(val);
            });
            *handler_id.borrow_mut() = Some(id);
        }
        Err(e) => log::error!("Failed to init VolumeManager: {}", e),
    }

    // ── Night Mode ───────────────────────────────────────────────
    let n_scale = night_mode_scale.clone();
    let store_nm = Rc::clone(&state_store);

    n_scale.set_format_value_func(move |scale, val| -> String {
        let max = scale.adjustment().upper();
        let kelvin = slider_to_kelvin(val, max);
        format!("{}K", kelvin.round() as i32)
    });

    match NightModeManager::new() {
        Ok(manager) => {
            let manager = Rc::new(manager);

            // Apply initial state from state.toml
            let night_enabled = store_nm.borrow().night_mode.enabled;
            let night_temp = store_nm.borrow().night_mode.temperature;

            let max = n_scale.adjustment().upper();
            n_scale.set_value(kelvin_to_slider(night_temp, max));

            if night_enabled {
                n_scale.set_sensitive(true);
                night_mode_btn.set_icon_name("night-light-symbolic");
                if let Err(e) = manager.set_temperature(night_temp) {
                    log::warn!("Failed to apply initial night mode temperature: {}", e);
                }
            } else {
                night_mode_btn.set_icon_name("night-light-disabled-symbolic");
            }

            // ── Moon button: toggle Night Mode on/off ────────────────
            let mgr_btn = Rc::clone(&manager);
            let n_scale_btn = n_scale.clone();
            let store_btn = Rc::clone(&state_store);

            night_mode_btn.connect_clicked(move |btn| {
                let currently_enabled = n_scale_btn.is_sensitive();
                let new_enabled = !currently_enabled;

                if new_enabled {
                    let temp = store_btn.borrow().night_mode.temperature;
                    n_scale_btn.set_sensitive(true);
                    btn.set_icon_name("night-light-symbolic");
                    if let Err(e) = mgr_btn.set_temperature(temp) {
                        log::warn!("Failed to enable night mode: {}", e);
                    }
                } else {
                    n_scale_btn.set_sensitive(false);
                    btn.set_icon_name("night-light-disabled-symbolic");
                    if let Err(e) = mgr_btn.set_temperature(NEUTRAL_TEMP_KELVIN) {
                        log::warn!("Failed to disable night mode: {}", e);
                    }
                }

                let mut store = store_btn.borrow_mut();
                store.night_mode.enabled = new_enabled;
                store.save();
            });

            // ── Slider drag: update temperature and persist ───────────
            let mgr_slider = Rc::clone(&manager);
            let store_slider = Rc::clone(&state_store);

            n_scale.connect_value_changed(move |scale: &gtk4::Scale| {
                if !scale.is_sensitive() {
                    return;
                }
                let val = scale.value();
                let max = scale.adjustment().upper();
                let kelvin = slider_to_kelvin(val, max);
                if let Err(e) = mgr_slider.set_temperature(kelvin) {
                    log::warn!("Failed to set night mode temperature: {}", e);
                }
                let mut store = store_slider.borrow_mut();
                store.night_mode.temperature = kelvin;
                store.save();
            });
        }
        Err(e) => log::error!("Failed to init NightModeManager: {}", e),
    }
}
