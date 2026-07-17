//! Application shell: Tauri builder, tray wiring, window lifecycle and the
//! background polling loop.
//!
//! The app lives in the tray/menu bar. The main window starts hidden and is
//! only shown on demand; closing it hides it rather than quitting. A
//! scheduler task keeps a current usage snapshot available and broadcasts
//! every change as a `usage-state` event — the single source of truth the
//! tray and UI subscribe to.

mod browser_import;
mod cache;
mod commands;
mod export;
mod io_util;
mod notifier;
mod scheduler;
mod settings;
mod store;
mod tray;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use commands::SessionStoreState;
use jiff::Timestamp;
use notifier::NotifierState;
use scheduler::{
    LiveTransport, PersistPaths, SchedulerCore, SchedulerHandle, SystemClock, USAGE_STATE_EVENT,
    run_loop,
};
use settings::SettingsState;
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
        .plugin(tauri_plugin_notification::init())
        .manage(SessionStoreState(session_store))
        .invoke_handler(tauri::generate_handler![
            commands::set_session_key,
            commands::session_status,
            commands::clear_session_key,
            browser_import::list_browser_sessions,
            browser_import::import_browser_session,
            commands::usage_state,
            commands::refresh_usage,
            commands::get_settings,
            commands::set_refresh_interval,
            commands::set_icon_style,
            commands::set_monochrome,
            commands::set_shown_scoped_models,
            commands::set_thresholds,
            commands::set_notify_on_reset,
        ])
        .setup(move |app| {
            // Menu-bar-only on macOS: no Dock icon, no app switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            configure_popover_window(app);
            let data_dir = app.path().app_data_dir().ok();
            // The disk cache is loaded before the tray is wired so the very
            // first icon reflects any restored snapshot instead of flashing
            // the empty placeholder until the scheduler's first broadcast.
            let cache_path = data_dir.as_ref().map(|dir| dir.join(cache::CACHE_FILE));
            // `~/.claudemeter/usage.json` (issue #8), not the app data dir:
            // the path is shared with the Swift ClaudeMeter app on purpose.
            // `None` when the home dir can't be resolved — the export is a
            // best-effort convenience for external tools, never load-bearing
            // for the app itself.
            let export_path = app
                .path()
                .home_dir()
                .ok()
                .map(|dir| export::export_path(&dir));
            let settings_path = data_dir.map(|dir| dir.join(settings::SETTINGS_FILE));
            let app_settings = settings_path
                .as_deref()
                .map_or_else(settings::AppSettings::default, settings::load);
            let core = SchedulerCore::new(
                app_settings.refresh_interval,
                cache_path.as_deref().and_then(cache::load),
            );
            let shown: HashSet<String> = app_settings.shown_scoped_models.iter().cloned().collect();
            // Tray before scheduler: the tray must be managed before the
            // first state broadcast or early updates would be dropped.
            tray::init(
                app.handle(),
                &core.state(Timestamp::now()),
                app_settings.icon_style,
                app_settings.monochrome,
                shown,
            )?;
            app.manage(SettingsState::new(settings_path, app_settings));
            // Managed before the scheduler starts broadcasting (mirrors the
            // tray init-before-scheduler ordering above), so the tracker's
            // very first observation establishes its startup baseline
            // instead of a broadcast racing ahead of it.
            app.manage(NotifierState::default());
            spawn_scheduler(
                app,
                scheduler_store,
                core,
                PersistPaths {
                    cache: cache_path,
                    export: export_path,
                },
            );
            Ok(())
        })
        .on_window_event(handle_window_event)
        .run(tauri::generate_context!())
}

/// The app keeps running in the tray, so closing the window hides it rather
/// than quitting; on macOS the popover-style window also auto-hides when it
/// loses focus. Linux keeps a normal window — the tray menu is the primary
/// surface there.
fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = window.hide();
        }
        #[cfg(target_os = "macos")]
        tauri::WindowEvent::Focused(false) => {
            let _ = window.hide();
        }
        _ => {}
    }
}

/// macOS-only: style the main window as a popover — frameless and always on
/// top, anchored under the tray icon on click (see `tray::popover`). On
/// Linux the window stays a regular decorated window.
fn configure_popover_window(app: &tauri::App) {
    #[cfg(target_os = "macos")]
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_decorations(false);
        let _ = window.set_always_on_top(true);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Expose the already-seeded scheduler core as managed state and start the
/// polling loop on Tauri's async runtime. The core arrives pre-loaded from
/// the disk cache (see `setup`) so the tray could render it immediately.
fn spawn_scheduler(
    app: &tauri::App,
    session_store: Arc<dyn SessionStore>,
    core: SchedulerCore,
    persist: PersistPaths,
) {
    let core = Arc::new(Mutex::new(core));
    let handle = SchedulerHandle::new(core, Arc::new(Notify::new()));
    app.manage(handle.clone());

    let emitter = app.handle().clone();
    tauri::async_runtime::spawn(run_loop(
        LiveTransport::new(session_store),
        SystemClock::default(),
        handle,
        persist,
        move |state| {
            let _ = emitter.emit(USAGE_STATE_EVENT, &state);
            // The tray subscribes to the same broadcast: icon and menu
            // reflect every state change within the same tick.
            tray::apply_state(&emitter, &state);
            // So does the notifier (issue #7): threshold crossings and
            // resets are decided from the very same state, one tick behind
            // nothing.
            notifier::apply_state(&emitter, &state);
        },
    ));
}
