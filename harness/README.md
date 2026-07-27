# Linux desktop harness

Run the real app on a real Linux desktop — GNOME and KDE — against demo data,
on this Mac. **No claude.ai session key and no network access to claude.ai are
involved anywhere in this loop.**

CI builds and unit-tests on Linux but never launches a desktop session, so the
things that make the Linux build distinctive have no automated coverage at all:
the StatusNotifierItem tray, GNOME Shell's AppIndicator requirement, Secret
Service storage, notifications, and the `.deb` / AppImage bundles themselves.
This harness is how those get exercised.

Nothing here runs in CI, by design. It is only as current as the last time
somebody ran it.

## What it is

| Piece | Where | Role |
| --- | --- | --- |
| Demo API server | `demo-server/` | Stands in for claude.ai's usage endpoints. Runs on the Mac. |
| `rcm-gnome` | Ubuntu 24.04 + GNOME | Installs the shipped **`.deb`**. Also builds both artifacts. |
| `rcm-kde` | Fedora + KDE Plasma | Runs the shipped **AppImage**. |
| `rcm-gnome-c` | Ubuntu 24.04 + GNOME, **container** | Same desktop and same `.deb` as `rcm-gnome`, in seconds instead of minutes. |
| `rcm-kde-c` | Fedora 43 + KDE Plasma, **container** | Same, against Plasma and the unbundled binary. |

Between them the two VMs cover both desktops *and* both Linux artifacts the
project releases. The containers are a faster route to everything except the
AppImage — see "Containers instead of VMs" below for what they do and do not
replace.

The app is pointed at the demo server by `RCM_API_BASE_URL`
(`src-tauri/src/api_base.rs`). That override ships in release builds on
purpose: the binary under test is bit-for-bit the one users install, not a
demo-only variant. It is never silent — the app logs it at startup and Settings
shows a **Demo endpoint active** banner, because demo data is otherwise
indistinguishable from real usage.

## Prerequisites

```console
$ brew install lima        # VMs
$ brew install jq          # optional, prettier scenario.sh output
```

## Runbook

```console
# 1. Demo API on the Mac (foreground; macOS asks once to allow incoming
#    connections — the VMs reach it at host.lima.internal:8787).
$ just demo-server

# 2. Bring up the GNOME VM. First run downloads a cloud image and installs a
#    desktop: budget ~15 minutes. Subsequent runs are fast.
$ just vm-up gnome

# 3. Build the .deb and AppImage inside it (~10 min cold, then incremental).
$ just linux-build

# 4. Install and launch.
$ just vm-install gnome
$ just vm-launch gnome
$ harness/bin/vm.sh logs gnome   # app output, incl. the demo-endpoint line

# 5. Watch it.
$ just vm-vnc gnome              # opens macOS Screen Sharing
$ just vm-shot gnome             # or screenshot it to harness/artifacts/

# 6. Change the data and watch the tray react — no restart of either side.
$ just scenario critical
$ just scenario ahead-of-pace
$ just scenario failure 401
$ just scenario                  # what's active, and what's available
```

Then the KDE VM, which consumes the artifacts already built:

```console
$ just vm-up kde
$ just vm-install kde
$ harness/bin/vm.sh launch kde binary   # not the AppImage — see below
$ just vm-vnc kde                       # port 5902, so both run at once
```

`launch kde binary` runs the unbundled binary rather than the AppImage.
`just vm-launch kde` still runs the AppImage, so the shipped artifact is
exercised rather than papered over.

The AppImage prints `EGL_BAD_PARAMETER ... Aborting...` in a GPU-less VM —
see "What the first run established" — but **that line is not the app dying.**
Re-measured on `rcm-kde`: it keeps running, registers its StatusNotifierItem
and draws the tray gauge, with no environment overrides. The `binary` variant
is about the *window*, not about getting the app up. Do not read the EGL line
as a failed launch; check the bus (`gdbus call … RegisteredStatusNotifierItems`)
before concluding anything, which is the same reason `status` exists.

Teardown is `just vm-down gnome` (or `harness/bin/vm.sh delete gnome` to
reclaim the disk).

## Containers instead of VMs

Both desktops also run in podman containers, which come up in a couple of
seconds rather than the VMs' fifteen minutes and can be thrown away and
recreated between checks. Same distributions as the VMs — Ubuntu 24.04 with
GNOME Shell and the AppIndicator extension, Fedora 43 with Plasma 6 — and the
same artifacts.

```console
$ brew install podman && podman machine init && podman machine start
$ just demo-server                     # as before, on the Mac

$ just container-up gnome              # image if needed, container, app, wizard
$ just container-tray gnome            # the tray icon, magnified, to artifacts/
$ just scenario critical               # same live scenario switching as the VMs
$ just container-shot gnome after      # whole desktop to artifacts/after.png
$ just container-down gnome
```

