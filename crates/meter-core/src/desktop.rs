//! Linux desktop-session identification — pure and testable so the setup
//! wizard's "install the GNOME `AppIndicator` extension" hint (issue #11) can
//! be unit-tested without ever reading an environment variable. Reading
//! `XDG_CURRENT_DESKTOP` is I/O and lives in the app shell (`src-tauri`);
//! this module only classifies whatever string it is handed.
//!
//! See the "Linux tray reality" note in the crate's top-level `CLAUDE.md`:
//! `StatusNotifierItem` gives no click events or tooltips at all, and GNOME
//! Shell additionally hides every `StatusNotifierItem` tray outright unless
//! the "`AppIndicator` and `KStatusNotifierItem` Support" extension is
//! installed — without it the app has no way to be reached once its window
//! is closed, so the wizard surfaces the hint proactively.

/// Whether `current_desktop` — the raw value of the `XDG_CURRENT_DESKTOP`
/// environment variable — identifies a GNOME session.
///
/// Per the freedesktop.org spec, `XDG_CURRENT_DESKTOP` is a colon-separated
/// list of desktop identifiers, most specific first (e.g. Ubuntu sets
/// `ubuntu:GNOME`, GNOME Classic sets `GNOME-Classic:GNOME`). Matching is
/// case-insensitive: the spec doesn't mandate a case, and desktop
/// environments have not been perfectly consistent about it in practice.
pub fn desktop_is_gnome(current_desktop: &str) -> bool {
    current_desktop.split(':').any(|part| {
        part.eq_ignore_ascii_case("gnome") || part.eq_ignore_ascii_case("gnome-classic")
    })
}

#[cfg(test)]
mod tests {
    use super::desktop_is_gnome;

    #[test]
    fn plain_gnome_matches() {
        assert!(desktop_is_gnome("GNOME"));
    }

    #[test]
    fn ubuntu_prefixed_gnome_matches() {
        assert!(desktop_is_gnome("ubuntu:GNOME"));
    }

    #[test]
    fn gnome_classic_matches() {
        assert!(desktop_is_gnome("GNOME-Classic:GNOME"));
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(desktop_is_gnome("gnome"));
        assert!(desktop_is_gnome("Gnome"));
    }

    #[test]
    fn other_desktops_do_not_match() {
        assert!(!desktop_is_gnome("KDE"));
        assert!(!desktop_is_gnome("XFCE"));
        assert!(!desktop_is_gnome("X-Cinnamon"));
    }

    #[test]
    fn empty_or_unset_does_not_match() {
        assert!(!desktop_is_gnome(""));
    }

    #[test]
    fn a_substring_that_is_not_a_whole_component_does_not_match() {
        // "gnomelike" must not be treated as GNOME just because it contains
        // the substring — only a whole colon-separated component counts.
        assert!(!desktop_is_gnome("gnomelike"));
    }
}
