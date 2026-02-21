use std::process::Command;

pub fn poweroff() {
    let _ = Command::new("systemctl").arg("poweroff").spawn();
}

pub fn reboot() {
    let _ = Command::new("systemctl").arg("reboot").spawn();
}

pub fn suspend() {
    let _ = Command::new("systemctl").arg("suspend").spawn();
}

pub fn logout() {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    
    if desktop.contains("hyprland") {
        let _ = Command::new("hyprctl").args(["dispatch", "exit"]).spawn();
    } else if desktop.contains("sway") {
        let _ = Command::new("swaymsg").arg("exit").spawn();
    } else if desktop.contains("river") {
        let _ = Command::new("riverctl").arg("exit").spawn();
    } else {
        log::error!("Unsupported or unknown Wayland compositor for logout: {}", desktop);
    }
}
