//! Application shell: Tauri builder, tray wiring, window lifecycle and the
//! background polling loop.
//!
//! The app lives in the tray/menu bar. The main window starts hidden and is
//! only shown on demand; closing it hides it rather than quitting. A
//! scheduler task keeps a current usage snapshot available and broadcasts
//! every change as a `usage-state` event — the single source of truth the
//! tray and UI subscribe to.

mod cache;
mod commands;
mod scheduler;
mod store;
mod tray;

use std::sync::{Arc, Mutex};

use commands::SessionStoreState;
use scheduler::{
    LiveTransport, RefreshInterval, SchedulerCore, SchedulerHandle, SystemClock, USAGE_STATE_EVENT,
    run_loop,
};
use store::{KeyringSessionStore, SessionStore};
use tauri::{Emitter, Manager};
use tokio::sync::Notify;

/// Build and run the app. Errors bubble to `main` instead of panicking so
/// the workspace-wide `clippy::expect_used` deny holds here too.
pub fn run() -> tauri::Result<()> {
    let session_store: Arc<dyn SessionStore> = Arc::new(KeyringSessionStore);
    let scheduler_store = Arc::clone(&session_store);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(SessionStoreState(session_store))
        .invoke_handler(tauri::generate_handler![
            commands::set_session_key,
            commands::session_status,
            commands::clear_session_key,
            commands::usage_state,
            commands::refresh_usage,
            commands::set_refresh_interval,
        ])
        .setup(move |app| {
            // Menu-bar-only on macOS: no Dock icon, no app switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::init(app.handle())?;
            spawn_scheduler(app, scheduler_store);
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

/// Seed the scheduler from the disk cache, expose its handle as managed
/// state and start the polling loop on Tauri's async runtime.
fn spawn_scheduler(app: &tauri::App, session_store: Arc<dyn SessionStore>) {
    let cache_path = app
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(cache::CACHE_FILE));
    let initial = cache_path.as_deref().and_then(cache::load);

    let core = Arc::new(Mutex::new(SchedulerCore::new(
        RefreshInterval::default(),
        initial,
    )));
    let handle = SchedulerHandle::new(core, Arc::new(Notify::new()));
    app.manage(handle.clone());

    let emitter = app.handle().clone();
    tauri::async_runtime::spawn(run_loop(
        LiveTransport::new(session_store),
        SystemClock::default(),
        handle,
        cache_path,
        move |state| {
            let _ = emitter.emit(USAGE_STATE_EVENT, &state);
        },
    ));
}
