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

### Optional: show pace

Add `--pace` and the segment gains an off-pace signal — the one thing Claude Code cannot tell you itself, since it reports a percentage but has no notion of whether you are spending it faster than the window replenishes:

```
5h 95% · 7d 40% · 🔥1.6×      burning ~1.6× sustainable pace
5h 14% · 7d 3%  · ❄️0.6×      on track to leave quota unspent
5h 37% · 7d 61%               nothing worth saying
```

It is **quiet by default**, and deliberately quieter than the tray: the flame needs 1.2× before it appears, not the 1.0× the tray badge uses. A status line lives in your prompt permanently and carries no context, so a ratio hovering either side of 1.0 would flicker in and out all afternoon and render as a self-contradictory `🔥1.0×`. Underuse shows below 0.8×.

Pace also needs 5% of a window elapsed before it means anything, so a freshly reset 5-hour window shows nothing for its first ~15 minutes.

Whether pace *shows* is this flag's business — the status line is its own surface, and the tray's own display preferences do not reach it. But the **numbers** come from the app: it mirrors your weekly pace basis and the pace master switch into `~/.claudemeter/statusline-config.json` on every settings change, and the bridge reads it fresh on each render. That matters more than it sounds: the same 75% weekly usage is `🔥1.3×` paced over 7 days and no signal at all paced over 5. Turning pace tracking off in Settings silences it here too.

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
| `statusline-config.json` | out | Your pace basis and pace master switch, mirrored for the bridge. Rewritten on every settings change. |

---

## What "no claude.ai requests" means, exactly

While this source is selected, **every** path that could reach claude.ai refuses:

- The scheduler reads the recorded file and never builds an HTTP client.
- Pasting a session key is refused before the key is stored — validating one is itself a claude.ai request.
- Importing a session from a browser is refused before the cookie store is read, so you get no keychain prompt either.

That is asserted rather than assumed: the test suite stands up a healthy mock claude.ai server, points the app at it with a valid stored key and consent granted, polls repeatedly, and requires that the server received **zero** requests — with a sibling test proving the same server *is* reached when the source is claude.ai, so the first cannot pass by accident.

Settings dims the Session section on this source and says why, so you are not invited to paste a key that would be refused.

---

## What this source reports

**It gives you** the 5-hour and 7-day headline windows: percentage used, reset time, and everything the app derives from those — pace, projections, notifications, tray icon and popover cards.

**It does not give you:**

- **Model-scoped limits.** The payload has no per-model breakdown, so the Model-scoped limits section has nothing to show.
- **Spend / cost view.** Not in the payload either.
- **Updates while Claude Code is closed.** The file only changes when Claude Code redraws its status line. Leave it for an afternoon and your numbers are an afternoon old — the app says so rather than presenting them as current (*"Claude Code last reported 3h ago"*, and the usual stale styling).

---

## How current the meter is

On this source the meter re-reads the file **every 15 seconds**, regardless of your refresh interval. That setting exists to pace requests to claude.ai, and there are none here — pacing a local file read at five minutes would leave the tray five minutes behind data that is seconds old. So the tray is at most ~15s behind whatever Claude Code last reported, and **Refresh Now** is immediate (the 55-second memory cache that protects claude.ai from repeated manual refreshes does not apply to a file read).

Your refresh interval still decides when a reading is called **stale** — at 2× the interval — which is the question that still matters here: not "how often do we look" but "how old may this get before it stops being worth trusting". Leave Claude Code closed for long enough and the tray says so.

Polling this fast does not churn the disk: the cache and the public `usage.json` export are only rewritten when the reading actually changed.
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
