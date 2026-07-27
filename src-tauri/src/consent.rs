//! The Terms-of-Service consent gate.
//!
//! Reading usage out of `claude.ai` means polling an internal, undocumented
//! endpoint with the user's web session cookie, from a non-browser client that
//! presents itself as Chrome. Anthropic's Consumer Terms §3 prohibit accessing
//! the Services "through automated or non-human means, whether through a bot,
//! script, or otherwise" without an API key or explicit permission — so on a
//! plain reading, that is what this app does. See `docs/terms-of-service.md`.
//!
//! That is a risk the user takes on their own account, so it must be their
//! choice, made knowingly, before any request happens — not a warning they can
//! scroll past while the app is already polling. This gate is how that is
//! enforced rather than merely stated: **closed by default**, including for
//! installs that upgrade into this version, and every path that would reach
//! claude.ai consults it first — the scheduler transport
//! (`scheduler::transport`), the session-key command (`commands`) and browser
//! import (`commands::browser`).
//!
//! It is a runtime mirror of `AppSettings::tos_acknowledged`, not a second
//! source of truth: `lib.rs` seeds it from the loaded settings at startup and
//! `commands::consent` moves the two together. An [`AtomicFlag`] rather than a
//! read through `SettingsState` because the polling transport checks it on
//! every tick from an async task, and because a gate that can never block or
//! poison is one fewer way for "did the user agree?" to fail open.
//!
//! It is a plain alias rather than a newtype wrapping the flag: the wrapper
//! bought nothing but two delegating accessors, which were themselves
//! duplicates of the identical pair on every other shared switch in the app
//! (see [`AtomicFlag`]'s own docs). `gate.get()` at a call site that names the
//! value `consent` reads clearly enough.

use crate::sync::AtomicFlag;

/// Shared, cheap-to-read consent flag: `true` once the user has accepted the
/// Terms-of-Service risk, and only then may anything contact claude.ai. Held
/// in Tauri managed state and cloned (via `Arc`) into the scheduler transport.
pub type ConsentGate = AtomicFlag;

/// A closed gate — no network access permitted. The default position, what a
/// fresh install starts from, and what a caller with nothing persisted should
/// use.
#[must_use]
pub const fn closed() -> ConsentGate {
    ConsentGate::new(false)
}

#[cfg(test)]
mod tests {
    use super::closed;

    #[test]
    fn the_default_gate_is_closed() {
        // The one thing this module adds over `AtomicFlag` (whose get/set/
        // default contract is covered in `sync.rs`): an install that has never
        // answered the question does not reach claude.ai.
        assert!(!closed().get());
    }
}
