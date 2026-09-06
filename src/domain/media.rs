//! Pure MPRIS data parsing, capability and arbitration rules.

use std::collections::HashMap;

/// Playback states used by the MPRIS Player interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
    Unknown,
}

impl PlaybackStatus {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "Playing" => Self::Playing,
            "Paused" => Self::Paused,
            "Stopped" => Self::Stopped,
            _ => Self::Unknown,
        }
    }

    pub(crate) fn eligible(self) -> bool {
        matches!(self, Self::Playing | Self::Paused)
    }
}

/// Metadata values after the D-Bus variant has been safely normalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MetadataValue {
    String(String),
    Strings(Vec<String>),
}

/// Parse only the metadata fields needed by the compact media card.
pub(crate) fn parse_metadata(
    metadata: &HashMap<String, MetadataValue>,
) -> Option<(String, Option<String>, Option<String>)> {
    let title = match metadata.get("xesam:title") {
        Some(MetadataValue::String(value)) => value.trim().to_string(),
        _ => String::new(),
    };
    if title.is_empty() {
        return None;
    }

    let artist = match metadata.get("xesam:artist") {
        Some(MetadataValue::String(value)) => non_empty(value.trim().to_string()),
        Some(MetadataValue::Strings(values)) => {
            let artists = values
                .iter()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            (!artists.is_empty()).then(|| artists.join(", "))
        }
        None => None,
    };
    let art_url = match metadata.get("mpris:artUrl") {
        Some(MetadataValue::String(value)) => non_empty(value.trim().to_string()),
        _ => None,
    };

    Some((title, artist, art_url))
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

/// Capabilities exposed by an MPRIS player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MediaCapabilities {
    pub(crate) can_control: bool,
    pub(crate) can_previous: bool,
    pub(crate) can_next: bool,
    pub(crate) can_play: bool,
    pub(crate) can_pause: bool,
}

impl MediaCapabilities {
    pub(crate) fn previous_enabled(self) -> bool {
        self.can_control && self.can_previous
    }

    pub(crate) fn next_enabled(self) -> bool {
        self.can_control && self.can_next
    }

    pub(crate) fn play_pause_enabled(self, status: PlaybackStatus) -> bool {
        self.can_control
            && match status {
                PlaybackStatus::Playing => self.can_pause,
                PlaybackStatus::Paused => self.can_play,
                PlaybackStatus::Stopped | PlaybackStatus::Unknown => false,
            }
    }
}

/// A normalized snapshot of one MPRIS player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaPlayerSnapshot {
    pub(crate) bus_name: String,
    pub(crate) identity: String,
    pub(crate) status: PlaybackStatus,
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) art_url: Option<String>,
    pub(crate) capabilities: MediaCapabilities,
    /// Monotonically increasing controller sequence used for paused-player
    /// arbitration. It avoids relying on wall-clock timestamps.
    pub(crate) updated_at: u64,
}

impl MediaPlayerSnapshot {
    pub(crate) fn eligible(&self) -> bool {
        self.status.eligible() && !self.title.trim().is_empty()
    }
}

/// Select the media player represented on Home. Playing players always win;
/// otherwise the most recently updated paused player wins. A bus-name tie
/// break keeps the result deterministic when updates have equal sequence.
pub(crate) fn choose_player(players: &[MediaPlayerSnapshot]) -> Option<&MediaPlayerSnapshot> {
    let playing = players
        .iter()
        .filter(|player| player.eligible() && player.status == PlaybackStatus::Playing)
        .collect::<Vec<_>>();
    let paused = players
        .iter()
        .filter(|player| player.eligible() && player.status == PlaybackStatus::Paused)
        .collect::<Vec<_>>();
    let candidates = if playing.is_empty() { paused } else { playing };
    candidates.into_iter().min_by(|left, right| {
        right.updated_at.cmp(&left.updated_at).then_with(|| left.bus_name.cmp(&right.bus_name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(bus_name: &str, status: PlaybackStatus, updated_at: u64) -> MediaPlayerSnapshot {
        MediaPlayerSnapshot {
            bus_name: bus_name.to_string(),
            identity: bus_name.to_string(),
            status,
            title: "Track".to_string(),
            artist: None,
            art_url: None,
            capabilities: MediaCapabilities::default(),
            updated_at,
        }
    }

    #[test]
    fn metadata_accepts_artist_string_arrays_and_rejects_missing_title() {
        let mut metadata = HashMap::new();
        metadata.insert("xesam:title".to_string(), MetadataValue::String("  Song  ".to_string()));
        metadata.insert(
            "xesam:artist".to_string(),
            MetadataValue::Strings(vec![" A ".to_string(), String::new(), "B".to_string()]),
        );
        let parsed = parse_metadata(&metadata).expect("title is present");
        assert_eq!(parsed.0, "Song");
        assert_eq!(parsed.1.as_deref(), Some("A, B"));
        assert!(parse_metadata(&HashMap::new()).is_none());
    }

    #[test]
    fn arbitration_prefers_playing_then_recent_paused_and_bus_name() {
        let players = vec![
            player("org.mpris.MediaPlayer2.paused", PlaybackStatus::Paused, 99),
            player("org.mpris.MediaPlayer2.playing", PlaybackStatus::Playing, 1),
        ];
        assert_eq!(choose_player(&players).unwrap().bus_name, "org.mpris.MediaPlayer2.playing");

        let paused = vec![
            player("org.mpris.MediaPlayer2.z", PlaybackStatus::Paused, 8),
            player("org.mpris.MediaPlayer2.a", PlaybackStatus::Paused, 8),
        ];
        assert_eq!(choose_player(&paused).unwrap().bus_name, "org.mpris.MediaPlayer2.a");
    }

    #[test]
    fn capability_actions_require_control_and_individual_capability() {
        let capabilities = MediaCapabilities {
            can_control: true,
            can_previous: true,
            can_next: false,
            can_play: false,
            can_pause: true,
        };
        assert!(capabilities.previous_enabled());
        assert!(!capabilities.next_enabled());
        assert!(capabilities.play_pause_enabled(PlaybackStatus::Playing));
        assert!(!capabilities.play_pause_enabled(PlaybackStatus::Paused));
        assert!(
            MediaCapabilities { can_play: true, ..capabilities }
                .play_pause_enabled(PlaybackStatus::Paused)
        );
        assert!(!capabilities.play_pause_enabled(PlaybackStatus::Stopped));
        assert!(
            !MediaCapabilities { can_control: false, ..capabilities }
                .play_pause_enabled(PlaybackStatus::Playing)
        );
    }
}
