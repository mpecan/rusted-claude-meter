// DOM rendering for usage cards and the countdown tick. Kept separate from
// `view-model.ts` so the pure state -> view-model mapping stays testable
// without a DOM, and separate from `main.ts` so wiring (event listeners,
// timers) doesn't tangle with markup construction.

import { formatCountdown, formatResetClock } from "./format";
import type { BannerKind, UsageCardViewModel } from "./view-model";

/** A minimal flame glyph for the pacing-at-risk badge — drawn, not an emoji,
 * so it inherits the card's status colour via `currentColor`. */
const FLAME_SVG =
  '<svg viewBox="0 0 16 16" class="flame-icon" aria-hidden="true" focusable="false">' +
  '<path fill="currentColor" d="M8 .6c.8 2.1 0 3-.8 4-.9 1.1-1.6 2.1-1.6 3.5a2.4 2.4 0 0 0 4.8 0 ' +
  "c0-.6-.1-1-.4-1.5 1 .8 1.7 2.1 1.7 3.6A3.7 3.7 0 0 1 8 13.9a3.7 3.7 0 0 1-3.7-3.7C4.3 6.4 6.8 5 8 .6z\" />" +
  "</svg>";

/** Rebuild the card list from scratch. Called once per broadcast state
 * (at most a few times a minute), so a full replace is simpler than diffing
 * and cheap enough not to matter. */
export function renderCards(container: HTMLElement, cards: UsageCardViewModel[]): void {
  container.replaceChildren(...cards.map(buildCard));
}

function buildCard(card: UsageCardViewModel): HTMLElement {
  const el = document.createElement("article");
  el.className = "card";
  el.dataset.cardId = card.id;
  el.dataset.resetsAt = card.resetsAt;
  el.dataset.status = card.status;

  const header = document.createElement("div");
  header.className = "card-header";
  const title = document.createElement("span");
  title.className = "card-title";
  title.textContent = card.title;
  const percent = document.createElement("span");
  percent.className = "card-percent";
  percent.textContent = `${card.percent}%`;
  header.append(title, percent);

  const track = document.createElement("div");
  track.className = "progress-track";
  track.setAttribute("role", "progressbar");
  track.setAttribute("aria-valuemin", "0");
  track.setAttribute("aria-valuemax", "100");
  track.setAttribute("aria-valuenow", String(card.percent));
  track.setAttribute("aria-label", card.title);
  const fill = document.createElement("div");
  fill.className = `progress-fill status-${card.status}`;
  fill.style.width = `${card.percent}%`;
  track.append(fill);

  const footer = document.createElement("div");
  footer.className = "card-footer";
  const countdown = document.createElement("span");
  countdown.className = "countdown";
  countdown.textContent = formatCountdown(new Date(card.resetsAt), new Date());
  footer.append(countdown);
  if (card.showResetTime) {
    // A separate, static span the per-minute countdown tick never rewrites —
    // the exact reset time (ClaudeMeter PR #26).
    const clock = document.createElement("span");
    clock.className = "reset-clock";
    clock.textContent = ` (${formatResetClock(new Date(card.resetsAt), card.useTimeOnlyResetTime)})`;
    footer.append(clock);
  }
  if (card.atRisk) {
    const flame = document.createElement("span");
    flame.className = "flame";
    flame.title = "Pacing faster than a sustainable rate";
    flame.innerHTML = FLAME_SVG;
    footer.append(flame);
  }

  el.append(header, track, footer);
  return el;
}

/** Re-read every rendered card's `data-resets-at` and refresh its countdown
 * text. Independent of any state broadcast — the client-side tick that
 * makes "resets in 2h 14m" count down without hitting the API again. */
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
