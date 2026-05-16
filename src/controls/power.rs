use std::process::Command;

fn run_systemctl(cmd: &str) -> Result<(), String> {
    match Command::new("systemctl").arg(cmd).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("systemctl {} exited with status: {}", cmd, status)),
        Err(e) => Err(format!("Failed to execute systemctl {}: {}", cmd, e)),
    }
}

pub fn poweroff() -> Result<(), String> {
    run_systemctl("poweroff")
}

pub fn reboot() -> Result<(), String> {
    run_systemctl("reboot")
}

pub fn suspend() -> Result<(), String> {
    run_systemctl("suspend")
}

fn execute_logout_command(program: &str, args: &[&str]) -> Result<(), String> {
    match Command::new(program).args(args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{} exited with status: {}", program, status)),
        Err(e) => Err(format!("Failed to execute {}: {}", program, e)),
    }
}

pub fn logout() -> Result<(), String> {
    // 1. Try UWSM if running (modern Hyprland recommended session manager)
    if std::env::var("UWSM_SESSION").is_ok() {
        log::info!("UWSM session detected, attempting graceful stop");
        if let Ok(_) = execute_logout_command("uwsm", &["stop"]) {
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
        if let Ok(_) = execute_logout_command("hyprctl", &["dispatch", "hl.dsp.exit()"]) {
            return Ok(());
        }
        // Fallback to legacy syntax (v0.54 and below)
        if let Ok(_) = execute_logout_command("hyprctl", &["dispatch", "exit"]) {
            return Ok(());
        }
    } else if desktop.contains("sway") {
        if let Ok(_) = execute_logout_command("swaymsg", &["exit"]) {
            return Ok(());
        }
    } else if desktop.contains("river") {
        if let Ok(_) = execute_logout_command("riverctl", &["exit"]) {
            return Ok(());
        }
    }

    // 3. Final nuclear fallback: terminate the session via loginctl
    // This works on almost any systemd-based distro regardless of the compositor.
    log::warn!("Compositor exit failed or unknown; falling back to loginctl");
    execute_logout_command("loginctl", &["terminate-user", ""])
}
