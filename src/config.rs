//! Application configuration loaded from `~/.config/wifi-manager/config.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Window position on screen.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

    /// Custom lock icon for secured networks
    pub lock_icon: String,

    /// Custom saved icon for saved networks
    pub saved_icon: String,

    /// Whether to show the panel when the daemon starts (default: false)
    pub show_on_start: bool,

    /// Hotspot SSID (default: "Linux-Hotspot")
    pub hotspot_ssid: String,

    /// Hotspot Password (default: random 8-char alphanumeric)
    pub hotspot_password: String,
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
            lock_icon: "".to_string(),
            saved_icon: "".to_string(),
            show_on_start: false,
            hotspot_ssid: "Linux-Hotspot".to_string(),
            hotspot_password: "".to_string(),
        }
    }
}

/// Helper: generate a random alphanumeric password of given length.
fn generate_random_password(length: usize) -> String {
    use rand::Rng;
    let charset: &[u8] = b"abcdefghijklmnopqrstuvwxyz\
                           ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                           0123456789";
    let mut rng = rand::thread_rng();

    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset[idx] as char
        })
        .collect()
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

    /// Save config to `~/.config/wifi-manager/config.toml`.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = config_file_path() else {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Config path not found"));
        };

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        match toml::to_string_pretty(self) {
            Ok(contents) => {
                std::fs::write(&path, contents)?;
                log::info!("Config saved to {:?}", path);
                Ok(())
            }
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
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
