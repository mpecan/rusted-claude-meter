//! Application shell: Tauri builder, tray wiring, window lifecycle and the
//! background polling loop.
//!
//! The app lives in the tray/menu bar. The main window starts hidden and is
//! only shown on demand; closing it hides it rather than quitting. A
//! scheduler task keeps a current usage snapshot available and broadcasts
//! every change as a `usage-state` event — the single source of truth the
//! tray and UI subscribe to.

mod api_base;
mod autostart;
mod browser_import;
mod cache;
mod commands;
mod consent;
#[cfg(feature = "browser-import")]
mod cookie_reader;
mod debug_log;
mod export;
mod io_util;
mod notifier;
mod scheduler;
mod settings;
mod settings_window;
mod signin;
mod source;
mod statusline;
mod store;
mod sync;
mod tray;
mod wizard;

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use commands::SessionStoreState;
use jiff::Timestamp;
use notifier::NotifierState;
use scheduler::{
    LiveTransport, PersistPaths, SchedulerCore, SchedulerHandle, SourcedTransport,
    StatuslineTransport, SystemClock, USAGE_STATE_EVENT, run_loop,
};
use settings::SettingsState;
use store::{KeyringSessionStore, SessionStore};
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::MacosLauncher;
use tokio::sync::Notify;

/// Handle `args` (argv **without** the program name) as a command-line
/// invocation, returning whether it was one.
///
/// `false` means "nothing CLI-shaped here" and the caller should launch the
/// GUI, so an ordinary double-click is unaffected. This is the crate's whole
/// CLI surface — deliberately one function rather than a public `statusline`
/// module, so the app shell's internals stay private.
#[must_use]
pub fn run_cli(args: &[String]) -> bool {
    let Some(invocation) = statusline::parse_args(args) else {
        return false;
    };
    statusline::execute(invocation);
    true
}

