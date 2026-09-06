//! Application configuration loaded from `~/.config/wifi-manager/config.toml`.

use serde::Deserialize;
use std::io;
use std::path::PathBuf;

const SAVED_CHARGE_LIMIT_PRESETS: [u8; 6] = [50, 60, 70, 80, 90, 100];

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

    /// Icon name for the Wi-Fi home tile.
    pub(crate) wifi_icon: String,

    /// Icon name for the Bluetooth home tile.
    pub(crate) bluetooth_icon: String,

    /// Icon name for the Audio home tile.
    pub(crate) audio_icon: String,

    /// Icon name for the Power / Battery home tile.
    pub(crate) power_battery_icon: String,

    /// Icon name used by the Media home card when artwork is unavailable.
    pub(crate) media_icon: String,

    /// Icon name for the System home tile.
    pub(crate) system_icon: String,

    /// Icon name for the brightness quick control.
    pub(crate) brightness_icon: String,

    /// Icon name for the volume quick control.
    pub(crate) volume_icon: String,

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

    /// Last charge limit successfully applied by the user, when known.
    #[serde(default)]
    pub(crate) charge_limit: Option<u8>,
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
            wifi_icon: "network-wireless-symbolic".to_string(),
            bluetooth_icon: "bluetooth-active-symbolic".to_string(),
            audio_icon: "audio-volume-high-symbolic".to_string(),
            power_battery_icon: "battery-symbolic".to_string(),
            media_icon: "audio-x-generic-symbolic".to_string(),
            system_icon: "utilities-system-monitor-symbolic".to_string(),
            brightness_icon: "display-brightness-symbolic".to_string(),
            volume_icon: "audio-volume-high-symbolic".to_string(),
            night_mode_on_icon: "".to_string(),
            night_mode_off_icon: "".to_string(),
            logout_icon: "".to_string(),
            reboot_icon: "".to_string(),
            suspend_icon: "󰒲".to_string(),
            poweroff_icon: "".to_string(),
            show_on_start: false,
            charge_limit: None,
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

    /// Return the saved charge limit only when it is one of the presets that
    /// the UI and root helper are allowed to apply.
    pub(crate) fn saved_charge_limit(&self) -> Option<u8> {
        self.charge_limit.filter(|value| SAVED_CHARGE_LIMIT_PRESETS.contains(value))
    }

    /// Persist a successfully verified charge-limit preset without rewriting
    /// the rest of the user's TOML file. Existing comments and unrelated
    /// settings are retained by updating only the top-level assignment.
    pub(crate) fn persist_charge_limit(value: u8) -> io::Result<()> {
        if !SAVED_CHARGE_LIMIT_PRESETS.contains(&value) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported charge-limit preset: {value}%"),
            ));
        }

        let path = config_file_path().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "home directory is not available")
        })?;
        let existing = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        let updated = upsert_charge_limit(&existing, value)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, updated)
    }
}

