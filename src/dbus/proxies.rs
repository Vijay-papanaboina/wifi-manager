//! D-Bus proxy trait definitions for NetworkManager interfaces.
//!
//! These traits are used by the zbus `#[proxy]` macro to generate
//! async and blocking proxy types for communicating with NetworkManager
//! over D-Bus.

use std::collections::HashMap;
use zbus::proxy;
use zbus::zvariant::OwnedObjectPath;

// ============================================================================
// D-Bus Proxy Traits (zbus v5 generates async + blocking proxies from these)
// ============================================================================

/// Proxy for org.freedesktop.NetworkManager
#[proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
pub(crate) trait NetworkManager {
    /// Get all network devices
    fn get_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// Activate an existing saved connection
    fn activate_connection(
        &self,
        connection: &zbus::zvariant::ObjectPath<'_>,
        device: &zbus::zvariant::ObjectPath<'_>,
        specific_object: &zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<OwnedObjectPath>;

    /// Add a new connection and activate it (for connecting to new networks)
    fn add_and_activate_connection(
        &self,
        connection: HashMap<String, HashMap<String, zbus::zvariant::Value<'_>>>,
        device: &zbus::zvariant::ObjectPath<'_>,
        specific_object: &zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<(OwnedObjectPath, OwnedObjectPath)>;

    /// Deactivate an active connection
    fn deactivate_connection(
        &self,
        active_connection: &zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;

    /// Whether wireless is enabled
    #[zbus(property)]
    fn wireless_enabled(&self) -> zbus::Result<bool>;

    /// Set wireless enabled state
    #[zbus(property)]
    fn set_wireless_enabled(&self, enabled: bool) -> zbus::Result<()>;

    /// List of active connections
    #[zbus(property)]
    fn active_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

/// Proxy for org.freedesktop.NetworkManager.Device
#[proxy(
    interface = "org.freedesktop.NetworkManager.Device",
    default_service = "org.freedesktop.NetworkManager"
)]
pub(crate) trait Device {
    /// Device type (2 = WiFi)
    #[zbus(property)]
    fn device_type(&self) -> zbus::Result<u32>;

    /// Current active connection
    #[zbus(property)]
    fn active_connection(&self) -> zbus::Result<OwnedObjectPath>;

    /// Device state changed (new_state, old_state, reason)
    #[zbus(signal)]
    fn state_changed(&self, new_state: u32, old_state: u32, reason: u32) -> zbus::Result<()>;
}

/// Proxy for org.freedesktop.NetworkManager.Device.Wireless
#[proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager"
)]
pub(crate) trait Wireless {
    /// Request a WiFi scan
    fn request_scan(&self, options: HashMap<String, zbus::zvariant::Value<'_>>)
    -> zbus::Result<()>;

    /// List of access point object paths
    #[zbus(property)]
    fn access_points(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// Signal: a new access point appeared
    #[zbus(signal)]
    fn access_point_added(&self, access_point: OwnedObjectPath) -> zbus::Result<()>;

    /// Signal: an access point disappeared
    #[zbus(signal)]
    fn access_point_removed(&self, access_point: OwnedObjectPath) -> zbus::Result<()>;
}

/// Proxy for org.freedesktop.NetworkManager.AccessPoint
#[proxy(
    interface = "org.freedesktop.NetworkManager.AccessPoint",
    default_service = "org.freedesktop.NetworkManager"
)]
pub(crate) trait AccessPoint {
    #[zbus(property)]
    fn ssid(&self) -> zbus::Result<Vec<u8>>;

    #[zbus(property)]
    fn strength(&self) -> zbus::Result<u8>;

    #[zbus(property)]
    fn frequency(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn flags(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn wpa_flags(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn rsn_flags(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn hw_address(&self) -> zbus::Result<String>;
}

/// Proxy for org.freedesktop.NetworkManager.Connection.Active
#[proxy(
    interface = "org.freedesktop.NetworkManager.Connection.Active",
    default_service = "org.freedesktop.NetworkManager"
)]
pub(crate) trait ActiveConnection {
    #[zbus(property)]
    fn connection(&self) -> zbus::Result<OwnedObjectPath>;

    /// The specific AP or other resource this connection is using
    #[zbus(property)]
    fn specific_object(&self) -> zbus::Result<OwnedObjectPath>;

    /// Connection state: 1=activating, 2=activated, 3=deactivating, 4=deactivated
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;

    #[zbus(property, name = "Type")]
    fn connection_type(&self) -> zbus::Result<String>;
}

/// Proxy for org.freedesktop.NetworkManager.Settings
#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings"
)]
pub(crate) trait Settings {
    /// List all saved connection profiles
    fn list_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

/// Proxy for a single saved connection profile
#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings.Connection",
    default_service = "org.freedesktop.NetworkManager"
)]
pub(crate) trait SettingsConnection {
    /// Get connection settings dict
    fn get_settings(
        &self,
    ) -> zbus::Result<HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>>>;

    /// Delete this connection profile
    fn delete(&self) -> zbus::Result<()>;
}
