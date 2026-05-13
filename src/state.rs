//! Dynamic runtime state persisted to `~/.config/wifi-manager/state.toml`.
//! This file is managed entirely by the application and is separate from
//! the user's static `config.toml` which is never written to.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persisted state for the Night Mode control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NightModeState {
    /// Whether Night Mode is currently enabled.
    pub enabled: bool,
    /// The last user-configured color temperature in Kelvin.
    pub temperature: f64,
}

impl Default for NightModeState {
    fn default() -> Self {
        Self {
            enabled: false,
            temperature: 4500.0,
        }
    }
}

/// Top-level runtime state store. Add new dynamic UI states here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppStateStore {
    #[serde(default)]
    pub night_mode: NightModeState,
}

impl AppStateStore {
    /// Load dynamic state from `~/.config/wifi-manager/state.toml`.
    /// Falls back to defaults silently if the file is missing or malformed.
    pub fn load() -> Self {
        let Some(path) = state_file_path() else {
            return Self::default();
        };

        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<AppStateStore>(&contents) {
                Ok(store) => {
                    log::debug!("State loaded from {:?}", path);
                    store
                }
                Err(e) => {
                    log::warn!("Failed to parse state file: {e}, using defaults");
                    Self::default()
                }
            },
            Err(e) => {
                log::warn!("Failed to read state file: {e}, using defaults");
                Self::default()
            }
        }
    }

    /// Persist the current state to `~/.config/wifi-manager/state.toml`.
    /// Creates the directory if it doesn't exist. Does not touch `config.toml`.
    pub fn save(&self) {
        let Some(path) = state_file_path() else {
            log::warn!("Cannot determine state file path, skipping save");
            return;
        };

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("Failed to create state directory: {e}");
                return;
            }
        }

        match toml::to_string_pretty(self) {
            Ok(contents) => {
                if let Err(e) = std::fs::write(&path, contents) {
                    log::warn!("Failed to write state file: {e}");
                } else {
                    log::debug!("State saved to {:?}", path);
                }
            }
            Err(e) => log::warn!("Failed to serialize state: {e}"),
        }
    }
}

/// Returns the path to `~/.config/wifi-manager/state.toml`.
fn state_file_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("wifi-manager")
            .join("state.toml"),
    )
}
