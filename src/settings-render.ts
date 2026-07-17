// DOM rendering for the Settings panel's per-model visibility toggles, plus
// a small shared helper for populating `<select>` elements from a data list.
// Kept separate from `main.ts` for the same reason `render.ts` is: markup
// construction shouldn't tangle with event wiring, and separate from the
// pure `settings-view-model.ts` so *that* stays DOM-free and testable.

import type { SelectOption } from "./types";

/** Rebuild a `<select>`'s `<option>` list from a single shared data source
 * (see `types.ts::ICON_STYLE_OPTIONS` / `REFRESH_INTERVAL_OPTIONS`), so the
 * Settings panel's selects and the wizard's customize-step selects can't
 * drift out of sync with each other. */
export function renderSelectOptions<T extends string>(
  select: HTMLSelectElement,
  options: readonly SelectOption<T>[],
): void {
  select.replaceChildren(
    ...options.map((option) => {
      const el = document.createElement("option");
      el.value = option.value;
      el.textContent = option.label;
      return el;
    }),
  );
}

/** Rebuild the model-toggle list from scratch — called once per snapshot
 * update or Settings-panel open, cheap enough not to matter (mirrors
 * `render.ts::renderCards`). */
export function renderModelToggles(
  container: HTMLElement,
  names: readonly string[],
  shown: ReadonlySet<string>,
  onToggle: (name: string, enabled: boolean) => void,
): void {
  if (names.length === 0) {
    const empty = document.createElement("p");
    empty.className = "model-toggles-empty";
    empty.textContent = "No model-scoped limits reported yet.";
    container.replaceChildren(empty);
    return;
  }
  container.replaceChildren(...names.map((name) => buildToggle(name, shown.has(name), onToggle)));
}

function buildToggle(
  name: string,
  checked: boolean,
  onToggle: (name: string, enabled: boolean) => void,
): HTMLElement {
  const label = document.createElement("label");
  label.className = "model-toggle";
  const input = document.createElement("input");
  input.type = "checkbox";
  input.checked = checked;
  input.addEventListener("change", () => onToggle(name, input.checked));
  const span = document.createElement("span");
  span.textContent = name;
  label.append(input, span);
  return label;
}
