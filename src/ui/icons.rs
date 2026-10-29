use gtk4::prelude::*;
use gtk4::{IconTheme, Image, Label, Widget, gio, glib};
use std::sync::Once;

const RESOURCE_ROOT: &str = "/com/github/wifi_manager/icons";
static REGISTER: Once = Once::new();

fn compiled_resource() -> gio::Resource {
    let bytes =
        glib::Bytes::from_static(include_bytes!(concat!(env!("OUT_DIR"), "/icons.gresource")));
    gio::Resource::from_data(&bytes).expect("compiled icon resource is valid")
}

pub(crate) fn register() {
    REGISTER.call_once(|| {
        let resource = compiled_resource();
        gio::resources_register(&resource);
        if let Some(display) = gtk4::gdk::Display::default() {
            IconTheme::for_display(&display).add_resource_path(RESOURCE_ROOT);
        }
    });
}

pub(crate) fn widget(value: &str, pixel_size: i32) -> Widget {
    let has_icon = value.starts_with("wifi-manager-")
        || gtk4::gdk::Display::default()
            .is_some_and(|display| IconTheme::for_display(&display).has_icon(value));
    if has_icon {
        let image = Image::from_icon_name(value);
        image.set_pixel_size(pixel_size);
        image.set_halign(gtk4::Align::Center);
        image.set_valign(gtk4::Align::Center);
        image.upcast()
    } else {
        let label = Label::new(Some(value));
        label.set_halign(gtk4::Align::Center);
        label.set_valign(gtk4::Align::Center);
        label.upcast()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON_RESOURCE_PREFIX: &str = "/com/github/wifi_manager/icons/scalable/actions";

    #[test]
    fn compiled_resource_contains_all_icons_without_a_display() {
        let resource = compiled_resource();
        let names = resource
            .enumerate_children(ICON_RESOURCE_PREFIX, gio::ResourceLookupFlags::NONE)
            .expect("compiled icon resource children are available");
        assert_eq!(names.len(), 45);
        for name in &names {
            let path = format!("{ICON_RESOURCE_PREFIX}/{name}");
            resource
                .lookup_data(&path, gio::ResourceLookupFlags::NONE)
                .unwrap_or_else(|error| panic!("missing resource {path}: {error}"));
        }
    }

    #[test]
    fn every_default_config_icon_resolves_in_the_bundle() {
        let config = crate::config::Config::default();
        let defaults = [
            &config.lock_icon,
            &config.saved_icon,
            &config.wifi_icon,
            &config.bluetooth_icon,
            &config.audio_icon,
            &config.power_battery_icon,
            &config.media_icon,
            &config.system_icon,
            &config.brightness_icon,
            &config.volume_icon,
            &config.night_mode_on_icon,
            &config.night_mode_off_icon,
            &config.logout_icon,
            &config.reboot_icon,
            &config.suspend_icon,
            &config.poweroff_icon,
        ];
        let resource = compiled_resource();
        for name in config.signal_icons.iter().chain(defaults) {
            let path = format!("{ICON_RESOURCE_PREFIX}/{name}.svg");
            resource
                .lookup_data(&path, gio::ResourceLookupFlags::NONE)
                .unwrap_or_else(|error| panic!("default icon does not resolve: {name}: {error}"));
        }
    }

    #[test]
    fn bundled_icons_use_supported_symbolic_shapes_and_local_paint() {
        let resource = compiled_resource();
        let names = resource
            .enumerate_children(ICON_RESOURCE_PREFIX, gio::ResourceLookupFlags::NONE)
            .expect("compiled icon resource children are available");
        for name in &names {
            let name = name.as_str();
            let path = format!("{ICON_RESOURCE_PREFIX}/{name}");
            let bytes = resource.lookup_data(&path, gio::ResourceLookupFlags::NONE).unwrap();
            let svg = std::str::from_utf8(bytes.as_ref()).unwrap();
            assert!(svg.contains("gpa:version=\"1\""), "missing symbolic format version in {name}");
            // GTK's symbolic parser ignores group styling and ellipse elements.
            assert!(!svg.contains("<g "), "inherited paint in {name}");
            assert!(!svg.contains("<ellipse"), "unsupported ellipse in {name}");
            assert!(!svg.contains(" opacity="), "unsupported opacity in {name}");
            for shape in
                svg.lines().filter(|line| line.contains("<path ") || line.contains("<circle "))
            {
                assert!(shape.contains("class="), "missing symbolic paint in {name}");
                if shape.contains("fill=\"none\"") {
                    assert!(shape.contains("transparent-fill"), "filled outline in {name}");
                }
                if shape.contains("stroke=\"#000\"") {
                    assert!(shape.contains("foreground-stroke"), "missing stroke in {name}");
                    assert!(shape.contains("stroke-width="), "missing stroke width in {name}");
                    assert!(
                        shape.contains("gpa:stroke-width=\"1.8 1.8 1.8\""),
                        "changed stroke weight in {name}"
                    );
                }
            }
            if [
                "wifi-manager-wifi-weak-symbolic.svg",
                "wifi-manager-wifi-fair-symbolic.svg",
                "wifi-manager-wifi-good-symbolic.svg",
            ]
            .contains(&name)
            {
                assert!(svg.contains("stroke-opacity=\"0.2\""), "lost dim arcs in {name}");
            }
        }
    }
}
