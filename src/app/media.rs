//! Signal-driven MPRIS controller for the Home media card.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use futures_util::StreamExt;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

use crate::dbus::media::{self, MPRIS_PATH, MPRIS_PREFIX, MediaAction};
use crate::domain::media::{MediaPlayerSnapshot, choose_player};
use crate::ui::media::MediaWidgets;
use crate::ui::window::PanelWidgets;

type Players = Rc<RefCell<HashMap<String, MediaPlayerSnapshot>>>;

/// Start initial session-bus discovery and signal subscriptions. An absent
/// session bus or absent MPRIS players is intentionally a normal hidden-card
/// state.
pub(crate) fn setup(widgets: &PanelWidgets) {
    let media_widgets = widgets.media.clone();
    glib::spawn_future_local(async move {
        let Ok(connection) = zbus::Connection::session().await else {
            log::debug!("MPRIS unavailable: no session bus");
            return;
        };

        let players: Players = Rc::new(RefCell::new(HashMap::new()));
        let watching = Rc::new(RefCell::new(HashSet::new()));
        let sequence = Rc::new(Cell::new(0_u64));
        let artwork_generation = Rc::new(Cell::new(0_u64));

        let names = match media::list_player_names(&connection).await {
            Ok(names) => names,
            Err(error) => {
                log::debug!("MPRIS discovery unavailable: {error}");
                return;
            }
        };

        for bus_name in names {
            start_player_watcher(
                connection.clone(),
                bus_name,
                Rc::clone(&players),
                Rc::clone(&watching),
                Rc::clone(&sequence),
                media_widgets.clone(),
                Rc::clone(&artwork_generation),
            );
        }

        connect_action(
            &media_widgets.previous,
            MediaAction::Previous,
            connection.clone(),
            Rc::clone(&players),
            Rc::clone(&sequence),
            media_widgets.clone(),
            Rc::clone(&artwork_generation),
        );
        connect_action(
            &media_widgets.play_pause,
            MediaAction::PlayPause,
            connection.clone(),
            Rc::clone(&players),
            Rc::clone(&sequence),
            media_widgets.clone(),
            Rc::clone(&artwork_generation),
        );
        connect_action(
            &media_widgets.next,
            MediaAction::Next,
            connection.clone(),
            Rc::clone(&players),
            sequence.clone(),
            media_widgets.clone(),
            artwork_generation.clone(),
        );

        let bus = match zbus::fdo::DBusProxy::new(&connection).await {
            Ok(bus) => bus,
            Err(error) => {
                log::debug!("MPRIS ownership watch unavailable: {error}");
                return;
            }
        };
        let mut names_stream = match bus.receive_name_owner_changed().await {
            Ok(stream) => stream,
            Err(error) => {
                log::debug!("MPRIS ownership watch unavailable: {error}");
                return;
            }
        };

        while let Some(signal) = names_stream.next().await {
            let Ok(args) = signal.args() else { continue };
            let name = args.name().to_string();
            if !name.starts_with(MPRIS_PREFIX) {
                continue;
            }
            let has_owner = args.new_owner().is_some();
            if !has_owner {
                players.borrow_mut().remove(&name);
                watching.borrow_mut().remove(&name);
                render_media(&media_widgets, &players, &artwork_generation);
                continue;
            }
            start_player_watcher(
                connection.clone(),
                name,
                Rc::clone(&players),
                Rc::clone(&watching),
                Rc::clone(&sequence),
                media_widgets.clone(),
                Rc::clone(&artwork_generation),
            );
        }
    });
}

