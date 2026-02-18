//! High-level WiFi manager that wraps NetworkManager D-Bus interactions.
//!
//! Uses proxy types from `proxies.rs` to communicate with NetworkManager.

use std::collections::HashMap;
use zbus::zvariant::OwnedObjectPath;

use super::access_point::{self, Band, Network, SecurityType};
use super::proxies::*;

/// The WiFi manager that wraps all NM D-Bus interactions.
#[derive(Clone)]
pub struct WifiManager {
    connection: zbus::Connection,
    wifi_device_path: OwnedObjectPath,
}

/// NM device type constant for WiFi
const NM_DEVICE_TYPE_WIFI: u32 = 2;

impl WifiManager {
    /// Connect to D-Bus and find the first WiFi device.
    pub async fn new() -> zbus::Result<Self> {
        let connection = zbus::Connection::system().await?;

        // Get the NM proxy
        let nm = NetworkManagerProxy::new(&connection).await?;

        // Find the WiFi device
        let devices = nm.get_devices().await?;
        let mut wifi_device_path: Option<OwnedObjectPath> = None;

        for device_path in devices {
            let device = DeviceProxy::builder(&connection)
                .path(device_path.clone())?
                .build()
                .await?;

            if device.device_type().await? == NM_DEVICE_TYPE_WIFI {
                wifi_device_path = Some(device_path);
                break;
            }
        }

        let wifi_device_path =
            wifi_device_path.ok_or_else(|| zbus::Error::Failure("No WiFi device found".into()))?;

        log::info!("Found WiFi device: {}", wifi_device_path);

        Ok(Self {
            connection,
            wifi_device_path,
        })
    }

    /// Trigger a WiFi scan.
    pub async fn request_scan(&self) -> zbus::Result<()> {
        let wireless = WirelessProxy::builder(&self.connection)
            .path(self.wifi_device_path.clone())?
            .build()
            .await?;

        wireless.request_scan(HashMap::new()).await?;
        log::info!("WiFi scan requested");
        Ok(())
    }

    /// Get a list of available networks (deduplicated by SSID).
    pub async fn get_networks(&self) -> zbus::Result<Vec<Network>> {
        let wireless = WirelessProxy::builder(&self.connection)
            .path(self.wifi_device_path.clone())?
            .build()
            .await?;

        let ap_paths = wireless.access_points().await?;
        let mut networks_by_ssid: HashMap<String, Network> = HashMap::new();

        // Get the currently active AP path (if any)
        let active_ap = self.get_active_ap_path().await.ok();

        // Get saved connection SSIDs
        let saved_ssids = self.get_saved_wifi_ssids().await.unwrap_or_default();

        for ap_path in ap_paths {
            let ap = AccessPointProxy::builder(&self.connection)
                .path(ap_path.clone())?
                .build()
                .await?;

            // Read AP properties
            let ssid_bytes = ap.ssid().await?;
            let ssid = String::from_utf8_lossy(&ssid_bytes).to_string();

            // Skip hidden networks (empty SSID)
            if ssid.is_empty() {
                continue;
            }

            let strength = ap.strength().await?;
            let frequency = ap.frequency().await?;
            let flags = ap.flags().await?;
            let wpa_flags = ap.wpa_flags().await?;
            let rsn_flags = ap.rsn_flags().await?;

            let security = access_point::security_from_flags(flags, wpa_flags, rsn_flags);
            let band = Band::from_frequency(frequency);
            let ap_path_str = ap_path.to_string();

            let is_connected = active_ap
                .as_ref()
                .map(|active| *active == ap_path_str)
                .unwrap_or(false);

            let is_saved = saved_ssids.contains_key(&ssid);
            let connection_path = saved_ssids.get(&ssid).cloned();

            // Deduplication: keep the AP with the strongest signal per SSID
            match networks_by_ssid.get(&ssid) {
                Some(existing) if existing.strength >= strength => {
                    // Existing entry is stronger, skip
                    continue;
                }
                _ => {
                    networks_by_ssid.insert(
                        ssid.clone(),
                        Network {
                            ssid,
                            strength,
                            security,
                            is_connected,
                            is_saved,
                            band,
                            ap_path: ap_path_str,
                            connection_path,
                        },
                    );
                }
            }
        }

        // Collect and sort: connected first → saved → by strength descending
        let mut networks: Vec<Network> = networks_by_ssid.into_values().collect();
        networks.sort_by(|a, b| {
            // Connected first
            b.is_connected
                .cmp(&a.is_connected)
                // Then saved
                .then(b.is_saved.cmp(&a.is_saved))
                // Then by signal strength
                .then(b.strength.cmp(&a.strength))
        });

        Ok(networks)
    }

