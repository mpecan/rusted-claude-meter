# AUR package

`PKGBUILD` builds `rusted-claude-meter` from the tagged GitHub source
release, mirroring the `.deb` bundle layout (`npm run tauri build`) into a
pacman package. Verified working end to end on Arch Linux via
`makepkg -f && sudo pacman -U rusted-claude-meter-*.pkg.tar.zst`.

Depends on system `webkit2gtk-4.1`/`libjxl` rather than bundling its own —
this sidesteps the version-skew crash documented in
[issue #50](https://github.com/mpecan/rusted-claude-meter/issues/50), where
the AppImage's bundled webkit libs expect a newer `libjxl` than some hosts
have installed. `depends()` pins minimum versions so `pacman` fails fast
with a clear dependency error instead of installing something that then
crashes or aborts at runtime.

`options=('!lto')` is required: `ring` (a transitive TLS dependency)
compiles its own C/assembly objects outside rustc's LTO awareness, and
makepkg.conf's default LTO setting breaks symbol resolution against those
objects at link time.

## Publishing to the AUR

1. Clone the AUR git repo (`ssh://aur@aur.archlinux.org/rusted-claude-meter.git`)
   — an AUR account and SSH key are required, and the package name must not
   already be taken.
2. Copy `PKGBUILD` and `.SRCINFO` into that clone, commit, and push.
3. Regenerate `.SRCINFO` after every `PKGBUILD` edit: `makepkg --printsrcinfo > .SRCINFO`.
4. Bump `pkgver`/`pkgrel` in step with each GitHub release tag.

This directory is the source of truth kept in-tree; the AUR git repo is a
mirror published from it.
