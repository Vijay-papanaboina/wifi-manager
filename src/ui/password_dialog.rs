//! Inline password entry section for secured networks.

use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, Entry, InputPurpose, Label, Orientation, Revealer,
    RevealerTransitionType,
};

/// Build the inline password entry section.
///
/// Returns `(revealer, password_entry, connect_button, cancel_button, error_label)`.
/// The revealer wraps the section — show/hide by calling `revealer.set_reveal_child()`.
pub fn build_password_section() -> (Revealer, Entry, Button, Button, Label) {
    let revealer = Revealer::new();
    revealer.add_css_class("password-revealer");
    revealer.set_transition_type(RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(200);
    revealer.set_reveal_child(false);

    let vbox = GtkBox::new(Orientation::Vertical, 6);
    vbox.add_css_class("password-section");

    // Password entry
    let entry = Entry::new();
    entry.add_css_class("password-entry");
    entry.set_placeholder_text(Some("Enter password"));
    entry.set_visibility(false); // hidden characters by default
    entry.set_input_purpose(InputPurpose::Password);

    // Show/hide toggle via secondary icon
    entry.set_secondary_icon_name(Some("view-reveal-symbolic"));
    entry.set_secondary_icon_tooltip_text(Some("Show password"));
    entry.set_secondary_icon_activatable(true);

    // Toggle password visibility when icon is clicked
    entry.connect_icon_release(|entry, _pos| {
        // Check current state by looking at the icon name
        let is_hidden = entry
            .secondary_icon_name()
            .is_none_or(|name| name == "view-reveal-symbolic");
        entry.set_visibility(is_hidden);
        if is_hidden {
            entry.set_secondary_icon_name(Some("view-conceal-symbolic"));
            entry.set_secondary_icon_tooltip_text(Some("Hide password"));
        } else {
            entry.set_secondary_icon_name(Some("view-reveal-symbolic"));
            entry.set_secondary_icon_tooltip_text(Some("Show password"));
        }
    });

    // Buttons row
    let button_box = GtkBox::new(Orientation::Horizontal, 8);
    button_box.add_css_class("password-buttons");
    button_box.set_halign(gtk4::Align::End);
    button_box.set_margin_top(4);

    let cancel_button = Button::with_label("Cancel");
    cancel_button.add_css_class("cancel-button");

    let connect_button = Button::with_label("Connect");
    connect_button.add_css_class("connect-button");

    button_box.append(&cancel_button);
    button_box.append(&connect_button);

    // Error label (hidden by default — set text to show)
    let error_label = Label::new(None);
    error_label.add_css_class("error-label");
    error_label.set_halign(gtk4::Align::Start);
    error_label.set_visible(false);

    vbox.append(&entry);
    vbox.append(&error_label);
    vbox.append(&button_box);
    revealer.set_child(Some(&vbox));

    (revealer, entry, connect_button, cancel_button, error_label)
}
