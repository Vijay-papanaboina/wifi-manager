//! Typed commands emitted by the panel.

/// The list targeted by the shared scan button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScanTarget {
    Wifi,
    Bluetooth,
    Vpn,
}

impl ScanTarget {
    pub(super) fn from_tabs(bluetooth_active: bool, vpn_active: bool) -> Self {
        if bluetooth_active {
            Self::Bluetooth
        } else if vpn_active {
            Self::Vpn
        } else {
            Self::Wifi
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prioritizes_bluetooth_then_vpn_then_wifi() {
        assert_eq!(ScanTarget::from_tabs(true, true), ScanTarget::Bluetooth);
        assert_eq!(ScanTarget::from_tabs(false, true), ScanTarget::Vpn);
        assert_eq!(ScanTarget::from_tabs(false, false), ScanTarget::Wifi);
    }
}
