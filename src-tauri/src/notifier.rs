//! Native notifications for threshold crossings and window resets (issue
//! #7).
//!
//! Wraps `meter_core::notify::NotificationTracker` — the pure dedup state
//! machine, unit-tested in `meter-core` — with the live settings that drive
//! it (warning/critical thresholds and the reset-notification opt-in, both
//! issue #6) and the OS-level `tauri-plugin-notification` call. Everything
//! here that *can* be tested without spinning up a Tauri app or an OS
//! notification centre — which windows get tracked, what a notification's
//! copy reads — is pure and covered below; [`apply_state`] itself is the
//! thin, untested glue that performs the actual OS call.
//!
//! Tap-to-open (issue #7's "activating the notification shows the
//! popover/window where the platform supports it") is **not implemented**:
//! `tauri-plugin-notification` 2.3.3's desktop backend (`notify_rust`
//! wrapping `mac-notification-sys` on macOS and the `org.freedesktop.Notifications`
//! D-Bus interface on Linux) fires the notification and returns — it exposes
//! no click/activation callback for the host app to hook. Implementing tap-
//! to-open would require bypassing the plugin for a lower-level integration
//! (native `UNUserNotificationCenter` delegate on macOS; the D-Bus
//! `ActionInvoked` signal on Linux), which needs a real desktop session to
//! verify and is out of scope here. This degrades gracefully: the
//! notification still fires, and the existing tray affordances (macOS
//! popover click, "Open Rusted Claude Meter" tray menu item on both
//! platforms) remain the way back into the app.

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

use meter_core::notify::{
    NotificationEvent, NotificationThresholds, NotificationTracker, ThresholdLevel, WindowId,
};
use meter_core::{LimitWindow, UsageSnapshot, UsageWindow};
use meter_render::round_percent;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;

use crate::scheduler::MeterState;
use crate::settings::SettingsState;

/// Managed Tauri state: the dedup tracker, alive for the process lifetime.
#[derive(Default)]
pub struct NotifierState(Mutex<NotificationTracker>);

impl NotifierState {
    fn lock(&self) -> MutexGuard<'_, NotificationTracker> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Live update path: fold one broadcast [`MeterState`] into the tracker and
/// fire a native notification for every event it returns.
///
/// A no-op without a snapshot, or before [`NotifierState`] / [`SettingsState`]
/// are managed yet (mirrors `tray::apply_state`'s defensive `try_state`
/// checks) — both are managed before the scheduler starts broadcasting, so
/// this only matters for a call that somehow arrives first.
pub fn apply_state<R: Runtime>(app: &AppHandle<R>, state: &MeterState) {
    let Some(snapshot) = &state.snapshot else {
        return;
    };
    let Some(notifier) = app.try_state::<NotifierState>() else {
        return;
    };
    let Some(settings) = app.try_state::<SettingsState>() else {
        return;
    };

    let settings = settings.get();
    let shown: HashSet<String> = settings.shown_scoped_models.iter().cloned().collect();
    let thresholds = NotificationThresholds {
        warning: settings.warning_threshold,
        critical: settings.critical_threshold,
    };

    let events = notifier.lock().observe(
        tracked_windows(snapshot, &shown),
        thresholds,
        settings.notify_on_reset,
        snapshot.fetched_at,
    );
    for event in &events {
        show(app, event);
    }
}

/// Every window this build notifies about: the headline windows (always),
/// plus each scoped model the user opted into showing (issue #6) that the
/// API currently reports active — the same gate `tray::model::menu_model`
/// uses for its usage lines, so "tracked" here means exactly what is
/// visible in the tray/popover (issue #7's scope: "headline or shown scoped
/// model").
fn tracked_windows<'a>(
    snapshot: &'a UsageSnapshot,
    shown: &'a HashSet<String>,
) -> impl Iterator<Item = (WindowId, &'a UsageWindow)> {
    snapshot
        .five_hour
        .iter()
        .map(|window| (WindowId::Headline(LimitWindow::FiveHour), window))
        .chain(
            snapshot
                .seven_day
                .iter()
                .map(|window| (WindowId::Headline(LimitWindow::SevenDay), window)),
        )
        .chain(
            snapshot
                .scoped
                .iter()
                .filter(move |limit| limit.is_visible(shown))
                .map(|limit| (WindowId::Scoped(limit.display_name.clone()), &limit.usage)),
        )
}

/// Fire the OS notification for one event. Best-effort: a failed send (no
/// notification daemon on a headless Linux session, permission denied,
/// etc.) is silently dropped — a missed notification must never crash or
/// otherwise disrupt polling.
fn show<R: Runtime>(app: &AppHandle<R>, event: &NotificationEvent) {
    let (title, body) = describe(event);
    let _ = emit(app, &title, &body);
}

/// The single OS-send chokepoint, shared by the scheduler-driven [`show`] and
/// the on-demand [`send_test_notification`]. Returns whether the platform
/// accepted the notification — `false` when the desktop backend rejects it
/// (no notification daemon on a headless Linux session, authorization denied
/// on macOS, etc.).
fn emit<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) -> bool {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .is_ok()
}

/// Fire a one-off test notification on demand, bypassing the dedup tracker
/// and the scheduler entirely, so the user can confirm from Settings that
/// banners actually reach them in their current environment (macOS Focus /
/// notification authorization, a Linux notification daemon, etc.) rather than
/// waiting for a real threshold crossing. Returns whether the OS accepted the
/// send so the Settings button can report success or a likely-suppressed
/// delivery.
///
/// `AppHandle` is taken by value: it is Tauri's command-extractor type, not a
/// choice this function makes.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn send_test_notification(app: tauri::AppHandle) -> bool {
    let (title, body) = describe_test();
    emit(&app, &title, &body)
}

