use futures_util::future::LocalBoxFuture;

use crate::error::{AppError, AppResult};
use crate::process;

async fn run_systemctl(cmd: &'static str) -> AppResult<()> {
    process::run("systemctl", [cmd]).await?.into_success("systemctl")?;
    Ok(())
}

pub(crate) fn poweroff() -> LocalBoxFuture<'static, AppResult<()>> {
    Box::pin(run_systemctl("poweroff"))
}

pub(crate) fn reboot() -> LocalBoxFuture<'static, AppResult<()>> {
    Box::pin(run_systemctl("reboot"))
}

pub(crate) fn suspend() -> LocalBoxFuture<'static, AppResult<()>> {
    Box::pin(run_systemctl("suspend"))
}

async fn execute_logout_command(program: &str, args: &[&str]) -> AppResult<()> {
    process::run(program, args).await?.into_success(program)?;
    Ok(())
}

fn current_user() -> Option<String> {
    ["USER", "USERNAME"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.trim().is_empty()))
}

pub(crate) fn logout() -> LocalBoxFuture<'static, AppResult<()>> {
    Box::pin(async move {
        // 1. Try UWSM if running (modern Hyprland recommended session manager)
        if std::env::var("UWSM_SESSION").is_ok() {
            log::info!("UWSM session detected, attempting graceful stop");
            if execute_logout_command("uwsm", &["stop"]).await.is_ok() {
                return Ok(());
            }
        }

        let desktop = match std::env::var("XDG_CURRENT_DESKTOP") {
            Ok(val) => val.to_lowercase(),
            Err(_) => "unknown".to_string(),
        };

        // 2. Try compositor-specific dispatchers
        if desktop.contains("hyprland") {
            log::info!("Hyprland detected, trying dispatchers");
            // Try new Lua syntax (v0.55+) first
            if execute_logout_command("hyprctl", &["dispatch", "hl.dsp.exit()"]).await.is_ok() {
                return Ok(());
            }
            // Fallback to legacy syntax (v0.54 and below)
            if execute_logout_command("hyprctl", &["dispatch", "exit"]).await.is_ok() {
                return Ok(());
            }
        } else if desktop.contains("sway") {
            if execute_logout_command("swaymsg", &["exit"]).await.is_ok() {
                return Ok(());
            }
        } else if desktop.contains("river")
            && execute_logout_command("riverctl", &["exit"]).await.is_ok()
        {
            return Ok(());
        }

        // 3. Final fallback: terminate the current user session. Supplying an
        // actual user is required; an empty loginctl argument always fails.
        log::warn!("Compositor exit failed or unknown; falling back to loginctl");
        let user =
            current_user().ok_or_else(|| AppError::from("could not determine current user"))?;
        execute_logout_command("loginctl", &["terminate-user", &user]).await
    })
}
