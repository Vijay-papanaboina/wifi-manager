//! Power and battery detail page widgets.
//!
//! UPower and power-profiles-daemon are optional.  The widgets therefore
//! start in explicit unavailable states and are updated by `app::system_power`
//! only when a real snapshot is available.

#![allow(deprecated)]

use gtk4::{Align, Box as GtkBox, ComboBoxText, Image, Label, Orientation, Separator, prelude::*};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Conservative values exposed by the UI. 100% means no limiting.
pub(crate) const CHARGE_LIMIT_PRESETS: [u8; 6] = [50, 60, 70, 80, 90, 100];
const CHARGE_LIMIT_ATTRIBUTE: &str = "charge_control_end_threshold";

/// Capability/result state for the standard power-supply charge-limit file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChargeLimitStatus {
    NotSupported,
    PermissionRequired,
    Supported(u8),
    Mixed,
    Error(String),
}

#[derive(Clone, Debug)]
struct ChargeLimitEndpoint {
    attribute: PathBuf,
    writable: bool,
}

/// Small, testable controller for the standard Linux charge-limit attribute.
///
/// The production controller is rooted at `/sys/class/power_supply`, while
/// tests can provide an isolated fake root. No endpoint names are assumed.
#[derive(Clone, Debug)]
pub(crate) struct ChargeLimitController {
    root: PathBuf,
}

impl ChargeLimitController {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub(crate) fn status(&self) -> ChargeLimitStatus {
        let endpoints = match discover_charge_limit_endpoints(&self.root, false) {
            Ok(endpoints) => endpoints,
            Err(error) => return ChargeLimitStatus::Error(error.to_string()),
        };

        if endpoints.is_empty() {
            return ChargeLimitStatus::NotSupported;
        }
        if endpoints.iter().any(|endpoint| !endpoint.writable) {
            return ChargeLimitStatus::PermissionRequired;
        }

        let mut values = Vec::with_capacity(endpoints.len());
        for endpoint in endpoints {
            match read_charge_limit(&endpoint.attribute) {
                Ok(value) => values.push(value),
                Err(error) => return ChargeLimitStatus::Error(error.to_string()),
            }
        }

        let Some(first) = values.first().copied() else {
            return ChargeLimitStatus::NotSupported;
        };
        if values.iter().all(|value| *value == first) {
            ChargeLimitStatus::Supported(first)
        } else {
            ChargeLimitStatus::Mixed
        }
    }

    /// Write a conservative preset to every writable endpoint, then reread
    /// all endpoints so the returned state reflects the kernel's result.
    pub(crate) fn set_limit(&self, value: u8) -> Result<ChargeLimitStatus, String> {
        if !CHARGE_LIMIT_PRESETS.contains(&value) {
            return Err(format!("unsupported charge-limit preset: {value}%"));
        }

        let endpoints =
            discover_charge_limit_endpoints(&self.root, rustix::process::geteuid().is_root())
                .map_err(|error| error.to_string())?;
        if endpoints.is_empty() {
            return Err("charge limit is not supported".to_string());
        }
        if endpoints.iter().any(|endpoint| !endpoint.writable) {
            return Err("permission required to change the charge limit".to_string());
        }

        for endpoint in &endpoints {
            write_charge_limit(&endpoint.attribute, value).map_err(|error| {
                format!("could not write {}: {error}", endpoint.attribute.display())
            })?;
        }

        Ok(self.status())
    }
}

fn discover_charge_limit_endpoints(
    root: &Path,
    allow_root_only: bool,
) -> io::Result<Vec<ChargeLimitEndpoint>> {
    let mut endpoints = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let supply = entry.path();
        if !supply.is_dir() {
            continue;
        }

        let attribute = supply.join(CHARGE_LIMIT_ATTRIBUTE);
        let metadata = match fs::metadata(&attribute) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        endpoints.push(ChargeLimitEndpoint {
            writable: attribute_is_writable(&attribute, &metadata, allow_root_only),
            attribute,
        });
    }
    Ok(endpoints)
}

fn attribute_is_writable(path: &Path, metadata: &fs::Metadata, allow_root_only: bool) -> bool {
    #[cfg(unix)]
    if !allow_root_only && metadata.permissions().mode() & 0o222 == 0 {
        return false;
    }
    #[cfg(not(unix))]
    if !allow_root_only && metadata.permissions().readonly() {
        return false;
    }

    OpenOptions::new().write(true).open(path).is_ok()
}

fn read_charge_limit(path: &Path) -> io::Result<u8> {
    let mut value = String::new();
    File::open(path)?.read_to_string(&mut value)?;
    let value = value.trim().parse::<u8>().map_err(|error| {
        io::Error::new(io::ErrorKind::InvalidData, format!("invalid charge limit: {error}"))
    })?;
    if value > 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "charge limit must be between 0% and 100%",
        ));
    }
    Ok(value)
}

fn write_charge_limit(path: &Path, value: u8) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(value.to_string().as_bytes())?;
    file.flush()?;
    // Regular fake files need truncation when changing digit count; sysfs
    // attributes may reject set_len, so that failure is intentionally ignored.
    let _ = file.set_len(value.to_string().len() as u64);
    Ok(())
}