    /// Connect to a network.
    ///
    /// - If the network has a saved connection profile, reactivate it.
    /// - If it's open, call AddAndActivateConnection with empty settings.
    /// - If it's secured (WPA2/WPA3), build settings with the provided password.
    ///
    /// Returns the active connection path on success.
    pub async fn connect_to_network(
        &self,
        network: &Network,
        password: Option<&str>,
    ) -> zbus::Result<String> {
        let nm = NetworkManagerProxy::new(&self.connection).await?;
        let device_path = zbus::zvariant::ObjectPath::try_from(self.wifi_device_path.as_str())
            .map_err(|e| zbus::Error::Failure(format!("Invalid device path: {e}")))?;
        let ap_path = zbus::zvariant::ObjectPath::try_from(network.ap_path.as_str())
            .map_err(|e| zbus::Error::Failure(format!("Invalid AP path: {e}")))?;

        // If there's a saved connection, reactivate it
        if let Some(ref conn_path_str) = network.connection_path {
            let conn_path = zbus::zvariant::ObjectPath::try_from(conn_path_str.as_str())
                .map_err(|e| zbus::Error::Failure(format!("Invalid connection path: {e}")))?;

            log::info!("Activating saved connection for '{}'", network.ssid);
            let active = nm
                .activate_connection(&conn_path, &device_path, &ap_path)
                .await?;
            return Ok(active.to_string());
        }

        // Build new connection settings based on security type
        let settings = match network.security {
            SecurityType::Open => {
                log::info!("Connecting to open network '{}'", network.ssid);
                super::connection::build_open_settings()
            }
            SecurityType::WPA2 => {
                let psk = password.ok_or_else(|| {
                    zbus::Error::Failure("Password required for WPA2 network".into())
                })?;
                log::info!("Connecting to WPA2 network '{}'", network.ssid);
                super::connection::build_wpa_psk_settings(&network.ssid, psk)
            }
            SecurityType::WPA3 => {
                let psk = password.ok_or_else(|| {
                    zbus::Error::Failure("Password required for WPA3 network".into())
                })?;
                log::info!("Connecting to WPA3 network '{}'", network.ssid);
                super::connection::build_wpa3_settings(&network.ssid, psk)
            }
            SecurityType::Enterprise => {
                return Err(zbus::Error::Failure(
                    "Enterprise (802.1X) networks are not yet supported".into(),
                ));
            }
        };

        let (_, active) = nm
            .add_and_activate_connection(settings, &device_path, &ap_path)
            .await?;
        Ok(active.to_string())
    }

    /// Disconnect from the current WiFi network.
    pub async fn disconnect(&self) -> zbus::Result<()> {
        let device = DeviceProxy::builder(&self.connection)
            .path(self.wifi_device_path.clone())?
            .build()
            .await?;

        let active_conn_path = device.active_connection().await?;
        if active_conn_path.as_str() == "/" {
            return Err(zbus::Error::Failure("Not connected to any network".into()));
        }

        let nm = NetworkManagerProxy::new(&self.connection).await?;
        let path = zbus::zvariant::ObjectPath::try_from(active_conn_path.as_str())
            .map_err(|e| zbus::Error::Failure(format!("Invalid active connection path: {e}")))?;
        nm.deactivate_connection(&path).await?;

        log::info!("Disconnected from WiFi");
        Ok(())
    }

