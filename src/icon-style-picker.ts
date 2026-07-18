// Visual icon-style picker: a grid of selectable buttons, each showing the
// actual rendered tray artwork for that style (from `icon_style_previews`),
// replacing the plain `<select>` in Settings and the setup wizard. Mirrors
// ClaudeMeter's `IconStylePicker`. DOM-only; the selection/persistence policy
// stays with the caller via `onSelect`.

import { ICON_STYLE_OPTIONS, type IconPreview, type IconStyle } from "./types";

export interface IconStylePickerHandle {
  /** Reflect an externally-changed selection (e.g. "Run setup again" reload). */
  setSelected(style: IconStyle): void;
  /** Redraw the previews (e.g. after they finish loading). */
  setPreviews(previews: readonly IconPreview[]): void;
}

/** Paint one straight-alpha RGBA preview into a canvas, sized to its logical
 * (1x) dimensions — the previews render at 2x for crispness on HiDPI. */
function drawPreview(canvas: HTMLCanvasElement, preview: IconPreview): void {
  canvas.width = preview.width;
  canvas.height = preview.height;
  canvas.style.width = `${preview.width / 2}px`;
  canvas.style.height = `${preview.height / 2}px`;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  const image = new ImageData(new Uint8ClampedArray(preview.rgba), preview.width, preview.height);
  ctx.putImageData(image, 0, 0);
}

/** Build the picker into `container`. Buttons render immediately (label-only);
 * call `setPreviews` once `iconStylePreviews()` resolves to draw the artwork.
 * `onSelect` fires only on a real user change to a different style. */
export function createIconStylePicker(
  container: HTMLElement,
  initial: IconStyle,
  onSelect: (style: IconStyle) => void,
): IconStylePickerHandle {
  container.replaceChildren();
  container.classList.add("icon-style-picker");
  container.setAttribute("role", "radiogroup");
  container.setAttribute("aria-label", "Tray icon style");

  let selected = initial;
  const buttons = new Map<IconStyle, HTMLButtonElement>();
  const canvases = new Map<IconStyle, HTMLCanvasElement>();

  function applySelectionState(): void {
    for (const [style, button] of buttons) {
      const isSelected = style === selected;
      button.classList.toggle("is-selected", isSelected);
      button.setAttribute("aria-checked", isSelected ? "true" : "false");
      button.tabIndex = isSelected ? 0 : -1;
    }
  }

  for (const option of ICON_STYLE_OPTIONS) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "icon-style-option";
    button.setAttribute("role", "radio");
    button.dataset.style = option.value;

    const canvas = document.createElement("canvas");
    canvas.className = "icon-style-option-preview";
    const label = document.createElement("span");
    label.className = "icon-style-option-label";
    label.textContent = option.label;

    button.append(canvas, label);
    button.addEventListener("click", () => {
      if (selected === option.value) {
        return;
      }
      selected = option.value;
      applySelectionState();
      onSelect(option.value);
    });

    buttons.set(option.value, button);
    canvases.set(option.value, canvas);
    container.append(button);
  }

  applySelectionState();

  return {
    setSelected(style: IconStyle): void {
      selected = style;
      applySelectionState();
    },
    setPreviews(previews: readonly IconPreview[]): void {
      for (const preview of previews) {
        const canvas = canvases.get(preview.style);
        if (canvas) {
          drawPreview(canvas, preview);
        }
      }
    },
  };
}
