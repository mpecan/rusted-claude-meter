//! Shared atomic-write idiom used by every on-disk contract (`cache.rs`,
//! `settings.rs`, `export.rs`): create the parent directory, write to a
//! sibling `.tmp` file, then rename over the destination. The rename is
//! atomic on the platforms this app targets, so a crash mid-write (or a
//! concurrent read by an external script) can never observe a truncated
//! file.

use std::fs;
use std::io;
use std::path::Path;

/// Write `body` to `path` atomically: `create_dir_all` the parent, write to
/// `path` with a `.tmp`-suffixed extension, then rename over `path`.
pub fn atomic_write(path: &Path, body: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body)?;
    fs::rename(&tmp, path)
}
