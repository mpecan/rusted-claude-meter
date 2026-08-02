# Reading usage from Claude Code

Rusted Claude Meter can take its numbers from **Claude Code** instead of polling claude.ai.

This is the one source that makes **no claude.ai request at all**, so it sidesteps the Terms-of-Service problem described in [terms-of-service.md](terms-of-service.md) entirely — there is no automated access, no session cookie, and nothing to acknowledge. It also reports less. Both halves of that trade are below; pick deliberately.

---

## How it works

Claude Code hands a JSON blob to whatever command you name in `statusLine.command`, on stdin, every time it redraws your status line. Since **Claude Code 2.1.216** that blob includes your plan's rate limits:

```json
{
  "rate_limits": {
    "five_hour": { "used_percentage": 14.0, "resets_at": 1785689400 },
    "seven_day": { "used_percentage": 3,    "resets_at": 1786273200 }
  }
}
```

Claude Code derives those from the `anthropic-ratelimit-unified-5h-*` and `-7d-*` response headers on **your own** API traffic. It is already making those requests, as itself, for its own reasons. All this app does is read what it hands over.

The bridge (`rusted-claude-meter statusline`) records each reading to `~/.claudemeter/statusline.json`, and the meter reads that file on its normal refresh interval.

> **Not to be confused with `~/.claudemeter/usage.json`.** That file is the app's *output*, for external scripts (see the README's External integrations). `statusline.json` flows the other way — into the app.

---

## Requirements

| | |
|---|---|
| **Claude Code** | **2.1.216 or newer.** Older versions omit `rate_limits` entirely. |
| **Rusted Claude Meter** | 0.1.7 or newer — earlier builds have no `statusline` subcommand and will **launch the GUI** if you point a status line at them. |
| **Claude Code auth** | A Claude subscription (Pro/Max/Team). Claude Code reports no rate limits at all on API-key, Bedrock or Vertex sessions. |

---

## Setup

### 1. Switch the source

**Settings → Usage source → Read from Claude Code.**

The setup block that appears contains the exact command for your install, with the binary's real path already filled in and quoted. Use **Copy command** — the path is not guessable, especially on macOS where the binary lives inside the app bundle.

### 2. Add it to your status line

Claude Code gives its status-line data to **exactly one command**, so this is designed to be added to whatever you already have rather than to replace it.

#### The easy way: let Claude Code do it

In any Claude Code session, run:

```
/statusline add the Rusted Claude Meter usage segment exactly as described in ~/.claudemeter/statusline-command.txt
```

That file is written by the app on every launch and holds this machine's exact command plus instructions for merging it into an existing status line. Naming it is the whole trick: the agent behind `/statusline` can read files and edit them and *nothing else*, so it cannot run the binary to discover where the binary is — but it can read a file that already says.

Settings has a **Copy /statusline command** button for the line above.

#### By hand

Open `~/.claude/settings.json` and find (or create) the `statusLine` block. If you have nothing yet, the copied command works as-is:

```json
{
  "statusLine": {
    "type": "command",
    "command": "input=$(cat); meter=$(printf '%s' \"$input\" | '/Applications/Rusted Claude Meter.app/Contents/MacOS/rusted-claude-meter' statusline); printf '%s' \"$meter\""
  }
}
```

If you already have a status line, keep it and splice in the middle piece. Your command almost certainly starts by consuming stdin — reuse that same `$input` rather than reading stdin twice, which would hang:

```sh
input=$(cat)
meter=$(printf '%s' "$input" | '<path>' statusline)
printf "%s@%s %s" "$(whoami)" "$(hostname -s)" "$meter"
```

The bridge prints one short segment — `5h 14% · 7d 3%` — and records the reading as a side effect. Put `$meter` wherever you want it.

**Want the recording but not the text?** Add `--quiet` and the bridge prints nothing:

```sh
printf '%s' "$input" | '<path>' statusline --quiet
```

### 3. Check it

Prompt Claude Code once. Within your refresh interval the tray should stop saying *"Waiting for Claude Code to report usage"*. You can confirm the bridge is firing at all with:

```sh
cat ~/.claudemeter/statusline.json
```

---

## The files in `~/.claudemeter/`

| File | Direction | What it is |
|---|---|---|
| `usage.json` | **out** | The app's public export for external scripts (see the README). |
| `statusline.json` | **in** | The reading the bridge records; what the meter reads. |
| `statusline-command.txt` | out | This machine's setup command, for `/statusline` and for you. Rewritten on every launch. |

---

## What this source reports

**It gives you** the 5-hour and 7-day headline windows: percentage used, reset time, and everything the app derives from those — pace, projections, notifications, tray icon and popover cards.

**It does not give you:**

- **Model-scoped limits.** The payload has no per-model breakdown, so the Model-scoped limits section has nothing to show.
- **Spend / cost view.** Not in the payload either.
- **Updates while Claude Code is closed.** The file only changes when Claude Code redraws its status line. Leave it for an afternoon and your numbers are an afternoon old — the app says so rather than presenting them as current (*"Claude Code last reported 3h ago"*, and the usual stale styling).
- **Anything before Claude Code's first API call.** `rate_limits` is absent until then, even on a supported version.

Nothing here can be fixed in this app; they are properties of what Claude Code exposes.

---

## Troubleshooting

**The tray says "Waiting for Claude Code to report usage" and never changes.**
Check `~/.claudemeter/statusline.json` exists. If it does not, the bridge is not running: verify the path in your `statusLine.command` is right, and that running it by hand prints a segment:

```sh
echo '{"rate_limits":{"five_hour":{"used_percentage":50,"resets_at":1785689400}}}' \
  | '<path>' statusline
```

**Claude Code freezes, or a window opens, when the status line renders.**
You are on a Rusted Claude Meter older than 0.1.7. It does not recognise `statusline` as a subcommand, so it treats it as a normal launch and starts the app. Upgrade.

**The file exists but the numbers never change.**
Claude Code is probably on an unsupported auth mode (API key, Bedrock, Vertex) or a version below 2.1.216 — in both cases it emits no `rate_limits`, and the bridge deliberately records nothing rather than overwriting a good reading with an empty one. Check `claude --version`.

**My status line went blank.**
The bridge prints nothing when it has nothing to report, which is normal on a cold session. If your own segments vanished too, `$input` is likely being consumed twice — capture stdin once into `input` and pipe a copy, as above.

---

## Switching back

**Settings → Usage source → Poll claude.ai.** The Terms-of-Service acknowledgement applies again from that moment; if you never accepted it, the meter parks until you do. Removing the status-line command is optional — a recorded file nobody reads is harmless.
