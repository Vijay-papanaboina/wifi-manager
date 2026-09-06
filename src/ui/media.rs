//! Home media-card widgets. The controller supplies all player state.

use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::domain::media::{MediaPlayerSnapshot, PlaybackStatus};

#[derive(Clone)]
pub(crate) struct MediaWidgets {
    pub(crate) container: GtkBox,
    placeholder_icon: Rc<RefCell<String>>,
    placeholder_displayed: Rc<Cell<bool>>,
    pub(crate) artwork: Image,
    pub(crate) title: Label,
    pub(crate) subtitle: Label,
    pub(crate) feedback: Label,
    pub(crate) previous: Button,
    pub(crate) play_pause: Button,
    pub(crate) next: Button,
}

impl MediaWidgets {
    pub(crate) fn new(placeholder_icon: &str) -> Self {
        let container = GtkBox::new(Orientation::Horizontal, 10);
        container.add_css_class("cc-media-card");
        container.set_margin_start(12);
        container.set_margin_end(12);
        container.set_margin_bottom(8);
        container.set_visible(false);

        let artwork = Image::from_icon_name(placeholder_icon);
        artwork.add_css_class("cc-media-artwork");
        artwork.set_pixel_size(48);
        artwork.set_valign(Align::Center);

        let content = GtkBox::new(Orientation::Vertical, 3);
        content.set_hexpand(true);
        content.set_valign(Align::Center);

        let title = Label::new(Some("No media"));
        title.add_css_class("cc-media-title");
        title.set_halign(Align::Start);
        title.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        let subtitle = Label::new(Some(""));
        subtitle.add_css_class("cc-media-subtitle");
        subtitle.set_halign(Align::Start);
        subtitle.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        let feedback = Label::new(Some(""));
        feedback.add_css_class("cc-media-feedback");
        feedback.set_halign(Align::Start);
        feedback.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        feedback.set_visible(false);

        let actions = GtkBox::new(Orientation::Horizontal, 4);
        actions.add_css_class("cc-media-actions");
        let previous = media_button("media-skip-backward-symbolic", "Previous track");
        let play_pause = media_button("media-playback-start-symbolic", "Play or pause");
        let next = media_button("media-skip-forward-symbolic", "Next track");
        actions.append(&previous);
        actions.append(&play_pause);
        actions.append(&next);

        content.append(&title);
        content.append(&subtitle);
        content.append(&feedback);
        content.append(&actions);

        container.append(&artwork);
        container.append(&content);

        Self {
            container,
            placeholder_icon: Rc::new(RefCell::new(placeholder_icon.to_string())),
            placeholder_displayed: Rc::new(Cell::new(true)),
            artwork,
            title,
            subtitle,
            feedback,
            previous,
            play_pause,
            next,
        }
    }

    pub(crate) fn render(&self, player: Option<&MediaPlayerSnapshot>) {
        let Some(player) = player else {
            self.container.set_visible(false);
            self.clear_artwork();
            return;
        };

        self.container.set_visible(true);
        self.title.set_text(&player.title);
        let artist = player.artist.as_deref().unwrap_or("Unknown artist");
        self.subtitle.set_text(&format!("{artist} · {}", player.identity));
        self.play_pause.set_icon_name(match player.status {
            PlaybackStatus::Playing => "media-playback-pause-symbolic",
            PlaybackStatus::Paused => "media-playback-start-symbolic",
            _ => "media-playback-start-symbolic",
        });
        self.previous.set_sensitive(player.capabilities.previous_enabled());
        self.next.set_sensitive(player.capabilities.next_enabled());
        self.play_pause.set_sensitive(player.capabilities.play_pause_enabled(player.status));
        self.feedback.set_visible(false);
    }

    pub(crate) fn clear_artwork(&self) {
        self.placeholder_displayed.set(true);
        self.artwork.set_icon_name(Some(self.placeholder_icon.borrow().as_str()));
    }

    pub(crate) fn set_artwork(&self, texture: &gdk::Texture) {
        self.placeholder_displayed.set(false);
        self.artwork.set_paintable(Some(texture));
    }

    /// Apply the configured placeholder without replacing artwork already
    /// loaded for the current player.
    pub(crate) fn apply_config(&self, placeholder_icon: &str) {
        *self.placeholder_icon.borrow_mut() = placeholder_icon.to_string();
        if self.placeholder_displayed.get() {
            self.artwork.set_icon_name(Some(placeholder_icon));
        }
    }

    pub(crate) fn set_feedback(&self, message: &str) {
        self.feedback.set_text(message);
        self.feedback.set_visible(true);
    }
}

fn media_button(icon: &str, tooltip: &str) -> Button {
    let button = Button::from_icon_name(icon);
    button.add_css_class("cc-media-action");
    button.set_tooltip_text(Some(tooltip));
    button
}
