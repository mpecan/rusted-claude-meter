// DOM rendering for the popover meters and the countdown tick. Kept separate
// from `view-model.ts` so the pure state -> view-model mapping stays testable
// without a DOM, and separate from `main.ts` so wiring (event listeners,
// timers) doesn't tangle with markup construction.
//
// Two layouts (redesign 1a/1c), chosen by the `popover_layout` setting:
//   - "rows": compact hairline-split meter rows in one panel (1a).
//   - "cards": roomier tinted status cards with a status pill (1c).
// The status colour and the escalating fire glyph both follow each window's
// status, which the view-model classified against the user's configured
// warning/critical thresholds.

import { formatCountdown, formatResetClock } from "./format";
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

/** A `<div class="meter-bar">` with a status-coloured fill at `percent`%. */
function meterBar(percent: number, status: UsageStatus, label: string): HTMLElement {
  const bar = document.createElement("div");
  bar.className = "meter-bar";
  bar.setAttribute("role", "progressbar");
  bar.setAttribute("aria-valuemin", "0");
  bar.setAttribute("aria-valuemax", "100");
  bar.setAttribute("aria-valuenow", String(percent));
  bar.setAttribute("aria-label", label);
  const fill = document.createElement("div");
  fill.className = `meter-bar-fill status-${status}`;
  fill.style.width = `${percent}%`;
  bar.append(fill);
  return bar;
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
  const fire = fireGlyph(card.status);
  if (fire) {
    name.append(" ", fire);
  }
  const percent = document.createElement("span");
  percent.className = `meter-percent status-${card.status}`;
  percent.textContent = `${card.percent}%`;
  head.append(name, percent);

  row.append(head, meterBar(card.percent, card.status, card.title), resetLine(card));
  return row;
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

  const pill = document.createElement("span");
  pill.className = `status-pill status-${card.status}`;
  const fire = fireGlyph(card.status);
  if (fire) {
    pill.append(fire);
  }
  pill.append(statusLabel(card.status));
  head.append(name, pill);

  const meter = document.createElement("div");
  meter.className = "status-card-meter";
  const percent = document.createElement("span");
  percent.className = `meter-percent status-${card.status}`;
  percent.textContent = `${card.percent}%`;
  meter.append(meterBar(card.percent, card.status, card.title), percent);

  el.append(head, meter, resetLine(card));
  return el;
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
