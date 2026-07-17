//! Bundled monospaced font and the shared `usvg` options that render text.
//!
//! `resvg`'s default [`Options`] carry no fonts, so `<text>` elements render
//! nothing. We vendor a tiny subset of **Roboto Mono** (digits `0`–`9` and
//! `%` only, ~3 KB, weight pinned) under the SIL Open Font License 1.1 — see
//! `assets/RobotoMono-LICENSE.txt` — and load it into a private
//! [`fontdb::Database`]. The database and the [`Options`] built around it are
//! constructed exactly once via a [`OnceLock`] and reused across every render,
//! so no per-icon font parsing happens and the pipeline stays deterministic
//! (only the bundled font is ever consulted; system fonts are compiled out).

use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use resvg::usvg::Options;
use resvg::usvg::fontdb::Database;

/// The subsetted Roboto Mono face, embedded in the binary.
static FONT_DATA: &[u8] = include_bytes!("../assets/RobotoMono-DigitsPercent.ttf");

/// Family name the bundled face registers under (verified against the asset).
/// The SVG `font-family` and [`Options::font_family`] must both use this exact
/// string so `usvg` resolves the glyphs to our face. The bundled face is
/// monospaced at `0.6em` per glyph (Roboto Mono: 1229/2048 units), which the
/// number-bearing styles account for when sizing their canvases.
pub const FAMILY: &str = "Roboto Mono";

/// Shared, lazily-built rendering options carrying the bundled font database.
/// Built once; every render borrows the same `&Options`.
pub fn options() -> &'static Options<'static> {
    static OPTIONS: OnceLock<Options<'static>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let mut db = Database::new();
        db.load_font_data(FONT_DATA.to_vec());
        let mut options = Options {
            font_family: FAMILY.to_owned(),
            ..Options::default()
        };
        options.fontdb = Arc::new(db);
        options
    })
}

/// Append a centered monospaced `<text>` element: horizontally centered on
/// `center.0`, vertically centered on `center.1` (via `dominant-baseline`),
/// filled with `fill`. Bakes the percentage number into number-bearing styles.
pub fn centered_text(out: &mut String, center: (f64, f64), font_size: f64, fill: &str, text: &str) {
    let (cx, cy) = center;
    let _ = write!(
        out,
        r#"<text x="{cx:.2}" y="{cy:.2}" font-family="{FAMILY}" font-size="{font_size}" text-anchor="middle" dominant-baseline="central" fill="{fill}">{text}</text>"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_built_once_and_carry_the_bundled_family() {
        let first = options();
        let second = options();
        assert!(
            std::ptr::eq(first, second),
            "OnceLock must reuse one Options"
        );
        assert_eq!(first.font_family, FAMILY);
        assert!(
            first.fontdb.faces().next().is_some(),
            "the bundled font must be loaded into the database"
        );
    }

    #[test]
    fn bundled_face_registers_under_the_declared_family() {
        // Guards against the asset being replaced by one whose family name no
        // longer matches `FAMILY` (which would silently render no glyphs).
        let db = &options().fontdb;
        let matches = db.faces().any(|face| {
            face.families
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(FAMILY))
        });
        assert!(matches, "no face registered under {FAMILY:?}");
    }

    #[test]
    fn centered_text_emits_the_family_and_content() {
        let mut out = String::new();
        centered_text(&mut out, (10.0, 11.0), 11.0, "#FF3B30", "42%");
        assert!(out.contains(r#"font-family="Roboto Mono""#));
        assert!(out.contains("42%"));
        assert!(out.contains("#FF3B30"));
    }
}
