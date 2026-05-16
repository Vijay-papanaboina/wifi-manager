//! VPN management via NetworkManager (D-Bus).
//!
//! This module is intentionally small: it lists VPN profiles (including WireGuard
//! profiles stored in NetworkManager) and allows connect/disconnect.

use std::collections::HashMap;

use zbus::zvariant::{ObjectPath, OwnedObjectPath};

use super::proxies::{
    ActiveConnectionProxy, NetworkManagerProxy, SettingsConnectionProxy, SettingsProxy,
};

#[derive(Debug, Clone)]
pub struct VpnProfile {
    /// Human readable name (connection.id)
    pub name: String,
    /// NM connection UUID
    pub uuid: String,
    /// Settings.Connection object path
    pub connection_path: String,
}

#[derive(Debug, Clone)]
pub struct VpnActive {
    /// ActiveConnection object path
    pub active_path: String,
    /// ActiveConnection state: 1=activating, 2=activated, 3=deactivating, 4=deactivated
    pub state: u32,
    /// Settings.Connection path for this active connection
    pub connection_path: String,
}

#[derive(Clone)]
pub struct VpnManager {
    conn: zbus::Connection,
}

impl VpnManager {
    pub fn new(conn: &zbus::Connection) -> Self {
        Self { conn: conn.clone() }
    }

    /// List all saved VPN profiles in NetworkManager.
    ///
    /// Includes:
    /// - `connection.type == "vpn"` (OpenVPN, etc)
    /// - `connection.type == "wireguard"`
    pub async fn list_profiles(&self) -> zbus::Result<Vec<VpnProfile>> {
        let settings = SettingsProxy::new(&self.conn).await?;
        let connections = settings.list_connections().await?;

        let mut profiles = Vec::new();
        for conn_path in connections {
            let conn = SettingsConnectionProxy::builder(&self.conn)
                .path(conn_path.clone())?
                .build()
                .await?;

            let settings = match conn.get_settings().await {
                Ok(s) => s,
                Err(_) => continue,
            };

            let Some(conn_settings) = settings.get("connection") else {
                continue;
            };

            let conn_type = conn_settings
                .get("type")
                .and_then(|v| <String>::try_from(v.clone()).ok())
                .unwrap_or_default();

            if conn_type != "vpn" && conn_type != "wireguard" {
                continue;
            }

            let name = conn_settings
                .get("id")
                .and_then(|v| <String>::try_from(v.clone()).ok())
                .unwrap_or_else(|| "VPN".to_string());
            let uuid = conn_settings
                .get("uuid")
                .and_then(|v| <String>::try_from(v.clone()).ok())
                .unwrap_or_default();

            profiles.push(VpnProfile {
                name,
                uuid,
                connection_path: conn_path.to_string(),
            });
        }

        profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(profiles)
    }

    /// Return active VPN connections keyed by Settings.Connection path.
    pub async fn active_by_connection_path(&self) -> zbus::Result<HashMap<String, VpnActive>> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let actives = nm.active_connections().await.unwrap_or_default();

        let mut out: HashMap<String, VpnActive> = HashMap::new();
        for active_path in actives {
            let active = ActiveConnectionProxy::builder(&self.conn)
                .path(active_path.clone())?
                .build()
                .await?;

            let conn_type = active.connection_type().await.unwrap_or_default();
            if conn_type != "vpn" && conn_type != "wireguard" {
                continue;
            }

            let connection_path = active.connection().await?.to_string();
            let state = active.state().await.unwrap_or(0);

            out.insert(
                connection_path.clone(),
                VpnActive {
                    active_path: active_path.to_string(),
                    state,
                    connection_path,
                },
            );
        }

        Ok(out)
    }

    pub async fn connect(&self, connection_path: &str) -> zbus::Result<OwnedObjectPath> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let conn_path = ObjectPath::try_from(connection_path)
            .map_err(|e| zbus::Error::Failure(format!("Invalid connection path: {e}")))?;

        // VPN activation does not require a device or specific object.
        let root = ObjectPath::try_from("/")
            .map_err(|e| zbus::Error::Failure(format!("Invalid root path: {e}")))?;

        nm.activate_connection(&conn_path, &root, &root).await
    }

    pub async fn disconnect(&self, active_path: &str) -> zbus::Result<()> {
        let nm = NetworkManagerProxy::new(&self.conn).await?;
        let act_path = ObjectPath::try_from(active_path)
            .map_err(|e| zbus::Error::Failure(format!("Invalid active path: {e}")))?;
        nm.deactivate_connection(&act_path).await
    }

    pub async fn delete_profile(&self, connection_path: &str) -> zbus::Result<()> {
        let conn_path = ObjectPath::try_from(connection_path)
            .map_err(|e| zbus::Error::Failure(format!("Invalid connection path: {e}")))?;
        let conn = SettingsConnectionProxy::builder(&self.conn)
            .path(conn_path)?
            .build()
            .await?;
        conn.delete().await
    }
}
