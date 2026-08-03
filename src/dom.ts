/** DOM lookups shared by every view.
 *
 * One bundle serves the popover, the Settings panel and the wizard, and all
 * three resolve their elements out of the same `index.html`. They had a
 * byte-identical `requireElement` each; this is that function, once.
 */

/** The element with `id`, or a throw naming what is missing.
 *
 * Throwing rather than returning `null` is the point: every caller resolves
 * its handles once at construction, so a markup rename fails immediately and
 * says which id, instead of surfacing later as a listener that silently never
 * fires.
 */
export function requireElement<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) {
    throw new Error(`missing #${id} in index.html`);
  }
  return el as T;
}
