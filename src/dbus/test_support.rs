//! Test-only NetworkManager service hosted on an isolated D-Bus daemon.

#![allow(dead_code)]

use std::collections::HashMap;
use std::error::Error;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use zbus::connection::Builder;
use zbus::interface;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

#[derive(Clone, Copy, Debug)]
pub(crate) enum WifiSignal {
    Added,
    Removed,
    StateChanged,
}

impl WifiSignal {
    async fn emit(self, connection: &zbus::Connection) -> zbus::Result<()> {
        let (interface, member) = match self {
            Self::Added => ("org.freedesktop.NetworkManager.Device.Wireless", "AccessPointAdded"),
            Self::Removed => {
                ("org.freedesktop.NetworkManager.Device.Wireless", "AccessPointRemoved")
            }
            Self::StateChanged => ("org.freedesktop.NetworkManager.Device", "StateChanged"),
        };
        if matches!(self, Self::StateChanged) {
            connection
                .emit_signal(
                    None::<&str>,
                    WIFI_DEVICE_PATH,
                    interface,
                    member,
                    &(100_u32, 30_u32, 0_u32),
                )
                .await
        } else {
            let path = OwnedObjectPath::try_from("/org/freedesktop/NetworkManager/AccessPoint/1")?;
            connection
                .emit_signal(None::<&str>, WIFI_DEVICE_PATH, interface, member, &(path,))
                .await
        }
    }
}

struct SignalBurst {
    kind: WifiSignal,
    count: usize,
    next_access_points: Vec<OwnedObjectPath>,
}

const NETWORK_MANAGER_NAME: &str = "org.freedesktop.NetworkManager";
const NETWORK_MANAGER_PATH: &str = "/org/freedesktop/NetworkManager";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const WIFI_DEVICE_PATH: &str = "/org/freedesktop/NetworkManager/Devices/1";

pub(crate) type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Debug)]
pub(crate) enum VanishedAccessPointError {
    UnknownObject,
    UnknownMethod,
}

/// Properties exposed by one fake NetworkManager access point.
#[derive(Clone, Debug)]
pub(crate) struct FakeAccessPointConfig {
    pub(crate) path: OwnedObjectPath,
    pub(crate) ssid: Vec<u8>,
    pub(crate) strength: u8,
    pub(crate) frequency: u32,
    pub(crate) flags: u32,
    pub(crate) wpa_flags: u32,
    pub(crate) rsn_flags: u32,
    pub(crate) hw_address: String,
    pub(crate) vanish_on_ssid_read: Option<VanishedAccessPointError>,
    pub(crate) remove_from_list_on_ssid_read: bool,
}

impl FakeAccessPointConfig {
    pub(crate) fn new(path_suffix: u32, ssid: &str) -> Self {
        Self {
            path: OwnedObjectPath::try_from(format!(
                "/org/freedesktop/NetworkManager/AccessPoint/{path_suffix}"
            ))
            .expect("the generated access-point path is valid"),
            ssid: ssid.as_bytes().to_vec(),
            strength: 75,
            frequency: 5_180,
            flags: 0,
            wpa_flags: 0,
            rsn_flags: 0,
            hw_address: format!("02:00:00:00:00:{path_suffix:02x}"),
            vanish_on_ssid_read: None,
            remove_from_list_on_ssid_read: false,
        }
    }

    pub(crate) fn vanishing_on_read(mut self, error: VanishedAccessPointError) -> Self {
        self.vanish_on_ssid_read = Some(error);
        self.remove_from_list_on_ssid_read = true;
        self
    }

    pub(crate) fn failing_on_read(mut self, error: VanishedAccessPointError) -> Self {
        self.vanish_on_ssid_read = Some(error);
        self
    }
}

/// Isolated fake service plus the client connection used by `WifiManager`.
pub(crate) struct FakeNetworkManager {
    client: zbus::Connection,
    service: zbus::Connection,
    access_points: Arc<Mutex<Vec<OwnedObjectPath>>>,
    burst: Arc<Mutex<Option<SignalBurst>>>,
    _bus: PrivateBus,
}

