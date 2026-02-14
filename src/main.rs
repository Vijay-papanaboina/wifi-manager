mod app;
mod dbus;
mod ui;

use clap::Parser;
use gtk4::prelude::*;
use gtk4::Application;
use log;

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
        // TODO: Send toggle signal to running daemon via D-Bus
        log::info!("Toggle requested — will send D-Bus signal to running instance");
        println!("Toggle mode — not yet implemented");
        return;
    }

    // Start the GTK application (daemon mode)
    log::info!("Starting wifi-manager daemon");

    let app = Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        log::info!("Application activated");
        ui::window::build_window(app);
    });

    app.run();
}