/// Build and run the app. Errors bubble to `main` instead of panicking so
/// the workspace-wide `clippy::expect_used` deny holds here too.
// Straight-line wiring: managed state, the (long) command handler list, and
// the setup closure. Splitting it would scatter the app's assembly for no
// real gain, so the length lint is allowed here.
#[allow(clippy::too_many_lines)]
pub fn run() -> tauri::Result<()> {
    // Announce a redirected API endpoint before anything else: a build pointed
    // at the demo harness is otherwise indistinguishable from a real one.
    api_base::log_override();
    let session_store: Arc<dyn SessionStore> = Arc::new(KeyringSessionStore);
    let scheduler_store = Arc::clone(&session_store);

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // `args: None` — the main window already starts hidden on every
        // launch (see the module docs on `autostart`), so autostart needs no
        // extra CLI flag to detect a login launch.
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None));

    // Host the `main` window inside a native NSPopover for the menu-bar
    // pop-down (arrow, slide animation, click-outside dismiss). macOS only;
    // on Linux the tray menu stays the primary surface.
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_plugin_nspopover::init());

    builder
        .manage(SessionStoreState(session_store))
        .invoke_handler(tauri::generate_handler![
            commands::set_session_key,
            commands::session_status,
            commands::clear_session_key,
            commands::browser::list_browser_sessions,
            commands::browser::import_browser_session,
            commands::usage_state,
            commands::refresh_usage,
            commands::get_settings,
            commands::set_refresh_interval,
            commands::set_icon_style,
            commands::icon_style_previews,
            commands::set_monochrome,
            commands::set_shown_scoped_models,
            commands::set_thresholds,
            commands::set_notify_on_reset,
            notifier::send_test_notification,
            commands::set_show_reset_time,
            commands::set_popover_layout,
            commands::set_usage_mode,
            commands::consent::set_tos_acknowledged,
            commands::source::set_usage_source,
            commands::source::statusline_command,
            commands::debug::set_debug_logging,
            commands::debug::debug_log_path,
            commands::debug::reveal_debug_log,
            commands::debug::api_base_override,
            commands::popover::set_popover_height,
            commands::pace::set_weekly_pace_days,
            commands::pace::set_pace_first_display,
            commands::pace::set_pace_tracking_enabled,
            autostart::autostart_status,
            autostart::set_autostart,
            settings_window::open_settings_window,
            wizard::wizard_should_run,
            wizard::wizard_mark_offered,
            wizard::wizard_complete,
            wizard::linux_desktop,
        ])
        .setup(move |app| {
            // Menu-bar-only on macOS: no Dock icon, no app switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

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
            // Captured before `settings::load` (which always returns
            // *something*, defaulted or not): "first run" per issue #11 is
            // "no settings.json existed yet", not "settings failed to
            // parse", so this must observe the file, not the load result.
            let settings_existed = settings_path.as_deref().is_some_and(Path::exists);
            let app_settings = settings_path
                .as_deref()
                .map_or_else(settings::AppSettings::default, settings::load);
            // Opt-in raw-API-response log (Settings' "Log API responses"). The
            // file lives in the OS log dir (`~/Library/Logs/<id>/` on macOS);
            // `None` when unresolvable, in which case logging is a no-op. Shared
            // between the command (which flips the toggle) and the scheduler's
            // transport (which writes through it).
            let response_log = Arc::new(debug_log::ResponseLog::new(
                app.path()
                    .app_log_dir()
                    .ok()
                    .map(|dir| dir.join(debug_log::LOG_FILE)),
                app_settings.debug_logging,
            ));
            app.manage(Arc::clone(&response_log));
            // The ToS consent gate (`crate::consent`), seeded from the
            // persisted answer — `false` for a fresh install *and* for one
            // upgrading from a build that predates the setting. Shared between
            // the command that flips it and the transport that checks it.
            let consent = Arc::new(consent::ConsentGate::new(app_settings.tos_acknowledged));
            app.manage(Arc::clone(&consent));
            // Which source the scheduler reads from (`crate::source`), seeded
            // from the persisted choice. Shared between the Settings command
            // that flips it and the transport that dispatches on it.
            let selection = Arc::new(source::selection(app_settings.usage_source));
            app.manage(Arc::clone(&selection));
            let mut core = SchedulerCore::new(
                app_settings.refresh_interval,
                cache_path.as_deref().and_then(cache::load),
            );
            // Start parked when consent is withheld, so the tray's very first
            // render already says "waiting for you to accept" instead of
            // showing a hopeful "polling" that the first tick immediately
            // contradicts. Only when claude.ai is the source: the status-line
            // source originates no request, so consent has no bearing on it
            // and parking there would strand a user who chose it *because*
            // they declined.
            if !consent.get() && !app_settings.usage_source.is_statusline() {
                core.record(scheduler::FetchOutcome::NotAcknowledged);
            }
            let shown: HashSet<String> = app_settings.shown_scoped_models.iter().cloned().collect();
            // Tray before scheduler: the tray must be managed before the
            // first state broadcast or early updates would be dropped.
            tray::init(
                app.handle(),
                &core.state(Timestamp::now()),
                tray::TraySeed {
                    style: app_settings.icon_style,
                    mono: app_settings.monochrome,
                    shown,
                    pace: tray::pace_options(&app_settings),
                    usage_mode: app_settings.usage_mode,
                },
            )?;
            // Host the main window in the NSPopover now that the tray (id
            // "main") exists — the plugin anchors to it and panics if it's
            // absent. Must run after `tray::init`. macOS-only (the NSPopover
            // plugin is); on Linux the main window stays a regular window.
            #[cfg(target_os = "macos")]
            configure_popover_window(app);
            app.manage(SettingsState::new(settings_path, app_settings));
            // Seeded `true` on a first run (no settings.json yet). Constructed
            // inline rather than through a `new` — see `FirstRunState`.
            app.manage(wizard::FirstRunState(sync::AtomicFlag::new(
                !settings_existed,
            )));
            // Managed before the scheduler starts broadcasting (mirrors the
            // tray init-before-scheduler ordering above), so the tracker's
            // very first observation establishes its startup baseline
            // instead of a broadcast racing ahead of it.
            app.manage(NotifierState::default());
            // Linux only: the main window is an ordinary resizable window
            // there, so content-fitting is a one-shot per show rather than
            // continuous. macOS has an NSPopover, which is always content-sized.
            #[cfg(target_os = "linux")]
            app.manage(commands::popover::ContentFit::default());
            // Read target for the Claude Code source, resolved once here for
            // the same reason `export_path` is: the bridge writes it under
            // `$HOME`, and the GUI has Tauri's path API to find it with.
            let statusline_path = app
                .path()
                .home_dir()
                .ok()
                .map(|dir| statusline::statusline_path(&dir));
            spawn_scheduler(
                app,
                SourcedTransport {
                    live: LiveTransport::with_base_url(scheduler_store, api_base::api_base_url())
                        .with_handles(scheduler::SharedHandles {
                            response_log,
                            consent,
                        }),
                    statusline: StatuslineTransport {
                        path: statusline_path,
                    },
                    selection,
                },
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

/// Window lifecycle differs by window. The popover/`main` window keeps the app
/// alive in the tray, so closing it hides it rather than quitting, and on macOS
/// it auto-hides on focus loss (popover feel). The Settings window is an
/// ordinary window the user dismisses explicitly: it really closes (and is
/// rebuilt on the next open), and on macOS its close drops the app back to
/// `Accessory` so it returns to menu-bar-only.
fn handle_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            if settings_window::is_settings_label(window.label()) {
                settings_window::set_app_foreground(window.app_handle(), false);
            } else {
                api.prevent_close();
                let _ = window.hide();
            }
        }
        // Only the popover auto-hides on focus loss; the Settings window stays
        // put until the user closes it. Route through `hide_popover` (not
        // `window.hide`) so the plugin's shown-state stays authoritative — a
        // raw hide would desync it and the next tray click would no-op.
        #[cfg(target_os = "macos")]
        tauri::WindowEvent::Focused(false) if settings_window::is_main_label(window.label()) => {
            use tauri_plugin_nspopover::AppExt as _;
            window.app_handle().hide_popover();
        }
        _ => {}
    }
}

/// macOS-only: host the main window inside a native `NSPopover` so a tray
/// click gives the menu-bar pop-down (arrow, slide animation) anchored under
/// the status item (see `tray::popover`). Must be called after the tray is
/// created — the plugin resolves the tray by id "main" and panics otherwise.
/// On Linux the window stays a regular decorated window and the tray menu is
/// the primary surface.
#[cfg(target_os = "macos")]
fn configure_popover_window(app: &tauri::App) {
    if let Some(window) = app.get_webview_window("main") {
        use tauri_plugin_nspopover::{ToPopoverOptions, WindowExt as _};
        window.to_popover(ToPopoverOptions {
            is_fullsize_content: true,
        });
    }
}

/// Expose the already-seeded scheduler core as managed state and start the
/// polling loop on Tauri's async runtime. The core arrives pre-loaded from
/// the disk cache (see `setup`) so the tray could render it immediately, and
/// the transport arrives fully assembled — which source it reads is `setup`'s
/// business, not this function's.
fn spawn_scheduler(
    app: &tauri::App,
    transport: SourcedTransport,
    core: SchedulerCore,
    persist: PersistPaths,
) {
    let core = Arc::new(Mutex::new(core));
    let handle = SchedulerHandle::new(core, Arc::new(Notify::new()));
    app.manage(handle.clone());

    let emitter = app.handle().clone();
    tauri::async_runtime::spawn(run_loop(
        transport,
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
