//! High-level Bluetooth manager that wraps BlueZ D-Bus interactions.
//!
//! Uses proxy types from `bluez_proxies.rs` to communicate with BlueZ.
//! Mirrors the structure of `network_manager.rs` for WiFi.

use zbus::zvariant::OwnedObjectPath;

use super::bluetooth_device::{BluetoothDevice, DeviceCategory};
use super::bluez_proxies::*;

/// The Bluetooth manager that wraps all BlueZ D-Bus interactions.
#[derive(Clone)]
pub struct BluetoothManager {
    connection: zbus::Connection,
    adapter_path: OwnedObjectPath,
}

#[allow(dead_code)]
impl BluetoothManager {
    /// Connect to D-Bus (system bus) and find the first Bluetooth adapter.
    ///
    /// Returns `None` if no Bluetooth adapter is available (not an error —
    /// the app should simply hide the Bluetooth tab).
    pub async fn new() -> Option<Self> {
        let connection = match zbus::Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to connect to system bus for BlueZ: {e}");
                return None;
            }
        };

        // Use ObjectManager to find the first adapter
        let obj_manager = match BluezObjectManagerProxy::new(&connection).await {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Failed to create BlueZ ObjectManager proxy: {e}");
                return None;
            }
        };

        let objects = match obj_manager.get_managed_objects().await {
            Ok(o) => o,
            Err(e) => {
                log::warn!("BlueZ not available (bluetoothd not running?): {e}");
                return None;
            }
        };

        // Find the first object that implements org.bluez.Adapter1
        let adapter_path = objects
            .iter()
            .find(|(_, ifaces)| ifaces.contains_key("org.bluez.Adapter1"))
            .map(|(path, _)| path.clone());

        let adapter_path = match adapter_path {
            Some(p) => p,
            None => {
                log::warn!("No Bluetooth adapter found");
                return None;
            }
        };

        log::info!("Found Bluetooth adapter: {}", adapter_path);

        Some(Self {
            connection,
            adapter_path,
        })
    }

    // ========================================================================
    // Discovery
    // ========================================================================

    /// Start scanning for nearby Bluetooth devices.
    pub async fn start_discovery(&self) -> zbus::Result<()> {
        let adapter = self.adapter_proxy().await?;
        // Ignore "already discovering" errors
        match adapter.start_discovery().await {
            Ok(()) => {
                log::info!("Bluetooth discovery started");
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("InProgress") || msg.contains("Already") {
                    log::debug!("Discovery already in progress");
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Stop an ongoing discovery session.
    pub async fn stop_discovery(&self) -> zbus::Result<()> {
        let adapter = self.adapter_proxy().await?;
        // Ignore "not discovering" errors
        match adapter.stop_discovery().await {
            Ok(()) => {
                log::info!("Bluetooth discovery stopped");
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("NotReady") || msg.contains("NotAuthorized") {
                    log::debug!("Discovery was not active");
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    // ========================================================================
    // Device enumeration
    // ========================================================================

    /// Get a list of all known Bluetooth devices (paired + discovered).
    ///
    /// Devices are sorted: connected first, then paired, then by name.
    pub async fn get_devices(&self) -> zbus::Result<Vec<BluetoothDevice>> {
        let obj_manager = BluezObjectManagerProxy::new(&self.connection).await?;
        let objects = obj_manager.get_managed_objects().await?;

        let adapter_prefix = format!("{}/", self.adapter_path);
        let mut devices = Vec::new();

        for (path, ifaces) in &objects {
            // Only look at Device1 objects under our adapter
            let path_str = path.as_str();
            if !path_str.starts_with(&adapter_prefix) {
                continue;
            }
            let Some(props) = ifaces.get("org.bluez.Device1") else {
                continue;
            };

            let device = self.parse_device_properties(path_str, props);
            devices.push(device);
        }

        devices.sort_by_cached_key(|a| a.sort_key());
        log::info!("Bluetooth device list: {} devices", devices.len());
        Ok(devices)
    }

    // ========================================================================
    // Device actions
    // ========================================================================

    /// Connect to a Bluetooth device (must be paired or "Just Works").
    pub async fn connect_device(&self, device_path: &str) -> zbus::Result<()> {
        let device = self.device_proxy(device_path).await?;
        log::info!("Connecting to Bluetooth device: {device_path}");
        device.connect().await
    }

    /// Disconnect a connected Bluetooth device.
    pub async fn disconnect_device(&self, device_path: &str) -> zbus::Result<()> {
        let device = self.device_proxy(device_path).await?;
        log::info!("Disconnecting Bluetooth device: {device_path}");
        device.disconnect().await
    }

    /// Pair with a Bluetooth device.
    ///
    /// For v1, only "Just Works" pairing is supported (no PIN agent).
    /// Devices requiring PIN entry should be paired via `bluetoothctl`.
    pub async fn pair_device(&self, device_path: &str) -> zbus::Result<()> {
        let device = self.device_proxy(device_path).await?;
        log::info!("Pairing with Bluetooth device: {device_path}");
        device.pair().await
    }

    /// Set the trusted state of a device (auto-connect on boot).
    pub async fn trust_device(&self, device_path: &str, trusted: bool) -> zbus::Result<()> {
        let device = self.device_proxy(device_path).await?;
        device.set_trusted(trusted).await?;
        log::info!(
            "Bluetooth device {} {}",
            device_path,
            if trusted { "trusted" } else { "untrusted" }
        );
        Ok(())
    }

    /// Remove (forget/unpair) a device from the adapter.
    pub async fn remove_device(&self, device_path: &str) -> zbus::Result<()> {
        let adapter = self.adapter_proxy().await?;
        let path = zbus::zvariant::ObjectPath::try_from(device_path)
            .map_err(|e| zbus::Error::Failure(format!("Invalid device path: {e}")))?;
        adapter.remove_device(&path).await?;
        log::info!("Removed Bluetooth device: {device_path}");
        Ok(())
    }

    // ========================================================================
    // Adapter power
    // ========================================================================

    /// Check if the Bluetooth adapter is powered on.
    pub async fn is_powered(&self) -> zbus::Result<bool> {
        let adapter = self.adapter_proxy().await?;
        adapter.powered().await
    }

    /// Enable or disable the Bluetooth adapter.
    pub async fn set_powered(&self, powered: bool) -> zbus::Result<()> {
        let adapter = self.adapter_proxy().await?;
        adapter.set_powered(powered).await?;
        log::info!(
            "Bluetooth adapter {}",
            if powered { "powered on" } else { "powered off" }
        );
        Ok(())
    }

    // ========================================================================
    // Accessors (for live_updates and other modules)
    // ========================================================================

    /// Get a reference to the D-Bus connection.
    pub fn connection(&self) -> &zbus::Connection {
        &self.connection
    }

    /// Get the adapter object path.
    pub fn adapter_path(&self) -> &str {
        self.adapter_path.as_str()
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    /// Create an Adapter1 proxy for our adapter.
    async fn adapter_proxy(&self) -> zbus::Result<Adapter1Proxy<'_>> {
        Adapter1Proxy::builder(&self.connection)
            .path(self.adapter_path.clone())?
            .build()
            .await
    }

    /// Create a Device1 proxy for a specific device path.
    async fn device_proxy<'a>(&self, path: &'a str) -> zbus::Result<Device1Proxy<'a>> {
        Device1Proxy::builder(&self.connection)
            .path(path)?
            .build()
            .await
    }

    /// Parse a Device1's properties from ObjectManager into a BluetoothDevice.
    fn parse_device_properties(
        &self,
        path: &str,
        props: &std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> BluetoothDevice {
        let address = props
            .get("Address")
            .and_then(|v| <String>::try_from(v.clone()).ok())
            .unwrap_or_default();

        let name = props
            .get("Name")
            .and_then(|v| <String>::try_from(v.clone()).ok())
            .unwrap_or_default();

        let alias = props
            .get("Alias")
            .and_then(|v| <String>::try_from(v.clone()).ok())
            .unwrap_or_default();

        let icon_hint = props
            .get("Icon")
            .and_then(|v| <String>::try_from(v.clone()).ok())
            .unwrap_or_default();

        let paired = props
            .get("Paired")
            .and_then(|v| <bool>::try_from(v.clone()).ok())
            .unwrap_or(false);

        let connected = props
            .get("Connected")
            .and_then(|v| <bool>::try_from(v.clone()).ok())
            .unwrap_or(false);

        let trusted = props
            .get("Trusted")
            .and_then(|v| <bool>::try_from(v.clone()).ok())
            .unwrap_or(false);

        let rssi = props
            .get("RSSI")
            .and_then(|v| <i16>::try_from(v.clone()).ok())
            .unwrap_or(0);

        // Display name: prefer alias, then name, then address
        let display_name = if !alias.is_empty() {
            alias
        } else if !name.is_empty() {
            name
        } else {
            address.clone()
        };

        let category = DeviceCategory::from_icon_hint(&icon_hint);

        BluetoothDevice {
            address,
            display_name,
            category,
            paired,
            connected,
            trusted,
            rssi,
            device_path: path.to_string(),
        }
    }
}
