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

use serde::{Deserialize, Serialize};

/// The desktop session, to the extent the app has to care.
///
/// Only two are named, and each earns it by imposing a constraint the app has
/// to warn about: GNOME hides every `StatusNotifierItem` without the
/// `AppIndicator` extension, and Plasma renders tray icons into a square cell
/// so a wide icon draws small. Everything else — and macOS — is [`Other`].
///
/// [`Other`]: LinuxDesktop::Other
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinuxDesktop {
    Gnome,
    Kde,
    Other,
}

impl LinuxDesktop {
    /// Classify `current_desktop` — the raw value of the `XDG_CURRENT_DESKTOP`
    /// environment variable.
    ///
    /// Per the freedesktop.org spec, `XDG_CURRENT_DESKTOP` is a
    /// colon-separated list of desktop identifiers, most specific first (e.g.
    /// Ubuntu sets `ubuntu:GNOME`, GNOME Classic sets `GNOME-Classic:GNOME`,
    /// Plasma sets `KDE`). Matching is case-insensitive: the spec doesn't
    /// mandate a case, and desktop environments have not been perfectly
    /// consistent about it in practice.
    ///
    /// The list is scanned most-specific first, so a session advertising both
    /// resolves to whichever it names first.
    #[must_use]
    pub fn classify(current_desktop: &str) -> Self {
        current_desktop
            .split(':')
            .find_map(|part| {
                if part.eq_ignore_ascii_case("gnome") || part.eq_ignore_ascii_case("gnome-classic")
                {
                    Some(Self::Gnome)
                } else if part.eq_ignore_ascii_case("kde") || part.eq_ignore_ascii_case("plasma") {
                    Some(Self::Kde)
                } else {
                    None
                }
            })
            .unwrap_or(Self::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::LinuxDesktop;

    fn classify(value: &str) -> LinuxDesktop {
        LinuxDesktop::classify(value)
    }

    #[test]
    fn plain_gnome_matches() {
        assert_eq!(classify("GNOME"), LinuxDesktop::Gnome);
    }

    #[test]
    fn ubuntu_prefixed_gnome_matches() {
        assert_eq!(classify("ubuntu:GNOME"), LinuxDesktop::Gnome);
    }

    #[test]
    fn gnome_classic_matches() {
        assert_eq!(classify("GNOME-Classic:GNOME"), LinuxDesktop::Gnome);
    }

    #[test]
    fn plasma_is_kde_under_either_name() {
        assert_eq!(classify("KDE"), LinuxDesktop::Kde);
        assert_eq!(classify("plasma"), LinuxDesktop::Kde);
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(classify("gnome"), LinuxDesktop::Gnome);
        assert_eq!(classify("Gnome"), LinuxDesktop::Gnome);
        assert_eq!(classify("kde"), LinuxDesktop::Kde);
    }

    #[test]
    fn the_most_specific_component_wins() {
        // Scanned left to right, which is the spec's most-specific-first order.
        assert_eq!(classify("KDE:GNOME"), LinuxDesktop::Kde);
        assert_eq!(classify("GNOME:KDE"), LinuxDesktop::Gnome);
    }

    #[test]
    fn other_desktops_do_not_match() {
        assert_eq!(classify("XFCE"), LinuxDesktop::Other);
        assert_eq!(classify("X-Cinnamon"), LinuxDesktop::Other);
    }

    #[test]
    fn empty_or_unset_is_other() {
        assert_eq!(classify(""), LinuxDesktop::Other);
    }

    #[test]
    fn a_substring_that_is_not_a_whole_component_does_not_match() {
        // "gnomelike" must not be treated as GNOME just because it contains
        // the substring — only a whole colon-separated component counts.
        assert_eq!(classify("gnomelike"), LinuxDesktop::Other);
        assert_eq!(classify("kdeconnect"), LinuxDesktop::Other);
    }
}
