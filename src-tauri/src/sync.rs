//! Shared locking helper.
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

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock `mutex`, recovering the data if a previous holder panicked.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
