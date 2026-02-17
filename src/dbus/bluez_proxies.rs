//! D-Bus proxy trait definitions for BlueZ interfaces.
//!
//! These traits are used by the zbus `#[proxy]` macro to generate
//! async proxy types for communicating with BlueZ (Bluetooth daemon)
//! over the **system** D-Bus bus.

use std::collections::HashMap;
use zbus::proxy;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

// ============================================================================
// D-Bus Proxy Traits for BlueZ
// ============================================================================

/// Proxy for org.bluez.Adapter1
///
/// Represents a Bluetooth adapter (e.g. hci0).
/// Controls power state and device discovery.
#[proxy(
    interface = "org.bluez.Adapter1",
    default_service = "org.bluez"
)]
pub(crate) trait Adapter1 {
    /// Start scanning for nearby Bluetooth devices.
    fn start_discovery(&self) -> zbus::Result<()>;

    /// Stop an ongoing discovery session.
    fn stop_discovery(&self) -> zbus::Result<()>;

    /// Remove a paired/discovered device from the adapter.
    fn remove_device(
        &self,
        device: &zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;

    /// Whether the adapter is powered on.
    #[zbus(property)]
    fn powered(&self) -> zbus::Result<bool>;

    /// Set the adapter power state.
    #[zbus(property)]
    fn set_powered(&self, powered: bool) -> zbus::Result<()>;

    /// Whether the adapter is currently discovering devices.
    #[zbus(property)]
    fn discovering(&self) -> zbus::Result<bool>;

    /// The Bluetooth address of this adapter.
    #[zbus(property)]
    fn address(&self) -> zbus::Result<String>;

    /// User-friendly name for this adapter.
    #[zbus(property)]
    fn alias(&self) -> zbus::Result<String>;
}

/// Proxy for org.bluez.Device1
///
/// Represents a remote Bluetooth device (discovered or paired).
#[proxy(
    interface = "org.bluez.Device1",
    default_service = "org.bluez"
)]
pub(crate) trait Device1 {
    /// Connect to all auto-connectable profiles on this device.
    fn connect(&self) -> zbus::Result<()>;

    /// Disconnect all profiles and the underlying connection.
    fn disconnect(&self) -> zbus::Result<()>;

    /// Initiate pairing with this device.
    fn pair(&self) -> zbus::Result<()>;

    /// Cancel an in-progress pairing attempt.
    fn cancel_pairing(&self) -> zbus::Result<()>;

    /// Bluetooth address (e.g. "AA:BB:CC:DD:EE:FF").
    #[zbus(property)]
    fn address(&self) -> zbus::Result<String>;

    /// Remote device name (may be absent for unknown devices).
    #[zbus(property)]
    fn name(&self) -> zbus::Result<String>;

    /// User-modifiable alias for this device.
    #[zbus(property)]
    fn alias(&self) -> zbus::Result<String>;

    /// Whether this device is paired.
    #[zbus(property)]
    fn paired(&self) -> zbus::Result<bool>;

    /// Whether this device is currently connected.
    #[zbus(property)]
    fn connected(&self) -> zbus::Result<bool>;

    /// Whether this device is trusted (auto-connect).
    #[zbus(property)]
    fn trusted(&self) -> zbus::Result<bool>;

    /// Set the trusted state.
    #[zbus(property)]
    fn set_trusted(&self, trusted: bool) -> zbus::Result<()>;

    /// Whether communication with this device is blocked.
    #[zbus(property)]
    fn blocked(&self) -> zbus::Result<bool>;

    /// Set the blocked state.
    #[zbus(property)]
    fn set_blocked(&self, blocked: bool) -> zbus::Result<()>;

    /// Icon name hint from BlueZ (e.g. "audio-headset", "input-keyboard").
    #[zbus(property)]
    fn icon(&self) -> zbus::Result<String>;

    /// Received Signal Strength Indicator (only valid during discovery).
    #[zbus(property, name = "RSSI")]
    fn rssi(&self) -> zbus::Result<i16>;

    /// The adapter this device belongs to.
    #[zbus(property)]
    fn adapter(&self) -> zbus::Result<OwnedObjectPath>;
}

/// Proxy for org.freedesktop.DBus.ObjectManager on the BlueZ service.
///
/// Used to enumerate all adapters and devices, and to receive
/// InterfacesAdded/InterfacesRemoved signals for live updates.
#[proxy(
    interface = "org.freedesktop.DBus.ObjectManager",
    default_service = "org.bluez",
    default_path = "/"
)]
pub(crate) trait BluezObjectManager {
    /// Get all managed objects with their interfaces and properties.
    ///
    /// Returns: `{ object_path: { interface_name: { property: value } } }`
    fn get_managed_objects(
        &self,
    ) -> zbus::Result<
        HashMap<
            OwnedObjectPath,
            HashMap<String, HashMap<String, OwnedValue>>,
        >,
    >;

    /// Signal: new interfaces appeared on an object.
    #[zbus(signal)]
    fn interfaces_added(
        &self,
        object_path: OwnedObjectPath,
        interfaces: HashMap<String, HashMap<String, OwnedValue>>,
    ) -> zbus::Result<()>;

    /// Signal: interfaces were removed from an object.
    #[zbus(signal)]
    fn interfaces_removed(
        &self,
        object_path: OwnedObjectPath,
        interfaces: Vec<String>,
    ) -> zbus::Result<()>;
}
