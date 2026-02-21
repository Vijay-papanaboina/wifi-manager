use std::cell::RefCell;
use std::rc::Rc;
use log::{error, info};
use gtk4::glib;

use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet};
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet, State};
use libpulse_binding::proplist::Proplist;
use libpulse_binding::volume::Volume;
use libpulse_glib_binding::Mainloop;

#[derive(Copy, Clone, Debug)]
pub struct VolumeState {
    pub percent: f64,
    pub muted: bool,
}

/// Manages the PulseAudio connection. The `mainloop` field must be retained 
/// to keep the GLib integration alive even if otherwise unused after construction.
pub struct VolumeManager {
    mainloop: Rc<RefCell<Mainloop>>,
    context: Rc<RefCell<Context>>,
    default_sink_name: Rc<RefCell<Option<String>>>,
    on_change: Rc<dyn Fn(VolumeState)>,
}

impl VolumeManager {
    pub fn new<F, C>(on_change: F, on_connected: C) -> Result<Rc<Self>, String> 
    where
        F: Fn(VolumeState) + 'static,
        C: FnOnce(Result<(), String>) + 'static,
    {
        let mut proplist = Proplist::new().ok_or("Failed to create PulseAudio proplist")?;
        proplist.set_str(libpulse_binding::proplist::properties::APPLICATION_NAME, "wifi-manager")
            .map_err(|_| "Failed to set application name in proplist")?;

        let mainloop = Mainloop::new(None).ok_or("Failed to create PulseAudio GLib mainloop")?;
        
        let context = Context::new_with_proplist(
            &mainloop,
            "wifi-manager-context",
            &proplist
        ).ok_or("Failed to create PulseAudio context")?;

        let manager = Rc::new(Self {
            mainloop: Rc::new(RefCell::new(mainloop)),
            context: Rc::new(RefCell::new(context)),
            default_sink_name: Rc::new(RefCell::new(None)),
            on_change: Rc::new(on_change),
        });

        manager.context.borrow_mut().connect(None, ContextFlagSet::NOFLAGS, None)
            .map_err(|e| format!("PulseAudio connect error: {}", e))?;
        let mgr_clone = Rc::downgrade(&manager);
        let retry_count = Rc::new(RefCell::new(0u32));
        let on_connected_cb = Rc::new(RefCell::new(Some(on_connected)));
        const MAX_RETRIES: u32 = 50; // 5 seconds at 100ms intervals
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            let mut on_connected_cb = on_connected_cb.borrow_mut();
            if let Some(mgr) = mgr_clone.upgrade() {
                *retry_count.borrow_mut() += 1;
                if *retry_count.borrow() > MAX_RETRIES {
                    let err = "PulseAudio context connection timed out".to_string();
                    error!("{}", err);
                    if let Some(cb) = on_connected_cb.take() {
                        cb(Err(err));
                    }
                    return glib::ControlFlow::Break;
                }
                let state = mgr.context.borrow().get_state();
                if state == State::Ready {
                    mgr.setup();
                    if let Some(cb) = on_connected_cb.take() {
                        cb(Ok(()));
                    }
                    glib::ControlFlow::Break
                } else if state == State::Failed || state == State::Terminated {
                    let err = "PulseAudio context failed or terminated".to_string();
                    error!("{}", err);
                    if let Some(cb) = on_connected_cb.take() {
                        cb(Err(err));
                    }
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            } else {
                glib::ControlFlow::Break
            }
        });

        Ok(manager)
    }

    fn setup(self: &Rc<Self>) {
        info!("PulseAudio context ready. Setting up subscriptions...");
        
        let mgr_weak = Rc::downgrade(self);
        
        let mut ctx = self.context.borrow_mut();
        ctx.set_subscribe_callback(Some(Box::new(move |fac, _op, _idx| {
            if let Some(mgr) = mgr_weak.upgrade()
                && (fac == Some(Facility::Sink) || fac == Some(Facility::Server)) {
                    mgr.refresh_state();
                }
        })));

        ctx.subscribe(InterestMaskSet::SINK | InterestMaskSet::SERVER, |success| {
            if !success {
                error!("Failed to subscribe to PulseAudio events");
            }
        });
        
        drop(ctx);
        self.refresh_state();
    }

    pub fn refresh_state(self: &Rc<Self>) {
        let mgr_weak = Rc::downgrade(self);
        
        let ctx = self.context.borrow();
        let intro = ctx.introspect();
        intro.get_server_info(move |info| {
            let mgr = match mgr_weak.upgrade() {
                Some(m) => m,
                None => return,
            };
            
            let sink_name = match &info.default_sink_name {
                Some(name) => name.to_string(),
                None => return,
            };
            
            *mgr.default_sink_name.borrow_mut() = Some(sink_name.clone());
            
            let mgr_weak2 = Rc::downgrade(&mgr);
            let ctx2 = mgr.context.borrow();
            let intro2 = ctx2.introspect();
            
            intro2.get_sink_info_by_name(&sink_name, move |res| {
                if let ListResult::Item(sink) = res
                    && let Some(mgr2) = mgr_weak2.upgrade() {
                        let avg_vol = sink.volume.avg();
                        let percent = ((avg_vol.0 as f64 / Volume::NORMAL.0 as f64) * 100.0).min(100.0);
                        let state = VolumeState {
                            percent,
                            muted: sink.mute,
                        };
                        (mgr2.on_change)(state);
                    }
            });
        });
    }
    pub fn set_volume_percent(self: &Rc<Self>, percent: f64) {
        // Clamp to valid range (0-100%). Over-amplification is not supported.
        let percent = percent.clamp(0.0, 100.0);
        let sink_name = self.default_sink_name.borrow().clone();
        if let Some(name) = sink_name {
            let vol_val = ((percent / 100.0) * Volume::NORMAL.0 as f64).round() as u32;
            let ctx = self.context.borrow();
            let intro = ctx.introspect();
            
            let name_clone = name.clone();
            let mgr_weak = Rc::downgrade(self);
            intro.get_sink_info_by_name(&name, move |res| {
                if let ListResult::Item(sink) = res
                    && let Some(mgr) = mgr_weak.upgrade() {
                        let mut new_vol = sink.volume;
                        let vol = Volume(vol_val);
                        new_vol.set(sink.channel_map.len(), vol);
                        
                        let ctx2 = mgr.context.borrow_mut();
                        let mut intro2 = ctx2.introspect();
                        intro2.set_sink_volume_by_name(&name_clone, &new_vol, Some(Box::new(|success| {
                            if !success {
                                error!("Failed to set PulseAudio volume on sink");
                            }
                        })));
                    }
            });
        } else {
            log::warn!("Cannot set volume: No default PulseAudio sink available");
        }
    }
}

impl Drop for VolumeManager {
    fn drop(&mut self) {
        if let Ok(mut ctx) = self.context.try_borrow_mut() {
            ctx.disconnect();
        }
    }
}
