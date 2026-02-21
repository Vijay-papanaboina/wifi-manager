//! Scrollable list of available WiFi networks.

use gtk4::prelude::*;
use gtk4::{Label, ListBox, PolicyType, ScrolledWindow, SelectionMode};
use crate::ui::window::{MIN_LIST_HEIGHT, MAX_LIST_HEIGHT};

use super::network_row;
use crate::dbus::access_point::Network;

/// Build a scrollable network list.
///
/// Returns `(scrolled_window, list_box)` — the list_box is needed to populate
/// rows and handle selection events.
pub fn build_network_list() -> (ScrolledWindow, ListBox) {
    let list_box = ListBox::new();
    list_box.add_css_class("network-list");
    list_box.set_selection_mode(SelectionMode::None);
    list_box.set_activate_on_single_click(true);

    let scrolled = ScrolledWindow::new();
    scrolled.add_css_class("network-scroll");
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_has_frame(false);
    scrolled.set_min_content_height(MIN_LIST_HEIGHT);
    scrolled.set_max_content_height(MAX_LIST_HEIGHT);
    scrolled.set_child(Some(&list_box));

    (scrolled, list_box)
}

/// Clear the list and repopulate with the given networks.
pub fn populate_network_list(
    list_box: &ListBox,
    networks: &[Network],
    config: &crate::config::Config,
    wifi: &crate::dbus::network_manager::WifiManager,
    status: &gtk4::Label,
) {
    use gtk4::{glib, prelude::*};
    
    // Remove all existing rows
    while let Some(row) = list_box.first_child() {
        list_box.remove(&row);
    }

    if networks.is_empty() {
        let empty = Label::new(Some("No networks found"));
        empty.add_css_class("empty-label");
        list_box.append(&empty);
        return;
    }

    let wifi = wifi.clone();
    let list_box_clone = list_box.clone();
    let status_clone = status.clone();

    for net in networks {
        let wifi_clone = wifi.clone();
        let list_box_clone2 = list_box_clone.clone();
        let status_clone2 = status_clone.clone();
        
        let row = network_row::build_network_row(net, config, move |ssid| {
            let wifi = wifi_clone.clone();
            let _list_box = list_box_clone2.clone();
            let status = status_clone2.clone();
            
            glib::spawn_future_local(async move {
                status.set_text(&format!("Forgetting {}...", ssid));
                match wifi.forget_network(&ssid).await {
                    Ok(_) => {
                        status.set_text(&format!("Forgot {}", ssid));
                        // Refresh will happen via live updates signal
                    }
                    Err(e) => {
                        log::error!("Forget failed: {e}");
                        status.set_text(&format!("Failed to forget: {}", e));
                    }
                }
            });
        });
        list_box.append(&row);
    }
}
