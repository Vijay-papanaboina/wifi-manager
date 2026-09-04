//! VPN utility helpers — pure functions with no GTK signal setup.
//!
//! Kept separate so `vpn.rs` stays focused on UI wiring and `refresh_vpn_list`.

use gtk4::prelude::*;

use crate::domain::vpn::{VpnActive, VpnProfile};
use crate::error::AppResult;
use crate::process;

use super::AppState;

/// Find the active path of another VPN that is currently connected/connecting,
/// which must be torn down before we can bring up `target_conn_path`.
pub(super) fn find_blocking_active_path_for_connect(
    st: &AppState,
    target_conn_path: &str,
) -> Option<String> {
    for net in st.vpn_active_by_conn.values() {
        if net.connection_path == target_conn_path {
            continue;
        }
        if net.is_connecting() || net.is_connected() {
            return Some(net.active_path.clone());
        }
    }
    None
}

/// Update the header status label to reflect the current VPN connection state.
pub(super) fn update_vpn_header_status(
    status: &gtk4::Label,
    profiles: &[VpnProfile],
    active_by_conn: &std::collections::HashMap<String, VpnActive>,
) {
    let mut connected_name: Option<&str> = None;
    let mut connecting_name: Option<&str> = None;
    let mut disconnecting_name: Option<&str> = None;

    for profile in profiles {
        if let Some(active) = active_by_conn.get(&profile.connection_path) {
            if active.is_connected() {
                connected_name = Some(&profile.name);
            } else if active.is_connecting() {
                connecting_name = Some(&profile.name);
            } else if active.is_disconnecting() {
                disconnecting_name = Some(&profile.name);
            }
        }
    }

    if let Some(name) = connected_name {
        status.set_text(&format!("VPN connected: {name}"));
    } else if let Some(name) = connecting_name {
        status.set_text(&format!("VPN connecting: {name}"));
    } else if let Some(name) = disconnecting_name {
        status.set_text(&format!("VPN disconnecting: {name}"));
    } else {
        status.set_text("VPN disconnected");
    }
}

/// Map common D-Bus / NM error strings to friendly messages.
pub(super) fn humanize_vpn_error(err: &str) -> String {
    let lower = err.to_lowercase();
    if lower.contains("no agents were available")
        || lower.contains("no secret agent")
        || lower.contains("secrets")
    {
        return "missing credentials/secrets".to_string();
    }
    if lower.contains("permission denied") || lower.contains("not authorized") {
        return "permission denied".to_string();
    }
    if lower.contains("timeout") {
        return "operation timed out".to_string();
    }
    if lower.contains("failed") && lower.contains("connect") {
        return "connection failed".to_string();
    }
    err.to_string()
}

/// Launch `nm-connection-editor`, optionally pre-opening a specific profile by UUID.
///
/// Hides the panel (via `PanelState`) or the window after a successful launch.
pub(super) fn launch_nm_connection_editor(
    uuid: Option<String>,
    panel_state: Option<&crate::daemon::PanelState>,
    window: Option<&gtk4::ApplicationWindow>,
) -> AppResult<()> {
    let command_args = uuid
        .filter(|uuid| !uuid.is_empty())
        .map(|uuid| vec!["--edit".to_string(), uuid])
        .unwrap_or_default();
    process::spawn("nm-connection-editor", command_args.iter()).map(|_| {
        if let Some(state) = panel_state {
            state.hide();
        } else if let Some(win) = window {
            win.set_visible(false);
        }
    })
}

/// Show a GTK confirmation dialog before deleting a VPN profile.
pub(super) fn confirm_delete_dialog(
    parent: &gtk4::ApplicationWindow,
    vpn_name: &str,
    on_confirm: impl Fn() + 'static,
) {
    let dialog = gtk4::AlertDialog::builder()
        .modal(true)
        .message("Delete VPN profile?")
        .detail(format!("Are you sure you want to delete \"{}\"?", vpn_name))
        .buttons(["Cancel", "Delete"])
        .cancel_button(0)
        .default_button(1)
        .build();

    let parent = parent.clone();
    gtk4::glib::spawn_future_local(async move {
        if dialog.choose_future(Some(&parent)).await == Ok(1) {
            on_confirm();
        }
    });
}
