//! Tauri commands for session-key management.
//!
//! Each `#[tauri::command]` is a thin adapter over a directly unit-testable
//! function that talks to a `dyn SessionStore` — no `tauri::State`
//! construction required in tests — so parse-error and store-error mapping
//! is covered without spinning up the Tauri runtime.
//!
//! Two invariants every session command honors:
//!
//! * A pasted key is **validated against claude.ai** before it is allowed to
//!   stick, with rollback on rejection (`browser_import::store_and_validate`
//!   — the same guarantee browser import and the wizard give).
//! * Credential-store I/O never runs on the UI thread: the commands are
//!   `async` and route Keychain / Secret-Service calls through
//!   [`run_store_op`]'s blocking pool, so a slow or stuck credential daemon
//!   can never freeze tray or window redraws.

use std::collections::HashSet;
use std::sync::Arc;

use meter_core::{SessionKey, SessionKeyError, UsageStatus};
use meter_render::{IconState, IconStyle, Scale, render_icon};
use serde::Serialize;
use tauri::{Emitter, State};

use crate::browser_import::{
    LiveSessionValidator, SessionValidator, StoreAndValidateError, store_and_validate,
};
use crate::scheduler::{MeterState, RefreshInterval, SchedulerHandle};
use crate::settings::{AppSettings, SettingsState};
use crate::store::{SessionStore, StoreError, run_store_op};
use crate::tray;

/// Managed Tauri state wrapping the active [`SessionStore`].
pub struct SessionStoreState(pub Arc<dyn SessionStore>);

/// Broadcast to every window whenever settings that another window renders
/// change. Now that Settings lives in its own window (see
/// `crate::settings_window`), the popover — a separate window — can no longer
/// see a model-visibility toggle by sharing the same `settings` object, so it
/// subscribes to this to re-filter its cards live. Carries the full
/// [`AppSettings`] so any future cross-window setting can piggyback on it.
pub const SETTINGS_CHANGED_EVENT: &str = "settings-changed";

/// Errors returned to the frontend by session commands.
///
/// Carries only human-readable summaries — never the session key — and
/// distinguishes input-validation failures (fixable by the user re-typing)
/// from backend failures (fixable by, e.g., unlocking the Keychain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum SessionCommandError {
    /// The pasted input failed [`SessionKey::parse`].
    Validation(String),
    /// The key parsed but claude.ai rejected it (expired/invalid). The
    /// previously stored key, if any, has already been restored.
    Rejected(String),
    /// The credential-store backend failed.
    Store(String),
}

impl From<SessionKeyError> for SessionCommandError {
    fn from(error: SessionKeyError) -> Self {
        Self::Validation(error.to_string())
    }
}

impl From<StoreError> for SessionCommandError {
    fn from(error: StoreError) -> Self {
        Self::Store(error.to_string())
    }
}

/// Whether a session key is currently stored, without exposing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Present,
    Absent,
}

/// Outcome of a validated session-key submission. Shared by every entry
/// point that accepts a pasted key — the popover's inline field, the
/// Settings panel field, and the wizard's paste step all go through
/// [`set_session_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SessionSubmission {
    /// Whether claude.ai confirmed the key. `false` means the key is stored
    /// but validation was skipped because claude.ai was unreachable — the
    /// scheduler validates it on its next poll (mirrors
    /// `browser_import::ImportSummary::validated`).
    pub validated: bool,
}

/// Parse, persist and validate a pasted session key via
/// [`store_and_validate`], so a pasted key gets the exact same
/// rollback-on-rejection guarantee an imported one does (issues #10/#11): a
/// key claude.ai rejects never clobbers a previously working one.
async fn submit_session_key_impl(
    store: &Arc<dyn SessionStore>,
    validator: &impl SessionValidator,
    input: &str,
) -> Result<SessionSubmission, SessionCommandError> {
    let key = SessionKey::parse(input)?;
    let validated =
        store_and_validate(store, validator, &key)
            .await
            .map_err(|error| {
                match error {
            StoreAndValidateError::Store(message) => SessionCommandError::Store(message),
            StoreAndValidateError::Rejected => SessionCommandError::Rejected(
                "claude.ai rejected that session key — it may be expired. Sign in to claude.ai \
                 again, copy a fresh key, and try again."
                    .to_owned(),
            ),
        }
            })?;
    Ok(SessionSubmission { validated })
}