/// Parse the only argument accepted by the non-GUI root helper.
pub(crate) fn parse_charge_limit_helper_argument(argument: &str) -> Result<u8, String> {
    let value =
        argument.parse::<u8>().map_err(|_| format!("invalid charge-limit preset: {argument}"))?;
    if CHARGE_LIMIT_PRESETS.contains(&value) {
        Ok(value)
    } else {
        Err(format!("unsupported charge-limit preset: {value}%"))
    }
}

/// Apply one validated preset from the dedicated `pkexec` root-helper mode.
/// This function performs no GTK initialization and always uses the fixed
/// production power-supply root.
pub(crate) fn run_charge_limit_helper(argument: &str) -> i32 {
    let value = match parse_charge_limit_helper_argument(argument) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("wifi-manager charge-limit helper: {error}");
            return 2;
        }
    };
    if !rustix::process::geteuid().is_root() {
        eprintln!("wifi-manager charge-limit helper: effective uid 0 is required");
        return 1;
    }

    let controller = ChargeLimitController::new("/sys/class/power_supply");
    match controller.set_limit(value) {
        Ok(ChargeLimitStatus::Supported(current)) if current == value => {
            println!("charge limit verified at {current}%");
            0
        }
        Ok(status) => {
            eprintln!("charge-limit helper: verification did not converge: {status:?}");
            1
        }
        Err(error) => {
            eprintln!("charge-limit helper: {error}");
            1
        }
    }
}

#[derive(Clone)]
pub(crate) struct PowerWidgets {
    pub(crate) container: GtkBox,
    pub(crate) status: Label,
    pub(crate) battery_icon: Image,
    pub(crate) battery_summary: Label,
    pub(crate) battery_details: Label,
    pub(crate) profile_section: GtkBox,
    pub(crate) profile_combo: ComboBoxText,
    pub(crate) profile_status: Label,
    pub(crate) charge_limit_combo: ComboBoxText,
    pub(crate) charge_limit_status: Label,
}

impl PowerWidgets {
    pub(crate) fn new() -> Self {
        let container = GtkBox::new(Orientation::Vertical, 10);
        container.add_css_class("cc-page");
        container.add_css_class("cc-power-page");
        container.set_margin_top(4);
        container.set_margin_bottom(12);
        container.set_margin_start(16);
        container.set_margin_end(16);

        let status = Label::new(Some("Checking power services…"));
        status.set_halign(Align::Start);
        status.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        status.add_css_class("cc-detail-status");
        let battery_heading = heading("Battery");
        container.append(&battery_heading);

        let battery_row = GtkBox::new(Orientation::Horizontal, 10);
        battery_row.add_css_class("cc-battery-row");
        let battery_icon = Image::from_icon_name("battery-missing-symbolic");
        battery_icon.set_pixel_size(28);
        battery_icon.set_valign(Align::Center);
        battery_icon.add_css_class("cc-battery-icon");

        let battery_text = GtkBox::new(Orientation::Vertical, 2);
        battery_text.set_hexpand(true);
        let battery_summary = Label::new(Some("No battery detected"));
        battery_summary.set_halign(Align::Start);
        battery_summary.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        battery_summary.add_css_class("cc-battery-summary");
        let battery_details = Label::new(Some("A desktop power supply may not expose a battery."));
        battery_details.set_halign(Align::Start);
        battery_details.set_wrap(true);
        battery_details.add_css_class("cc-battery-details");
        battery_text.append(&battery_summary);
        battery_text.append(&battery_details);
        battery_row.append(&battery_icon);
        battery_row.append(&battery_text);
        container.append(&battery_row);

        let separator = Separator::new(Orientation::Horizontal);
        separator.add_css_class("cc-section-separator");
        container.append(&separator);

        let profile_section = GtkBox::new(Orientation::Vertical, 6);
        profile_section.add_css_class("cc-profile-section");
        let profile_heading = heading("Power profile");
        profile_section.append(&profile_heading);
        let profile_row = GtkBox::new(Orientation::Horizontal, 8);
        let profile_label = Label::new(Some("Profile"));
        profile_label.set_halign(Align::Start);
        profile_label.set_hexpand(true);
        profile_label.add_css_class("cc-section-label");
        let profile_combo = ComboBoxText::new();
        profile_combo.set_hexpand(false);
        profile_combo.set_tooltip_text(Some("Select the active power profile"));
        profile_combo.set_sensitive(false);
        profile_row.append(&profile_label);
        profile_row.append(&profile_combo);
        profile_section.append(&profile_row);
        let profile_status = Label::new(Some("Power profiles unavailable"));
        profile_status.set_halign(Align::Start);
        profile_status.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        profile_status.add_css_class("cc-profile-status");
        profile_section.append(&profile_status);
        profile_section.set_visible(false);
        container.append(&profile_section);

        let charge_limit_section = GtkBox::new(Orientation::Vertical, 6);
        charge_limit_section.add_css_class("cc-charge-limit-section");
        charge_limit_section.append(&heading("Charge limit"));
        let charge_limit_row = GtkBox::new(Orientation::Horizontal, 8);
        charge_limit_row.add_css_class("cc-charge-limit-row");
        let charge_limit_label = Label::new(Some("Maximum charge"));
        charge_limit_label.set_halign(Align::Start);
        charge_limit_label.set_hexpand(true);
        charge_limit_label.add_css_class("cc-section-label");
        let charge_limit_combo = ComboBoxText::new();
        for preset in CHARGE_LIMIT_PRESETS {
            let id = preset.to_string();
            let label =
                if preset == 100 { "100% (no limit)".to_string() } else { format!("{preset}%") };
            charge_limit_combo.append(Some(&id), &label);
        }
        charge_limit_combo.set_sensitive(false);
        charge_limit_combo.set_tooltip_text(Some("Select a standard charge limit preset"));
        charge_limit_combo.add_css_class("cc-charge-limit-combo");
        charge_limit_row.append(&charge_limit_label);
        charge_limit_row.append(&charge_limit_combo);
        charge_limit_section.append(&charge_limit_row);
        let charge_limit_status = Label::new(Some("Checking charge-limit support…"));
        charge_limit_status.set_halign(Align::Start);
        charge_limit_status.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        charge_limit_status.set_wrap(true);
        charge_limit_status.add_css_class("cc-charge-limit-status");
        charge_limit_section.append(&charge_limit_status);
        container.append(&charge_limit_section);

        Self {
            container,
            status,
            battery_icon,
            battery_summary,
            battery_details,
            profile_section,
            profile_combo,
            profile_status,
            charge_limit_combo,
            charge_limit_status,
        }
    }
}

