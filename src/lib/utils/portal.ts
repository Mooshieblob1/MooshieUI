/**
 * Svelte action that moves an element to `document.body`.
 *
 * `app.css` gives every `.overflow-y-auto` / `.overflow-auto` element
 * `contain: content`, which makes it the containing block for `position: fixed`
 * descendants. A `fixed inset-0` overlay rendered inside one of those scroll
 * areas is laid out against the scroll content instead of the viewport, so it
 * opens off screen once the area is scrolled. Portaling escapes that.
 */
export function portal(node: HTMLElement) {
  document.body.appendChild(node);
  return {
    destroy() {
      node.remove();
    },
  };
}
