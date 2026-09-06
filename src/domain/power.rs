//! Pure battery and power-profile models and mappers.

use std::time::Duration;

/// Battery state values defined by `org.freedesktop.UPower.Device`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum BatteryState {
    #[default]
    Unknown,
    Charging,
    Discharging,
    Empty,
    FullyCharged,
    PendingCharge,
    PendingDischarge,
}

impl BatteryState {
    /// Convert the numeric state used by UPower without treating unknown
    /// future values as a known state.
    pub(crate) fn from_upower(value: u32) -> Self {
        match value {
            1 => Self::Charging,
            2 => Self::Discharging,
            3 => Self::Empty,
            4 => Self::FullyCharged,
            5 => Self::PendingCharge,
            6 => Self::PendingDischarge,
            _ => Self::Unknown,
        }
    }

    /// Human-readable state text for the detail page and home summary.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Charging => "Charging",
            Self::Discharging => "Discharging",
            Self::Empty => "Empty",
            Self::FullyCharged => "Fully charged",
            Self::PendingCharge => "Charge pending",
            Self::PendingDischarge => "Discharge pending",
        }
    }
}

/// Battery data collected from UPower's display device.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BatterySnapshot {
    pub(crate) present: bool,
    pub(crate) percentage: Option<u8>,
    pub(crate) state: BatteryState,
    pub(crate) time_to_empty: Option<Duration>,
    pub(crate) time_to_full: Option<Duration>,
    pub(crate) energy_rate: Option<f64>,
    pub(crate) warning_level: u32,
    pub(crate) icon_name: Option<String>,
}

impl BatterySnapshot {
    /// Snapshot used when UPower is not installed or has no display device.
    pub(crate) fn unavailable() -> Self {
        Self {
            present: false,
            percentage: None,
            state: BatteryState::Unknown,
            time_to_empty: None,
            time_to_full: None,
            energy_rate: None,
            warning_level: 0,
            icon_name: None,
        }
    }
}

impl Default for BatterySnapshot {
    fn default() -> Self {
        Self::unavailable()
    }
}

/// Convert UPower's percentage while guarding against invalid floating-point
/// values and values outside its documented 0--100 range.
pub(crate) fn percentage_from_upower(value: f64) -> Option<u8> {
    if !value.is_finite() {
        return None;
    }

    Some(value.clamp(0.0, 100.0).round() as u8)
}

/// Convert UPower seconds to a duration. UPower uses zero (and can use a
/// negative sentinel) when the estimate is unknown.
pub(crate) fn duration_from_upower(seconds: i64) -> Option<Duration> {
    (seconds > 0).then(|| Duration::from_secs(seconds as u64))
}

/// Keep finite energy rates, including the negative values UPower uses while
/// charging.
pub(crate) fn energy_rate_from_upower(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

/// Raw UPower display-device properties used to build a stable battery
/// snapshot.
pub(crate) struct BatteryProperties {
    pub(crate) present: bool,
    pub(crate) percentage: f64,
    pub(crate) state: u32,
    pub(crate) time_to_empty: i64,
    pub(crate) time_to_full: i64,
    pub(crate) energy_rate: f64,
    pub(crate) warning_level: u32,
    pub(crate) icon_name: String,
}

/// Map the documented UPower display-device fields into a domain snapshot.
pub(crate) fn battery_snapshot_from_upower(properties: BatteryProperties) -> BatterySnapshot {
    BatterySnapshot {
        present: properties.present,
        percentage: percentage_from_upower(properties.percentage),
        state: BatteryState::from_upower(properties.state),
        time_to_empty: duration_from_upower(properties.time_to_empty),
        time_to_full: duration_from_upower(properties.time_to_full),
        energy_rate: energy_rate_from_upower(properties.energy_rate),
        warning_level: properties.warning_level,
        icon_name: (!properties.icon_name.is_empty()).then_some(properties.icon_name),
    }
}

/// A profile advertised by power-profiles-daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PowerProfile {
    pub(crate) id: String,
    pub(crate) driver: Option<String>,
}

/// Power profile data collected from power-profiles-daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PowerProfileSnapshot {
    pub(crate) available: bool,
    pub(crate) active_profile: Option<String>,
    pub(crate) profiles: Vec<PowerProfile>,
    pub(crate) performance_degraded: Option<String>,
}

impl PowerProfileSnapshot {
    /// Snapshot used when power-profiles-daemon is not available.
    pub(crate) fn unavailable() -> Self {
        Self {
            available: false,
            active_profile: None,
            profiles: Vec::new(),
            performance_degraded: None,
        }
    }

    /// Build an available snapshot and apply the domain's deterministic
    /// profile ordering.
    pub(crate) fn available(
        active_profile: String,
        mut profiles: Vec<PowerProfile>,
        performance_degraded: String,
    ) -> Self {
        sort_power_profiles(&mut profiles);

        Self {
            available: true,
            active_profile: (!active_profile.is_empty()).then_some(active_profile),
            profiles,
            performance_degraded: (!performance_degraded.is_empty())
                .then_some(performance_degraded),
        }
    }
}

impl Default for PowerProfileSnapshot {
    fn default() -> Self {
        Self::unavailable()
    }
}

/// Sort profiles using a stable, exact identifier order. Unknown profile IDs
/// remain valid and are not replaced with a fixed built-in profile list.
pub(crate) fn sort_power_profiles(profiles: &mut [PowerProfile]) {
    profiles.sort_by(|left, right| left.id.cmp(&right.id));
}

