//! Process startup and GTK/D-Bus wiring.

use gtk4::Application;
use gtk4::glib;
use gtk4::prelude::*;

use crate::Args;
use crate::{app, config, daemon, dbus, ui};

const APP_ID: &str = "com.github.wifi_manager.WifiManager";

/// Start the daemon, or send a command to an already-running instance.
pub(crate) fn run(args: Args) {
    if let Some(preset) = args.charge_limit_helper {
        let exit_code = ui::power::run_charge_limit_helper(&preset);
        std::process::exit(exit_code);
    }

    if args.toggle {
        dispatch_toggle();
        return;
    }

    if args.reload {
        dispatch_reload();
        return;
    }

    start_application();
}

fn dispatch_toggle() {
    let context = glib::MainContext::default();
    context.block_on(async {
        if daemon::is_instance_running().await {
            match daemon::send_toggle().await {
                Ok(()) => log::info!("Toggle sent to running instance"),
                Err(error) => {
                    log::error!("Failed to send toggle: {error}");
                    eprintln!("Error: could not toggle — is wifi-manager running?");
                }
            }
        } else {
            eprintln!("No running instance found. Start with: wifi-manager");
        }
    });
}

fn dispatch_reload() {
    let context = glib::MainContext::default();
    context.block_on(async {
        if daemon::is_instance_running().await {
            match daemon::send_reload().await {
                Ok(()) => {
                    log::info!("Reload sent to running instance");
                    println!("Config and CSS reloaded");
                }
                Err(error) => {
                    log::error!("Failed to send reload: {error}");
                    eprintln!("Error: could not reload — is wifi-manager running?");
                }
            }
        } else {
            eprintln!("No running instance found. Start with: wifi-manager");
        }
    });
}

fn start_application() {
    log::info!("Starting wifi-manager daemon");

    let application = Application::builder().application_id(APP_ID).build();

    // Catch kill signals to cleanly shut down GTK and drop hardware locks.
    for signal in [2, 15] {
        let application = application.clone();
        glib::unix_signal_add_local(signal, move || {
            log::info!("Received signal {signal}, gracefully shutting down");
            application.quit();
            glib::ControlFlow::Break
        });
    }

    application.connect_activate(|application| {
        log::info!("Application activated");

        let widgets = ui::window::build_window(application);
        let window_ref: glib::SendWeakRef<gtk4::ApplicationWindow> = {
            use gtk4::glib::object::ObjectExt;
            widgets.window.downgrade().into()
        };

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

        // Audio, battery, display, and power actions do not depend on a
        // successful NetworkManager connection.  Start them immediately so
        // the Control Center remains navigable with optional services absent.
        app::setup_system_controls(&widgets);

        let daemon_state = panel_state.clone();
        glib::spawn_future_local(async move {
            match daemon::register_service(daemon_state).await {
                Ok(_connection) => {
                    log::info!("Daemon D-Bus service ready");
                    std::future::pending::<()>().await;
                }
                Err(error) => log::error!("Failed to register D-Bus service: {error}"),
            }
        });

        let app_panel_state = panel_state.clone();
        glib::spawn_future_local(async move {
            match dbus::network_manager::WifiManager::new().await {
                Ok(wifi) => {
                    log::info!("NetworkManager D-Bus connection established");
                    let config = config::Config::load();
                    app::setup(
                        &widgets,
                        wifi,
                        app_panel_state.scan_requested.clone(),
                        app_panel_state.clone(),
                    );

                    if config.show_on_start {
                        app_panel_state.show();
                    }
                }
                Err(error) => {
                    log::error!("Failed to connect to NetworkManager: {error}");
                    widgets.status_label.set_text("Error: NetworkManager unavailable");
                    app_panel_state.show();
                }
            }
        });
    });

    application.run();

    // Allow pending D-Bus responses and GTK callbacks to complete before exit.
    let context = glib::MainContext::default();
    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(150);
    while context.pending() && started.elapsed() < timeout {
        context.iteration(false);
    }
}