fn session_status_impl(store: &dyn SessionStore) -> Result<SessionStatus, StoreError> {
    Ok(if store.load()?.is_some() {
        SessionStatus::Present
    } else {
        SessionStatus::Absent
    })
}

/// Parse, store and **validate** a pasted session key (raw `sk-ant-...`
/// value or a full `Cookie` header containing `sessionKey=...`) against
/// claude.ai, rolling back to the previously stored key if claude.ai
/// rejects it, then wake the polling loop so a scheduler parked on "session
/// expired" retries with the new key.
///
/// `State` and `String` are required by value here: they are Tauri's
/// command-extractor types, not a choice this function makes.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub async fn set_session_key(
    state: State<'_, SessionStoreState>,
    scheduler: State<'_, SchedulerHandle>,
    input: String,
) -> Result<SessionSubmission, SessionCommandError> {
    // Owned handles, so nothing borrowed from the `State` guards is held
    // across the await (mirrors `import_browser_session`).
    let store = Arc::clone(&state.0);
    let scheduler = (*scheduler).clone();

    let submission = submit_session_key_impl(&store, &LiveSessionValidator::new(), &input).await?;
    scheduler.resume_polling();
    Ok(submission)
}

/// Report whether a session key is currently stored. Async and routed
/// through the blocking pool: the credential-store round trip must never
/// run on the UI thread (see [`run_store_op`]).
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub async fn session_status(
    state: State<'_, SessionStoreState>,
) -> Result<SessionStatus, SessionCommandError> {
    Ok(run_store_op(&state.0, session_status_impl).await?)
}

/// Remove the stored session key and tell the scheduler directly, so the
/// broadcast state flips to "awaiting session" immediately instead of on
/// the next scheduled tick.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub async fn clear_session_key(
    state: State<'_, SessionStoreState>,
    scheduler: State<'_, SchedulerHandle>,
) -> Result<(), SessionCommandError> {
    run_store_op(&state.0, |store| store.clear()).await?;
    scheduler.mark_no_session();
    Ok(())
}

/// Current scheduler state, for the initial render before the first
/// `usage-state` event arrives.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn usage_state(scheduler: State<'_, SchedulerHandle>) -> MeterState {
    scheduler.state_now()
}

/// Ask for a refresh now. TTL-guarded: a snapshot younger than ~55s is
/// served from memory instead of re-hitting the API.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn refresh_usage(scheduler: State<'_, SchedulerHandle>) {
    scheduler.request_refresh();
}

/// Change the polling cadence (60 / 300 / 600 seconds) and persist the
/// choice (Settings, issue #6) so it survives a restart.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_refresh_interval(
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    interval: RefreshInterval,
) {
    scheduler.set_interval(interval);
    settings.update(|s| s.refresh_interval = interval);
}

/// Change the tray icon style (Settings, issue #9) and apply it
/// immediately, so switching styles never needs a restart. Persisted
/// (Settings, issue #6) so it survives a restart too.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_icon_style(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    style: IconStyle,
) {
    settings.update(|s| s.icon_style = style);
    tray::set_style(&app, style, &scheduler.state_now());
}

