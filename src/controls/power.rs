use std::process::Command;

pub fn poweroff() -> Result<(), String> {
    match Command::new("systemctl").arg("poweroff").status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("systemctl poweroff exited with status: {}", status)),
        Err(e) => Err(format!("Failed to execute systemctl poweroff: {}", e)),
    }
}

pub fn reboot() -> Result<(), String> {
    match Command::new("systemctl").arg("reboot").status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("systemctl reboot exited with status: {}", status)),
        Err(e) => Err(format!("Failed to execute systemctl reboot: {}", e)),
    }
}

pub fn suspend() -> Result<(), String> {
    match Command::new("systemctl").arg("suspend").status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("systemctl suspend exited with status: {}", status)),
        Err(e) => Err(format!("Failed to execute systemctl suspend: {}", e)),
    }
}

pub fn logout() -> Result<(), String> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    
    if desktop.contains("hyprland") {
        match Command::new("hyprctl").args(["dispatch", "exit"]).status() {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("hyprctl dispatch exit exited with status: {}", status)),
            Err(e) => Err(format!("Failed to execute hyprctl dispatch exit: {}", e)),
        }
    } else if desktop.contains("sway") {
        match Command::new("swaymsg").arg("exit").status() {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("swaymsg exit exited with status: {}", status)),
            Err(e) => Err(format!("Failed to execute swaymsg exit: {}", e)),
        }
    } else if desktop.contains("river") {
        match Command::new("riverctl").arg("exit").status() {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("riverctl exit exited with status: {}", status)),
            Err(e) => Err(format!("Failed to execute riverctl exit: {}", e)),
        }
    } else {
        Err(format!("Unsupported or unknown Wayland compositor for logout: {}", desktop))
    }
}
