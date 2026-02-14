mod app;
mod daemon;
mod dbus;
mod ui;

use clap::Parser;
use gtk4::Application;
use gtk4::glib;
use gtk4::prelude::*;

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
        // Send Toggle() to running daemon and exit
        let rt = glib::MainContext::default();
        rt.block_on(async {
            if daemon::is_instance_running().await {
                match daemon::send_toggle().await {
                    Ok(_) => log::info!("Toggle sent to running instance"),
                    Err(e) => {
                        log::error!("Failed to send toggle: {e}");
                        eprintln!("Error: could not toggle — is wifi-manager running?");
                    }
                }
            } else {
                eprintln!("No running instance found. Start with: wifi-manager");
            }
        });
        return;
    }

    // Start the GTK application (daemon mode)
    log::info!("Starting wifi-manager daemon");

    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        log::info!("Application activated");

        // Build the UI (starts hidden)
        let widgets = ui::window::build_window(app);

        // Create a send-safe weak reference for cross-thread window access
        let window_ref = {
            use gtk4::glib::object::ObjectExt;
            widgets.window.downgrade().into() // SendWeakRef
        };
        let window_ref: glib::SendWeakRef<gtk4::ApplicationWindow> = window_ref;

        // Create panel state with visibility toggle callback
        // This callback is called from the D-Bus thread, so it dispatches
        // to the GTK main thread via MainContext::invoke (thread-safe).
        let panel_state = daemon::PanelState::new(move |visible| {
            let window_ref = window_ref.clone();
            glib::MainContext::default().invoke(move || {
                if let Some(window) = window_ref.upgrade() {
                    if visible {
                        window.present();
                    } else {
                        window.set_visible(false);
                    }
                }
            });
        });

        // Register the D-Bus daemon service
        let panel_state_clone = panel_state.clone();
        glib::spawn_future_local(async move {
            match daemon::register_service(panel_state_clone).await {
                Ok(_conn) => {
                    log::info!("Daemon D-Bus service ready");
                    // _conn is kept alive by the async task
                    // It will be dropped when the app exits
                    std::future::pending::<()>().await;
                }
                Err(e) => {
                    log::error!("Failed to register D-Bus service: {e}");
                }
            }
        });

        // Connect to NetworkManager and set up the app controller
        let panel_state_for_app = panel_state.clone();
        glib::spawn_future_local(async move {
            match dbus::network_manager::WifiManager::new().await {
                Ok(wifi) => {
                    log::info!("NetworkManager D-Bus connection established");
                    app::setup(
                        &widgets,
                        wifi,
                        panel_state_for_app.scan_requested.clone(),
                    );

                    // Show the panel on first launch
                    panel_state_for_app.show();
                }
                Err(e) => {
                    log::error!("Failed to connect to NetworkManager: {e}");
                    widgets
                        .status_label
                        .set_text("Error: NetworkManager unavailable");
                    // Still show the panel so user sees the error
                    panel_state_for_app.show();
                }
            }
        });
    });

    app.run();
}
