//! Shortcuts — keyboard and D-Bus triggered actions (Escape, reload).

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;

use crate::config::Config;
use crate::ui::window::PanelWidgets;

use super::{AppState, refresh_list};

/// Set up Escape key handler to hide panel (with proper state tracking).
pub(super) fn setup_escape_key(widgets: &PanelWidgets, panel_state: crate::daemon::PanelState) {
    use gtk4::{EventControllerKey, gdk, glib, prelude::*};

    let key_controller = EventControllerKey::new();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            panel_state.hide();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    widgets.window.add_controller(key_controller);
}

/// Poll the reload_requested flag and reload config/CSS when set.
pub(super) fn setup_reload_on_request(
    widgets: &PanelWidgets,
    state: Rc<RefCell<AppState>>,
    reload_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let list_box = widgets.network_list_box.clone();
    let status = widgets.status_label.clone();
    let window = widgets.window.clone();
    let controls = widgets.controls.clone();
    let home = widgets.home.clone();
    let media = widgets.media.clone();

    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if reload_requested.swap(false, std::sync::atomic::Ordering::Relaxed) {
            log::info!("Reload requested - refreshing network list with new config");
            let state = Rc::clone(&state);
            let list_box = list_box.clone();
            let status = status.clone();
            let window = window.clone();
            let controls = controls.clone();
            let home = home.clone();
            let media = media.clone();

            glib::spawn_future_local(async move {
                // Reload CSS
                crate::ui::window::reload_css();
                // Reapply placement and configurable control glyphs.
                crate::ui::window::apply_runtime_config(
                    &window,
                    &controls,
                    &home,
                    &media,
                    &Config::load(),
                );
                // Refresh network list (which also reloads network icons).
                refresh_list(&state, &list_box, &status).await;
            });
        }
        glib::ControlFlow::Continue
    });
}
