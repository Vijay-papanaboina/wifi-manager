//! Shortcuts — keyboard and D-Bus triggered actions (Escape, reload).

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib;

use crate::ui::window::PanelWidgets;

use super::{AppState, refresh_list};

/// Set up Escape key handler to hide panel (with proper state tracking).
pub(super) fn setup_escape_key(widgets: &PanelWidgets, panel_state: crate::daemon::PanelState) {
    use gtk4::{gdk, glib, prelude::*, EventControllerKey};
    
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

    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if reload_requested.swap(false, std::sync::atomic::Ordering::Relaxed) {
            log::info!("Reload requested - refreshing network list with new config");
            let state = Rc::clone(&state);
            let list_box = list_box.clone();
            let status = status.clone();

            glib::spawn_future_local(async move {
                // Reload CSS
                crate::ui::window::reload_css();
                // Refresh network list (which will reload config for icons)
                refresh_list(&state, &list_box, &status).await;
            });
        }
        glib::ControlFlow::Continue
    });
}
