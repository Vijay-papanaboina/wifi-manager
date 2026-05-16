//! VPN utility helpers — pure functions with no GTK signal setup.
//!
//! Kept separate so `vpn.rs` stays focused on UI wiring and `refresh_vpn_list`.

use gtk4::prelude::*;

use crate::dbus::vpn_manager::{VpnActive, VpnProfile};

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
        if net.state == 1 || net.state == 2 {
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
            match active.state {
                2 => connected_name = Some(&profile.name),
                1 => connecting_name = Some(&profile.name),
                3 => disconnecting_name = Some(&profile.name),
                _ => {}
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
) -> Result<(), String> {
    let mut cmd = std::process::Command::new("nm-connection-editor");
    if let Some(uuid) = uuid {
        if !uuid.is_empty() {
            cmd.arg("--edit").arg(uuid);
        }
    }
    cmd.spawn()
        .map(|_| {
            if let Some(state) = panel_state {
                state.hide();
            } else if let Some(win) = window {
                win.set_visible(false);
            }
        })
        .map_err(|e| format!("launch error: {e}"))
}

/// Show a GTK confirmation dialog before deleting a VPN profile.
pub(super) fn confirm_delete_dialog(
    parent: &gtk4::ApplicationWindow,
    vpn_name: &str,
    on_confirm: impl Fn() + 'static,
) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .text("Delete VPN profile?")
        .secondary_text(format!(
            "Are you sure you want to delete \"{}\"?",
            vpn_name
        ))
        .build();
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.add_button("Delete", gtk4::ResponseType::Accept);
    dialog.connect_response(move |d: &gtk4::MessageDialog, resp| {
        if resp == gtk4::ResponseType::Accept {
            on_confirm();
        }
        d.close();
    });
    dialog.present();
}
