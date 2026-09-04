use std::fmt;

/// Human-facing category derived from the BlueZ icon hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceCategory {
    Audio,
    Input,
    Mouse,
    Computer,
    Phone,
    Peripheral,
    Other,
}

impl fmt::Display for DeviceCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Audio => "Audio",
            Self::Input => "Input",
            Self::Mouse => "Mouse",
            Self::Computer => "Computer",
            Self::Phone => "Phone",
            Self::Peripheral => "Peripheral",
            Self::Other => "Device",
        })
    }
}

impl DeviceCategory {
    pub(crate) fn from_icon_hint(icon: &str) -> Self {
        if icon.starts_with("audio") {
            Self::Audio
        } else if icon == "input-mouse" {
            Self::Mouse
        } else if icon.starts_with("input") {
            Self::Input
        } else if icon.starts_with("computer") {
            Self::Computer
        } else if icon.starts_with("phone") {
            Self::Phone
        } else if icon.starts_with("modem")
            || icon.starts_with("network")
            || icon.starts_with("printer")
            || icon.starts_with("camera")
            || icon.starts_with("video")
        {
            Self::Peripheral
        } else {
            Self::Other
        }
    }

    pub(crate) fn default_icon(self) -> &'static str {
        match self {
            Self::Audio => "󰋋",
            Self::Mouse => "󰍽",
            Self::Input => "󰌌",
            Self::Computer => "󰍹",
            Self::Phone => "󰏲",
            Self::Peripheral => "󰐻",
            Self::Other => "󰂯",
        }
    }
}

/// A Bluetooth device presented to the application/UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BluetoothDevice {
    pub(crate) address: String,
    pub(crate) display_name: String,
    pub(crate) category: DeviceCategory,
    pub(crate) paired: bool,
    pub(crate) connected: bool,
    pub(crate) trusted: bool,
    pub(crate) rssi: i16,
    pub(crate) device_path: String,
}

impl BluetoothDevice {
    pub(crate) fn is_in_range(&self) -> bool {
        self.rssi != 0
    }

    pub(crate) fn sort_key(&self) -> (u8, u8, String, String) {
        (
            if self.connected { 0 } else { 1 },
            if self.paired { 0 } else { 1 },
            self.display_name.to_lowercase(),
            self.device_path.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_bluez_hints_to_categories() {
        assert_eq!(DeviceCategory::from_icon_hint("audio-headset"), DeviceCategory::Audio);
        assert_eq!(DeviceCategory::from_icon_hint("input-mouse"), DeviceCategory::Mouse);
        assert_eq!(DeviceCategory::from_icon_hint("unknown"), DeviceCategory::Other);
    }

    #[test]
    fn sorts_connected_then_paired_then_name_and_path() {
        let device = |name: &str, paired: bool, connected: bool, path: &str| BluetoothDevice {
            address: String::new(),
            display_name: name.to_string(),
            category: DeviceCategory::Other,
            paired,
            connected,
            trusted: false,
            rssi: 0,
            device_path: path.to_string(),
        };

        let connected_unpaired = device("zulu", false, true, "/dev/z");
        let paired_disconnected = device("alpha", true, false, "/dev/a");
        assert!(connected_unpaired.sort_key() < paired_disconnected.sort_key());

        let first = device("same", false, false, "/dev/a");
        let second = device("same", false, false, "/dev/b");
        assert!(first.sort_key() < second.sort_key());
    }
}
