//! VPN profile import — file picker, nmcli invocation, and post-import refresh.
//!
//! Isolated because all of this logic is about file I/O and subprocess calls;
//! none of it sets up persistent GTK signal handlers.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

use super::AppState;

/// Open a file-chooser dialog and import the selected `.ovpn`/`.conf` profile.
pub(super) fn open_import_dialog(
    state: Rc<RefCell<AppState>>,
    window: gtk4::ApplicationWindow,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
    spinner: gtk4::Spinner,
    scrolled: gtk4::ScrolledWindow,
    import_btn: gtk4::Button,
    open_btn: gtk4::Button,
    on_done: impl Fn() + 'static,
) {
    let chooser = gtk4::FileDialog::builder()
        .title("Import VPN Profile")
        .accept_label("Import")
        .build();

    let filter = gtk4::FileFilter::new();
    filter.add_pattern("*.ovpn");
    filter.add_pattern("*.conf");
    filter.set_name(Some("VPN Profiles (*.ovpn, *.conf)"));
    let filter_store = gio::ListStore::new::<gtk4::FileFilter>();
    filter_store.append(&filter);
    chooser.set_filters(Some(&filter_store));
    chooser.set_default_filter(Some(&filter));

    let on_done = Rc::new(on_done);
    glib::spawn_future_local(async move {
        match chooser.open_future(None::<&gtk4::Window>).await {
            Ok(file) => {
                let Some(path) = file.path() else {
                    status.set_text("Import failed: selected file path is unavailable");
                    on_done();
                    return;
                };

                match import_vpn_profile(&path) {
                    Ok(msg) => {
                        status.set_text(&msg);
                        schedule_post_import_refresh(
                            Rc::clone(&state),
                            window.clone(),
                            list_box.clone(),
                            status.clone(),
                            spinner.clone(),
                            scrolled.clone(),
                            import_btn.clone(),
                            open_btn.clone(),
                        );
                    }
                    Err(e) => {
                        status.set_text(&format!("Import failed: {e}"));
                    }
                }
            }
            Err(e) => {
                // User cancel should be quiet.
                if !e.matches(gtk4::DialogError::Dismissed) {
                    status.set_text(&format!("Import failed: {e}"));
                }
            }
        }

        on_done();
    });
}

/// Schedule a burst of VPN list refreshes after an import completes.
///
/// NM may finish activating the connection slightly after `nmcli` returns,
/// so we poll at 0 ms, 800 ms, 1800 ms, and 3200 ms.
pub(super) fn schedule_post_import_refresh(
    state: Rc<RefCell<AppState>>,
    window: gtk4::ApplicationWindow,
    list_box: gtk4::ListBox,
    status: gtk4::Label,
    spinner: gtk4::Spinner,
    scrolled: gtk4::ScrolledWindow,
    import_btn: gtk4::Button,
    open_btn: gtk4::Button,
) {
    let delays_ms = [0_u64, 800, 1800, 3200];
    for delay in delays_ms {
        let state = Rc::clone(&state);
        let window = window.clone();
        let list_box = list_box.clone();
        let status = status.clone();
        let spinner = spinner.clone();
        let scrolled = scrolled.clone();
        let import_btn = import_btn.clone();
        let open_btn = open_btn.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(delay), move || {
            glib::spawn_future_local({
                let state = Rc::clone(&state);
                let window = window.clone();
                let list_box = list_box.clone();
                let status = status.clone();
                let spinner = spinner.clone();
                let scrolled = scrolled.clone();
                let import_btn = import_btn.clone();
                let open_btn = open_btn.clone();
                async move {
                    super::vpn::refresh_vpn_list(
                        state,
                        window,
                        list_box,
                        status,
                        spinner,
                        scrolled,
                        import_btn,
                        open_btn,
                    )
                        .await;
                }
            });
            glib::ControlFlow::Break
        });
    }
}

/// Import a VPN profile via `nmcli connection import`.
///
/// Disables autoconnect and tears down the connection if NM activated it
/// immediately — the user controls when to connect.
fn import_vpn_profile(path: &Path) -> Result<String, String> {
    let before = list_vpn_profile_uuids()?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let nm_type = match ext.as_str() {
        "ovpn" => "openvpn",
        "conf" => "wireguard",
        _ => return Err("unsupported file type (use .ovpn or .conf)".to_string()),
    };

    let output = Command::new("nmcli")
        .arg("connection")
        .arg("import")
        .arg("type")
        .arg(nm_type)
        .arg("file")
        .arg(path.as_os_str())
        .output()
        .map_err(|e| format!("failed to run nmcli: {e}"))?;

    if output.status.success() {
        let after = list_vpn_profile_uuids()?;
        let imported: Vec<String> = after.difference(&before).cloned().collect();
        for uuid in &imported {
            let _ = run_nmcli(&[
                "connection", "modify", "uuid", uuid,
                "connection.autoconnect", "no",
            ]);
            // Bring down the connection if NM auto-activated it.
            let _ = run_nmcli(&["connection", "down", "uuid", uuid]);
        }
        if imported.is_empty() {
            Ok("VPN profile imported".to_string())
        } else {
            Ok("VPN profile imported (autoconnect disabled)".to_string())
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err("nmcli import failed".to_string())
        } else if stderr.contains("already exists")
            || stderr.contains("exists")
            || stderr.contains("duplicate")
        {
            Err("profile already exists (same name/UUID). Rename it or delete the old profile and retry".to_string())
        } else {
            Err(stderr)
        }
    }
}

/// Return the set of UUIDs for all VPN/WireGuard profiles known to NM.
fn list_vpn_profile_uuids() -> Result<HashSet<String>, String> {
    let output = run_nmcli(&["-t", "-f", "UUID,TYPE", "connection", "show"])?;
    if !output.status.success() {
        return Err("failed to list NetworkManager connections".to_string());
    }
    let mut out = HashSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.splitn(2, ':');
        let uuid = parts.next().unwrap_or("").trim();
        let kind = parts.next().unwrap_or("").trim();
        if uuid.is_empty() {
            continue;
        }
        if kind == "vpn" || kind == "wireguard" {
            out.insert(uuid.to_string());
        }
    }
    Ok(out)
}

/// Thin wrapper around `Command::new("nmcli")`.
fn run_nmcli(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("nmcli")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run nmcli: {e}"))
}
