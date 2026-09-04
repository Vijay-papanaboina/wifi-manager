//! Pure VPN models used by the application and UI.

/// A saved NetworkManager VPN or WireGuard profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VpnProfile {
    pub(crate) name: String,
    pub(crate) uuid: String,
    pub(crate) connection_path: String,
}

/// An active NetworkManager VPN connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VpnActive {
    pub(crate) active_path: String,
    pub(crate) state: u32,
    pub(crate) connection_path: String,
}

impl VpnActive {
    pub(crate) fn is_connecting(&self) -> bool {
        self.state == 1
    }

    pub(crate) fn is_connected(&self) -> bool {
        self.state == 2
    }

    pub(crate) fn is_disconnecting(&self) -> bool {
        self.state == 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_network_manager_states() {
        let active = VpnActive {
            active_path: "/active/1".to_string(),
            state: 2,
            connection_path: "/settings/1".to_string(),
        };
        assert!(active.is_connected());
        assert!(!active.is_connecting());
        assert!(!active.is_disconnecting());
    }
}
