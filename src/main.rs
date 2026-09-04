use clap::Parser;

fn main() {
    env_logger::init();
    wifi_manager::run(wifi_manager::Args::parse());
}