fn upsert_charge_limit(contents: &str, value: u8) -> io::Result<String> {
    let mut document = contents.parse::<toml_edit::DocumentMut>().map_err(|error| {
        io::Error::new(io::ErrorKind::InvalidData, format!("invalid config TOML: {error}"))
    })?;

    if let Some(item) = document.get_mut("charge_limit") {
        if !item.is_value() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "top-level charge_limit must be an integer",
            ));
        }
        let (prefix, suffix) = {
            let current = item.as_value().expect("value item checked above");
            (
                current.decor().prefix().and_then(|raw| raw.as_str()).unwrap_or("").to_owned(),
                current.decor().suffix().and_then(|raw| raw.as_str()).unwrap_or("").to_owned(),
            )
        };
        let mut replacement = toml_edit::value(i64::from(value));
        {
            let replacement_value = replacement.as_value_mut().expect("new integer is a value");
            replacement_value.decor_mut().set_prefix(prefix);
            replacement_value.decor_mut().set_suffix(suffix);
        }
        *item = replacement;
    } else {
        let first_table = document.iter().find_map(|(key, item)| {
            item.as_table()
                .or_else(|| item.as_array_of_tables().and_then(|tables| tables.get(0)))
                .map(|table| {
                    (
                        key.to_owned(),
                        table
                            .decor()
                            .prefix()
                            .and_then(|raw| raw.as_str())
                            .unwrap_or("")
                            .to_owned(),
                    )
                })
        });
        let replacement = toml_edit::value(i64::from(value));
        if let Some((table_key, prefix)) = first_table
            && !prefix.is_empty()
        {
            if let Some(item) = document.get_mut(&table_key) {
                match item {
                    toml_edit::Item::Table(table) => table.decor_mut().set_prefix(""),
                    toml_edit::Item::ArrayOfTables(tables) => {
                        if let Some(table) = tables.get_mut(0) {
                            table.decor_mut().set_prefix("");
                        }
                    }
                    _ => {}
                }
            }
            document.insert("charge_limit", replacement);
            if let Some(mut key) = document.key_mut("charge_limit") {
                key.leaf_decor_mut().set_prefix(prefix);
            }
            return Ok(document.to_string());
        }
        document.insert("charge_limit", replacement);
    }

    Ok(document.to_string())
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
        assert_eq!(config.wifi_icon, "network-wireless-symbolic");
        assert_eq!(config.bluetooth_icon, "bluetooth-active-symbolic");
        assert_eq!(config.audio_icon, "audio-volume-high-symbolic");
        assert_eq!(config.power_battery_icon, "battery-symbolic");
        assert_eq!(config.media_icon, "audio-x-generic-symbolic");
        assert_eq!(config.system_icon, "utilities-system-monitor-symbolic");
        assert_eq!(config.brightness_icon, "display-brightness-symbolic");
        assert_eq!(config.volume_icon, "audio-volume-high-symbolic");
        assert!(!config.show_on_start);
        assert_eq!(config.charge_limit, None);
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

    #[test]
    fn parses_custom_control_center_icons() {
        let config: Config = toml::from_str(
            r#"
                wifi_icon = "custom-wifi"
                bluetooth_icon = "custom-bluetooth"
                audio_icon = "custom-audio"
                power_battery_icon = "custom-power"
                media_icon = "custom-media"
                system_icon = "custom-system"
                brightness_icon = "custom-brightness"
                volume_icon = "custom-volume"
                night_mode_on_icon = "custom-night-on"
                night_mode_off_icon = "custom-night-off"
            "#,
        )
        .expect("valid custom icon configuration");

        assert_eq!(config.wifi_icon, "custom-wifi");
        assert_eq!(config.bluetooth_icon, "custom-bluetooth");
        assert_eq!(config.audio_icon, "custom-audio");
        assert_eq!(config.power_battery_icon, "custom-power");
        assert_eq!(config.media_icon, "custom-media");
        assert_eq!(config.system_icon, "custom-system");
        assert_eq!(config.brightness_icon, "custom-brightness");
        assert_eq!(config.volume_icon, "custom-volume");
        assert_eq!(config.night_mode_on_icon, "custom-night-on");
        assert_eq!(config.night_mode_off_icon, "custom-night-off");
    }

    #[test]
    fn saved_charge_limit_accepts_only_presets() {
        let valid: Config = toml::from_str("charge_limit = 80").expect("valid charge limit");
        assert_eq!(valid.saved_charge_limit(), Some(80));

        let invalid: Config = toml::from_str("charge_limit = 55").expect("parse invalid preset");
        assert_eq!(invalid.saved_charge_limit(), None);
    }

    #[test]
    fn charge_limit_update_preserves_comments_and_other_tables() {
        let original = "# keep this comment\nposition = \"center\"\ncharge_limit = 70 # keep annotation\n\n[display]\ncharge_limit = 50\n";
        let updated = upsert_charge_limit(original, 90).expect("valid config TOML");
        assert!(updated.contains("# keep this comment"));
        assert!(updated.contains("position = \"center\""));
        assert!(updated.contains("charge_limit = 90 # keep annotation"));
        assert!(updated.contains("[display]\ncharge_limit = 50\n"));
    }

    #[test]
    fn charge_limit_update_inserts_at_top_level_before_a_table() {
        let updated = upsert_charge_limit("# settings\n[display]\nbrightness = 80\n", 60)
            .expect("valid config TOML");
        assert_eq!(updated, "# settings\ncharge_limit = 60\n[display]\nbrightness = 80\n");
    }

    #[test]
    fn charge_limit_update_handles_quoted_keys() {
        let updated = upsert_charge_limit("\"charge_limit\" = 70\n", 90)
            .expect("valid quoted-key config TOML");
        assert_eq!(updated, "\"charge_limit\" = 90\n");
        let config: Config = toml::from_str(&updated).expect("updated config TOML");
        assert_eq!(config.saved_charge_limit(), Some(90));
    }

    #[test]
    fn charge_limit_update_rejects_invalid_toml() {
        let error = upsert_charge_limit("charge_limit = [", 80).expect_err("invalid TOML");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("invalid config TOML"));
    }
}
