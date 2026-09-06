//! wifi-manager application library.
//!
//! The binary is intentionally thin; startup and runtime wiring live here so
//! they can be compiled and exercised independently of command-line parsing.

mod app;
mod bootstrap;
mod config;
mod controls;
mod daemon;
mod dbus;
mod domain;
mod error;
mod process;
mod state;
mod ui;

use clap::Parser;

/// Command-line arguments understood by the daemon launcher.
#[derive(Parser, Debug)]
#[command(name = "wifi-manager", version, about)]
pub struct Args {
    /// Toggle the panel visibility (sends signal to running daemon).
    #[arg(long)]
    pub toggle: bool,

    /// Reload config and CSS (sends signal to running daemon).
    #[arg(long)]
    pub reload: bool,

    /// Apply one validated charge-limit preset as the non-GUI Polkit helper.
    #[arg(long = "charge-limit-helper", value_name = "PERCENT", hide = true)]
    pub charge_limit_helper: Option<String>,
}

/// Start the application or dispatch a control command to an existing daemon.
pub fn run(args: Args) {
    bootstrap::run(args);
}
