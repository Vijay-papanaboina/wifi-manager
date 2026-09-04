//! VPN profile import — file picker, nmcli invocation, and post-import refresh.
//!
//! Isolated because all of this logic is about file I/O and subprocess calls;
//! none of it sets up persistent GTK signal handlers.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

use crate::error::{AppError, AppResult};
use crate::process::{self, CommandOutput};

use super::AppState;
use super::vpn::VpnView;

/// Open a file-chooser dialog and import the selected `.ovpn`/`.conf` profile.
pub(super) fn open_import_dialog(
    state: Rc<RefCell<AppState>>,
    view: VpnView,
    on_done: impl Fn() + 'static,
) {
    let VpnView { window, list_box, status, spinner, scrolled, import_btn, open_btn } = view;
    let chooser =
        gtk4::FileDialog::builder().title("Import VPN Profile").accept_label("Import").build();

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

                match import_vpn_profile(path).await {
                    Ok(msg) => {
                        status.set_text(&msg);
                        schedule_post_import_refresh(
                            Rc::clone(&state),
                            VpnView {
                                window: window.clone(),
                                list_box: list_box.clone(),
                                status: status.clone(),
                                spinner: spinner.clone(),
                                scrolled: scrolled.clone(),
                                import_btn: import_btn.clone(),
                                open_btn: open_btn.clone(),
                            },
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
pub(super) fn schedule_post_import_refresh(state: Rc<RefCell<AppState>>, view: VpnView) {
    let delays_ms = [0_u64, 800, 1800, 3200];
    for delay in delays_ms {
        let state = Rc::clone(&state);
        let view = view.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(delay), move || {
            glib::spawn_future_local({
                let state = Rc::clone(&state);
                let view = view.clone();
                async move {
                    super::vpn::refresh_vpn_list(state, view).await;
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
async fn import_vpn_profile(path: PathBuf) -> AppResult<String> {
    let before = list_vpn_profile_uuids().await?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let nm_type = match ext.as_str() {
        "ovpn" => "openvpn",
        "conf" => "wireguard",
        _ => return Err(AppError::from("unsupported file type (use .ovpn or .conf)")),
    };

    let args = [
        OsString::from("connection"),
        OsString::from("import"),
        OsString::from("type"),
        OsString::from(nm_type),
        OsString::from("file"),
        path.as_os_str().to_os_string(),
    ];
    let output = process::run("nmcli", args.iter()).await?;

    if output.success() {
        let after = list_vpn_profile_uuids().await?;
        let imported: Vec<String> = after.difference(&before).cloned().collect();
        for uuid in &imported {
            run_nmcli(&["connection", "modify", "uuid", uuid, "connection.autoconnect", "no"])
                .await?
                .into_success("nmcli")?;
            // Bring down the connection if NM auto-activated it.
            run_nmcli(&["connection", "down", "uuid", uuid]).await?.into_success("nmcli")?;
        }
        if imported.is_empty() {
            Ok("VPN profile imported".to_string())
        } else {
            Ok("VPN profile imported (autoconnect disabled)".to_string())
        }
    } else {
        let stderr = output.stderr.trim().to_string();
        if stderr.is_empty() {
            Err(AppError::from("nmcli import failed"))
        } else if stderr.contains("already exists")
            || stderr.contains("exists")
            || stderr.contains("duplicate")
        {
            Err(AppError::from(
                "profile already exists (same name/UUID). Rename it or delete the old profile and retry",
            ))
        } else {
            Err(AppError::from(stderr))
        }
    }
}

/// Return the set of UUIDs for all VPN/WireGuard profiles known to NM.
async fn list_vpn_profile_uuids() -> AppResult<HashSet<String>> {
    let output =
        run_nmcli(&["-t", "-f", "UUID,TYPE", "connection", "show"]).await?.into_success("nmcli")?;
    let mut out = HashSet::new();
    for line in output.stdout.lines() {
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

/// Thin wrapper around the shared asynchronous subprocess adapter.
async fn run_nmcli(args: &[&str]) -> AppResult<CommandOutput> {
    process::run("nmcli", args).await
}
