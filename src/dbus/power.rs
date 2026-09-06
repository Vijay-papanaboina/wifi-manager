//! UPower and power-profiles-daemon D-Bus adapters.

use std::collections::HashMap;

use futures_util::StreamExt;
use zbus::Connection;
use zbus::fdo::PropertiesProxy;
use zbus::proxy;
use zbus::zvariant::OwnedValue;

use crate::domain::power::{
    BatteryProperties, BatterySnapshot, PowerProfile, PowerProfileSnapshot,
    battery_snapshot_from_upower,
};

const UPOWER_SERVICE: &str = "org.freedesktop.UPower";
const UPOWER_DISPLAY_DEVICE_PATH: &str = "/org/freedesktop/UPower/devices/DisplayDevice";

const CURRENT_POWER_PROFILES_SERVICE: &str = "org.freedesktop.UPower.PowerProfiles";
const CURRENT_POWER_PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const LEGACY_POWER_PROFILES_SERVICE: &str = "net.hadess.PowerProfiles";
const LEGACY_POWER_PROFILES_PATH: &str = "/net/hadess/PowerProfiles";

/// Proxy for the UPower daemon object.
#[proxy(
    interface = "org.freedesktop.UPower",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower"
)]
pub(crate) trait UPower {
    /// Whether the system is currently running on battery power.
    #[zbus(property)]
    fn on_battery(&self) -> zbus::Result<bool>;
}