    /// Enable or disable WiFi radio.
    pub async fn set_wifi_enabled(&self, enabled: bool) -> zbus::Result<()> {
        let nm = NetworkManagerProxy::new(&self.connection).await?;
        nm.set_wireless_enabled(enabled).await?;
        log::info!("WiFi {}", if enabled { "enabled" } else { "disabled" });
        Ok(())
    }

    /// Check if WiFi radio is currently enabled.
    pub async fn is_wifi_enabled(&self) -> zbus::Result<bool> {
        let nm = NetworkManagerProxy::new(&self.connection).await?;
        nm.wireless_enabled().await
    }
    /// Forget (delete) a saved network by its SSID.
    pub async fn forget_network(&self, ssid: &str) -> zbus::Result<()> {
        let saved = self.get_saved_wifi_ssids().await?;
        if let Some(conn_path) = saved.get(ssid) {
            let conn = SettingsConnectionProxy::builder(&self.connection)
                .path(conn_path.as_str())?
                .build()
                .await?;
            conn.delete().await?;
            log::info!("Forgot network: {ssid}");
            Ok(())
        } else {
            log::warn!("Network not found in saved connections: {ssid}");
            Err(zbus::Error::Failure(format!(
                "No saved connection for '{ssid}'"
            )))
        }
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    /// Get the D-Bus path of the AP the device is currently connected to.
    async fn get_active_ap_path(&self) -> zbus::Result<String> {
        let device = DeviceProxy::builder(&self.connection)
            .path(self.wifi_device_path.clone())?
            .build()
            .await?;

        let active_conn_path = device.active_connection().await?;
        if active_conn_path.as_str() == "/" {
            return Err(zbus::Error::Failure("No active connection".into()));
        }

        let active_conn = ActiveConnectionProxy::builder(&self.connection)
            .path(active_conn_path)?
            .build()
            .await?;

        let specific_object = active_conn.specific_object().await?;
        Ok(specific_object.to_string())
    }

    /// Get a map of SSID → saved connection D-Bus path for WiFi connections.
    async fn get_saved_wifi_ssids(&self) -> zbus::Result<HashMap<String, String>> {
        let settings = SettingsProxy::new(&self.connection).await?;
        let connections = settings.list_connections().await?;

        let mut ssid_map: HashMap<String, String> = HashMap::new();

        for conn_path in connections {
            let conn = SettingsConnectionProxy::builder(&self.connection)
                .path(conn_path.clone())?
                .build()
                .await?;

            if let Ok(settings) = conn.get_settings().await {
                // Check if this is a WiFi connection
                if let Some(conn_settings) = settings.get("connection") {
                    let conn_type = conn_settings
                        .get("type")
                        .and_then(|v| <String>::try_from(v.clone()).ok());

                    if conn_type.as_deref() != Some("802-11-wireless") {
                        continue;
                    }
                }

                // Get the SSID from 802-11-wireless settings
                if let Some(wifi_settings) = settings.get("802-11-wireless")
                    && let Some(ssid_val) = wifi_settings.get("ssid")
                        && let Ok(ssid_bytes) = <Vec<u8>>::try_from(ssid_val.clone()) {
                            let ssid = String::from_utf8_lossy(&ssid_bytes).to_string();
                            if !ssid.is_empty() {
                                ssid_map.insert(ssid, conn_path.to_string());
                            }
                        }
            }
        }

        Ok(ssid_map)
    }

    /// Get the frequency (in MHz) of the currently active AP, if connected.
    pub async fn get_active_frequency(&self) -> Option<u32> {
        let path = self.get_active_ap_path().await.ok()?;
        let ap = AccessPointProxy::builder(&self.connection)
            .path(path).ok()?
            .build().await.ok()?;
        ap.frequency().await.ok()
    }

    /// Get a reference to the D-Bus connection (for use in other modules).
    pub fn connection(&self) -> &zbus::Connection {
        &self.connection
    }

    /// Get the WiFi device path as a &str.
    pub fn wifi_device_path(&self) -> &str {
        self.wifi_device_path.as_str()
    }

    /// Get the WiFi device path as an OwnedObjectPath.
    pub fn device_path(&self) -> zbus::zvariant::OwnedObjectPath {
        self.wifi_device_path.clone()
    }
}