impl FakeNetworkManager {
    pub(crate) async fn start() -> TestResult<Self> {
        let bus = PrivateBus::start()?;
        let access_points = Arc::new(Mutex::new(Vec::new()));
        let burst = Arc::new(Mutex::new(None));
        let device_path = OwnedObjectPath::try_from(WIFI_DEVICE_PATH)?;

        let service = Builder::address(bus.address.as_str())?
            .name(NETWORK_MANAGER_NAME)?
            .serve_at(NETWORK_MANAGER_PATH, FakeNetworkManagerRoot { device_path })?
            .serve_at(WIFI_DEVICE_PATH, FakeDevice)?
            .serve_at(WIFI_DEVICE_PATH, FakeWireless { access_points: Arc::clone(&access_points) })?
            .serve_at(
                SETTINGS_PATH,
                FakeSettings {
                    burst: Arc::clone(&burst),
                    access_points: Arc::clone(&access_points),
                },
            )?
            .build()
            .await?;

        let client = Builder::address(bus.address.as_str())?.build().await?;

        Ok(Self { client, service, access_points, burst, _bus: bus })
    }

    pub(crate) fn client_connection(&self) -> zbus::Connection {
        self.client.clone()
    }

    pub(crate) async fn emit_signal(&self, kind: WifiSignal) -> TestResult<()> {
        kind.emit(&self.service).await?;
        Ok(())
    }

    pub(crate) async fn signal_stream(&self, kind: WifiSignal) -> TestResult<zbus::MessageStream> {
        let (interface, member) = match kind {
            WifiSignal::Added => {
                ("org.freedesktop.NetworkManager.Device.Wireless", "AccessPointAdded")
            }
            WifiSignal::Removed => {
                ("org.freedesktop.NetworkManager.Device.Wireless", "AccessPointRemoved")
            }
            WifiSignal::StateChanged => ("org.freedesktop.NetworkManager.Device", "StateChanged"),
        };
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(self.service.unique_name().expect("service has a bus name").clone())?
            .path(WIFI_DEVICE_PATH)?
            .interface(interface)?
            .member(member)?
            .build();
        Ok(zbus::MessageStream::for_match_rule(rule, &self.client, None).await?)
    }

    pub(crate) fn set_visible_access_points(&self, paths: Vec<OwnedObjectPath>) {
        *self.access_points.lock().expect("fake AP list lock poisoned") = paths;
    }

    /// Deliver the entire burst before replying to the next enumeration's
    /// ListConnections call. This deterministically puts signals ahead of the
    /// reply that an inline signal consumer is waiting for.
    pub(crate) fn burst_on_next_enumeration(
        &self,
        kind: WifiSignal,
        count: usize,
        next_access_points: Vec<OwnedObjectPath>,
    ) {
        *self.burst.lock().expect("fake burst lock poisoned") =
            Some(SignalBurst { kind, count, next_access_points });
    }

    pub(crate) async fn add_access_point(&self, config: FakeAccessPointConfig) -> TestResult<()> {
        let path = config.path.clone();
        let access_point = FakeAccessPoint::new(config, Arc::clone(&self.access_points));
        self.service.object_server().at(path.clone(), access_point).await?;
        self.access_points.lock().expect("fake AP list lock poisoned").push(path);
        Ok(())
    }

    pub(crate) async fn remove_access_point(&self, path: &OwnedObjectPath) -> TestResult<()> {
        self.access_points
            .lock()
            .expect("fake AP list lock poisoned")
            .retain(|candidate| candidate != path);
        self.service.object_server().remove::<FakeAccessPoint, _>(path.clone()).await?;
        Ok(())
    }
}

struct PrivateBus {
    address: String,
    child: Child,
}

