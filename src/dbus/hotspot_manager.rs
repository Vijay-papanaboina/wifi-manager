//! WiFi hotspot manager — start/stop/query AP mode via NetworkManager D-Bus.
//!
//! Uses the official NM pattern: create a persistent connection profile with
//! a fixed UUID, then activate/deactivate it.  NM handles NAT, DHCP, and DNS.

use std::collections::HashMap;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

use super::proxies::*;

/// Fixed UUID for our hotspot connection profile.
/// This lets us find and reuse it across app restarts.
const HOTSPOT_UUID: &str = "5b481f9c-7d77-4d95-8e1c-e08821ffeba9";

/// Manages the WiFi hotspot via NetworkManager D-Bus.
#[derive(Clone)]
pub struct HotspotManager {
    connection: zbus::Connection,
    wifi_device_path: OwnedObjectPath,
}

impl HotspotManager {
    /// Create a new HotspotManager using an existing D-Bus connection
    /// and the WiFi device path (from WifiManager).
    pub fn new(connection: zbus::Connection, wifi_device_path: OwnedObjectPath) -> Self {
        Self {
            connection,
            wifi_device_path,
        }
    }

    /// Start a WiFi hotspot with the given SSID and password.
    ///
    /// Creates the connection profile on first use, reuses it afterwards.
    /// NetworkManager auto-selects the best band and channel.
    pub async fn start_hotspot(
        &self,
        ssid: &str,
        password: Option<&str>,
    ) -> zbus::Result<OwnedObjectPath> {
        log::warn!("DEBUG: start_hotspot called for SSID: {}", ssid);
        // Always update profile to match current config
        let conn_path = self.ensure_profile(ssid, password).await?;

        // Activate it on our WiFi device
        let nm = NetworkManagerProxy::new(&self.connection).await?;
        let conn = ObjectPath::try_from(conn_path.as_str())
            .map_err(|e| zbus::Error::Failure(format!("Invalid path: {e}")))?;
        let device = ObjectPath::try_from(self.wifi_device_path.as_str())
            .map_err(|e| zbus::Error::Failure(format!("Invalid path: {e}")))?;
        let none = ObjectPath::try_from("/")
            .map_err(|e| zbus::Error::Failure(format!("Invalid path: {e}")))?;

        let active = nm.activate_connection(&conn, &device, &none).await?;
        log::info!("Hotspot started: SSID={ssid}");
        Ok(active)
    }

    /// Stop the active hotspot by disconnecting the WiFi device's AP interface.
    pub async fn stop_hotspot(&self) -> zbus::Result<()> {
        let device = DeviceProxy::builder(&self.connection)
            .path(self.wifi_device_path.clone())?
            .build()
            .await?;
        device.disconnect().await?;
        log::info!("Hotspot stopped");
        Ok(())
    }