Swap `gnome` for `kde` throughout; the two are independent and can run at once.
`just container-up` is four steps in one — run them separately (`just container
up gnome`, then `install` / `launch` / `setup`) when one of them is what you are
looking at.

`just container setup <target>` drives the first-run wizard by clicking at
fixed coordinates with a fabricated `sk-ant-sid01-…` key. The demo server
accepts any key of the right shape, so validation, the Secret Service write and
the first poll are all real. On KDE it first runs `just container wallet kde`,
which is where the interesting difference is — see below.

**They also produce the docs.** `just linux-screenshots` drives both desktops
and rewrites `docs/screenshots/linux/`, which is what [`docs/linux.md`](../docs/linux.md)
shows — so the user-facing Linux page is regenerated rather than hand-curated,
and cannot quietly go stale. Menu crops are fixed coordinates (GNOME's menu is
Shell-drawn, so there is no window to target); check the output before
committing it.

**What the containers cannot do.** No AppImage (it aborts without a GPU — see
"What the first run established"), no VNC to watch them live, and nothing
GPU-dependent. When a result matters, confirm it in the VM; the containers are
for the fast loop, not for the record.

**Asserting instead of eyeballing.** `status` reads the tray off the bus rather
than off a screenshot, and the first section is the same query on both desktops
because it *is* the StatusNotifierItem contract:

```console
$ just container status gnome
StatusNotifierItems registered:
  :1.21@/org/ayatana/NotificationItem/tray_icon_tray_app_main
GNOME panel status area:
  …
  appindicator-:1.21@/org/ayatana/NotificationItem/tray_icon_tray_app_main
extension ubuntu-appindicators@ubuntu.com:
  ENABLED
```

`just container appindicator gnome off` then prints:

```console
StatusNotifierItems registered:
  (no StatusNotifierWatcher on the bus — nothing can register one)
```

That is the sharpest form the "Linux tray reality" claim has ever taken here.
It is not that GNOME hides the icon — with the extension off there is no
watcher on the session bus at all, so no application can publish a tray icon in
the first place. Run the identical command against `kde` and the watcher is
always there, with no extension installed. Same app, same protocol, opposite
out-of-the-box result, both observable in one line each.

The GNOME session additionally runs `gnome-shell --unsafe-mode`, which exposes
the Shell's D-Bus `Eval` endpoint:

```console
$ just container eval gnome 'Main.panel.statusArea.quickSettings.visible'
true
```

JS sent through `eval` must not contain backslash escapes — gdbus's GVariant
parser eats them, so `"\n"` arrives as a real line break and comes back as a
syntax error. Write `String.fromCharCode(10)`. Plasma has no equivalent worth
wrapping, so `eval` is GNOME-only and `status` is what KDE gets.

**KWallet is driven, not dodged.** Plasma's Secret Service provider is KWallet,
and a wallet cannot be created unattended without weakening it. `container.sh
wallet kde` answers the dialogs the way a user would — blowfish, blank
password, accept the strength warning — rather than substituting gnome-keyring,
because KDE's real credential path is most of the point of having a KDE target.
It is also how the VM's "KWallet prompts once" note stops being a manual step.

Worth knowing: **the app reports the credential store as unavailable rather
than waiting for the wallet dialog.** Saving a key before any wallet exists
produces `the OS credential store is unavailable: Couldn't access platform
storage: SS error: result not returned from SS API` while the KWallet wizard is
still on screen behind the message. That is a correct-looking `StoreError`
message, and it is arguably the right call, but it means a first-run KDE user
who takes a moment over the wallet dialog sees an error rather than a wait.
Not investigated further.

**Things the images get wrong if you rebuild them carelessly.** All found the
hard way and all commented in `container/`:

- `gjs` is only a *Recommends* of `gnome-shell`, so `--no-install-recommends`
  drops it — and `org.freedesktop.Notifications` is D-Bus-activated through
  `gjs`. Without it every notification in the session fails with
  `ServiceUnknown`, which reads as "the app does not notify".
- WebKit needs **both** `WEBKIT_DISABLE_DMABUF_RENDERER=1` and
  `WEBKIT_DISABLE_COMPOSITING_MODE=1`. With only the first, the app's window
  opens with a correct title bar and never paints — a blank white rectangle
  that looks like a frontend bug rather than a missing GPU.
- `kwin-x11` is its own package since Plasma 6.3 split the X11 and Wayland
  compositors. Without it `startplasma-x11` comes up with no window manager, so
  windows have no decorations and nothing can be raised or focused — which
  makes every `xdotool` click land somewhere unintended.

