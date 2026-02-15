//! Application configuration loaded from `~/.config/wifi-manager/config.toml`.

use serde::Deserialize;
use std::path::PathBuf;

/// Window position on screen.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Position {
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

impl Default for Position {
    fn default() -> Self {
        Self::Center
    }
}

/// Application configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Window position (default: "center")
    pub position: Position,

    /// Margin from top edge in pixels
    pub margin_top: i32,

    /// Margin from right edge in pixels
    pub margin_right: i32,

    /// Margin from bottom edge in pixels
    pub margin_bottom: i32,

    /// Margin from left edge in pixels
    pub margin_left: i32,

    /// Custom signal strength icons [weak, fair, good, strong]
    pub signal_icons: [String; 4],
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
                "󰤟".to_string(),  // weak
                "󰤢".to_string(),  // fair
                "󰤥".to_string(),  // good
                "󰤨".to_string(),  // strong
            ],
        }
    }
}

impl Config {
    /// Load config from `~/.config/wifi-manager/config.toml`.
    /// Falls back to defaults if file doesn't exist or has errors.
    pub fn load() -> Self {
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
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("wifi-manager")
            .join("config.toml"),
    )
}
