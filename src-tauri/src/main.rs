// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The Claude Code status-line bridge runs as a short-lived CLI: read one
    // JSON blob from stdin, record it, exit. Dispatched before anything Tauri
    // touches, so a status-line render never builds an app, and an ordinary
    // launch (no arguments) falls straight through to the GUI.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if rusted_claude_meter_lib::run_cli(&args) {
        return;
    }

    if let Err(error) = rusted_claude_meter_lib::run() {
        eprintln!("failed to start Rusted Claude Meter: {error}");
        std::process::exit(1);
    }
}