fn start_player_watcher(
    connection: zbus::Connection,
    bus_name: String,
    players: Players,
    watching: Rc<RefCell<HashSet<String>>>,
    sequence: Rc<Cell<u64>>,
    widgets: MediaWidgets,
    artwork_generation: Rc<Cell<u64>>,
) {
    if !watching.borrow_mut().insert(bus_name.clone()) {
        return;
    }

    glib::spawn_future_local(async move {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(bus_name.as_str())
            .and_then(|rule| rule.interface("org.freedesktop.DBus.Properties"))
            .and_then(|rule| rule.member("PropertiesChanged"))
            .and_then(|rule| rule.path(MPRIS_PATH))
            .map(|rule| rule.build());
        let Ok(rule) = rule else {
            watching.borrow_mut().remove(&bus_name);
            return;
        };
        let Ok(mut stream) = zbus::MessageStream::for_match_rule(rule, &connection, Some(32)).await
        else {
            watching.borrow_mut().remove(&bus_name);
            return;
        };

        refresh_player(&connection, &bus_name, &players, &sequence, &widgets, &artwork_generation)
            .await;

        while let Some(message) = stream.next().await {
            if message.is_err() {
                continue;
            }
            refresh_player(
                &connection,
                &bus_name,
                &players,
                &sequence,
                &widgets,
                &artwork_generation,
            )
            .await;
        }

        watching.borrow_mut().remove(&bus_name);
        players.borrow_mut().remove(&bus_name);
        render_media(&widgets, &players, &artwork_generation);
    });
}

async fn refresh_player(
    connection: &zbus::Connection,
    bus_name: &str,
    players: &Players,
    sequence: &Cell<u64>,
    widgets: &MediaWidgets,
    artwork_generation: &Rc<Cell<u64>>,
) {
    let next_sequence = sequence.get().wrapping_add(1);
    sequence.set(next_sequence);
    match media::read_player(connection, bus_name, next_sequence).await {
        Ok(snapshot) => {
            players.borrow_mut().insert(bus_name.to_string(), snapshot);
        }
        Err(error) => {
            log::debug!("MPRIS player {bus_name} disappeared: {error}");
            players.borrow_mut().remove(bus_name);
        }
    }
    render_media(widgets, players, artwork_generation);
}

fn render_media(widgets: &MediaWidgets, players: &Players, artwork_generation: &Rc<Cell<u64>>) {
    let selected = players.borrow().values().cloned().collect::<Vec<_>>();
    let selected = choose_player(&selected).cloned();
    widgets.render(selected.as_ref());
    widgets.clear_artwork();
    artwork_generation.set(artwork_generation.get().wrapping_add(1));
    let generation = artwork_generation.get();
    let Some(player) = selected else { return };
    let Some(url) = player.art_url.clone() else { return };
    let file = gio::File::for_uri(&url);
    let widgets = widgets.clone();
    let artwork_generation = Rc::clone(artwork_generation);
    glib::spawn_future_local(async move {
        let Ok((bytes, _etag)) = file.load_bytes_future().await else { return };
        let Ok(texture) = gdk::Texture::from_bytes(&bytes) else { return };
        if generation == artwork_generation.get() {
            widgets.set_artwork(&texture);
        }
    });
}

fn connect_action(
    button: &gtk4::Button,
    action: MediaAction,
    connection: zbus::Connection,
    players: Players,
    sequence: Rc<Cell<u64>>,
    widgets: MediaWidgets,
    artwork_generation: Rc<Cell<u64>>,
) {
    button.connect_clicked(move |_| {
        let selected = players.borrow().values().cloned().collect::<Vec<_>>();
        let Some(player) = choose_player(&selected).cloned() else { return };
        let connection = connection.clone();
        let players = Rc::clone(&players);
        let sequence = Rc::clone(&sequence);
        let widgets = widgets.clone();
        let artwork_generation = Rc::clone(&artwork_generation);
        glib::spawn_future_local(async move {
            match media::send_action(&connection, &player.bus_name, action).await {
                Ok(()) => {
                    refresh_player(
                        &connection,
                        &player.bus_name,
                        &players,
                        &sequence,
                        &widgets,
                        &artwork_generation,
                    )
                    .await;
                }
                Err(error) => {
                    widgets.set_feedback(&format!("Media action failed: {error}"));
                }
            }
        });
    });
}
