//! Data model for Bluetooth devices as presented to the UI.
//!
//! Equivalent to `access_point.rs` for WiFi networks.

use std::fmt;

/// Category of a Bluetooth device, derived from the BlueZ `Icon` property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceCategory {
    Audio,
    Input,
    Computer,
    Phone,
    Peripheral,
    Other,
}

impl fmt::Display for DeviceCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceCategory::Audio => write!(f, "Audio"),
            DeviceCategory::Input => write!(f, "Input"),
            DeviceCategory::Computer => write!(f, "Computer"),
            DeviceCategory::Phone => write!(f, "Phone"),
            DeviceCategory::Peripheral => write!(f, "Peripheral"),
            DeviceCategory::Other => write!(f, "Device"),
        }
    }
}

impl DeviceCategory {
    /// Map a BlueZ icon string (e.g. "audio-headset") to a category.
    pub fn from_icon_hint(icon: &str) -> Self {
        if icon.starts_with("audio") {
            DeviceCategory::Audio
        } else if icon.starts_with("input") {
            DeviceCategory::Input
        } else if icon.starts_with("computer") {
            DeviceCategory::Computer
        } else if icon.starts_with("phone") {
            DeviceCategory::Phone
        } else if icon.starts_with("modem")
            || icon.starts_with("network")
            || icon.starts_with("printer")
            || icon.starts_with("camera")
            || icon.starts_with("video")
        {
            DeviceCategory::Peripheral
        } else {
            DeviceCategory::Other
        }
    }

    /// Default Nerd Font icon for this device category.
    pub fn default_icon(&self) -> &'static str {
        match self {
            DeviceCategory::Audio => "󰋋",       // headphones
            DeviceCategory::Input => "󰌌",       // keyboard
            DeviceCategory::Computer => "󰍹",    // monitor/desktop
            DeviceCategory::Phone => "󰏲",       // phone
            DeviceCategory::Peripheral => "󰐻",  // device
            DeviceCategory::Other => "󰂯",       // bluetooth
        }
    }
}

/// A Bluetooth device as presented to the UI.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BluetoothDevice {
    /// Bluetooth MAC address (e.g. "AA:BB:CC:DD:EE:FF").
    pub address: String,
    /// Friendly display name (alias preferred, then name, then address).
    pub display_name: String,
    /// Category derived from BlueZ icon hint.
    pub category: DeviceCategory,
    /// Whether this device is paired.
    pub paired: bool,
    /// Whether this device is currently connected.
    pub connected: bool,
    /// Whether this device is trusted (auto-connect).
    pub trusted: bool,
    /// RSSI signal strength (only valid during discovery, 0 otherwise).
    pub rssi: i16,
    /// D-Bus object path for this device.
    pub device_path: String,
}

impl BluetoothDevice {
    /// Sort key: connected first, then paired, then by name.
    pub fn sort_key(&self) -> (u8, u8, String) {
        let connected_order = if self.connected { 0 } else { 1 };
        let paired_order = if self.paired { 0 } else { 1 };
        (connected_order, paired_order, self.display_name.to_lowercase())
    }
}
