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
    let desktop = match std::env::var("XDG_CURRENT_DESKTOP") {
        Ok(val) => val.to_lowercase(),
        Err(_) => return Err("XDG_CURRENT_DESKTOP environment variable is not set".to_string()),
    };
    
    if desktop.contains("hyprland") {
        execute_logout_command("hyprctl", &["dispatch", "exit"])
    } else if desktop.contains("sway") {
        execute_logout_command("swaymsg", &["exit"])
    } else if desktop.contains("river") {
        execute_logout_command("riverctl", &["exit"])
    } else {
        Err(format!("Unsupported or unknown Wayland compositor for logout: {}", desktop))
    }
}