/// One rendered preview for the Settings/wizard icon-style picker: straight
/// RGBA the frontend paints into a `<canvas>` so the buttons show the actual
/// tray artwork (issue #9's visual picker, mirroring `ClaudeMeter`'s
/// `IconStylePicker`).
#[derive(Serialize)]
pub struct IconPreview {
    pub style: IconStyle,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Render every icon style at one representative sample state (65% session /
/// 45% weekly, warning) for the picker. Always rendered in colour (a
/// template/monochrome preview would be black-on-dark and invisible in the
/// picker) — the picker communicates *shape*, mirroring `ClaudeMeter`'s
/// coloured `IconStylePicker`. Styles that fail to render are omitted rather
/// than erroring the whole picker.
#[tauri::command]
pub fn icon_style_previews() -> Vec<IconPreview> {
    const SAMPLE_PRIMARY: u8 = 65;
    const SAMPLE_SECONDARY: u8 = 45;
    const STYLES: [IconStyle; 6] = [
        IconStyle::Battery,
        IconStyle::Circular,
        IconStyle::Minimal,
        IconStyle::Segments,
        IconStyle::DualBar,
        IconStyle::Gauge,
    ];
    STYLES
        .into_iter()
        .filter_map(|style| {
            let state = IconState {
                style,
                percent: SAMPLE_PRIMARY,
                secondary_percent: SAMPLE_SECONDARY,
                status: UsageStatus::Warning,
                at_risk: false,
                mono: false,
                scale: Scale::X2,
            };
            let icon = render_icon(&state).ok()?;
            Some(IconPreview {
                style,
                width: icon.width,
                height: icon.height,
                rgba: icon.rgba,
            })
        })
        .collect()
}

/// The current settings, for the Settings panel's initial render.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn get_settings(settings: State<'_, SettingsState>) -> AppSettings {
    settings.get()
}

/// Toggle the tray/popover between template (monochrome) and full-colour
/// icon artwork, and apply it immediately (Settings, issue #6). Persisted so
/// it survives a restart.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_monochrome(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    monochrome: bool,
) {
    settings.update(|s| s.monochrome = monochrome);
    tray::set_mono(&app, monochrome, &scheduler.state_now());
}

/// Replace the opt-in set of scoped-model display names the popover and
/// Linux tray menu are allowed to show (Settings, issue #6), and apply it
/// immediately. `models` need not be deduplicated — the tray only ever reads
/// it as a set.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_shown_scoped_models(
    app: tauri::AppHandle,
    scheduler: State<'_, SchedulerHandle>,
    settings: State<'_, SettingsState>,
    models: Vec<String>,
) {
    let updated = settings.update(|s| s.shown_scoped_models = models);
    // Tell the popover window (which filters its cards by this set) before we
    // consume `updated` for the tray update below.
    let _ = app.emit(SETTINGS_CHANGED_EVENT, &updated);
    let shown: HashSet<String> = updated.shown_scoped_models.into_iter().collect();
    tray::set_shown_scoped_models(&app, shown, &scheduler.state_now());
}

/// Update the warning/critical notification thresholds (Settings, issue #6;
/// consumed by notifications, issue #7). Both are clamped to `0..=100` by
/// the settings store; the resolved values are returned so the frontend's
/// sliders can reflect the clamp.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_thresholds(
    settings: State<'_, SettingsState>,
    warning: f64,
    critical: f64,
) -> AppSettings {
    settings.update(|s| {
        s.warning_threshold = warning;
        s.critical_threshold = critical;
    })
}

/// Toggle the extra "limit reset" notification (issue #7) on or off.
/// Threshold-crossing notifications are always on; this only gates the
/// noisier reset notice.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_notify_on_reset(settings: State<'_, SettingsState>, enabled: bool) -> AppSettings {
    store_notify_on_reset(&settings, enabled)
}

/// Persist the notify-on-reset toggle. Split from the command so the settings
/// mutation is unit-testable without a Tauri runtime.
fn store_notify_on_reset(settings: &SettingsState, enabled: bool) -> AppSettings {
    settings.update(|s| s.notify_on_reset = enabled)
}

