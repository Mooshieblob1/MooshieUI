/**
 * Svelte action that moves an element to `document.body`, so a modal or popup
 * always covers the whole app instead of the panel that rendered it.
 *
 * `app.css` gives every `.overflow-y-auto` / `.overflow-auto` element
 * `contain: content`, and the side, bottom and mobile panels sit inside
 * transformed or blurred wrappers. Each of those becomes the containing block
 * for `position: fixed` descendants, so a `fixed inset-0` overlay rendered in a
 * panel is laid out (and clipped) against that panel instead of the viewport.
 * Portaling escapes all of them.
 *
 * Put it on the outermost overlay element, and keep that element the only
 * top-level node of its `{#if}` branch or component. Svelte removes a block by
 * walking siblings from its first node to its last; if the portaled element is
 * the first or last of several, that walk runs through `document.body` or the
 * old parent and removes unrelated nodes. Wrap it in its own `{#if}` when it
 * needs siblings.
 */
export function portal(node: HTMLElement) {
  // Child actions (autofocus helpers) run before this one, and moving a node
  // drops focus from anything inside it, so put focus back afterwards.
  const active = document.activeElement;
  document.body.appendChild(node);
  if (active instanceof HTMLElement && active !== document.activeElement && node.contains(active)) {
    active.focus({ preventScroll: true });
  }
  return {
    destroy() {
      node.remove();
    },
  };
}
