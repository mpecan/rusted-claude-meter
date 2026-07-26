// DOM rendering for the popover meters and the countdown tick. Kept separate
// from `view-model.ts` so the pure state -> view-model mapping stays testable
// without a DOM, and separate from `main.ts` so wiring (event listeners,
// timers) doesn't tangle with markup construction.
//
// Two layouts (redesign 1a/1c), chosen by the `popover_layout` setting:
//   - "rows": compact hairline-split meter rows in one panel (1a).
//   - "cards": roomier tinted status cards with a status pill (1c).
// The status colour and the escalating fire glyph both follow each window's
// status. In pace-first mode (issue #16) the primary metric swaps to the pace
// ratio, coloured by its pace band, with the quota % demoted to secondary and
// a flame/snowflake verdict badge — mirroring upstream's `UsageCardView`.

import type { CostViewModel } from "./cost-view-model";
import { describeRemaining, formatCountdown, formatHitTime, formatResetClock } from "./format";
import { RISK_THRESHOLD, UNDERUSE_THRESHOLD } from "./pacing";
import { type UsageStatus, statusLabel } from "./status";
import type { PopoverLayout } from "./types";
import type { BannerKind, UsageCardViewModel } from "./view-model";

/** Rebuild the meter list from scratch in the chosen layout. Called once per
 * broadcast (a few times a minute), so a full replace is simpler than diffing
 * and cheap enough not to matter. */
export function renderCards(
  container: HTMLElement,
  cards: UsageCardViewModel[],
  layout: PopoverLayout,
): void {
  container.dataset.layout = layout;
  if (layout === "cards") {
    container.replaceChildren(...cards.map(buildStatusCard));
    return;
  }
  // Rows share one panel; nothing to render if there are no meters.
  if (cards.length === 0) {
    container.replaceChildren();
    return;
  }
  const panel = document.createElement("div");
  panel.className = "meters-rows";
  cards.forEach((card, i) => {
    if (i > 0) {
      const divider = document.createElement("div");
      divider.className = "meter-divider";
      panel.append(divider);
    }
    panel.append(buildRow(card));
  });
  container.replaceChildren(panel);
}

/** The escalating fire glyph — absent when safe, dim at warning, glowing at
 * critical. Emoji (not SVG) to match the design; the glow is CSS. */
function fireGlyph(status: UsageStatus): HTMLElement | null {
  if (status === "safe") {
    return null;
  }
  const fire = document.createElement("span");
  fire.className = `fire fire-${status}`;
  fire.textContent = "🔥";
  fire.setAttribute("aria-hidden", "true");
  return fire;
}

/** The pace verdict for a ratio: flame overusing / snowflake underusing /
 * check on-pace. Mirrors upstream `UsageCardView.paceVerdict`. */
function paceVerdict(ratio: number): { label: string; glyph: string } {
  if (ratio > RISK_THRESHOLD) {
    return { label: "Overusing", glyph: "🔥" };
  }
  if (ratio < UNDERUSE_THRESHOLD) {
    return { label: "Underusing", glyph: "❄️" };
  }
  return { label: "On Pace", glyph: "✓" };
}

function formatRatio(ratio: number): string {
  return `${ratio.toFixed(1)}×`;
}

/** A `<div class="meter-bar">` with a fill at `percent`% (coloured by
 * `fillModifier`) and, when supplied, an expected-by-now tick. */
function meterBar(
  percent: number,
  fillModifier: string,
  label: string,
  expectedPercent: number | null,
): HTMLElement {
  const bar = document.createElement("div");
  bar.className = "meter-bar";
  bar.setAttribute("role", "progressbar");
  bar.setAttribute("aria-valuemin", "0");
  bar.setAttribute("aria-valuemax", "100");
  bar.setAttribute("aria-valuenow", String(percent));
  bar.setAttribute("aria-label", label);
  const fill = document.createElement("div");
  fill.className = `meter-bar-fill ${fillModifier}`;
  fill.style.width = `${percent}%`;
  bar.append(fill);
  if (expectedPercent !== null) {
    const tick = document.createElement("div");
    tick.className = "meter-tick";
    tick.style.left = `${Math.min(expectedPercent, 100)}%`;
    tick.setAttribute("aria-hidden", "true");
    bar.append(tick);
  }
  return bar;
}

/** The secondary pace line under the meter. In consumption mode it reads
 * "🔥 1.8× pace · 40% expected"; in pace-first mode the ratio is already the
 * primary, so the line demotes the quota "65% used · 40% expected". `null`
 * when there is no pace ratio (grace period / after reset). */
