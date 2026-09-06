//! Pure audio device models and rules.

/// Whether an audio device is used for playback or capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioDeviceKind {
    Output,
    Input,
}

/// An audio device exposed by the audio server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioDevice {
    /// Server-provided device name. This is also the stable action identifier.
    pub(crate) id: String,
    /// Human-facing description, falling back to `id` when the server omits it.
    pub(crate) label: String,
    pub(crate) kind: AudioDeviceKind,
    pub(crate) is_default: bool,
    pub(crate) volume_percent: u8,
    pub(crate) muted: bool,
}

/// A complete point-in-time view of the audio server's devices and defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct AudioSnapshot {
    pub(crate) outputs: Vec<AudioDevice>,
    pub(crate) inputs: Vec<AudioDevice>,
    pub(crate) default_output_id: Option<String>,
    pub(crate) default_input_id: Option<String>,
}

/// Return a trimmed non-empty description, or the server-provided device id.
pub(crate) fn description_or_id(description: Option<&str>, id: &str) -> String {
    description
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .unwrap_or(id)
        .to_string()
}

/// Convert a server-reported percentage to the UI's bounded whole-percent form.
///
/// Non-finite values are treated as zero. Rounding happens before clamping so values just outside
/// the normal range still map to the nearest valid endpoint.
pub(crate) fn clamp_percent(percent: f64) -> u8 {
    if !percent.is_finite() {
        return 0;
    }
    percent.round().clamp(0.0, 100.0) as u8
}

/// Whether a PulseAudio source is a real capture device rather than a sink monitor.
pub(crate) fn source_is_eligible(monitor_of_sink: Option<u32>) -> bool {
    monitor_of_sink.is_none()
}

/// Sort devices with defaults first and then deterministic label/id tie-breaks.
pub(crate) fn sort_audio_devices(devices: &mut [AudioDevice]) {
    devices.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// Render the only device summary the home tile can honestly show: the
/// server-provided label, with mute state when the device is muted.
pub(crate) fn device_status_label(device: &AudioDevice) -> String {
    if device.muted { format!("{} (muted)", device.label) } else { device.label.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, label: &str, is_default: bool) -> AudioDevice {
        AudioDevice {
            id: id.to_string(),
            label: label.to_string(),
            kind: AudioDeviceKind::Output,
            is_default,
            volume_percent: 50,
            muted: false,
        }
    }

    #[test]
    fn falls_back_for_missing_or_blank_descriptions() {
        assert_eq!(description_or_id(None, "alsa_output.pci"), "alsa_output.pci");
        assert_eq!(description_or_id(Some("   "), "alsa_output.pci"), "alsa_output.pci");
        assert_eq!(description_or_id(Some("  Built-in Audio  "), "id"), "Built-in Audio");
    }

    #[test]
    fn rounds_and_clamps_percentages() {
        assert_eq!(clamp_percent(-1.0), 0);
        assert_eq!(clamp_percent(12.49), 12);
        assert_eq!(clamp_percent(12.5), 13);
        assert_eq!(clamp_percent(100.4), 100);
        assert_eq!(clamp_percent(f64::NAN), 0);
        assert_eq!(clamp_percent(f64::INFINITY), 0);
    }

    #[test]
    fn excludes_monitor_sources() {
        assert!(source_is_eligible(None));
        assert!(!source_is_eligible(Some(7)));
    }

    #[test]
    fn sorts_default_then_label_then_id() {
        let mut devices = vec![
            device("z", "same", false),
            device("b", "Beta", false),
            device("a", "alpha", true),
            device("a", "alpha", false),
        ];

        sort_audio_devices(&mut devices);

        assert_eq!(
            devices
                .iter()
                .map(|device| (device.id.as_str(), device.label.as_str()))
                .collect::<Vec<_>>(),
            vec![("a", "alpha"), ("b", "Beta"), ("a", "alpha"), ("z", "same"),]
        );
        assert!(devices[0].is_default);
    }

    #[test]
    fn status_label_includes_real_mute_state() {
        let mut muted = device("sink", "Speakers", true);
        assert_eq!(device_status_label(&muted), "Speakers");
        muted.muted = true;
        assert_eq!(device_status_label(&muted), "Speakers (muted)");
    }
}