    /// Check if our hotspot is currently active.
    pub async fn is_hotspot_active(&self) -> bool {
        let Ok(nm) = NetworkManagerProxy::new(&self.connection).await else {
            return false;
        };
        let Ok(actives) = nm.active_connections().await else {
            return false;
        };

        for active_path in actives {
            if let Ok(active) = ActiveConnectionProxy::builder(&self.connection)
                .path(active_path.clone())
                .and_then(|b| Ok(b.build()))
            {
                if let Ok(active) = active.await {
                    if let Ok(conn_path) = active.connection().await {
                        if self.is_our_hotspot(&conn_path.to_string()).await {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    // ========================================================================
    // Private helpers
    // ========================================================================

    /// Ensure our hotspot profile exists and is up-to-date.
    async fn ensure_profile(
        &self,
        ssid: &str,
        password: Option<&str>,
    ) -> zbus::Result<String> {
        let settings_dict = build_hotspot_settings(ssid, password);
        let settings = SettingsProxy::new(&self.connection).await?;

        // If it exists, delete it first to ensure no leftover WPA settings
        if let Some(path) = self.find_hotspot_profile().await {
            log::info!("Deleting existing hotspot profile for a clean start: {}", path);
            if let Ok(conn) = SettingsConnectionProxy::builder(&self.connection)
                .path(path.clone())?
                .build()
                .await
            {
                let _ = conn.delete().await;
            }
        }

        let path = settings.add_connection(settings_dict).await?;
        let mode_str = if password.is_some() && !password.unwrap().is_empty() { "SECURED" } else { "OPEN" };
        log::info!("Hotspot profile created ({}): {}", mode_str, path);
        Ok(path.to_string())
    }

    /// Find our existing hotspot profile by UUID.
    async fn find_hotspot_profile(&self) -> Option<String> {
        let settings = SettingsProxy::new(&self.connection).await.ok()?;
        let connections = settings.list_connections().await.ok()?;

        for conn_path in connections {
            if self.is_our_hotspot(&conn_path.to_string()).await {
                return Some(conn_path.to_string());
            }
        }
        None
    }

    /// Check if a connection profile is our hotspot (by UUID).
    async fn is_our_hotspot(&self, conn_path: &str) -> bool {
        let Ok(conn) = SettingsConnectionProxy::builder(&self.connection)
            .path(conn_path)
            .and_then(|b| Ok(b.build()))
        else {
            return false;
        };
        let Ok(conn) = conn.await else { return false };
        let Ok(settings) = conn.get_settings().await else {
            return false;
        };

        settings
            .get("connection")
            .and_then(|c| c.get("uuid"))
            .and_then(|v| <String>::try_from(v.clone()).ok())
            .map(|uuid| uuid == HOTSPOT_UUID)
            .unwrap_or(false)
    }
}

/// Build the NM connection settings dict for a WiFi Access Point.
/// Minimal settings — let NetworkManager auto-select band and channel.
/// Open network (no password) for now.
fn build_hotspot_settings(
    ssid: &str,
    password: Option<&str>,
) -> HashMap<String, HashMap<String, Value<'static>>> {
    let mut settings: HashMap<String, HashMap<String, Value>> = HashMap::new();

    // connection
    let mut conn = HashMap::new();
    conn.insert("type".to_string(), Value::from("802-11-wireless"));
    conn.insert("uuid".to_string(), Value::from(HOTSPOT_UUID));
    conn.insert("id".to_string(), Value::from("Hotspot"));
    conn.insert("autoconnect".to_string(), Value::from(false));
    settings.insert("connection".to_string(), conn);

    // 802-11-wireless
    let mut wifi = HashMap::new();
    wifi.insert("mode".to_string(), Value::from("ap"));
    wifi.insert("ssid".to_string(), Value::from(ssid.as_bytes().to_vec()));
    
    // If password provided, add security
    if let Some(pass) = password {
        if !pass.is_empty() {
            wifi.insert("security".to_string(), Value::from("802-11-wireless-security"));
            
            let mut security = HashMap::new();
            security.insert("key-mgmt".to_string(), Value::from("wpa-psk"));
            security.insert("psk".to_string(), Value::from(pass.to_string()));
            security.insert("proto".to_string(), Value::from(vec!["rsn"]));
            security.insert("pairwise".to_string(), Value::from(vec!["ccmp"]));
            security.insert("group".to_string(), Value::from(vec!["ccmp"]));
            settings.insert("802-11-wireless-security".to_string(), security);
        }
    }
    
    settings.insert("802-11-wireless".to_string(), wifi);

    // ipv4 — shared mode enables NAT + DHCP for clients
    let mut ipv4 = HashMap::new();
    ipv4.insert("method".to_string(), Value::from("shared"));
    settings.insert("ipv4".to_string(), ipv4);

    // ipv6 — disabled for simplicity
    let mut ipv6 = HashMap::new();
    ipv6.insert("method".to_string(), Value::from("ignore"));
    settings.insert("ipv6".to_string(), ipv6);

    settings
}
