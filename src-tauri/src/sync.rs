//! Shared concurrency helpers: the poison-tolerant [`lock`], and
//! [`AtomicFlag`] for the app's shared on/off switches.
//!
//! Every mutex in the app wants the same thing on poisoning: carry on with the
//! data anyway. A poisoned lock here means some other thread panicked while
//! holding it, and none of this state is a safety invariant — the tray's
//! rendered strings, the scheduler's snapshot, the notifier's thresholds. The
//! alternative, propagating the poison, would turn one unrelated panic into a
//! permanently dead tray.
//!
//! Written once because `unwrap_or_else(PoisonError::into_inner)` is easy to
//! get subtly wrong (`unwrap` is denied workspace-wide precisely so this
//! decision is explicit), and because the same three lines repeated per module
//! is exactly what the duplication ratchet exists to catch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock `mutex`, recovering the data if a previous holder panicked.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A shared boolean that any thread may read or flip.
///
/// The app has three of these — the Terms-of-Service consent gate
/// (`crate::consent`), the debug-response-log switch (`crate::debug_log`) and
/// the first-run wizard's consume-once flag (`crate::wizard`) — and each was
/// its own hand-rolled `AtomicBool` with the same two `Relaxed` accessors,
/// which is precisely the repetition the duplication ratchet exists to catch.
/// Written once here for the same reason [`lock`] is.
///
/// `Relaxed` throughout: every user is a standalone switch whose value is read
/// on its own, never as a release/acquire signal publishing other writes.
#[derive(Debug, Default)]
pub struct AtomicFlag(AtomicBool);

impl AtomicFlag {
    #[must_use]
    pub const fn new(value: bool) -> Self {
        Self(AtomicBool::new(value))
    }

    /// The current value.
    pub fn get(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Set the value; visible to every other holder from the next [`Self::get`].
    pub fn set(&self, value: bool) {
        self.0.store(value, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    // Poisoning a mutex is the point of one of these tests, and the only way
    // to poison one is to panic while holding it.
    #![allow(clippy::panic)]

    use super::lock;
    use std::sync::{Arc, Mutex};

    #[test]
    fn an_uncontended_lock_yields_the_value() {
        let cell = Mutex::new(7);
        assert_eq!(*lock(&cell), 7);
    }

    #[test]
    fn a_poisoned_lock_still_yields_the_value() {
        // The whole reason this helper exists: a panic in one thread must not
        // permanently wedge everything else that shares the mutex.
        let cell = Arc::new(Mutex::new(7));
        let poisoner = Arc::clone(&cell);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock();
            panic!("poison it");
        })
        .join();

        assert!(cell.is_poisoned(), "expected the mutex to be poisoned");
        assert_eq!(*lock(&cell), 7);
    }

    #[test]
    fn writes_through_the_guard_are_visible() {
        let cell = Mutex::new(1);
        *lock(&cell) = 2;
        assert_eq!(*lock(&cell), 2);
    }
}

#[cfg(test)]
mod flag_tests {
    use super::AtomicFlag;

    #[test]
    fn a_flag_starts_where_it_was_built() {
        assert!(AtomicFlag::new(true).get());
        assert!(!AtomicFlag::new(false).get());
    }

    #[test]
    fn the_default_flag_is_off() {
        // Every current user wants "off" as the safe resting state — most
        // importantly the consent gate, where on-by-default would mean
        // contacting claude.ai without being asked.
        assert!(!AtomicFlag::default().get());
    }

    #[test]
    fn a_flag_moves_both_ways() {
        let flag = AtomicFlag::new(false);
        flag.set(true);
        assert!(flag.get());
        flag.set(false);
        assert!(!flag.get());
    }

    #[test]
    fn a_flag_is_shared_through_a_reference() {
        // The property every caller relies on: flipping it through one handle
        // is observed through another, with no rebuild.
        let flag = std::sync::Arc::new(AtomicFlag::new(false));
        let other = std::sync::Arc::clone(&flag);
        flag.set(true);
        assert!(other.get());
    }
}
