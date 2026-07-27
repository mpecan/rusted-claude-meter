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

**Automated.** The `publish-aur` job in `.github/workflows/release.yml` runs on
every published release and does the whole thing: sets `pkgver` from the tag,
resets `pkgrel`, refreshes `sha256sums` with `updpkgsums`, regenerates
`.SRCINFO`, **builds the package to prove it still works**, pushes to
`ssh://aur@aur.archlinux.org/rusted-claude-meter.git`, and opens a PR syncing
the result back here.

Nothing needs pushing between releases. The `source=` is a tagged tarball, so
the package cannot change until there is a new tag.

Three values are computed rather than maintained, because each goes stale
silently and in a different way:

| Value | Why it cannot be hand-maintained |
| --- | --- |
| `pkgver` | Follows the tag. Left behind, `yay` simply never offers the update. |
| `sha256sums` | Of a tarball that does not exist until the tag is cut, so it is *never* correct at commit time. |
| `.SRCINFO` | Generated, and it is what pacman clients read **instead of** the PKGBUILD — stale, it advertises the wrong version whatever the PKGBUILD says. |

Setup is one secret: **`AUR_SSH_PRIVATE_KEY`**, the private half of a key
registered on the AUR account that owns the package. Without it the job logs a
skip and the release still ships — the AUR just lags, the same way a missing
Apple secret degrades signing rather than failing the run. The AUR host key is
pinned in the workflow (`SHA256:RFzBCUItH9LZS0cKB5UE6ceAYhBD5C8GeOBip8Z11+4`)
rather than accepted on first use.

### The first publish is still manual

The AUR creates a package from its first push, so that one has to be done by a
human who has confirmed the name is free:

```sh
git clone ssh://aur@aur.archlinux.org/rusted-claude-meter.git
cp packaging/aur/{PKGBUILD,.SRCINFO} rusted-claude-meter/
cd rusted-claude-meter && git add . && git commit -m "Initial import" && git push
```

Then swap the build-from-source block above — and the README's Linux section —
for `yay -S rusted-claude-meter`.

### Bumping without a release

If the *packaging* changes but the version does not (a dependency fix, say),
that is a `pkgrel` bump: increment it here and push the two files by hand. The
release job resets `pkgrel=1` on the next real version, which is correct — it
counts rebuilds of one version, not releases.

### Remaining cost

`build()` needs the network (`npm ci`, `cargo fetch`). Normal for Tauri
packages in the AUR, but it does mean `makepkg` cannot run offline.