function paceLine(card: UsageCardViewModel): HTMLElement | null {
  if (card.paceRatio === null || card.paceBand === null) {
    return null;
  }
  const line = document.createElement("div");
  line.className = "meter-pace";
  const expected =
    card.expectedPercent === null ? "" : ` · ${Math.round(card.expectedPercent)}% expected`;
  if (card.paceFirst) {
    const used = document.createElement("span");
    used.className = "meter-secondary";
    used.textContent = `${card.percent}% used${expected}`;
    line.append(used);
    return line;
  }
  const pace = document.createElement("span");
  pace.className = `pace-${card.paceBand}`;
  const verdict = paceVerdict(card.paceRatio);
  const showGlyph = card.paceRatio > RISK_THRESHOLD || card.paceRatio < UNDERUSE_THRESHOLD;
  pace.textContent = `${showGlyph ? `${verdict.glyph} ` : ""}${formatRatio(card.paceRatio)} pace`;
  line.append(pace);
  if (expected) {
    const exp = document.createElement("span");
    exp.className = "meter-expected";
    exp.textContent = expected;
    line.append(exp);
  }
  return line;
}

/** The current-rate projection line ("Limit reached" / "Hits limit ~1:10 PM,
 * 50 minutes before reset" / "On pace to end at ~29% (71% unused)"). `null`
 * when nothing can be projected yet. */
function projectionLine(card: UsageCardViewModel): HTMLElement | null {
  const projection = card.projection;
  if (projection === null) {
    return null;
  }
  const el = document.createElement("div");
  el.className = "meter-projection";
  switch (projection.kind) {
    case "reached":
      el.classList.add("projection-reached");
      el.textContent = "Limit reached";
      break;
    case "hits": {
      el.classList.add("projection-hits");
      // Colour the line by pace severity (orange overuse, red heavy overuse)
      // instead of a fixed warning, so it matches the ratio. Mirrors upstream
      // `UsageCardView.projectionLine` using `PacePalette.color(for:)`.
      if (card.paceBand !== null) {
        el.classList.add(`pace-${card.paceBand}`);
      }
      const at = formatHitTime(new Date(projection.hitAt), new Date());
      el.textContent = `Hits limit ~${at}, ${describeRemaining(projection.secondsBeforeReset)} before reset`;
      break;
    }
    case "ends":
      el.classList.add("projection-ends");
      if (projection.unusedPercent !== null) {
        el.classList.add("projection-underuse");
        el.textContent = `On pace to end at ~${projection.endPercent}% (${projection.unusedPercent}% unused)`;
      } else {
        el.textContent = `On pace to end at ~${projection.endPercent}%`;
      }
      break;
  }
  return el;
}

/** Fill-colour modifier for the meter bar and primary metric: the pace band in
 * pace-first mode, the quota status otherwise. */
function primaryModifier(card: UsageCardViewModel): string {
  return card.paceFirst && card.paceBand !== null ? `pace-${card.paceBand}` : `status-${card.status}`;
}

/** The primary metric text: the pace ratio in pace-first mode, the quota % otherwise. */
function primaryMetric(card: UsageCardViewModel): HTMLElement {
  const span = document.createElement("span");
  span.className = `meter-percent ${primaryModifier(card)}`;
  span.textContent =
    card.paceFirst && card.paceRatio !== null ? formatRatio(card.paceRatio) : `${card.percent}%`;
  return span;
}

/** The reset line: the live countdown plus, when enabled, the exact reset
 * time after a "·" separator. The countdown is a separate span so the tick
 * can rewrite it without touching the static exact time. */
function resetLine(card: UsageCardViewModel): HTMLElement {
  const reset = document.createElement("div");
  reset.className = "meter-reset";
  const countdown = document.createElement("span");
  countdown.className = "countdown";
  countdown.textContent = formatCountdown(new Date(card.resetsAt), new Date());
  reset.append(countdown);
  if (card.showResetTime) {
    const clock = document.createElement("span");
    clock.className = "reset-clock";
    clock.textContent = ` · ${formatResetClock(new Date(card.resetsAt), card.useTimeOnlyResetTime)}`;
    reset.append(clock);
  }
  return reset;
}

/** One compact meter row (layout 1a). */
function buildRow(card: UsageCardViewModel): HTMLElement {
  const row = document.createElement("div");
  row.className = "meter-row";
  row.dataset.resetsAt = card.resetsAt;
  row.dataset.status = card.status;

  const head = document.createElement("div");
  head.className = "meter-row-head";
  const name = document.createElement("span");
  name.className = "meter-name";
  name.textContent = card.title;
  // Pace-first leads the name with the verdict glyph; consumption keeps the
  // escalating fire.
  if (card.paceFirst && card.paceRatio !== null) {
    const glyph = document.createElement("span");
    glyph.className = "fire";
    glyph.setAttribute("aria-hidden", "true");
    glyph.textContent = paceVerdict(card.paceRatio).glyph;
    name.append(" ", glyph);
  } else {
    const fire = fireGlyph(card.status);
    if (fire) {
      name.append(" ", fire);
    }
  }
  head.append(name, primaryMetric(card));

  row.append(head, meterBar(card.percent, primaryModifier(card), card.title, card.expectedPercent));
  const pace = paceLine(card);
  if (pace) {
    row.append(pace);
  }
  const projection = projectionLine(card);
  if (projection) {
    row.append(projection);
  }
  row.append(resetLine(card));
  return row;
}

/** The header badge: the pace verdict in pace-first mode, the quota status
 * otherwise. */