/// Notification copy for one event. Pure and unit-tested separately from
/// the OS call above, which cannot run headlessly.
fn describe(event: &NotificationEvent) -> (String, String) {
    match event {
        NotificationEvent::ThresholdCrossed {
            window,
            level,
            utilization,
        } => {
            let name = window_label(window);
            let severity = match level {
                ThresholdLevel::Warning => "Warning",
                ThresholdLevel::Critical => "Critical",
            };
            (
                format!("{severity}: {name} usage"),
                format!(
                    "{name} is at {}% of its limit.",
                    round_percent(*utilization)
                ),
            )
        }
        NotificationEvent::WindowReset { window } => {
            let name = window_label(window);
            (
                format!("{name} limit reset"),
                format!("Your {name} usage window has reset."),
            )
        }
    }
}

/// Notification copy for the on-demand test notification (Settings' "Send
/// test notification"). Pure and unit-tested, like [`describe`], since the
/// OS send in [`send_test_notification`] cannot run headlessly.
fn describe_test() -> (String, String) {
    (
        "Rusted Claude Meter".to_owned(),
        "Test notification — if you can read this, notifications are working.".to_owned(),
    )
}

fn window_label(id: &WindowId) -> String {
    match id {
        WindowId::Headline(LimitWindow::FiveHour) => "5-hour".to_owned(),
        WindowId::Headline(LimitWindow::SevenDay) => "7-day".to_owned(),
        WindowId::Scoped(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use jiff::{SignedDuration, Timestamp};
    use meter_core::ScopedLimit;
    use pretty_assertions::assert_eq;

    fn now() -> Timestamp {
        "2026-07-17T12:00:00Z".parse().unwrap()
    }

    fn window(utilization: f64) -> UsageWindow {
        UsageWindow {
            utilization,
            resets_at: now() + SignedDuration::from_hours(1),
            window: LimitWindow::FiveHour,
        }
    }

    fn snapshot() -> UsageSnapshot {
        UsageSnapshot {
            five_hour: Some(window(42.0)),
            seven_day: Some(window(10.0)),
            scoped: vec![
                ScopedLimit {
                    display_name: "Fable".to_owned(),
                    model_id: None,
                    usage: window(55.0),
                    is_active: true,
                },
                ScopedLimit {
                    display_name: "Sonnet".to_owned(),
                    model_id: None,
                    usage: window(60.0),
                    is_active: false,
                },
                ScopedLimit {
                    display_name: "CodeOnly".to_owned(),
                    model_id: None,
                    usage: window(70.0),
                    is_active: true,
                },
            ],
            spend: None,
            fetched_at: now(),
        }
    }

    #[test]
    fn tracked_windows_always_includes_headline_windows() {
        let shown = HashSet::new();
        let ids: Vec<WindowId> = tracked_windows(&snapshot(), &shown)
            .map(|(id, _)| id)
            .collect();
        assert_eq!(
            ids,
            vec![
                WindowId::Headline(LimitWindow::FiveHour),
                WindowId::Headline(LimitWindow::SevenDay),
            ]
        );
    }

    #[test]
    fn tracked_windows_only_includes_shown_and_active_scoped_models() {
        // "Sonnet" is shown but not active, "CodeOnly" is active but not
        // shown, "Fable" is both — only Fable's scoped window is tracked.
        let shown: HashSet<String> = ["Fable", "Sonnet"].into_iter().map(String::from).collect();
        let ids: Vec<WindowId> = tracked_windows(&snapshot(), &shown)
            .map(|(id, _)| id)
            .collect();
        assert_eq!(
            ids,
            vec![
                WindowId::Headline(LimitWindow::FiveHour),
                WindowId::Headline(LimitWindow::SevenDay),
                WindowId::Scoped("Fable".to_owned()),
            ]
        );
    }

    #[test]
    fn describe_formats_threshold_crossings() {
        let event = NotificationEvent::ThresholdCrossed {
            window: WindowId::Headline(LimitWindow::FiveHour),
            level: ThresholdLevel::Critical,
            utilization: 91.4,
        };
        assert_eq!(
            describe(&event),
            (
                "Critical: 5-hour usage".to_owned(),
                "5-hour is at 91% of its limit.".to_owned(),
            )
        );
    }

    #[test]
    fn describe_formats_scoped_model_crossings_by_display_name() {
        let event = NotificationEvent::ThresholdCrossed {
            window: WindowId::Scoped("Fable".to_owned()),
            level: ThresholdLevel::Warning,
            utilization: 76.0,
        };
        assert_eq!(
            describe(&event),
            (
                "Warning: Fable usage".to_owned(),
                "Fable is at 76% of its limit.".to_owned(),
            )
        );
    }

    #[test]
    fn describe_test_reads_as_a_recognisable_test_banner() {
        let (title, body) = describe_test();
        assert_eq!(title, "Rusted Claude Meter");
        assert!(
            body.contains("Test notification"),
            "body should name itself a test: {body}"
        );
    }

    #[test]
    fn describe_formats_resets() {
        let event = NotificationEvent::WindowReset {
            window: WindowId::Headline(LimitWindow::SevenDay),
        };
        assert_eq!(
            describe(&event),
            (
                "7-day limit reset".to_owned(),
                "Your 7-day usage window has reset.".to_owned(),
            )
        );
    }
}
