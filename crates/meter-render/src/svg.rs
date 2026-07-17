//! Shared SVG document envelope for every icon-style template.
//!
//! Each style module ([`crate::battery`], [`crate::circular`], ...) only
//! draws its own body; the `<svg>` open/close tags, the per-style `viewBox`
//! (height is always [`crate::state::BASE_HEIGHT`]; width is style-dependent,
//! so number-bearing styles are wider than tall) and the output buffer
//! allocation live here so all styles stay structurally identical.

use std::fmt::Write as _;

/// Build one SVG document: allocate `capacity` bytes, emit the standard
/// `<svg ... viewBox="0 0 {width} {height}">` open tag, let `body` append the
/// style's shapes, and close the document.
pub fn svg_document(
    width: u32,
    height: u32,
    capacity: usize,
    body: impl FnOnce(&mut String),
) -> String {
    let mut out = String::with_capacity(capacity);
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}">"#
    );
    body(&mut out);
    out.push_str("</svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_the_body_in_the_standard_envelope() {
        let doc = svg_document(63, 22, 64, |out| out.push_str("<rect/>"));
        assert_eq!(
            doc,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 63 22"><rect/></svg>"#
        );
    }
}