/// Toggle whether popover cards append the exact reset wall-clock time
/// (`ClaudeMeter` PR #26). Emits `settings-changed` so the popover window
/// re-renders its cards immediately, since the toggle lives in the separate
/// Settings window.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_show_reset_time(
    app: tauri::AppHandle,
    settings: State<'_, SettingsState>,
    enabled: bool,
) -> AppSettings {
    let updated = store_show_reset_time(&settings, enabled);
    let _ = app.emit(SETTINGS_CHANGED_EVENT, &updated);
    updated
}

/// Persist the show-reset-time toggle. Split from the command so the settings
/// mutation is unit-testable without a Tauri `AppHandle`.
fn store_show_reset_time(settings: &SettingsState, enabled: bool) -> AppSettings {
    settings.update(|s| s.show_reset_time = enabled)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::browser_import::ValidationError;
    use crate::store::FakeSessionStore;
    use pretty_assertions::assert_eq;

    const VALID: &str = "sk-ant-sid01-abcDEF123456_-xyz789";

    /// A validator returning a fixed verdict without any network.
    struct FakeValidator(Result<(), ValidationError>);

    impl SessionValidator for FakeValidator {
        async fn validate<'a>(&'a self, _key: &'a SessionKey) -> Result<(), ValidationError> {
            self.0
        }
    }

    fn ok_validator() -> FakeValidator {
        FakeValidator(Ok(()))
    }

    fn empty_store() -> Arc<dyn SessionStore> {
        Arc::new(FakeSessionStore::new())
    }

    #[test]
    fn show_reset_time_toggle_persists_both_ways() {
        let state = SettingsState::new(None, AppSettings::default());
        // Default is on; toggling off then on round-trips through the store.
        assert!(!store_show_reset_time(&state, false).show_reset_time);
        assert!(store_show_reset_time(&state, true).show_reset_time);
    }

    #[test]
    fn notify_on_reset_toggle_persists_both_ways() {
        let state = SettingsState::new(None, AppSettings::default());
        assert!(store_notify_on_reset(&state, true).notify_on_reset);
        assert!(!store_notify_on_reset(&state, false).notify_on_reset);
    }

    #[test]
    fn icon_style_previews_renders_every_style_with_pixels() {
        let previews = icon_style_previews();
        // Every one of the six styles renders — none dropped.
        assert_eq!(previews.len(), 6);
        for preview in &previews {
            assert_eq!(
                preview.rgba.len(),
                (preview.width * preview.height * 4) as usize,
                "{:?} rgba length must match its dimensions",
                preview.style
            );
            assert!(
                preview.rgba.iter().any(|&b| b != 0),
                "{:?} preview must contain visible pixels",
                preview.style
            );
            // Sample state renders at 2x (44px tall) and wider than tall for
            // the text styles, so the picker shows real artwork.
            assert_eq!(preview.height, 44);
        }
    }

    async fn submit(
        store: &Arc<dyn SessionStore>,
        validator: &FakeValidator,
        input: &str,
    ) -> Result<SessionSubmission, SessionCommandError> {
        submit_session_key_impl(store, validator, input).await
    }

    #[tokio::test]
    async fn set_session_key_rejects_invalid_input_without_touching_the_store() {
        let store = empty_store();
        let result = submit(&store, &ok_validator(), "not-a-key").await;
        assert_eq!(
            result,
            Err(SessionCommandError::Validation(
                SessionKeyError::MissingPrefix.to_string()
            ))
        );
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn set_session_key_maps_every_parse_error_variant() {
        let store = empty_store();
        assert_eq!(
            submit(&store, &ok_validator(), "").await,
            Err(SessionCommandError::Validation(
                SessionKeyError::Empty.to_string()
            ))
        );
        assert_eq!(
            submit(&store, &ok_validator(), "sk-ant-short").await,
            Err(SessionCommandError::Validation(
                SessionKeyError::TooShort.to_string()
            ))
        );
        assert_eq!(
            submit(&store, &ok_validator(), "sk-ant-sid01-abc DEF123456789").await,
            Err(SessionCommandError::Validation(
                SessionKeyError::InvalidCharacters.to_string()
            ))
        );
    }

    #[tokio::test]
    async fn set_session_key_stores_a_confirmed_key_and_reports_validated() {
        let store = empty_store();
        let submission = submit(&store, &ok_validator(), VALID).await.unwrap();
        assert!(submission.validated);
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }

    #[tokio::test]
    async fn set_session_key_keeps_an_unconfirmed_key_on_a_transient_failure() {
        let store = empty_store();
        let submission = submit(
            &store,
            &FakeValidator(Err(ValidationError::Transient)),
            VALID,
        )
        .await
        .unwrap();
        assert!(!submission.validated);
        // Kept, because the failure might be a network blip, not a bad key.
        assert_eq!(store.load().unwrap().unwrap().expose(), VALID);
    }

    #[tokio::test]
    async fn set_session_key_rolls_back_a_rejected_key() {
        let store = empty_store();
        let error = submit(
            &store,
            &FakeValidator(Err(ValidationError::Unauthorized)),
            VALID,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, SessionCommandError::Rejected(_)));
        // The rejected key must not linger.
        assert_eq!(store.load().unwrap(), None);
    }

    #[tokio::test]
    async fn set_session_key_restores_the_previously_stored_key_on_rejection() {
        let previous = SessionKey::parse("sk-ant-sid01-previousKEY_123-456789").unwrap();
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::with_key(previous.clone()));
        let error = submit(
            &store,
            &FakeValidator(Err(ValidationError::Unauthorized)),
            VALID,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, SessionCommandError::Rejected(_)));
        // A bad paste must never clobber a working key.
        assert_eq!(store.load().unwrap(), Some(previous));
    }

    #[tokio::test]
    async fn set_session_key_never_leaks_the_raw_value_in_its_error() {
        // A valid-looking prefix but invalid body triggers a validation
        // error; assert the offending raw text never round-trips through it.
        let bad = "sk-ant-sid01-abc DEF123456789";
        let error = submit(&empty_store(), &ok_validator(), bad)
            .await
            .unwrap_err();
        assert!(!format!("{error:?}").contains("abc DEF"));
    }

    #[tokio::test]
    async fn set_session_key_surfaces_store_errors() {
        let store: Arc<dyn SessionStore> = Arc::new(FakeSessionStore::unavailable());
        let result = submit(&store, &ok_validator(), VALID).await;
        assert!(matches!(result, Err(SessionCommandError::Store(_))));
    }

    #[test]
    fn session_status_reports_absent_then_present() {
        let store = FakeSessionStore::new();
        assert_eq!(session_status_impl(&store).unwrap(), SessionStatus::Absent);
        store.save(&SessionKey::parse(VALID).unwrap()).unwrap();
        assert_eq!(session_status_impl(&store).unwrap(), SessionStatus::Present);
    }

    #[test]
    fn session_status_surfaces_store_errors() {
        let store = FakeSessionStore::unavailable();
        assert!(matches!(
            session_status_impl(&store),
            Err(StoreError::Unavailable(_))
        ));
    }

    // `clear_session_key` delegates straight to `SessionStore::clear` via
    // `run_store_op`; clearing behaviour (including idempotence and backend
    // errors) is pinned by `store.rs`'s own tests.

    #[test]
    fn command_error_serializes_with_a_discriminant_tag() {
        let error = SessionCommandError::Validation("session key is empty".to_owned());
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["kind"], "Validation");
        assert_eq!(json["message"], "session key is empty");

        let rejected = SessionCommandError::Rejected("rejected".to_owned());
        let json = serde_json::to_value(&rejected).unwrap();
        assert_eq!(json["kind"], "Rejected");
    }

    #[test]
    fn session_submission_serializes_the_validated_flag() {
        let json = serde_json::to_value(SessionSubmission { validated: false }).unwrap();
        assert_eq!(json["validated"], false);
    }
}
