use std::cell::{Cell, RefCell};
use std::rc::Rc;
use gtk4::prelude::*;
use gtk4::glib;

use crate::controls::brightness::BrightnessManager;
use crate::controls::volume::VolumeManager;
use crate::ui::window::PanelWidgets;

pub fn setup_controls(widgets: &PanelWidgets) {
    let brightness_scale = widgets.controls.brightness_scale.clone();
    let volume_scale = widgets.controls.volume_scale.clone();
    let volume_icon = widgets.controls.volume_icon.clone();

    // ── Brightness ───────────────────────────────────────────────
    let b_scale = brightness_scale.clone();
    glib::spawn_future_local(async move {
        match BrightnessManager::new().await {
            Ok(manager) => {
                let manager = Rc::new(manager);
                
                // Set initial value
                if let Some(pct) = manager.get_brightness_percent() {
                    b_scale.set_value(pct);
                }

                // Listen for UI slider changes -> tell backend (debounced)
                let mgr_clone = Rc::clone(&manager);
                let pending_source: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
                
                b_scale.connect_value_changed(move |scale| {
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
    let v_icon = volume_icon.clone();
    let is_volume_updating = Rc::new(Cell::new(false));
    let is_volume_updating_cb = is_volume_updating.clone();
    
    // Init backend with a reactive callback that updates the UI
    match VolumeManager::new(move |state| {
        is_volume_updating_cb.set(true);
        v_scale.set_value(state.percent);
        is_volume_updating_cb.set(false);
        
        let icon_name = if state.muted || state.percent < 1.0 {
            "audio-volume-muted-symbolic"
        } else if state.percent < 33.0 {
            "audio-volume-low-symbolic"
        } else if state.percent < 66.0 {
            "audio-volume-medium-symbolic"
        } else {
            "audio-volume-high-symbolic"
        };
        v_icon.set_icon_name(Some(icon_name));
    }) {
        Ok(manager) => {
            // Listen for UI slider changes -> tell backend
            volume_scale.connect_value_changed(move |scale| {
                if !is_volume_updating.get() {
                    let val = scale.value();
                    manager.set_volume_percent(val);
                }
            });
        }
        Err(e) => log::error!("Failed to init VolumeManager: {}", e),
    }
}