impl PrivateBus {
    fn start() -> TestResult<Self> {
        // A self-contained configuration avoids /etc/dbus-1/session.conf and
        // host service activation, neither of which belongs in an isolated test
        // (and the former is absent in a Nix build sandbox).
        let child = Command::new("dbus-daemon")
            .arg(concat!("--config-file=", env!("CARGO_MANIFEST_DIR"), "/resources/test-dbus.conf"))
            .args(["--nofork", "--nopidfile", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        // Install the cleanup guard before any fallible startup I/O.
        let mut bus = Self { address: String::new(), child };
        let stdout = bus.child.stdout.take().ok_or("dbus-daemon did not expose stdout")?;
        let mut address = String::new();
        BufReader::new(stdout).read_line(&mut address)?;
        let address = address.trim().to_owned();
        if address.is_empty() {
            return Err("dbus-daemon did not print a bus address".into());
        }

        bus.address = address;
        Ok(bus)
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct FakeNetworkManagerRoot {
    device_path: OwnedObjectPath,
}

#[interface(name = "org.freedesktop.NetworkManager")]
impl FakeNetworkManagerRoot {
    fn get_devices(&self) -> Vec<OwnedObjectPath> {
        vec![self.device_path.clone()]
    }

    #[zbus(property)]
    fn wireless_enabled(&self) -> bool {
        true
    }
}

struct FakeDevice;

#[interface(name = "org.freedesktop.NetworkManager.Device")]
impl FakeDevice {
    #[zbus(property)]
    fn device_type(&self) -> u32 {
        2
    }

    #[zbus(property)]
    fn active_connection(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from("/").expect("root is a valid object path")
    }
}

struct FakeWireless {
    access_points: Arc<Mutex<Vec<OwnedObjectPath>>>,
}

#[interface(name = "org.freedesktop.NetworkManager.Device.Wireless")]
impl FakeWireless {
    fn request_scan(&self, _options: HashMap<String, OwnedValue>) {}

    #[zbus(property)]
    fn access_points(&self) -> Vec<OwnedObjectPath> {
        self.access_points.lock().expect("fake AP list lock poisoned").clone()
    }
}

struct FakeSettings {
    burst: Arc<Mutex<Option<SignalBurst>>>,
    access_points: Arc<Mutex<Vec<OwnedObjectPath>>>,
}

#[interface(name = "org.freedesktop.NetworkManager.Settings")]
impl FakeSettings {
    async fn list_connections(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> zbus::fdo::Result<Vec<OwnedObjectPath>> {
        let burst = self.burst.lock().expect("fake burst lock poisoned").take();
        if let Some(burst) = burst {
            *self.access_points.lock().expect("fake AP list lock poisoned") =
                burst.next_access_points;
            for _ in 0..burst.count {
                burst.kind.emit(connection).await?;
            }
        }
        Ok(Vec::new())
    }
}

struct FakeAccessPoint {
    path: OwnedObjectPath,
    ssid: Vec<u8>,
    strength: u8,
    frequency: u32,
    flags: u32,
    wpa_flags: u32,
    rsn_flags: u32,
    hw_address: String,
    vanish_on_ssid_read: Option<VanishedAccessPointError>,
    remove_from_list_on_ssid_read: bool,
    access_points: Arc<Mutex<Vec<OwnedObjectPath>>>,
}

impl FakeAccessPoint {
    fn new(config: FakeAccessPointConfig, access_points: Arc<Mutex<Vec<OwnedObjectPath>>>) -> Self {
        Self {
            path: config.path,
            ssid: config.ssid,
            strength: config.strength,
            frequency: config.frequency,
            flags: config.flags,
            wpa_flags: config.wpa_flags,
            rsn_flags: config.rsn_flags,
            hw_address: config.hw_address,
            vanish_on_ssid_read: config.vanish_on_ssid_read,
            remove_from_list_on_ssid_read: config.remove_from_list_on_ssid_read,
            access_points,
        }
    }
}

#[interface(name = "org.freedesktop.NetworkManager.AccessPoint")]
impl FakeAccessPoint {
    #[zbus(property)]
    fn ssid(&self) -> zbus::fdo::Result<Vec<u8>> {
        let message = "Object does not exist at the requested access-point path".to_string();
        if self.remove_from_list_on_ssid_read {
            self.access_points
                .lock()
                .expect("fake AP list lock poisoned")
                .retain(|candidate| candidate != &self.path);
        }

        match self.vanish_on_ssid_read {
            Some(VanishedAccessPointError::UnknownObject) => {
                return Err(zbus::fdo::Error::UnknownObject(message));
            }
            Some(VanishedAccessPointError::UnknownMethod) => {
                return Err(zbus::fdo::Error::UnknownMethod(message));
            }
            None => {}
        }

        Ok(self.ssid.clone())
    }

    #[zbus(property)]
    fn strength(&self) -> u8 {
        self.strength
    }

    #[zbus(property)]
    fn frequency(&self) -> u32 {
        self.frequency
    }

    #[zbus(property)]
    fn flags(&self) -> u32 {
        self.flags
    }

    #[zbus(property)]
    fn wpa_flags(&self) -> u32 {
        self.wpa_flags
    }

    #[zbus(property)]
    fn rsn_flags(&self) -> u32 {
        self.rsn_flags
    }

    #[zbus(property)]
    fn hw_address(&self) -> &str {
        &self.hw_address
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbus::network_manager::WifiManager;

    #[test]
    fn inline_refresh_stalls_until_the_signal_queue_is_drained() -> TestResult<()> {
        use futures_util::{
            StreamExt,
            future::{Either, select},
        };
        use gtk4::glib;
        use std::time::Duration;

        let context = glib::MainContext::new();
        context.with_thread_default(|| {
            context.block_on(async {
                let scenario = async {
                    let fake = FakeNetworkManager::start().await?;
                    let wifi = WifiManager::from_connection(fake.client_connection()).await?;
                    let mut signals = fake.signal_stream(WifiSignal::Removed).await?;
                    // Consume a trigger just as the old AP-removed loop did, then
                    // await a refresh without polling the stream again.
                    fake.emit_signal(WifiSignal::Removed).await?;
                    signals.next().await.expect("trigger delivered")?;
                    fake.burst_on_next_enumeration(WifiSignal::Removed, 128, Vec::new());
                    let refresh = Box::pin(wifi.get_networks());
                    let deadline = Box::pin(glib::timeout_future(Duration::from_millis(250)));
                    let refresh = match select(refresh, deadline).await {
                        Either::Left((result, _)) => {
                            panic!("inline refresh unexpectedly completed: {result:?}")
                        }
                        Either::Right(((), refresh)) => refresh,
                    };

                    // Resume draining: the SAME pending call must now complete.
                    // This distinguishes queue backpressure from a broken service
                    // or an unrelated timeout and avoids leaving a wedged client.
                    let drain = async {
                        for _ in 0..128 {
                            signals.next().await.expect("burst signal delivered")?;
                        }
                        Ok::<_, Box<dyn Error + Send + Sync>>(())
                    };
                    let (networks, drained) = futures_util::join!(refresh, drain);
                    assert!(networks?.is_empty());
                    drained?;
                    Ok(())
                };
                match select(
                    Box::pin(scenario),
                    Box::pin(glib::timeout_future(Duration::from_secs(5))),
                )
                .await
                {
                    Either::Left((result, _)) => result,
                    Either::Right(_) => Err("signal backpressure reproduction timed out".into()),
                }
            })
        })?
    }

    #[test]
    fn serves_a_stable_network_snapshot() -> TestResult<()> {
        zbus::block_on(async {
            let fake = FakeNetworkManager::start().await?;
            let access_point = FakeAccessPointConfig::new(1, "Test Network");
            let access_point_path = access_point.path.clone();
            fake.add_access_point(access_point).await?;

            let wifi = WifiManager::from_connection(fake.client_connection()).await?;
            let networks = wifi.get_networks().await?;

            assert_eq!(networks.len(), 1);
            assert_eq!(networks[0].ssid, "Test Network");
            assert_eq!(networks[0].ap_path, access_point_path.as_str());

            fake.remove_access_point(&access_point_path).await?;
            assert!(wifi.get_networks().await?.is_empty());

            Ok(())
        })
    }

    async fn assert_vanished_access_point_is_skipped(
        error: VanishedAccessPointError,
    ) -> TestResult<()> {
        let fake = FakeNetworkManager::start().await?;
        fake.add_access_point(FakeAccessPointConfig::new(1, "Stable Network")).await?;
        fake.add_access_point(
            FakeAccessPointConfig::new(2, "Vanishing Network").vanishing_on_read(error),
        )
        .await?;

        let wifi = WifiManager::from_connection(fake.client_connection()).await?;
        let networks = wifi.get_networks().await?;

        assert_eq!(networks.len(), 1);
        assert_eq!(networks[0].ssid, "Stable Network");

        Ok(())
    }

    #[test]
    fn skips_an_access_point_that_returns_unknown_object() -> TestResult<()> {
        zbus::block_on(assert_vanished_access_point_is_skipped(
            VanishedAccessPointError::UnknownObject,
        ))
    }

    #[test]
    fn skips_an_access_point_that_returns_unknown_method() -> TestResult<()> {
        zbus::block_on(assert_vanished_access_point_is_skipped(
            VanishedAccessPointError::UnknownMethod,
        ))
    }

    #[test]
    fn propagates_unknown_method_when_access_point_still_exists() -> TestResult<()> {
        zbus::block_on(async {
            let fake = FakeNetworkManager::start().await?;
            fake.add_access_point(
                FakeAccessPointConfig::new(1, "Broken Network")
                    .failing_on_read(VanishedAccessPointError::UnknownMethod),
            )
            .await?;

            let wifi = WifiManager::from_connection(fake.client_connection()).await?;
            assert!(wifi.get_networks().await.is_err());

            Ok(())
        })
    }
}