## Scenarios

`demo-server/scenarios/*.json` are in claude.ai's response shape — the same
shape as `crates/meter-api/tests/fixtures/usage_response.json`. Edit one and
the next poll picks it up; the file is re-read per request.

| Scenario | What it is for |
| --- | --- |
| `fresh` | Early in both windows, light usage. The default. |
| `warning` | Past the warning threshold on both headline windows. |
| `critical` | Past the critical threshold. Red icon, crossing notification. |
| `limit-hit` | Session window fully consumed with time still to run. |
| `ahead-of-pace` | Overuse band — flame badge, "Used 85% vs 40%". |
| `behind-pace` | Underuse band — snowflake badge. |
| `scoped-models` | The full model-scoped limits contract, incomplete entries included. |
| `spend` | A token/cost account, so the Cost view auto-detects. |

`resets_at` values are written as `{{now+3h}}` tokens resolved per request.
This is not decoration: `meter_core::pacing` derives elapsed time from
`resets_at`, so hardcoded dates would make every scenario read as fully elapsed
and every pace ratio collapse to the same value.

Scenarios are decoded through the real parser by
`crates/meter-api/tests/harness_scenarios.rs` on every `just check`, so a
malformed fixture fails on the Mac in seconds instead of showing up as an
unexplained "no data" inside a VM.

## Checks worth running

The point of a real desktop session is the things a headless build cannot show.

**Tray and data flow**
1. Paste any well-formed `sk-ant-sid01-…` string in the wizard. The demo server
   accepts any key with the right shape — it has no notion of a *correct* one —
   but rejects a missing cookie with a 401, so the validation path is real.
2. Confirm the key persists to the Secret Service and survives an app restart.
3. `just scenario fresh` → `critical`: icon colour and percentage change, tray
   menu text updates, and a threshold-crossing notification fires.
4. `just scenario ahead-of-pace` / `behind-pace`: flame / snowflake badge.
5. `just scenario failure 401` and `failure 500`: the app degrades rather than
   hanging. `failure hang` exercises the client timeout.

**GNOME's AppIndicator requirement** — the one this harness exists for.
Provisioning enables the extension, so the tray works out of the box. Turn it
off inside the session and the tray disappears entirely:

```console
$ harness/bin/vm.sh shell gnome env DISPLAY=:1 gnome-extensions disable ubuntu-appindicators@ubuntu.com
```

Confirm the setup wizard's GNOME hint fires (`meter_core::LinuxDesktop`),
then re-enable it. This is the behaviour behind "Linux tray reality" in the
project's `CLAUDE.md`, and it has never before been observable in a test.

**Secret Service unavailable** — the app deliberately has no plaintext
fallback:

```console
$ harness/bin/vm.sh shell gnome pkill gnome-keyring-daemon
```

Saving a key must surface `StoreError::Unavailable` as a clear message, and
must not write the key anywhere on disk.

## What the first run established

Recorded from the run that built this harness (Ubuntu 24.04 GNOME, `.deb`), so a
later run has something to compare against:

- The tray renders and tracks live. `fresh` → **8% green**, `critical` → **94%
  red**, `ahead-of-pace` → **85% amber with the overuse badge**, each picked up
  on the next poll with no restart of the app or the server.
- The wizard accepts a fabricated `sk-ant-sid01-…` key, validates it against the
  demo server, saves it to the Secret Service and reports "connected and
  verified".
- Settings shows the **Demo endpoint active** banner naming
  `http://host.lima.internal:8787`, and the app logs the same on stdout.
- **Click-to-popover works on both desktops** (the parity work). GNOME:
  double-click opens the frameless popover under the panel, single-click still
  opens the menu. KDE: single-click opens it, anchored above the bottom panel.
  Clicking away dismisses it. The icon keeps its 3:1 aspect on both.
- **Disabling the AppIndicator extension removes the tray icon entirely** —
  nothing but the GNOME power button remains. Re-enabling brings it straight
  back. This is the behaviour behind "Linux tray reality" in the project's
  `CLAUDE.md`, and as far as this repo goes it had never been observed running
  until now.

The GNOME container reproduces every one of those results — same tray colours
and badge on the same scenarios, same wizard outcome, same demo banner (naming
`http://host.containers.internal:8787` instead), same disappearance when the
AppIndicator extension is switched off, and the key survives an app restart
through the Secret Service. It adds one observation the VM run did not record:

- **The tray menu is live and correct** — `Updated under 1m ago`, `5-hour: 85%
  — resets in 2h 59m`, `7-day: 75% — resets in 4d 23h`, then Open / Settings /
  Refresh Now / Quit. That menu is the whole Linux surface, and this is the
  first time its contents have been read off a running desktop.

