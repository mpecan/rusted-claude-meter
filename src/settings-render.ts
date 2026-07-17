// DOM rendering for the Settings panel's per-model visibility toggles. Kept
// separate from `main.ts` for the same reason `render.ts` is: markup
// construction shouldn't tangle with event wiring, and separate from the
// pure `settings-view-model.ts` so *that* stays DOM-free and testable.

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