fn heading(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_halign(Align::Start);
    label.add_css_class("cc-section-heading");
    label
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);

    struct FakeSysfs(PathBuf);

    impl FakeSysfs {
        fn new() -> Self {
            let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("wifi-manager-charge-limit-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).expect("create fake sysfs root");
            Self(root)
        }

        fn endpoint(&self, name: &str, value: u8, writable: bool) {
            let dir = self.0.join(name);
            fs::create_dir_all(&dir).expect("create fake power-supply entry");
            let path = dir.join(CHARGE_LIMIT_ATTRIBUTE);
            fs::write(&path, format!("{value}\n")).expect("write fake attribute");
            let mut permissions = fs::metadata(&path).expect("stat fake attribute").permissions();
            #[cfg(unix)]
            permissions.set_mode(if writable { 0o644 } else { 0o444 });
            #[cfg(not(unix))]
            permissions.set_readonly(!writable);
            fs::set_permissions(path, permissions).expect("set fake attribute permissions");
        }
    }

    impl Drop for FakeSysfs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_standard_attribute_is_not_supported() {
        let sysfs = FakeSysfs::new();
        fs::create_dir(sysfs.0.join("AC0")).expect("create fake adapter");
        assert_eq!(
            ChargeLimitController::new(sysfs.0.clone()).status(),
            ChargeLimitStatus::NotSupported
        );
    }

    #[test]
    fn helper_arguments_accept_only_conservative_presets() {
        assert_eq!(parse_charge_limit_helper_argument("50"), Ok(50));
        assert_eq!(parse_charge_limit_helper_argument("100"), Ok(100));
        assert!(parse_charge_limit_helper_argument("55").is_err());
        assert!(parse_charge_limit_helper_argument("not-a-number").is_err());
    }

    #[test]
    fn writable_and_mixed_endpoints_are_classified() {
        let sysfs = FakeSysfs::new();
        sysfs.endpoint("BAT0", 80, true);
        let controller = ChargeLimitController::new(sysfs.0.clone());
        assert_eq!(controller.status(), ChargeLimitStatus::Supported(80));

        sysfs.endpoint("BAT1", 70, true);
        assert_eq!(controller.status(), ChargeLimitStatus::Mixed);
    }

    #[test]
    fn read_only_attribute_requires_permission() {
        let sysfs = FakeSysfs::new();
        sysfs.endpoint("BAT0", 80, false);
        assert_eq!(
            ChargeLimitController::new(sysfs.0.clone()).status(),
            ChargeLimitStatus::PermissionRequired
        );
    }

    #[test]
    fn writes_all_endpoints_and_rereads() {
        let sysfs = FakeSysfs::new();
        sysfs.endpoint("BAT0", 70, true);
        sysfs.endpoint("BAT1", 70, true);
        let controller = ChargeLimitController::new(sysfs.0.clone());
        assert_eq!(
            controller.set_limit(90).expect("write charge limit"),
            ChargeLimitStatus::Supported(90)
        );
        assert_eq!(
            fs::read_to_string(sysfs.0.join("BAT0").join(CHARGE_LIMIT_ATTRIBUTE))
                .expect("read BAT0")
                .trim(),
            "90"
        );
        assert_eq!(
            fs::read_to_string(sysfs.0.join("BAT1").join(CHARGE_LIMIT_ATTRIBUTE))
                .expect("read BAT1")
                .trim(),
            "90"
        );
    }
}
