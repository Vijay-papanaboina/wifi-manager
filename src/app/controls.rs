use std::cell::{Cell, RefCell};
use std::rc::Rc;
use gtk4::prelude::*;
use gtk4::{glib, Scale};

use crate::controls::brightness::BrightnessManager;
use crate::controls::volume::VolumeManager;
use crate::controls::night_mode::NightModeManager;
use crate::ui::window::PanelWidgets;

const NEUTRAL_TEMP_KELVIN: f64 = 6500.0;

pub fn setup_controls(widgets: &PanelWidgets) {
    let brightness_scale = widgets.controls.brightness_scale().clone();
    let volume_scale = widgets.controls.volume_scale().clone();
    let volume_icon = widgets.controls.volume_icon().clone();
    let night_mode_scale = widgets.controls.night_mode_scale().clone();

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
                    
                    // Cancel any pending update
                    if let Some(source_id) = pending_source.borrow_mut().take() {
                        source_id.remove();
                    }
                    
                    let mgr = Rc::clone(&mgr_clone);
                    let pending_clone = Rc::clone(&pending_source);
                    
                    // Schedule new update
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
            }
            Err(e) => log::error!("Failed to initialize BrightnessManager: {}", e),
        }
    });

    // ── Volume ───────────────────────────────────────────────────
    let v_scale = volume_scale.clone();
    v_scale.set_format_value_func(percent_formatter);
    let v_icon = volume_icon.clone();
    let handler_id: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
    let handler_id_cb = handler_id.clone();
    
    // Init backend with a reactive callback that updates the UI
    match VolumeManager::new(
        // on_change callback
        move |state| {
            if let Some(id) = handler_id_cb.borrow().as_ref() {
                v_scale.block_signal(id);
            }
            v_scale.set_value(state.percent);
            if let Some(id) = handler_id_cb.borrow().as_ref() {
                v_scale.unblock_signal(id);
            }
            
            let icon_name = if state.muted {
                "audio-volume-muted-symbolic"
            } else if state.percent < 1.0 {
                "audio-volume-low-symbolic"
            } else if state.percent < 33.0 {
                "audio-volume-low-symbolic"
            } else if state.percent < 66.0 {
                "audio-volume-medium-symbolic"
            } else {
                "audio-volume-high-symbolic"
            };
            v_icon.set_icon_name(Some(icon_name));
        },
        // on_connected callback
        move |result| {
            match result {
                Ok(_) => {
                    log::info!("Volume control connected successfully");
                }
                Err(e) => {
                    log::error!("Failed to connect Volume controls: {}", e);
                }
            }
        },
    ) {
        Ok(manager) => {
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
    
    n_scale.set_format_value_func(|_, val| -> String {
        let kelvin: f64 = NEUTRAL_TEMP_KELVIN - val;
        format!("{}K", kelvin.round() as i32)
    });

    match NightModeManager::new() {
        Ok(manager) => {
            let manager = Rc::new(manager);
            let n_scale_watcher = n_scale.clone();
            
            // Set initial value
            let current_kelvin = manager.get_temperature_kelvin();
            n_scale_watcher.set_value(NEUTRAL_TEMP_KELVIN - current_kelvin);

            // Listen for UI slider changes -> tell backend
            let mgr_clone = Rc::clone(&manager);
            n_scale.connect_value_changed(move |scale: &gtk4::Scale| {
                let val = scale.value();
                let kelvin = NEUTRAL_TEMP_KELVIN - val;
                if let Err(e) = mgr_clone.set_temperature(kelvin) {
                    log::warn!("Failed to set night mode temperature: {}", e);
                }
            });
        }
        Err(e) => log::error!("Failed to init NightModeManager: {}", e),
    }
}