/// Render a battery summary without inventing a percentage for desktops or
/// devices that do not expose one.
pub(crate) fn battery_status_label(snapshot: &BatterySnapshot) -> String {
    if !snapshot.present {
        return "No battery detected".to_string();
    }

    match snapshot.percentage {
        Some(percentage) => format!("{percentage}% · {}", snapshot.state.label()),
        None => format!("Battery · {}", snapshot.state.label()),
    }
}

/// Render a profile summary using only the daemon-advertised active ID.
pub(crate) fn profile_status_label(snapshot: &PowerProfileSnapshot) -> String {
    if !snapshot.available {
        return "Power profiles unavailable".to_string();
    }

    match snapshot.active_profile.as_deref() {
        Some(profile) => format!("Active: {profile}"),
        None => "No active profile reported".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_all_documented_upower_states_and_unknown_values() {
        assert_eq!(BatteryState::from_upower(0), BatteryState::Unknown);
        assert_eq!(BatteryState::from_upower(1), BatteryState::Charging);
        assert_eq!(BatteryState::from_upower(2), BatteryState::Discharging);
        assert_eq!(BatteryState::from_upower(3), BatteryState::Empty);
        assert_eq!(BatteryState::from_upower(4), BatteryState::FullyCharged);
        assert_eq!(BatteryState::from_upower(5), BatteryState::PendingCharge);
        assert_eq!(BatteryState::from_upower(6), BatteryState::PendingDischarge);
        assert_eq!(BatteryState::from_upower(99), BatteryState::Unknown);
    }

    #[test]
    fn clamps_and_rounds_percentages() {
        assert_eq!(percentage_from_upower(-4.0), Some(0));
        assert_eq!(percentage_from_upower(12.4), Some(12));
        assert_eq!(percentage_from_upower(12.5), Some(13));
        assert_eq!(percentage_from_upower(100.6), Some(100));
        assert_eq!(percentage_from_upower(f64::NAN), None);
        assert_eq!(percentage_from_upower(f64::INFINITY), None);
    }

    #[test]
    fn ignores_non_positive_time_and_rounds_positive_minutes_up() {
        assert_eq!(duration_from_upower(-1), None);
        assert_eq!(duration_from_upower(0), None);
        assert_eq!(duration_from_upower(1), Some(Duration::from_secs(1)));
        assert_eq!(duration_from_upower(60), Some(Duration::from_secs(60)));
        assert_eq!(duration_from_upower(61), Some(Duration::from_secs(61)));
    }

    #[test]
    fn maps_battery_fields_and_empty_icon_to_optional_values() {
        let snapshot = battery_snapshot_from_upower(BatteryProperties {
            present: true,
            percentage: 87.6,
            state: 2,
            time_to_empty: 61,
            time_to_full: 0,
            energy_rate: -4.5,
            warning_level: 3,
            icon_name: String::new(),
        });

        assert!(snapshot.present);
        assert_eq!(snapshot.percentage, Some(88));
        assert_eq!(snapshot.state, BatteryState::Discharging);
        assert_eq!(snapshot.time_to_empty, Some(Duration::from_secs(61)));
        assert_eq!(snapshot.time_to_full, None);
        assert_eq!(snapshot.energy_rate, Some(-4.5));
        assert_eq!(snapshot.warning_level, 3);
        assert_eq!(snapshot.icon_name, None);
    }

    #[test]
    fn sorts_profiles_stably_by_exact_identifier() {
        let mut profiles = vec![
            PowerProfile { id: "zeta".to_string(), driver: None },
            PowerProfile { id: "alpha".to_string(), driver: Some("one".to_string()) },
            PowerProfile { id: "alpha".to_string(), driver: Some("two".to_string()) },
        ];

        sort_power_profiles(&mut profiles);

        assert_eq!(profiles[0].driver.as_deref(), Some("one"));
        assert_eq!(profiles[1].driver.as_deref(), Some("two"));
        assert_eq!(profiles[2].id, "zeta");
    }

    #[test]
    fn unavailable_snapshots_are_explicit() {
        assert_eq!(BatterySnapshot::unavailable(), BatterySnapshot::default());
        assert_eq!(PowerProfileSnapshot::unavailable(), PowerProfileSnapshot::default());
        assert!(!PowerProfileSnapshot::unavailable().available);
    }

    #[test]
    fn summaries_do_not_invent_desktop_battery_values_or_profiles() {
        let unavailable = BatterySnapshot::unavailable();
        assert_eq!(battery_status_label(&unavailable), "No battery detected");
        assert_eq!(
            profile_status_label(&PowerProfileSnapshot::unavailable()),
            "Power profiles unavailable"
        );

        let battery = battery_snapshot_from_upower(BatteryProperties {
            present: true,
            percentage: 42.0,
            state: 2,
            time_to_empty: 0,
            time_to_full: 0,
            energy_rate: 0.0,
            warning_level: 0,
            icon_name: String::new(),
        });
        assert_eq!(battery_status_label(&battery), "42% · Discharging");
        let profiles = PowerProfileSnapshot::available(
            "balanced".to_string(),
            vec![PowerProfile { id: "balanced".to_string(), driver: None }],
            String::new(),
        );
        assert_eq!(profile_status_label(&profiles), "Active: balanced");
    }
}
