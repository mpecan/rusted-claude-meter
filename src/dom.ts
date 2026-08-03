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

/** The descendant of `root` marked `data-role="<role>"`, or a throw naming
 * what is missing.
 *
 * [`requireElement`] resolves a *global* id, which silently caps a component
 * at one instance per document. This is the same contract scoped to a
 * subtree, for blocks that legitimately appear more than once — the
 * status-line setup block is in both Settings and the wizard. Roles rather
 * than ids because a duplicated id is invalid HTML and `getElementById`
 * would hand both instances the first one's elements.
 */
export function requireChild<T extends HTMLElement>(root: HTMLElement, role: string): T {
  const el = root.querySelector<T>(`[data-role="${role}"]`);
  if (!el) {
    throw new Error(`missing [data-role="${role}"] inside #${root.id} in index.html`);
  }
  return el;
}
