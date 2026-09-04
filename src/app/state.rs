//! GTK-thread application state.
//!
//! This module contains no signal wiring or side effects. Keeping the state
//! definition separate makes ownership and lifecycle changes easier to review.

use std::collections::HashMap;
use std::time::Instant;

use gtk4::glib;

use crate::dbus::bluetooth_manager::BluetoothManager;
use crate::dbus::network_manager::WifiManager;
use crate::dbus::vpn_manager::VpnManager;
use crate::domain::bluetooth::BluetoothDevice;
use crate::domain::network::Network;
use crate::domain::vpn::VpnActive;

#[derive(Clone)]
pub(crate) struct PendingVpnAction {
    pub(crate) label: String,
    pub(crate) started_at: Instant,
}

/// Top-level page that currently owns the shared header status label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActiveTab {
    Wifi,
    Bluetooth,
}

/// All mutable state owned by the GTK application controller.
///
/// The controller is single-threaded (`Rc<RefCell<_>>`); asynchronous work
/// must clone service handles and never hold a borrow across an await point.
pub(crate) struct AppState {
    pub(crate) wifi: WifiManager,
    pub(crate) vpn: VpnManager,
    pub(crate) active_tab: ActiveTab,
    pub(crate) networks: Vec<Network>,
    pub(crate) selected_ssid: Option<String>,
    pub(crate) bluetooth: Option<BluetoothManager>,
    pub(crate) bt_devices: Vec<BluetoothDevice>,
    pub(crate) bt_row_paths: Vec<Option<String>>,
    pub(crate) bt_pending: HashMap<String, String>,
    pub(crate) bt_scan_in_progress: bool,
    pub(crate) bt_auto_scan_source: Option<glib::SourceId>,
    pub(crate) bt_live_refresh_source: Option<glib::SourceId>,
    pub(crate) bt_auto_scan_active: bool,
    pub(crate) bt_task_generation: u64,
    pub(crate) bt_power_transition_in_progress: bool,
    pub(crate) bt_menu_open: bool,
    pub(crate) wifi_scan_in_progress: bool,
    pub(crate) wifi_auto_scan_source: Option<glib::SourceId>,
    pub(crate) wifi_bg_reconnect_source: Option<glib::SourceId>,
    pub(crate) wifi_row_ssids: Vec<Option<String>>,
    pub(crate) wifi_pending: HashMap<String, String>,
    pub(crate) vpn_pending: HashMap<String, PendingVpnAction>,
    pub(crate) vpn_active_by_conn: HashMap<String, VpnActive>,
    pub(crate) vpn_refresh_source: Option<glib::SourceId>,
    pub(crate) vpn_view_active: bool,
    pub(crate) vpn_view_generation: u64,
    pub(crate) vpn_busy_count: usize,
    pub(crate) vpn_normalizing: bool,
}

impl AppState {
    /// Mark the VPN page active and invalidate refreshes from older visits.
    pub(crate) fn activate_vpn_view(&mut self) -> u64 {
        self.vpn_view_generation = self.vpn_view_generation.wrapping_add(1);
        self.vpn_view_active = true;
        self.vpn_view_generation
    }

    /// Mark the VPN page inactive and invalidate in-flight UI updates.
    pub(crate) fn deactivate_vpn_view(&mut self) {
        self.vpn_view_generation = self.vpn_view_generation.wrapping_add(1);
        self.vpn_view_active = false;
    }

    pub(crate) fn is_current_vpn_view(&self, token: u64) -> bool {
        self.vpn_view_active && self.vpn_view_generation == token
    }
}
