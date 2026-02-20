use std::fs;
use std::path::PathBuf;
use zbus::{Connection, Result as ZbusResult};
use log::{debug, info, warn};

/// Proxy for the systemd-logind Session interface.
#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Session {
    /// Set the brightness value via systemd-logind (bypasses need for sudo/udev rules).
    fn set_brightness(&self, subsystem: &str, name: &str, brightness: u32) -> ZbusResult<()>;
}

pub(crate) struct BacklightInfo {
    dir: PathBuf,
    name: String,
}

/// Manages screen brightness via sysfs (reading) and systemd-logind (writing).
pub struct BrightnessManager {
    proxy: SessionProxy<'static>,
    backlight: Option<BacklightInfo>,
}

impl BrightnessManager {
    /// Creates a new BrightnessManager. Discovers the hardware path dynamically.
    pub async fn new() -> ZbusResult<Self> {
        let connection = Connection::system().await?;
        
        let proxy = SessionProxy::builder(&connection)
            .path("/org/freedesktop/login1/session/auto")?
            .build()
            .await?;
            
        // Dynamically find the first available backlight device
        let mut backlight = None;
        
        if let Ok(entries) = fs::read_dir("/sys/class/backlight") {
            let mut devices: Vec<_> = entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .collect();
            // Sort for deterministic selection
            devices.sort_by_key(|e| e.file_name());
            
            if let Some(entry) = devices.into_iter().next() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    let name_str = name.to_string();
                    backlight = Some(BacklightInfo {
                        dir: path,
                        name: name_str,
                    });
                }
            }
        }
        
        if let Some(info) = &backlight {
            info!("Found backlight device: {:?}", info.name);
        } else {
            warn!("No backlight devices found in /sys/class/backlight");
        }
            
        Ok(Self { proxy, backlight })
    }

    /// Reads a u32 value from a sysfs file.
    fn read_sysfs_u32(path: &std::path::Path) -> Option<u32> {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
    }

    /// Gets the current brightness as a percentage (0.0 to 100.0).
    pub fn get_brightness_percent(&self) -> Option<f64> {
        let info = self.backlight.as_ref()?;
        let max = Self::read_sysfs_u32(&info.dir.join("max_brightness"))?;
        let current = Self::read_sysfs_u32(&info.dir.join("brightness"))?;
        
        if max == 0 {
            return None;
        }
        
        Some((current as f64 / max as f64) * 100.0)
    }

    /// Sets the brightness given a percentage. Clamps to a minimum of MIN_BRIGHTNESS_PERCENT to
    /// prevent the screen from turning completely off.
    pub async fn set_brightness_percent(&self, percent: f64) -> Result<(), Box<dyn std::error::Error>> {
        let info = match &self.backlight {
            Some(i) => i,
            None => {
                let msg = "Cannot set brightness: No backlight device found";
                warn!("{}", msg);
                return Err(msg.into());
            }
        };
        
        let max = match Self::read_sysfs_u32(&info.dir.join("max_brightness")) {
            Some(m) => m,
            None => {
                let msg = "Cannot set brightness: Missing or unreadable max_brightness";
                warn!("{}", msg);
                return Err(msg.into());
            }
        };
        
        const MIN_BRIGHTNESS_PERCENT: f64 = 5.0;
        
        // Treat NaN/infinity as minimum brightness
        let percent = if percent.is_finite() { percent } else { MIN_BRIGHTNESS_PERCENT };
        // Clamp to minimum so the screen doesn't turn off completely
        let percent = percent.clamp(MIN_BRIGHTNESS_PERCENT, 100.0);
        let target = ((percent / 100.0) * max as f64).round() as u32;
        
        debug!("Setting brightness of {} to {} ({}%)", info.name, target, percent);
        // Using "backlight" as the subsystem, which is standard for /sys/class/backlight
        self.proxy.set_brightness("backlight", &info.name, target).await?;
        Ok(())
    }
}
