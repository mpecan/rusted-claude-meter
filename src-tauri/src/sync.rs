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

use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock `mutex`, recovering the data if a previous holder panicked.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A shared boolean that any thread may read or flip, carrying the identity of
/// *which* switch it is in its type.
///
/// The app has four of these — the Terms-of-Service consent gate
/// (`crate::consent`), the usage-source selection (`crate::source`), the
/// debug-response-log switch (`crate::debug_log`) and the first-run wizard's
/// consume-once flag (`crate::wizard`) — and each was its own hand-rolled
/// `AtomicBool` with the same two `Relaxed` accessors, which is precisely the
/// repetition the duplication ratchet exists to catch. Written once here for
/// the same reason [`lock`] is.
///
/// **`Tag` is what makes two switches two types, and it is mandatory for that
/// reason (issue #86).** Tauri's managed state is a map keyed by `TypeId`, and
/// `Manager::manage` *silently ignores* a registration of a type it already
/// holds — it returns `false` rather than erroring, and every call site in the
/// app discards that. So while the consent gate and the source selection were
/// both a plain `AtomicFlag`, `app.manage` kept the first and dropped the
/// second, and both `State<'_, Arc<ConsentGate>>` and
/// `State<'_, Arc<SourceSelection>>` resolved to the *same* flag: accepting the
/// Terms of Service switched the usage source to Claude Code, and choosing
/// claude.ai withdrew consent, so no session key could ever be stored. A
/// phantom tag costs nothing at runtime and makes that unrepresentable, which
/// is worth more than the two characters it costs at each use — a defaulted tag
/// would leave the trap one `pub type NewSwitch = AtomicFlag;` away.
///
/// `fn() -> Tag` rather than `Tag`, so the flag stays `Send + Sync` (and
/// covariant) whatever the tag is — tags are markers, never constructed.
///
/// `Relaxed` throughout: every user is a standalone switch whose value is read
/// on its own, never as a release/acquire signal publishing other writes.
pub struct AtomicFlag<Tag> {
    value: AtomicBool,
    tag: PhantomData<fn() -> Tag>,
}

impl<Tag> AtomicFlag<Tag> {
    #[must_use]
    pub const fn new(value: bool) -> Self {
        Self {
            value: AtomicBool::new(value),
            tag: PhantomData,
        }
    }

    /// The current value.
    pub fn get(&self) -> bool {
        self.value.load(Ordering::Relaxed)
    }

    /// Set the value; visible to every other holder from the next [`Self::get`].
    pub fn set(&self, value: bool) {
        self.value.store(value, Ordering::Relaxed);
    }
}

// Written out rather than derived: `derive` would bound `Tag: Debug` /
// `Tag: Default`, which markers have no reason to satisfy.
impl<Tag> fmt::Debug for AtomicFlag<Tag> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AtomicFlag").field(&self.get()).finish()
    }
}

impl<Tag> Default for AtomicFlag<Tag> {
    fn default() -> Self {
        Self::new(false)
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
    use std::any::TypeId;

    /// Two switches, exactly as the app declares its own.
    struct One;
    struct Two;

    type FlagOne = AtomicFlag<One>;
    type FlagTwo = AtomicFlag<Two>;

    #[test]
    fn a_flag_starts_where_it_was_built() {
        assert!(FlagOne::new(true).get());
        assert!(!FlagOne::new(false).get());
    }

    #[test]
    fn the_default_flag_is_off() {
        // Every current user wants "off" as the safe resting state — most
        // importantly the consent gate, where on-by-default would mean
        // contacting claude.ai without being asked.
        assert!(!FlagOne::default().get());
    }

    #[test]
    fn a_flag_moves_both_ways() {
        let flag = FlagOne::new(false);
        flag.set(true);
        assert!(flag.get());
        flag.set(false);
        assert!(!flag.get());
    }

    #[test]
    fn a_flag_is_shared_through_a_reference() {
        // The property every caller relies on: flipping it through one handle
        // is observed through another, with no rebuild.
        let flag = std::sync::Arc::new(FlagOne::new(false));
        let other = std::sync::Arc::clone(&flag);
        flag.set(true);
        assert!(other.get());
    }

    /// Why the tag exists at all (issue #86). Tauri's managed state is a map
    /// keyed by `TypeId`, so two switches that share a type are one entry: the
    /// second `manage` is dropped and both `State` lookups resolve to the
    /// first. Asserted on `Arc<_>` because that is the shape the app manages.
    #[test]
    fn differently_tagged_flags_are_different_types() {
        use std::sync::Arc;
        assert_ne!(TypeId::of::<Arc<FlagOne>>(), TypeId::of::<Arc<FlagTwo>>());
        // Not vacuous: the same tag is the same type, which is exactly the
        // collision the app used to have.
        assert_eq!(
            TypeId::of::<Arc<FlagOne>>(),
            TypeId::of::<Arc<AtomicFlag<One>>>()
        );
    }
}
