# Terms of Service: how this app accesses claude.ai, and why that's a risk

**Short version: the way Rusted Claude Meter reads your usage is not permitted by
Anthropic's Consumer Terms of Service. Using it puts your Claude account at some
risk of suspension or termination. That risk is yours, on your own account, and
you should decide knowingly.**

This page exists because that fact is easy to miss and important to know. It is
written by the project maintainers, not by lawyers, and it is not legal advice —
it is an honest reading of the public terms, with the clauses quoted so you can
check the reasoning yourself.

Last reviewed: **27 July 2026**.

## What the app actually does

There is no official, documented API for reading Claude subscription usage.
This app therefore does what the SwiftUI [ClaudeMeter](https://github.com/eddmann/ClaudeMeter)
it was ported from does:

1. It takes your `claude.ai` **web session cookie** (`sessionKey`) — either pasted
   by you, or read out of your browser's cookie store when you click Import.
2. On a timer (every 1–15 minutes, your choice), it calls an **internal,
   undocumented `claude.ai` usage endpoint** with that cookie.
3. It sends **browser-shaped request headers** — a Chrome `User-Agent`, plus
   `Origin`/`Referer`/`Sec-Fetch-*` set to look like a page request from
   `claude.ai` itself — because the endpoint sits behind Cloudflare and rejects
   obviously non-browser clients. See
   [`crates/meter-api/src/headers.rs`](../crates/meter-api/src/headers.rs).

So: an automated, scheduled, non-browser client, authenticating with a web
session cookie, presenting itself as a browser, against a private endpoint.

## The clauses that apply

### Anthropic Consumer Terms of Service (effective 8 October 2025), Section 3

Section 3 lists what you agree not to do. Two items are directly on point:

> "Except when you are accessing our Services via an Anthropic API Key or where
> we otherwise explicitly permit it, to access the Services through automated or
> non-human means, whether through a bot, script, or otherwise."

> "To crawl, scrape, or otherwise harvest data or information from our Services
> other than as permitted under these Terms."

This app is a script, it accesses the Services automatically, it is not using an
Anthropic API Key, and it has no explicit permission from Anthropic. It harvests
data (your usage numbers) from the Services. On a plain reading, **both clauses
cover what this app does.** We are not aware of any carve-out for read-only,
personal, low-volume access, and the text does not contain one.

The browser-shaped headers make this worse rather than better: they exist
specifically so an automated client is not turned away as an automated client.

### Claude Code "Legal and compliance" (Anthropic docs)

Anthropic's [legal and compliance page](https://code.claude.com/docs/en/legal-and-compliance)
adds, under *Authentication and credential use*:

> "**OAuth authentication** is intended exclusively for purchasers of Claude
> Free, Pro, Max, Team, and Enterprise subscription plans and is designed to
> support ordinary use of Claude Code and other native Anthropic applications."

> "**Developers** building products or services that interact with Claude's
> capabilities … should use API key authentication … Anthropic does not permit
> third-party developers to offer Claude.ai login or to route requests through
> Free, Pro, or Max plan credentials on behalf of their users."

> "Anthropic reserves the right to take measures to enforce these restrictions
> and may do so without prior notice."

This passage is about OAuth tokens and about *routing inference* through
subscription credentials, so it does not describe this app exactly. What it does
establish is the principle — subscription credentials are for Anthropic's own
apps — and the enforcement posture: **without prior notice.**

### Anthropic Usage Policy (effective 15 September 2025)

The [Usage Policy](https://www.anthropic.com/legal/aup) prohibits gaining
"unauthorized access to systems, networks, applications, or devices through
technical attacks or social engineering". We do **not** think this applies: you
are using your own account and your own credential to read your own data. We
mention it only so you know we looked and are not quietly leaning on it.

### Consequences

Consumer Terms Section 13 (*Termination*):

> "We may suspend or terminate your access to the Services at any time without
> notice to you if: we believe that you have materially breached these Terms …"

The exposure is your **Claude account**, not just this app breaking.

## What this app does *not* do

Being straight about the risk means being equally straight about what is not
wrong here, so you can weigh it accurately:

- **No credential sharing.** Consumer Terms Section 2 forbids sharing your login
  or making your account available to others. Your session key stays on your
  machine, in the OS keychain, and is only ever sent to `claude.ai`. It is never
  sent to the maintainers or to any third party — there is no server in this
  project.
- **No inference.** The app never sends prompts, never runs a model, never spends
  your token allowance. It reads counters. This is the opposite end of the
  spectrum from the third-party agent harnesses Anthropic acted against in 2026.
- **No routing on anyone's behalf.** It does not offer "Sign in with Claude", does
  not proxy other people's requests, and has no multi-user component.
- **No rate-limit or paywall circumvention.** It does not unlock capacity you
  have not paid for; it reports capacity you already have.
- **No reverse engineering of the model or the Services** beyond calling one JSON
  endpoint your own browser calls.

## Enforcement history — what we know

Anthropic's posture toward third-party use of subscription credentials moved a
great deal during 2026, in both directions:

| When | What happened |
|---|---|
| Jan 2026 | Anthropic acted against tools "spoofing the Claude Code harness"; subscription OAuth tokens briefly blocked for third-party tools, reversed within days after backlash. |
| Feb 2026 | Anthropic *clarified* (rather than newly introduced) that OAuth tokens from Free/Pro/Max accounts may not be used in other products, tools or services. OpenCode removed Claude subscription support citing legal requests. |
| 4 Apr 2026 | Outright enforcement against third-party agents using subscription credentials, citing "outsized strain" on infrastructure. |
| Jun 2026 | The planned Agent SDK separate-credit billing change was announced, then **cancelled** before taking effect on 15 June; programmatic usage kept drawing on subscription limits. |

Two things follow, and they pull in opposite directions:

- **Against panic:** every publicly reported enforcement action targeted
  *inference* through subscription credentials — the expensive thing. We know of
  **no** public case of an account being actioned for a read-only usage meter,
  and this app's traffic is a handful of JSON GETs per hour.
- **Against complacency:** the policy has changed repeatedly and quickly, it is
  enforced without prior notice, and "nobody has been caught yet" is not
  permission. An endpoint that is undocumented today can start rejecting
  browser-shaped clients, or flagging them, tomorrow.

## What we're doing about it

The durable fix is to stop needing the web session cookie at all: authenticate
with a credential **scoped to read-only usage data, with no inference access**.
That would remove the browser-cookie read, bound the blast radius of a leaked
token, and — most importantly — be a supported way in rather than an
unsupported one.

That is blocked upstream: Anthropic's subscription OAuth uses a hardcoded Claude
Code client id and publishes no usage-read scope, and there is no public usage
API for subscription plans. It is tracked in
[issue #40](https://github.com/mpecan/rusted-claude-meter/issues/40).

If you work at Anthropic and want to talk about a sanctioned way to do this, or
want this project to stop: please open an issue, or contact the maintainer
directly. We would rather be supported than tolerated, and we will comply.

## Your options

- **Accept the risk** — it appears small today, it is not zero, and it is yours.
- **Use a lower refresh interval** to reduce your request volume (Settings →
  Refresh interval).
- **Don't use the app.** If your Claude account matters to you more than a tray
  gauge does — a work account, an account your team depends on, an Enterprise
  seat you don't own — that is an entirely reasonable call, and we would rather
  you make it deliberately than find out later.

## Sources

- [Anthropic Consumer Terms of Service](https://www.anthropic.com/legal/consumer-terms) (effective 8 October 2025)
- [Anthropic Usage Policy](https://www.anthropic.com/legal/aup) (effective 15 September 2025)
- [Claude Code — Legal and compliance](https://code.claude.com/docs/en/legal-and-compliance)
- [The Register — Anthropic clarifies ban on third-party tool access to Claude](https://www.theregister.com/software/2026/02/20/anthropic-clarifies-ban-on-third-party-tool-access-to-claude/5014546) (20 February 2026)
- [VentureBeat — Anthropic cuts off the ability to use Claude subscriptions with OpenClaw and third-party AI agents](https://venturebeat.com/technology/anthropic-cuts-off-the-ability-to-use-claude-subscriptions-with-openclaw-and)
- [The New Stack — Anthropic pauses Claude Agent SDK subscription change](https://thenewstack.io/anthropic-pauses-claude-agent-sdk-subscription-change/)
