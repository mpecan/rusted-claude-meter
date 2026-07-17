//! Application shell: Tauri builder, tray wiring and window lifecycle.
//!
//! The app lives in the tray/menu bar. The main window starts hidden and is
//! only shown on demand; closing it hides it rather than quitting.

mod tray;

/// Build and run the app. Errors bubble to `main` instead of panicking so
/// the workspace-wide `clippy::expect_used` deny holds here too.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Menu-bar-only on macOS: no Dock icon, no app switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::init(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // The app keeps running in the tray; closing the window hides it.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
}
