use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Label};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

/// Build the main floating panel window with layer-shell support.
pub fn build_window(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("WiFi Manager")
        .default_width(380)
        .default_height(400)
        .build();

    // Initialize layer shell on the window
    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    // Anchor to top-right with margin
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_margin(Edge::Top, 10);
    window.set_margin(Edge::Right, 10);

    // Don't push other windows
    window.set_exclusive_zone(-1);

    // Placeholder content — will be replaced in Phase 3
    let label = Label::new(Some("WiFi Manager — Panel loaded"));
    label.set_margin_top(20);
    label.set_margin_bottom(20);
    label.set_margin_start(20);
    label.set_margin_end(20);
    window.set_child(Some(&label));

    window.present();
    log::info!("Layer-shell window presented");
}
