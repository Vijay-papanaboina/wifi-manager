//! Application configuration loaded from `~/.config/wifi-manager/config.toml`.

use serde::Deserialize;
use std::path::PathBuf;

/// Window position on screen.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Position {
    #[default]
    Center,
    TopRight,
    TopCenter,
    TopLeft,
    BottomRight,
    BottomCenter,
    BottomLeft,
    CenterRight,
    CenterLeft,
}

/// Application configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct Config {
    /// Window position (default: "center")
    pub(crate) position: Position,

    /// Margin from top edge in pixels
    pub(crate) margin_top: i32,

    /// Margin from right edge in pixels
    pub(crate) margin_right: i32,

    /// Margin from bottom edge in pixels
    pub(crate) margin_bottom: i32,

    /// Margin from left edge in pixels
    pub(crate) margin_left: i32,

    /// Custom signal strength icons [weak, fair, good, strong]
    pub(crate) signal_icons: [String; 4],

    /// Custom lock icon for secured networks
    pub(crate) lock_icon: String,

    /// Custom saved icon for saved networks
    pub(crate) saved_icon: String,

    /// Custom icon for Night Mode enabled
    pub(crate) night_mode_on_icon: String,

    /// Custom icon for Night Mode disabled
    pub(crate) night_mode_off_icon: String,

    /// Custom icon for logout action
    pub(crate) logout_icon: String,

    /// Custom icon for reboot action
    pub(crate) reboot_icon: String,

    /// Custom icon for suspend / sleep action
    pub(crate) suspend_icon: String,

    /// Custom icon for power off action
    pub(crate) poweroff_icon: String,

    /// Whether to show the panel when the daemon starts (default: false)
    pub(crate) show_on_start: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            position: Position::default(),
            margin_top: 10,
            margin_right: 10,
            margin_bottom: 10,
            margin_left: 10,
            signal_icons: [
                "󰤟".to_string(), // weak
                "󰤢".to_string(), // fair
                "󰤥".to_string(), // good
                "󰤨".to_string(), // strong
            ],
            lock_icon: "󰌾".to_string(),
            saved_icon: "".to_string(),
            night_mode_on_icon: "".to_string(),
            night_mode_off_icon: "".to_string(),
            logout_icon: "".to_string(),
            reboot_icon: "".to_string(),
            suspend_icon: "󰒲".to_string(),
            poweroff_icon: "".to_string(),
            show_on_start: false,
        }
    }
}

impl Config {
    /// Load config from `~/.config/wifi-manager/config.toml`.
    /// Falls back to defaults if file doesn't exist or has errors.
    pub(crate) fn load() -> Self {
        let Some(path) = config_file_path() else {
            return Self::default();
        };

        if !path.exists() {
            log::info!("No config file found, using defaults");
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<Config>(&contents) {
                Ok(config) => {
                    log::info!("Config loaded from {:?}", path);
                    config
                }
                Err(e) => {
                    log::warn!("Failed to parse config file: {e}, using defaults");
                    Self::default()
                }
            },
            Err(e) => {
                log::warn!("Failed to read config file: {e}, using defaults");
                Self::default()
            }
        }
    }
}

/// Get the config file path: ~/.config/wifi-manager/config.toml
fn config_file_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config").join("wifi-manager").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_stable() {
        let config = Config::default();
        assert_eq!(config.position, Position::Center);
        assert_eq!(config.margin_top, 10);
        assert_eq!(config.signal_icons.len(), 4);
        assert!(!config.show_on_start);
    }

    #[test]
    fn parses_partial_user_configuration_with_defaults() {
        let config: Config = toml::from_str(
            r#"
                position = "top-right"
                margin_right = 24
                show_on_start = true
            "#,
        )
        .expect("valid partial config fixture");

        assert_eq!(config.position, Position::TopRight);
        assert_eq!(config.margin_right, 24);
        assert_eq!(config.margin_left, 10);
        assert!(config.show_on_start);
    }
}
