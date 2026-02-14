use std::fmt;

/// Represents the security type of a WiFi network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityType {
    Open,
    WPA2,
    WPA3,
    Enterprise,
}

impl fmt::Display for SecurityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecurityType::Open => write!(f, "Open"),
            SecurityType::WPA2 => write!(f, "WPA2"),
            SecurityType::WPA3 => write!(f, "WPA3"),
            SecurityType::Enterprise => write!(f, "Enterprise"),
        }
    }
}

/// Represents the frequency band of a WiFi network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Band {
    TwoGhz,
    FiveGhz,
}

impl fmt::Display for Band {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Band::TwoGhz => write!(f, "2.4 GHz"),
            Band::FiveGhz => write!(f, "5 GHz"),
        }
    }
}

/// Determine band from frequency in MHz.
impl Band {
    pub fn from_frequency(freq: u32) -> Self {
        if freq >= 4900 {
            Band::FiveGhz
        } else {
            Band::TwoGhz
        }
    }
}

/// NM AP flags — maps to NM80211ApFlags.
/// Bit 0x1 = privacy (network requires encryption).
const NM_802_11_AP_FLAGS_PRIVACY: u32 = 0x1;

/// NM AP security flags — maps to NM80211ApSecurityFlags.
const NM_802_11_AP_SEC_KEY_MGMT_PSK: u32 = 0x100;
const NM_802_11_AP_SEC_KEY_MGMT_SAE: u32 = 0x400;
const NM_802_11_AP_SEC_KEY_MGMT_802_1X: u32 = 0x200;

/// Determine security type from NM AP flags.
pub fn security_from_flags(flags: u32, wpa_flags: u32, rsn_flags: u32) -> SecurityType {
    let all_sec_flags = wpa_flags | rsn_flags;

    // Check for Enterprise (802.1X)
    if all_sec_flags & NM_802_11_AP_SEC_KEY_MGMT_802_1X != 0 {
        return SecurityType::Enterprise;
    }

    // Check for WPA3 (SAE)
    if all_sec_flags & NM_802_11_AP_SEC_KEY_MGMT_SAE != 0 {
        return SecurityType::WPA3;
    }

    // Check for WPA2/WPA (PSK)
    if all_sec_flags & NM_802_11_AP_SEC_KEY_MGMT_PSK != 0 {
        return SecurityType::WPA2;
    }

    // Check basic privacy flag (WEP or similar)
    if flags & NM_802_11_AP_FLAGS_PRIVACY != 0 {
        return SecurityType::WPA2; // Treat WEP as "secured" — rare these days
    }

    SecurityType::Open
}

/// A WiFi network as presented to the UI.
/// May represent multiple APs with the same SSID (deduplicated).
#[derive(Debug, Clone)]
pub struct Network {
    pub ssid: String,
    pub strength: u8,
    pub security: SecurityType,
    pub is_connected: bool,
    pub is_saved: bool,
    pub band: Band,
    /// D-Bus path of the strongest AP for this SSID (used when connecting).
    pub ap_path: String,
    /// D-Bus path of the saved connection profile, if any.
    pub connection_path: Option<String>,
}
