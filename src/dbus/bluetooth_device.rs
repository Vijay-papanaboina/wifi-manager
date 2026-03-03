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
    /// Indicates whether the device is currently in range based on RSSI.
    ///
    /// `true` if the device's RSSI is not zero, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// let dev = BluetoothDevice { rssi: -42, address: "".into(), display_name: "".into(), category: DeviceCategory::Other, paired: false, connected: false, trusted: false, device_path: "".into() };
    /// assert!(dev.is_in_range());
    ///
    /// let out_of_range = BluetoothDevice { rssi: 0, ..dev.clone() };
    /// assert!(!out_of_range.is_in_range());
    /// ```
    pub fn is_in_range(&self) -> bool {
        self.rssi != 0
    }

    /// Compute a sort key that orders devices by connected status, in-range status, paired status, then display name.
    ///
    /// The tuple elements are, in order:
    /// 1. `connected_order` — `0` if connected, `1` otherwise.
    /// 2. `in_range_order` — `0` if RSSI indicates the device is in range, `1` otherwise.
    /// 3. `paired_order` — `0` if paired, `1` otherwise.
    /// 4. lowercase display name used as the final tiebreaker.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crate::dbus::{BluetoothDevice, DeviceCategory};
    /// let d_connected = BluetoothDevice {
    ///     address: "AA:BB:CC:DD:EE:FF".into(),
    ///     display_name: "Keyboard".into(),
    ///     category: DeviceCategory::Input,
    ///     paired: true,
    ///     connected: true,
    ///     trusted: false,
    ///     rssi: -40,
    ///     device_path: "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF".into(),
    /// };
    /// let d_near_paired = BluetoothDevice {
    ///     address: "11:22:33:44:55:66".into(),
    ///     display_name: "Mouse".into(),
    ///     category: DeviceCategory::Peripheral,
    ///     paired: true,
    ///     connected: false,
    ///     trusted: false,
    ///     rssi: -50,
    ///     device_path: "/org/bluez/hci0/dev_11_22_33_44_55_66".into(),
    /// };
    ///
    /// // Connected device sorts before a paired-but-not-connected device
    /// assert!(d_connected.sort_key() < d_near_paired.sort_key());
    /// ```
    pub fn sort_key(&self) -> (u8, u8, u8, String) {
        let connected_order = if self.connected { 0 } else { 1 };
        let in_range_order = if self.is_in_range() { 0 } else { 1 };
        let paired_order = if self.paired { 0 } else { 1 };
        (
            connected_order,
            in_range_order,
            paired_order,
            self.display_name.to_lowercase(),
        )
    }
}
