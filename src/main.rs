mod app;
mod dbus;
mod ui;

use clap::Parser;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::Application;

/// A floating WiFi manager for Wayland compositors (Hyprland/Sway)
#[derive(Parser, Debug)]
#[command(name = "wifi-manager", version, about)]
struct Args {
    /// Toggle the panel visibility (sends signal to running daemon)
    #[arg(long)]
    toggle: bool,
}

const APP_ID: &str = "com.github.wifi_manager.WifiManager";

fn main() {
    // Initialize logging
    env_logger::init();

    let args = Args::parse();

    if args.toggle {
        // TODO: Send toggle signal to running daemon via D-Bus (Phase 4)
        log::info!("Toggle requested — will send D-Bus signal to running instance");
        println!("Toggle mode — not yet implemented");
        return;
    }

    // Start the GTK application
    log::info!("Starting wifi-manager");

    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        log::info!("Application activated");

        // Build the UI
        let widgets = ui::window::build_window(app);

        // Connect to D-Bus and set up the app controller
        glib::spawn_future_local(async move {
            match dbus::network_manager::WifiManager::new().await {
                Ok(wifi) => {
                    log::info!("D-Bus connection established");
                    app::setup(&widgets, wifi);
                }
                Err(e) => {
                    log::error!("Failed to connect to NetworkManager: {e}");
                    widgets.status_label.set_text("Error: NetworkManager unavailable");
                }
            }
        });
    });

    app.run();
}