function headBadge(card: UsageCardViewModel): HTMLElement {
  const pill = document.createElement("span");
  if (card.paceFirst && card.paceRatio !== null && card.paceBand !== null) {
    const verdict = paceVerdict(card.paceRatio);
    pill.className = `status-pill pace-${card.paceBand}`;
    const glyph = document.createElement("span");
    glyph.setAttribute("aria-hidden", "true");
    glyph.textContent = verdict.glyph;
    pill.append(glyph, verdict.label);
    return pill;
  }
  pill.className = `status-pill status-${card.status}`;
  const fire = fireGlyph(card.status);
  if (fire) {
    pill.append(fire);
  }
  pill.append(statusLabel(card.status));
  return pill;
}

/** One roomier status card (layout 1c). */
function buildStatusCard(card: UsageCardViewModel): HTMLElement {
  const el = document.createElement("article");
  el.className = `status-card status-${card.status}`;
  el.dataset.resetsAt = card.resetsAt;
  el.dataset.status = card.status;

  const head = document.createElement("div");
  head.className = "status-card-head";
  const name = document.createElement("span");
  name.className = "meter-name";
  name.textContent = card.title;
  head.append(name, headBadge(card));

  const meter = document.createElement("div");
  meter.className = "status-card-meter";
  meter.append(
    meterBar(card.percent, primaryModifier(card), card.title, card.expectedPercent),
    primaryMetric(card),
  );

  el.append(head, meter);
  const pace = paceLine(card);
  if (pace) {
    el.append(pace);
  }
  const projection = projectionLine(card);
  if (projection) {
    el.append(projection);
  }
  el.append(resetLine(card));
  return el;
}

// ---- Token/cost view (Enterprise accounts + the allowance cost-summary) ----

/** Title of the spend card, in both the full cost view and the allowance-mode
 * summary — the same card in both places. */
const COST_CARD_TITLE = "Spend this period";

/** The spend card: spend to date, an optional budget gauge with its percentage
 * caption, and the hard cap when it differs from the budget. Shared by the full
 * cost view (`renderCostCards`) and the allowance-mode summary (appended after
 * the limit cards by the popover). */
export function buildCostCard(vm: CostViewModel): HTMLElement {
  const el = document.createElement("article");
  el.className = "cost-card";
  if (vm.gauge) {
    el.classList.add(`status-${vm.gauge.status}`);
  }

  const head = document.createElement("div");
  head.className = "status-card-head";
  const name = document.createElement("span");
  name.className = "meter-name";
  name.textContent = COST_CARD_TITLE;
  const value = document.createElement("span");
  value.className = `meter-percent ${vm.gauge ? `status-${vm.gauge.status}` : ""}`.trim();
  value.textContent = vm.used ?? "—";
  head.append(name, value);
  el.append(head);

  if (vm.gauge) {
    const meter = document.createElement("div");
    meter.className = "status-card-meter";
    meter.append(
      meterBar(vm.gauge.percent, `status-${vm.gauge.status}`, `${COST_CARD_TITLE} vs budget`, null),
    );
    el.append(meter);
    const caption = document.createElement("div");
    caption.className = "cost-caption";
    caption.textContent = `${vm.gauge.percent}% of ${vm.gauge.budget}`;
    el.append(caption);
  }

  if (vm.cap) {
    const cap = document.createElement("div");
    cap.className = "cost-caption";
    cap.textContent = `Hard cap ${vm.cap}`;
    el.append(cap);
  }
  return el;
}

/** A light one-time hint, shown when Auto resolves to the cost view so the
 * switch to a spend-based display is explained (and points at Settings). */
function buildCostHint(text: string): HTMLElement {
  const hint = document.createElement("p");
  hint.className = "cost-hint";
  hint.textContent = text;
  return hint;
}

/** Render the full cost view: an optional hint, then the spend card. Replaces
 * the card container's contents like `renderCards`. */
export function renderCostCards(
  container: HTMLElement,
  vm: CostViewModel,
  hint: string | null = null,
): void {
  container.dataset.layout = "cost";
  const nodes: HTMLElement[] = [];
  if (hint) {
    nodes.push(buildCostHint(hint));
  }
  nodes.push(buildCostCard(vm));
  container.replaceChildren(...nodes);
}

/** Re-read every rendered meter's `data-resets-at` and refresh its countdown
 * text. Independent of any state broadcast — the client-side tick that makes
 * "resets in 2h 14m" count down without hitting the API again. Works for both
 * layouts (each meter carries `data-resets-at` and a `.countdown`). */
export function tickCountdowns(container: HTMLElement, now: Date): void {
  for (const el of container.querySelectorAll<HTMLElement>("[data-resets-at]")) {
    const resetsAt = el.dataset.resetsAt;
    const countdown = el.querySelector<HTMLElement>(".countdown");
    if (!resetsAt || !countdown) {
      continue;
    }
    countdown.textContent = formatCountdown(new Date(resetsAt), now);
  }
}

/** Apply the banner kind and its message to the status-line element. The
 * `data-banner` attribute drives the colour/styling in `styles.css`. */
export function applyBanner(el: HTMLElement, kind: BannerKind, statusLine: string): void {
  el.textContent = statusLine;
  el.dataset.banner = kind;
}