/// Proxy for UPower's guaranteed composite display device.
#[proxy(
    interface = "org.freedesktop.UPower.Device",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower/devices/DisplayDevice"
)]
pub(crate) trait UPowerDevice {
    #[zbus(property)]
    fn is_present(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn percentage(&self) -> zbus::Result<f64>;

    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn time_to_empty(&self) -> zbus::Result<i64>;

    #[zbus(property)]
    fn time_to_full(&self) -> zbus::Result<i64>;

    #[zbus(property)]
    fn energy_rate(&self) -> zbus::Result<f64>;

    #[zbus(property)]
    fn warning_level(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn icon_name(&self) -> zbus::Result<String>;
}

/// Current upstream power-profiles-daemon proxy.
#[proxy(
    interface = "org.freedesktop.UPower.PowerProfiles",
    default_service = "org.freedesktop.UPower.PowerProfiles",
    default_path = "/org/freedesktop/UPower/PowerProfiles"
)]
pub(crate) trait CurrentPowerProfiles {
    /// Each dictionary is an `a{sv}` profile description. Unknown keys are
    /// intentionally retained by the wire type and ignored by the mapper.
    #[zbus(property)]
    fn profiles(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;

    #[zbus(property)]
    fn active_profile(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn set_active_profile(&self, profile: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn performance_degraded(&self) -> zbus::Result<String>;
}

/// Legacy power-profiles-daemon proxy retained for older installations.
#[proxy(
    interface = "net.hadess.PowerProfiles",
    default_service = "net.hadess.PowerProfiles",
    default_path = "/net/hadess/PowerProfiles"
)]
pub(crate) trait LegacyPowerProfiles {
    #[zbus(property)]
    fn profiles(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;

    #[zbus(property)]
    fn active_profile(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn set_active_profile(&self, profile: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn performance_degraded(&self) -> zbus::Result<String>;
}

/// Read the UPower display-device properties and expose a stable snapshot.
#[derive(Clone)]
pub(crate) struct BatteryManager {
    connection: Connection,
}

impl BatteryManager {
    /// Connect to the system bus. UPower itself is optional; its absence is
    /// represented by `BatterySnapshot::unavailable` from `refresh`.
    pub(crate) async fn connect() -> zbus::Result<Self> {
        Ok(Self { connection: Connection::system().await? })
    }

    /// Read the current battery snapshot.
    ///
    /// A missing UPower name or display object is an unavailable battery, not
    /// a backend failure. Other D-Bus errors are returned to the caller.
    pub(crate) async fn refresh(&self) -> zbus::Result<BatterySnapshot> {
        match self.read_snapshot().await {
            Ok(snapshot) => Ok(snapshot),
            Err(error) if is_unavailable_error(&error) => Ok(BatterySnapshot::unavailable()),
            Err(error) => Err(error),
        }
    }

    async fn read_snapshot(&self) -> zbus::Result<BatterySnapshot> {
        // Reading a documented root property both verifies the UPower name
        // and keeps the root proxy part of this backend's contract.
        let root = UPowerProxy::new(&self.connection).await?;
        let _ = root.on_battery().await?;

        let device = UPowerDeviceProxy::new(&self.connection).await?;
        let present = device.is_present().await?;
        let percentage = device.percentage().await?;
        let state = device.state().await?;
        let time_to_empty = device.time_to_empty().await?;
        let time_to_full = device.time_to_full().await?;
        let energy_rate = device.energy_rate().await?;
        let warning_level = device.warning_level().await?;
        let icon_name = device.icon_name().await?;

        Ok(battery_snapshot_from_upower(BatteryProperties {
            present,
            percentage,
            state,
            time_to_empty,
            time_to_full,
            energy_rate,
            warning_level,
            icon_name,
        }))
    }

    /// Watch UPower property changes and call `callback` with a fresh
    /// snapshot. The initial snapshot is delivered once so an integration
    /// does not need a separate race-prone initialization step.
    pub(crate) async fn watch_changes<F>(&self, mut callback: F) -> zbus::Result<()>
    where
        F: FnMut(BatterySnapshot),
    {
        let properties = PropertiesProxy::builder(&self.connection)
            .destination(UPOWER_SERVICE)?
            .path(UPOWER_DISPLAY_DEVICE_PATH)?
            .build()
            .await?;

        callback(self.refresh().await?);

        let mut stream = match properties.receive_properties_changed().await {
            Ok(stream) => stream,
            Err(error) if is_unavailable_error(&error) => return Ok(()),
            Err(error) => return Err(error),
        };

        while stream.next().await.is_some() {
            match self.refresh().await {
                Ok(snapshot) => callback(snapshot),
                Err(error) if is_unavailable_error(&error) => {
                    callback(BatterySnapshot::unavailable());
                }
                Err(error) => log::warn!("Failed to refresh battery after a D-Bus change: {error}"),
            }
        }

        Ok(())
    }
}

/// Read and change profiles exposed by power-profiles-daemon.
#[derive(Clone, Copy)]
enum PowerProfilesEndpoint {
    Current,
    Legacy,
    Unavailable,
}

impl PowerProfilesEndpoint {
    fn service(self) -> Option<&'static str> {
        match self {
            Self::Current => Some(CURRENT_POWER_PROFILES_SERVICE),
            Self::Legacy => Some(LEGACY_POWER_PROFILES_SERVICE),
            Self::Unavailable => None,
        }
    }

    fn path(self) -> Option<&'static str> {
        match self {
            Self::Current => Some(CURRENT_POWER_PROFILES_PATH),
            Self::Legacy => Some(LEGACY_POWER_PROFILES_PATH),
            Self::Unavailable => None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct PowerProfileManager {
    connection: Connection,
    endpoint: PowerProfilesEndpoint,
}

impl PowerProfileManager {
    /// Connect to the system bus. power-profiles-daemon is optional; its
    /// absence is represented by `PowerProfileSnapshot::unavailable` from
    /// `refresh`.
    pub(crate) async fn connect() -> zbus::Result<Self> {
        let connection = Connection::system().await?;
        let endpoint = match select_power_profiles_endpoint(&connection).await {
            Ok(endpoint) => endpoint,
            Err(error) if is_unavailable_error(&error) => PowerProfilesEndpoint::Unavailable,
            Err(error) => return Err(error),
        };
        Ok(Self { connection, endpoint })
    }

    /// Read the current available, active, and inhibition state.
    pub(crate) async fn refresh(&self) -> zbus::Result<PowerProfileSnapshot> {
        match self.read_snapshot().await {
            Ok(snapshot) => Ok(snapshot),
            Err(error) if is_unavailable_error(&error) => Ok(PowerProfileSnapshot::unavailable()),
            Err(error) => Err(error),
        }
    }

    async fn read_snapshot(&self) -> zbus::Result<PowerProfileSnapshot> {
        match self.endpoint {
            PowerProfilesEndpoint::Unavailable => Ok(PowerProfileSnapshot::unavailable()),
            PowerProfilesEndpoint::Current => {
                let proxy = CurrentPowerProfilesProxy::new(&self.connection).await?;
                let profiles = decode_profiles(&proxy.profiles().await?);
                let active_profile = proxy.active_profile().await?;
                let performance_degraded = proxy.performance_degraded().await.unwrap_or_default();

                Ok(PowerProfileSnapshot::available(active_profile, profiles, performance_degraded))
            }
            PowerProfilesEndpoint::Legacy => {
                let proxy = LegacyPowerProfilesProxy::new(&self.connection).await?;
                let profiles = decode_profiles(&proxy.profiles().await?);
                let active_profile = proxy.active_profile().await?;
                let performance_degraded = proxy.performance_degraded().await.unwrap_or_default();

                Ok(PowerProfileSnapshot::available(active_profile, profiles, performance_degraded))
            }
        }
    }

    /// Watch profile property changes and call `callback` with a fresh
    /// snapshot. The callback receives an explicit unavailable snapshot if
    /// the optional daemon disappears after startup.
    pub(crate) async fn watch_changes<F>(&self, mut callback: F) -> zbus::Result<()>
    where
        F: FnMut(PowerProfileSnapshot),
    {
        let (Some(service), Some(path)) = (self.endpoint.service(), self.endpoint.path()) else {
            callback(PowerProfileSnapshot::unavailable());
            return Ok(());
        };

        let properties = PropertiesProxy::builder(&self.connection)
            .destination(service)?
            .path(path)?
            .build()
            .await?;

        callback(self.refresh().await?);

        let mut stream = match properties.receive_properties_changed().await {
            Ok(stream) => stream,
            Err(error) if is_unavailable_error(&error) => return Ok(()),
            Err(error) => return Err(error),
        };

        while stream.next().await.is_some() {
            match self.refresh().await {
                Ok(snapshot) => callback(snapshot),
                Err(error) if is_unavailable_error(&error) => {
                    callback(PowerProfileSnapshot::unavailable());
                }
                Err(error) => {
                    log::warn!("Failed to refresh power profiles after a D-Bus change: {error}");
                }
            }
        }

        Ok(())
    }

    /// Request a profile change through D-Bus. Errors from the daemon,
    /// including authorization or an invalid profile ID, are deliberately
    /// propagated to the user-action caller.
    pub(crate) async fn set_active_profile(&self, profile: &str) -> zbus::Result<()> {
        match self.endpoint {
            PowerProfilesEndpoint::Unavailable => {
                Err(zbus::Error::Failure("power-profiles-daemon is unavailable".to_string()))
            }
            PowerProfilesEndpoint::Current => {
                let proxy = CurrentPowerProfilesProxy::new(&self.connection).await?;
                proxy.set_active_profile(profile).await
            }
            PowerProfilesEndpoint::Legacy => {
                let proxy = LegacyPowerProfilesProxy::new(&self.connection).await?;
                proxy.set_active_profile(profile).await
            }
        }
    }
}

/// Prefer the current UPower-owned endpoint, falling back to the legacy
/// endpoint only when its ActiveProfile property cannot be read.
async fn select_power_profiles_endpoint(
    connection: &Connection,
) -> zbus::Result<PowerProfilesEndpoint> {
    let current_error = match CurrentPowerProfilesProxy::new(connection).await {
        Ok(proxy) => match proxy.active_profile().await {
            Ok(_) => return Ok(PowerProfilesEndpoint::Current),
            Err(error) => error,
        },
        Err(error) => error,
    };
    log::debug!("Current power-profile endpoint unavailable: {current_error}");

    let legacy_result = match LegacyPowerProfilesProxy::new(connection).await {
        Ok(proxy) => proxy.active_profile().await,
        Err(error) => Err(error),
    };
    match legacy_result {
        Ok(_) => Ok(PowerProfilesEndpoint::Legacy),
        Err(legacy_error) => {
            if is_unavailable_error(&current_error) && is_unavailable_error(&legacy_error) {
                Ok(PowerProfilesEndpoint::Unavailable)
            } else if !is_unavailable_error(&current_error) {
                Err(current_error)
            } else {
                Err(legacy_error)
            }
        }
    }
}

/// Decode only the documented `Profile` and optional `Driver` keys. New
/// profile kinds and additional dictionary keys remain forward-compatible.
pub(crate) fn decode_profiles(dictionaries: &[HashMap<String, OwnedValue>]) -> Vec<PowerProfile> {
    let mut profiles = dictionaries
        .iter()
        .filter_map(|dictionary| {
            let id = dictionary
                .get("Profile")
                .and_then(|value| String::try_from(value.clone()).ok())
                .filter(|id| !id.is_empty())?;
            let driver = dictionary
                .get("Driver")
                .and_then(|value| String::try_from(value.clone()).ok())
                .filter(|driver| !driver.is_empty());

            Some(PowerProfile { id, driver })
        })
        .collect::<Vec<_>>();

    crate::domain::power::sort_power_profiles(&mut profiles);
    profiles
}

/// Identify optional-service/object failures while leaving transport,
/// permission, and malformed-property errors visible to callers.
fn is_unavailable_error(error: &zbus::Error) -> bool {
    match error {
        zbus::Error::FDO(error) => matches!(
            error.as_ref(),
            zbus::fdo::Error::ServiceUnknown(_)
                | zbus::fdo::Error::NameHasNoOwner(_)
                | zbus::fdo::Error::UnknownObject(_)
                | zbus::fdo::Error::UnknownInterface(_)
        ),
        zbus::Error::MethodError(name, ..) => matches!(
            name.as_str(),
            "org.freedesktop.DBus.Error.ServiceUnknown"
                | "org.freedesktop.DBus.Error.NameHasNoOwner"
                | "org.freedesktop.DBus.Error.UnknownObject"
                | "org.freedesktop.DBus.Error.UnknownInterface"
        ),
        _ => false,
    }
}
