/**
 * Svelte action: click-to-capture scroll wheel on slider controls.
 *
 * Usage: <div use:scrollCapture> wrapping a <label> + <input type="range">.
 * - Click the slider thumb or value label to capture.
 * - While captured, ALL scroll wheel events (anywhere on page) adjust the slider.
 * - Click anywhere else to release capture.
 * - Adds `.scroll-captured` class on the wrapper while active.
 */

let capturedElement: HTMLElement | null = null;
export function scrollCapture(node: HTMLElement) {
  let captured = false;

  function findRange(): HTMLInputElement | null {
    return node.querySelector('input[type="range"]');
  }

  function capture() {
    if (captured) return;
    // Release any previously captured element
    if (capturedElement && capturedElement !== node) {
      capturedElement.classList.remove("scroll-captured");
      capturedElement.dispatchEvent(new CustomEvent("scrollcapturerelease"));
    }
    captured = true;
    capturedElement = node;
    node.classList.add("scroll-captured");
    // Take over ALL wheel events on the entire page
    document.addEventListener("wheel", onWheel, { passive: false, capture: true });
  }

  function release() {
    if (!captured) return;
    captured = false;
    if (capturedElement === node) capturedElement = null;
    node.classList.remove("scroll-captured");
    document.removeEventListener("wheel", onWheel, true);
  }

  function onClick(e: MouseEvent) {
    const target = e.target as HTMLElement;

    // Check if click is on the range input (thumb or track)
    const range = findRange();
    if (range && (target === range || range.contains(target))) {
      capture();
      return;
    }

    // Check if click is on a value label (EditableValue button or number display)
    if (
      target.closest("button.tabular-nums") ||
      target.closest(".editable-value") ||
      target.tagName === "INPUT"
    ) {
      capture();
      return;
    }
  }

  function onWheel(e: WheelEvent) {
    const range = findRange();
    if (!range) return;

    e.preventDefault();
    e.stopPropagation();

    // Cancel any active text edit (EditableValue) so it doesn't override
    // the scroll-adjusted value when it eventually blurs
    const activeEl = document.activeElement;
    if (
      activeEl instanceof HTMLInputElement &&
      activeEl.type !== "range" &&
      node.contains(activeEl)
    ) {
      // Remove the blur handler temporarily so commit() doesn't fire,
      // then restore the display to reactive mode
      activeEl.blur();
    }

    const min = parseFloat(range.min) || 0;
    const max = parseFloat(range.max) || 100;
    const step = parseFloat(range.step) || 1;
    const current = parseFloat(range.value) || 0;

    // Scroll down (positive deltaY) = decrease, scroll up = increase
    const direction = e.deltaY > 0 ? -1 : 1;
    const newValue = Math.min(max, Math.max(min, current + step * direction));

    // Round to step precision to avoid float drift
    const decimals = (step.toString().split(".")[1] || "").length;
    range.value = newValue.toFixed(decimals);

    // Trigger input event so Svelte's bind:value picks up the change
    range.dispatchEvent(new Event("input", { bubbles: true }));
  }

  function onDocumentClick(e: MouseEvent) {
    if (!captured) return;
    // If click is outside our node, release
    if (!node.contains(e.target as Node)) {
      release();
    }
  }

  function onRelease() {
    release();
  }

  node.addEventListener("click", onClick);
  node.addEventListener("scrollcapturerelease", onRelease);
  document.addEventListener("click", onDocumentClick, true);

  return {
    destroy() {
      release();
      node.removeEventListener("click", onClick);
      node.removeEventListener("scrollcapturerelease", onRelease);
      document.removeEventListener("click", onDocumentClick, true);
    },
  };
}
