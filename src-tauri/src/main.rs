// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = rusted_claude_meter_lib::run() {
        eprintln!("failed to start Rusted Claude Meter: {error}");
        std::process::exit(1);
    }
}
