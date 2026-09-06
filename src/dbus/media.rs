//! Small session-bus helpers for the MPRIS media-player protocol.

use std::collections::HashMap;

use zbus::proxy;
use zbus::zvariant::OwnedValue;

use crate::domain::media::{MediaCapabilities, MediaPlayerSnapshot, MetadataValue, PlaybackStatus};

pub(crate) const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
pub(crate) const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";

#[proxy(interface = "org.mpris.MediaPlayer2", default_path = "/org/mpris/MediaPlayer2")]
pub(crate) trait MprisRoot {
    #[zbus(property)]
    fn identity(&self) -> zbus::Result<String>;
}

#[proxy(interface = "org.mpris.MediaPlayer2.Player", default_path = "/org/mpris/MediaPlayer2")]
pub(crate) trait MprisPlayer {
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
    fn play_pause(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, OwnedValue>>;

    #[zbus(property)]
    fn can_control(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn can_go_previous(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn can_go_next(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn can_play(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn can_pause(&self) -> zbus::Result<bool>;
}

/// Return currently-owned well-known MPRIS names. No player polling is done;
/// this is only the initial discovery snapshot before signal subscriptions.
pub(crate) async fn list_player_names(connection: &zbus::Connection) -> zbus::Result<Vec<String>> {
    let bus = zbus::fdo::DBusProxy::new(connection).await?;
    Ok(bus
        .list_names()
        .await?
        .into_iter()
        .map(|name| name.to_string())
        .filter(|name| name.starts_with(MPRIS_PREFIX))
        .collect())
}

pub(crate) async fn read_player(
    connection: &zbus::Connection,
    bus_name: &str,
    updated_at: u64,
) -> zbus::Result<MediaPlayerSnapshot> {
    let root = MprisRootProxy::builder(connection).destination(bus_name)?.build().await?;
    let player = MprisPlayerProxy::builder(connection).destination(bus_name)?.build().await?;

    let identity = root.identity().await.unwrap_or_else(|_| bus_name.to_string());
    let status = PlaybackStatus::parse(&player.playback_status().await?);
    let metadata = player.metadata().await?;
    let metadata = normalize_metadata(metadata);
    let (title, artist, art_url) = crate::domain::media::parse_metadata(&metadata)
        .unwrap_or_else(|| (String::new(), None, None));
    let capabilities = MediaCapabilities {
        can_control: player.can_control().await.unwrap_or(false),
        can_previous: player.can_go_previous().await.unwrap_or(false),
        can_next: player.can_go_next().await.unwrap_or(false),
        can_play: player.can_play().await.unwrap_or(false),
        can_pause: player.can_pause().await.unwrap_or(false),
    };

    Ok(MediaPlayerSnapshot {
        bus_name: bus_name.to_string(),
        identity,
        status,
        title,
        artist,
        art_url,
        capabilities,
        updated_at,
    })
}

pub(crate) async fn send_action(
    connection: &zbus::Connection,
    bus_name: &str,
    action: MediaAction,
) -> zbus::Result<()> {
    let player = MprisPlayerProxy::builder(connection).destination(bus_name)?.build().await?;
    match action {
        MediaAction::Previous => player.previous().await,
        MediaAction::PlayPause => player.play_pause().await,
        MediaAction::Next => player.next().await,
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MediaAction {
    Previous,
    PlayPause,
    Next,
}

fn normalize_metadata(values: HashMap<String, OwnedValue>) -> HashMap<String, MetadataValue> {
    values
        .into_iter()
        .filter_map(|(key, value)| {
            if let Ok(value) = String::try_from(value.clone()) {
                return Some((key, MetadataValue::String(value)));
            }
            if let Ok(value) = Vec::<String>::try_from(value) {
                return Some((key, MetadataValue::Strings(value)));
            }
            None
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::media::parse_metadata;

    #[test]
    fn malformed_metadata_is_ignored_without_panicking() {
        let mut values = HashMap::new();
        values.insert("xesam:title".to_string(), MetadataValue::Strings(vec!["bad".into()]));
        assert!(parse_metadata(&values).is_none());
    }
}
