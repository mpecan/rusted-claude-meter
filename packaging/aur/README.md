# AUR package

`PKGBUILD` builds `rusted-claude-meter` from the tagged GitHub source
release, mirroring the `.deb` bundle layout (`npm run tauri build`) into a
pacman package. Verified working end to end on Arch Linux via
`makepkg -f && sudo pacman -U rusted-claude-meter-*.pkg.tar.zst`.

It links the system `webkit2gtk-4.1` rather than shipping its own, which is
the point: the AppImage bundles a WebKit built on `ubuntu-22.04` and then
runs it against whatever the host has, and
[issue #50](https://github.com/mpecan/rusted-claude-meter/issues/50) is what
that mismatch looks like on a rolling distro. Building against system libs
removes the mismatch by construction, so no version pinning is needed here —
and `depends()` deliberately carries no `libjxl` entry, because `libjxl`
reaches us only through `webkit2gtk-4.1`, which already declares its own
soname dependency on a compatible version.

`depends()` otherwise mirrors `bundle.linux.deb.depends` in
`src-tauri/tauri.conf.json`. Note `xdotool`: it is what provides `libxdo.so`
on Arch, so it belongs in `depends`, not `makedepends`.

`options=('!lto')` is required: `ring` (a transitive TLS dependency)
compiles its own C/assembly objects outside rustc's LTO awareness, and
makepkg.conf's default LTO setting breaks symbol resolution against those
objects at link time.

`package()` unpacks the `.deb` that `build()` produced instead of reading
Tauri's staging directory next to it — that layout is an undocumented
internal, and a glob over it fails silently rather than loudly if it ever
holds anything other than exactly one directory.

## Installing it today

**Not yet published to the AUR** (see below), so there is no `yay -S` path
yet. Build it from this directory:

```sh
git clone https://github.com/mpecan/rusted-claude-meter
cd rusted-claude-meter/packaging/aur && makepkg -si
```

## Publishing to the AUR

1. Clone the AUR git repo (`ssh://aur@aur.archlinux.org/rusted-claude-meter.git`)
   — an AUR account and SSH key are required, and the package name must not
   already be taken.
2. Copy `PKGBUILD` and `.SRCINFO` into that clone, commit, and push.
3. Regenerate `.SRCINFO` after every `PKGBUILD` edit: `makepkg --printsrcinfo > .SRCINFO`.
4. Bump `pkgver`/`pkgrel` in step with each GitHub release tag, and refresh
   the checksum with `updpkgsums`.

Once it is live, swap the build-from-source block above (and the README's
Linux section) for `yay -S rusted-claude-meter`.

This directory is the source of truth kept in-tree; the AUR git repo is a
mirror published from it.

Two things this costs, both worth knowing before the first publish:

- **`pkgver` is a third place the version lives.** `docs/packaging.md`'s
  premise is that release-please bumps every version automatically; this file
  and `.SRCINFO` are outside that, so they need either a release-please extra
  file or a `just` recipe, or they will drift.
- **`build()` needs the network** (`npm ci`, `cargo fetch`). That is normal
  for Tauri packages in the AUR but does mean `makepkg` cannot run offline.