The KDE container reproduces the Plasma results below, and sharpens two of
them:

- **Plasma publishes `org.kde.StatusNotifierWatcher` with nothing installed**,
  where GNOME has no watcher on the bus at all until the AppIndicator
  extension is enabled. The same one-line query answers both.
- **The square-cell rendering is visible rather than inferred.** Plasma pins a
  tray icon's width to its height, so the 66x22 gauge draws at roughly a third
  of panel height — legible as a colour, not as a number. `just container-tray
  kde` next to `just container-tray gnome`, same app and same scenario, is the
  clearest statement of that this repo has.

On Fedora KDE (unbundled binary):

- **Plasma renders the tray with no extension installed** — the direct control
  against GNOME's behaviour above. Same app, same StatusNotifierItem, opposite
  out-of-the-box result.
- The window, the Settings page and the demo banner all render correctly.

Three things worth following up:

- **The AppImage cannot start in a GPU-less VM.** It aborts with
  `Could not create default EGL display: EGL_BAD_PARAMETER`, while the exact
  same build as an unbundled binary starts fine on the same machine — so it is
  the AppImage's bundled GL/WebKit stack, not Fedora and not the app.
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` does not help it (it does help the
  unbundled binary). Unknown whether this affects real users on GPU-less or
  remote-desktop setups; it has only been observed under llvmpipe. This is why
  `vm.sh launch kde binary` exists.
- **The app's notifications reach GNOME and are then dropped.** Found in the
  container, and the sharpest thing it has turned up so far. `dbus-monitor`
  shows all four crossing notifications going out correctly on a
  `fresh` → `critical` switch — `Warning: 5-hour usage`, `Warning: 7-day
  usage`, `Critical: 5-hour usage`, `Critical: 7-day usage` — and being
  forwarded to the Shell. No banner appears, and
  `Main.messageTray.getSources()` stays empty, with nothing in the journal.
  The control rules out the environment: `notify-send` in the same session,
  even with `-a rusted-claude-meter`, banners and shows up in that same list.
  So GNOME is dropping these specific notifications, not failing to receive
  them. One difference stands out and has not been confirmed as the cause —
  the app sends each notification from a *new* D-Bus connection which then
  closes immediately (four notifications arrive as four senders), so by the
  time GNOME resolves the sender it may be gone. **Not yet reproduced in the
  GNOME VM**, so it is not established whether real users are affected or
  whether something about the container makes the race certain. That is the
  next thing to run.
- **`UsageClient` sets no reqwest timeout** (`crates/meter-api/src/client.rs`).
  `just scenario failure hang` exists to probe what happens to the poll loop
  when claude.ai accepts a connection and never answers; that path has not been
  run to a conclusion yet. Injecting a **401** behaves correctly — polling
  pauses and the tray keeps its last known reading rather than blanking.

## Notes and rough edges

- **KWallet prompts once on KDE.** Plasma's Secret Service provider is KWallet,
  and creating a wallet cannot be done unattended without weakening it. The
  first key save pops a wallet-creation dialog in the VNC window; accept it
  (a blank password is fine — the VM holds only demo data). This is deliberate:
  substituting gnome-keyring to dodge the dialog would mean not testing KDE's
  actual credential path.
- **Software rendering.** There is no GPU behind the vz display, so GNOME Shell
  and Plasma both run on llvmpipe (`LIBGL_ALWAYS_SOFTWARE=1`). Animations are
  sluggish; nothing functional is affected, and screenshots are unaffected.
- **VNC has no password** (`-SecurityTypes None`). Safe only because the port is
  reachable only through Lima's forward to the Mac's loopback.
- **The repo is mounted read-only** at `/repo` in the GNOME VM. Builds run from
  an rsynced copy in the VM's own filesystem, so the working tree on the Mac is
  never written to and the Tauri target dir is not on a virtiofs mount.
- **Provisioning is not in the Lima `provision:` block** — `vm.sh` runs
  `harness/provision/*.sh` over `limactl shell` instead, so a failed step can be
  read, fixed and re-run on its own rather than only at instance creation.
- **The session runs as a `systemd --user` unit with lingering enabled**, not as
  a system service wrapped in `dbus-run-session`. The obvious shape fails: a
  private bus is not the systemd user bus, so gnome-session cannot reach
  `org.freedesktop.systemd1`, finds no logind session, and every required
  component dies behind "Oh no! Something has gone wrong." Related: the session
  wrapper must export `XDG_SESSION_TYPE=x11`, or GNOME's launcher picks Wayland,
  finds no display to attach to, and takes the session down with it. Both are
  worth knowing before changing `harness/provision/common.sh`.
