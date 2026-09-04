use std::fmt;

/// Security capabilities reported by NetworkManager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecurityType {
    Open,
    WPA2,
    WPA3,
    Enterprise,
}

impl fmt::Display for SecurityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Open => "Open",
            Self::WPA2 => "WPA2",
            Self::WPA3 => "WPA3",
            Self::Enterprise => "Enterprise",
        };
        f.write_str(label)
    }
}

/// Frequency band derived from an access point frequency in MHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Band {
    TwoGhz,
    FiveGhz,
}

impl fmt::Display for Band {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TwoGhz => "2.4 GHz",
            Self::FiveGhz => "5 GHz",
        })
    }
}

impl Band {
    pub(crate) fn from_frequency(freq: u32) -> Self {
        if freq >= 4900 { Self::FiveGhz } else { Self::TwoGhz }
    }
}

const NM_802_11_AP_FLAGS_PRIVACY: u32 = 0x1;
const NM_802_11_AP_SEC_KEY_MGMT_PSK: u32 = 0x100;
const NM_802_11_AP_SEC_KEY_MGMT_802_1X: u32 = 0x200;
const NM_802_11_AP_SEC_KEY_MGMT_SAE: u32 = 0x400;

/// Classify NetworkManager security flags without requiring a D-Bus runtime.
pub(crate) fn security_from_flags(flags: u32, wpa_flags: u32, rsn_flags: u32) -> SecurityType {
    let all_security_flags = wpa_flags | rsn_flags;

    if all_security_flags & NM_802_11_AP_SEC_KEY_MGMT_802_1X != 0 {
        return SecurityType::Enterprise;
    }
    if all_security_flags & NM_802_11_AP_SEC_KEY_MGMT_SAE != 0 {
        return SecurityType::WPA3;
    }
    if all_security_flags & NM_802_11_AP_SEC_KEY_MGMT_PSK != 0 {
        return SecurityType::WPA2;
    }
    if flags & NM_802_11_AP_FLAGS_PRIVACY != 0 {
        // WEP is still treated as a secured network by the existing UI.
        return SecurityType::WPA2;
    }

    SecurityType::Open
}

/// A deduplicated Wi-Fi network presented to the application/UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Network {
    pub(crate) ssid: String,
    pub(crate) strength: u8,
    pub(crate) security: SecurityType,
    pub(crate) is_connected: bool,
    pub(crate) is_saved: bool,
    pub(crate) band: Band,
    pub(crate) ap_path: String,
    pub(crate) connection_path: Option<String>,
}

/// Return whether `candidate` should replace an existing AP for one SSID.
/// The active BSSID wins first so a connected row cannot disappear when a
/// stronger AP for the same SSID is also visible. Stronger signal then wins;
/// equal-strength ties use the object path so results do not depend on
/// NetworkManager's iteration order.
pub(crate) fn candidate_is_preferred(existing: &Network, candidate: &Network) -> bool {
    (candidate.is_connected && !existing.is_connected)
        || (candidate.is_connected == existing.is_connected
            && (candidate.strength > existing.strength
                || (candidate.strength == existing.strength
                    && candidate.ap_path < existing.ap_path)))
}

/// Sort networks using the stable UI priority: connected, saved, signal, name,
/// then AP path as a final deterministic tie-break.
pub(crate) fn sort_networks(networks: &mut [Network]) {
    networks.sort_by(|a, b| {
        b.is_connected
            .cmp(&a.is_connected)
            .then(b.is_saved.cmp(&a.is_saved))
            .then(b.strength.cmp(&a.strength))
            .then_with(|| a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
            .then(a.ap_path.cmp(&b.ap_path))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_frequency_to_expected_band() {
        assert_eq!(Band::from_frequency(2412), Band::TwoGhz);
        assert_eq!(Band::from_frequency(4900), Band::FiveGhz);
    }

    #[test]
    fn prioritizes_enterprise_then_wpa3_then_wpa2() {
        assert_eq!(
            security_from_flags(0, NM_802_11_AP_SEC_KEY_MGMT_PSK, NM_802_11_AP_SEC_KEY_MGMT_802_1X),
            SecurityType::Enterprise
        );
        assert_eq!(
            security_from_flags(0, NM_802_11_AP_SEC_KEY_MGMT_PSK, NM_802_11_AP_SEC_KEY_MGMT_SAE),
            SecurityType::WPA3
        );
        assert_eq!(security_from_flags(0, NM_802_11_AP_SEC_KEY_MGMT_PSK, 0), SecurityType::WPA2);
    }

    fn network(ssid: &str, strength: u8, ap_path: &str) -> Network {
        Network {
            ssid: ssid.to_string(),
            strength,
            security: SecurityType::Open,
            is_connected: false,
            is_saved: false,
            band: Band::TwoGhz,
            ap_path: ap_path.to_string(),
            connection_path: None,
        }
    }

    #[test]
    fn prefers_stronger_and_deterministically_breaks_equal_signal_ties() {
        let existing = network("Cafe", 50, "/ap/z");
        assert!(candidate_is_preferred(&existing, &network("Cafe", 60, "/ap/z")));
        assert!(candidate_is_preferred(&existing, &network("Cafe", 50, "/ap/a")));
        assert!(!candidate_is_preferred(&existing, &network("Cafe", 50, "/ap/z")));
    }

    #[test]
    fn preserves_the_active_bssid_over_a_stronger_duplicate() {
        let existing = Network { is_connected: true, ..network("Cafe", 30, "/ap/z") };
        let candidate = network("Cafe", 90, "/ap/a");
        assert!(!candidate_is_preferred(&existing, &candidate));
        assert!(candidate_is_preferred(&candidate, &existing));
    }

    #[test]
    fn sorts_connected_saved_and_signal_before_name() {
        let mut networks = vec![
            network("zulu", 90, "/ap/z"),
            Network { is_connected: true, ..network("alpha", 10, "/ap/a") },
            Network { is_saved: true, ..network("bravo", 80, "/ap/b") },
        ];
        sort_networks(&mut networks);
        assert_eq!(networks[0].ssid, "alpha");
        assert_eq!(networks[1].ssid, "bravo");
        assert_eq!(networks[2].ssid, "zulu");
    }
}
