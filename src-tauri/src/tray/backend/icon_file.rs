//! Publishing the tray icon as a PNG on disk.
//!
//! `StatusNotifierItem` offers two ways to supply an icon: raw ARGB bytes via
//! `IconPixmap`, or a themed icon name resolved against `IconThemePath`. The
//! pixmap route is simpler and needs no files — but GNOME's `AppIndicator`
//! extension renders it into a square box:
//!
//! ```js
//! const scaledSize = iconSize * scaleFactor;
//! this._setImageContent(imageContent, scaledSize, scaledSize);
//! ```
//!
//! Our gauge is 66x22 logically — three times wider than tall — so that
//! squashes it to an unreadable smudge. libappindicator always wrote a file and
//! pointed `IconThemePath` at it, which is why the icon looked right before we
//! took over the protocol. So we write a file too.
//!
//! The name has to *change* on every update: panels cache by icon name, and
//! rewriting the same path leaves the old image on screen.

use std::path::{Path, PathBuf};

use meter_render::RenderedIcon;

/// Writes each new icon to its own file and hands back the name to publish.
pub(super) struct IconFiles {
    dir: PathBuf,
    /// Bumped per write so the panel never serves a cached image.
    counter: u64,
    /// The file written last, removed once its successor is in place.
    previous: Option<PathBuf>,
}

impl IconFiles {
    /// Somewhere private and short-lived: `$XDG_RUNTIME_DIR` when set (tmpfs,
    /// cleaned on logout), the temp dir otherwise.
    pub(super) fn new() -> Self {
        let base =
            std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
        Self::at(base.join("rusted-claude-meter"))
    }

    /// Split out so tests get their own directory: the counter restarts per
    /// instance, so two sharing one directory would write the same filenames
    /// and delete each other's files on drop.
    const fn at(dir: PathBuf) -> Self {
        Self {
            dir,
            counter: 0,
            previous: None,
        }
    }

    pub(super) fn dir(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    /// Write `icon` and return the value to publish as `IconName`, or `None`
    /// when the write failed — in which case the caller keeps the previous
    /// name and the panel keeps showing the previous image.
    ///
    /// The value is an **absolute path**, not a themed name. That is off-spec
    /// but load-bearing: a bare name goes through icon-theme lookup and ends
    /// up in a square `St.Icon`, which crushes a 3:1 gauge into a sliver.
    /// GNOME's extension has a branch specifically for this, comment included
    /// — "HACK: icon is a path name. This is not specified by the API, but at
    /// least indicator-sensors uses it" — and loads the file at its natural
    /// aspect. It is also what libappindicator published, so this is the
    /// behaviour the tray already relied on before we took over the protocol.
    pub(super) fn write(&mut self, icon: &RenderedIcon) -> Option<String> {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return None;
        }
        self.counter = self.counter.wrapping_add(1);
        let path = self.dir.join(format!("tray-{}.png", self.counter));
        if encode_png(icon, &path).is_err() {
            return None;
        }
        let name = path.to_string_lossy().into_owned();
        // Only after the replacement exists, so there is never a moment with
        // no icon file on disk.
        if let Some(stale) = self.previous.replace(path) {
            let _ = std::fs::remove_file(stale);
        }
        Some(name)
    }
}

impl Drop for IconFiles {
    fn drop(&mut self) {
        if let Some(last) = self.previous.take() {
            let _ = std::fs::remove_file(last);
        }
        // Only if we left it empty; never recursive, so a surprising path
        // value can't take anything else with it.
        let _ = std::fs::remove_dir(&self.dir);
    }
}

/// `RenderedIcon` is already straight-alpha RGBA, which is exactly what PNG
/// wants — no conversion, just framing.
fn encode_png(icon: &RenderedIcon, path: &Path) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), icon.width, icon.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(as_io_error)?;
    writer.write_image_data(&icon.rgba).map_err(as_io_error)
}

/// `png::EncodingError` is not an `io::Error`, and the caller only cares that
/// the write failed.
fn as_io_error(error: png::EncodingError) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{IconFiles, encode_png};
    use meter_render::RenderedIcon;

    fn icon(width: u32, height: u32) -> RenderedIcon {
        RenderedIcon {
            width,
            height,
            rgba: vec![0x7F; (width * height * 4) as usize],
            is_template: false,
        }
    }

    #[test]
    fn a_written_icon_is_a_real_png() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("icon.png");
        encode_png(&icon(66, 22), &path).expect("encode");

        let bytes = std::fs::read(&path).expect("read back");
        // PNG magic; proves we wrote an image rather than raw bytes.
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn the_non_square_aspect_survives_the_round_trip() {
        // The whole reason this module exists: a 3:1 icon must stay 3:1.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("icon.png");
        encode_png(&icon(132, 44), &path).expect("encode");

        let file = std::io::BufReader::new(std::fs::File::open(&path).expect("open"));
        let decoder = png::Decoder::new(file);
        let reader = decoder.read_info().expect("header");
        let info = reader.info();
        assert_eq!((info.width, info.height), (132, 44));
    }

    #[test]
    fn each_write_gets_a_fresh_name_so_panels_cannot_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut files = IconFiles::at(dir.path().join("icons"));
        let first = files.write(&icon(66, 22)).expect("first write");
        let second = files.write(&icon(66, 22)).expect("second write");
        assert_ne!(first, second);
    }

    #[test]
    fn the_published_name_is_an_absolute_path() {
        // A bare name would be theme-looked-up and squared; the path route is
        // what keeps the gauge's aspect ratio. See `write`.
        let dir = tempfile::tempdir().expect("temp dir");
        let mut files = IconFiles::at(dir.path().join("icons"));
        let name = files.write(&icon(66, 22)).expect("write");
        assert!(
            name.starts_with('/'),
            "IconName must be an absolute path, got {name}"
        );
        assert_eq!(
            std::path::Path::new(&name).extension(),
            Some(std::ffi::OsStr::new("png")),
            "expected a PNG path, got {name}"
        );
    }

    #[test]
    fn the_superseded_file_is_cleaned_up() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut files = IconFiles::at(dir.path().join("icons"));
        let first_path = std::path::PathBuf::from(files.write(&icon(66, 22)).expect("first write"));
        assert!(first_path.exists(), "expected the first icon on disk");

        files.write(&icon(66, 22)).expect("second write");
        assert!(
            !first_path.exists(),
            "the superseded icon should have been removed"
        );
    }
}
