//! Launch-at-login (issue #12): a settings toggle backed by
//! `tauri-plugin-autostart`, which registers a macOS Launch Agent or an XDG
//! `.desktop` autostart entry on Linux.
//!
//! Deliberately *not* mirrored into `settings::AppSettings`: the OS-level
//! registration (the plist / `.desktop` file) is itself the persisted state
//! and can be flipped from outside the app (System Settings on macOS, or a
//! user editing/removing the autostart entry on Linux). Caching our own copy
//! in `settings.json` would let the two drift; instead every read asks the
//! plugin for ground truth, same discipline as `commands::session_status`
//! asking the keyring instead of trusting a cached flag.
//!
//! The main window already starts hidden regardless of how the app was
//! launched (`tauri.conf.json`'s `"visible": false`, only flipped by an
//! explicit tray click — see `tray::mod::show_main_window`), so "start
//! hidden when launched at login" needs no extra code here.

use tauri_plugin_autostart::ManagerExt;

/// Thin seam over `tauri_plugin_autostart::AutoLaunchManager` so the
/// round-trip logic below is unit-testable without touching the real OS
/// registration a test run would otherwise create.
pub trait AutostartController: Send + Sync {
    fn enable(&self) -> Result<(), String>;
    fn disable(&self) -> Result<(), String>;
    fn is_enabled(&self) -> Result<bool, String>;
}

impl AutostartController for tauri_plugin_autostart::AutoLaunchManager {
    fn enable(&self) -> Result<(), String> {
        self.enable().map_err(|error| error.to_string())
    }

    fn disable(&self) -> Result<(), String> {
        self.disable().map_err(|error| error.to_string())
    }

    fn is_enabled(&self) -> Result<bool, String> {
        self.is_enabled().map_err(|error| error.to_string())
    }
}

fn autostart_status_impl(controller: &dyn AutostartController) -> Result<bool, String> {
    controller.is_enabled()
}

fn set_autostart_impl(controller: &dyn AutostartController, enabled: bool) -> Result<bool, String> {
    if enabled {
        controller.enable()?;
    } else {
        controller.disable()?;
    }
    // Re-read rather than trust `enabled`: the OS call is what's real, and
    // the frontend uses the returned value to reconcile the checkbox.
    //
    // But a failure here is a *read* failure, not a *write* failure — the
    // enable()/disable() above already succeeded, so it must not be folded
    // into the same `Err`. The frontend treats an `Err` as "the toggle
    // failed" and reverts the checkbox to the opposite of `enabled`, which
    // would then contradict the OS registration this call just successfully
    // changed. Fall back to the requested value (known-good) and log the
    // read failure instead of surfacing it as if the write itself failed.
    controller.is_enabled().or_else(|error| {
        eprintln!("autostart: write succeeded but confirmatory read failed: {error}");
        Ok(enabled)
    })
}

/// Whether launch-at-login is currently registered with the OS, for the
/// Settings panel's toggle — queried fresh every time it's asked, never
/// cached, since the registration can change outside the app.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn autostart_status(app: tauri::AppHandle) -> Result<bool, String> {
    autostart_status_impl(&*app.autolaunch())
}

/// Enable or disable launch-at-login and report back the resulting
/// registration state (not just an echo of `enabled`), so the toggle can
/// reconcile itself against whatever actually happened.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    set_autostart_impl(&*app.autolaunch(), enabled)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::Mutex;

    /// In-memory stand-in for the real `AutoLaunchManager`: exercises the
    /// toggle round-trip and error propagation without writing a Launch
    /// Agent plist or an XDG autostart entry to the test-runner's real
    /// filesystem.
    struct FakeController {
        enabled: Mutex<bool>,
        fail: bool,
        fail_read: bool,
    }

    impl FakeController {
        fn new(initial: bool) -> Self {
            Self {
                enabled: Mutex::new(initial),
                fail: false,
                fail_read: false,
            }
        }

        fn failing() -> Self {
            Self {
                enabled: Mutex::new(false),
                fail: true,
                fail_read: false,
            }
        }

        /// `enable()`/`disable()` succeed and actually flip the backing
        /// state, but the confirmatory `is_enabled()` read fails — the
        /// scenario `set_autostart_impl` must not fold into a single `Err`.
        fn read_fails(initial: bool) -> Self {
            Self {
                enabled: Mutex::new(initial),
                fail: false,
                fail_read: true,
            }
        }
    }

    impl AutostartController for FakeController {
        fn enable(&self) -> Result<(), String> {
            if self.fail {
                return Err("boom".to_owned());
            }
            *self.enabled.lock().unwrap() = true;
            Ok(())
        }

        fn disable(&self) -> Result<(), String> {
            if self.fail {
                return Err("boom".to_owned());
            }
            *self.enabled.lock().unwrap() = false;
            Ok(())
        }

        fn is_enabled(&self) -> Result<bool, String> {
            if self.fail || self.fail_read {
                return Err("boom".to_owned());
            }
            Ok(*self.enabled.lock().unwrap())
        }
    }

    #[test]
    fn status_reflects_the_controller() {
        assert_eq!(autostart_status_impl(&FakeController::new(true)), Ok(true));
        assert_eq!(
            autostart_status_impl(&FakeController::new(false)),
            Ok(false)
        );
    }

    #[test]
    fn set_autostart_enables_and_reports_back() {
        let controller = FakeController::new(false);
        assert_eq!(set_autostart_impl(&controller, true), Ok(true));
    }

    #[test]
    fn set_autostart_disables_and_reports_back() {
        let controller = FakeController::new(true);
        assert_eq!(set_autostart_impl(&controller, false), Ok(false));
    }

    #[test]
    fn set_autostart_round_trips_on_and_off() {
        let controller = FakeController::new(false);
        assert_eq!(set_autostart_impl(&controller, true), Ok(true));
        assert_eq!(autostart_status_impl(&controller), Ok(true));
        assert_eq!(set_autostart_impl(&controller, false), Ok(false));
        assert_eq!(autostart_status_impl(&controller), Ok(false));
    }

    #[test]
    fn set_autostart_is_idempotent() {
        let controller = FakeController::new(true);
        assert_eq!(set_autostart_impl(&controller, true), Ok(true));
        assert_eq!(set_autostart_impl(&controller, true), Ok(true));
    }

    #[test]
    fn status_surfaces_controller_errors() {
        assert_eq!(
            autostart_status_impl(&FakeController::failing()),
            Err("boom".to_owned())
        );
    }

    #[test]
    fn set_autostart_surfaces_controller_errors() {
        assert_eq!(
            set_autostart_impl(&FakeController::failing(), true),
            Err("boom".to_owned())
        );
    }

    #[test]
    fn set_autostart_falls_back_to_requested_value_when_confirmatory_read_fails() {
        let controller = FakeController::read_fails(false);
        // The write succeeded (the fake's backing state actually flipped);
        // only the read-back afterward fails. That must not surface as an
        // `Err`, which the frontend would treat as "the write failed" and
        // revert the checkbox to contradict the registration this call just
        // made.
        assert_eq!(set_autostart_impl(&controller, true), Ok(true));
        assert!(*controller.enabled.lock().unwrap());
    }
}
