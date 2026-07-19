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

/// Monospaced advance per glyph, as a fraction of the em (Roboto Mono:
/// 1229/2048). The number-bearing styles size their canvases around it.
pub const ADVANCE_EM: f64 = 0.6;

/// Append a monospaced pace label ("1.8×" / compact "1.8"), centered on
/// `center`, filled with `fill`.
///
/// The bundled font is subset to digits and `%` only, so the decimal point and
/// the multiplication sign carry no glyph — they are drawn as vector shapes
/// (a low dot, a small cross) in the same monospaced cells the digits occupy,
/// keeping the digit rendering — and every existing snapshot — byte-identical.
pub fn pace_label(out: &mut String, center: (f64, f64), font_size: f64, fill: &str, text: &str) {
    let (cx, cy) = center;
    let advance = ADVANCE_EM * font_size;
    let cells = text.chars().count();
    // Left edge of the first cell, so the whole run stays centered on `cx`.
    #[allow(clippy::cast_precision_loss)]
    let start = cx - advance * cells as f64 / 2.0;
    for (index, ch) in text.chars().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let cell_cx = advance.mul_add(index as f64 + 0.5, start);
        match ch {
            '.' => {
                // A period sits low in the cell, on the digit baseline.
                let radius = 0.09 * font_size;
                let dot_cy = 0.30_f64.mul_add(font_size, cy);
                let _ = write!(
                    out,
                    r#"<circle cx="{cell_cx:.2}" cy="{dot_cy:.2}" r="{radius:.2}" fill="{fill}"/>"#
                );
            }
            '\u{00D7}' => {
                // A multiplication sign: two short strokes crossing on the cell
                // centre (which is where the digits' `central` baseline sits).
                let reach = 0.20 * font_size;
                let width = 0.11 * font_size;
                let (l, r) = (cell_cx - reach, cell_cx + reach);
                let (t, b) = (cy - reach, cy + reach);
                let _ = write!(
                    out,
                    r#"<path d="M{l:.2},{t:.2}L{r:.2},{b:.2}M{l:.2},{b:.2}L{r:.2},{t:.2}" stroke="{fill}" stroke-width="{width:.2}" stroke-linecap="round"/>"#
                );
            }
            digit => {
                let mut buf = [0_u8; 4];
                centered_text(
                    out,
                    (cell_cx, cy),
                    font_size,
                    fill,
                    digit.encode_utf8(&mut buf),
                );
            }
        }
    }
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

    #[test]
    fn pace_label_draws_digits_as_font_text() {
        // The digits go through the bundled font; two of them here.
        let mut out = String::new();
        pace_label(&mut out, (30.0, 11.0), 13.0, "#007AFF", "18");
        assert_eq!(
            out.matches("<text").count(),
            2,
            "one text element per digit"
        );
        assert!(out.contains(">1<") && out.contains(">8<"));
    }

    #[test]
    fn pace_label_draws_the_dot_and_cross_the_font_lacks() {
        // "1.8×": the subset font has no `.`/`×`, so they render as vector
        // shapes — a dot (circle) and a cross (path) — while the digits are text.
        let mut out = String::new();
        pace_label(&mut out, (30.0, 11.0), 13.0, "#FF9500", "1.8\u{00D7}");
        assert_eq!(out.matches("<text").count(), 2, "1 and 8 as text");
        assert_eq!(out.matches("<circle").count(), 1, "the decimal point");
        assert_eq!(out.matches("<path").count(), 1, "the multiply sign");
        // Every drawn piece carries the requested fill colour.
        assert!(out.contains("#FF9500"));
    }

    #[test]
    fn pace_label_stays_centered_on_the_anchor() {
        // Two symmetric cells: their centres straddle the anchor x by ±0.3em.
        let mut out = String::new();
        pace_label(&mut out, (30.0, 11.0), 10.0, "#000000", "18");
        assert!(out.contains(r#"x="27.00""#), "left digit cell: {out}");
        assert!(out.contains(r#"x="33.00""#), "right digit cell: {out}");
    }
}
