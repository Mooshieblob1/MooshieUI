/**
 * Svelte action: lock wheel scrolling to whatever scrollable is under the
 * cursor when the wheel gesture STARTS.
 *
 * Usage: <div use:wheelScrollLock> on a scrollable section that contains
 * nested scrollables (prompt textareas, lists, etc.).
 *
 * - Gesture starts over a nested scrollable → only that element scrolls,
 *   even when it hits its end (no scroll chaining to the section).
 * - Gesture starts over anything else → only the section scrolls, even if
 *   the cursor drifts over a nested scrollable mid-gesture.
 * - A pause in wheel events ends the gesture; the next wheel event picks a
 *   new target from the cursor position.
 */

const GESTURE_PAUSE_MS = 250;
const LINE_HEIGHT_PX = 16;

function isScrollableY(el: HTMLElement): boolean {
  if (el.scrollHeight <= el.clientHeight + 1) return false;
  const overflowY = getComputedStyle(el).overflowY;
  return overflowY === "auto" || overflowY === "scroll";
}

export function wheelScrollLock(node: HTMLElement) {
  let lockedTarget: HTMLElement | null = null;
  let lastWheelAt = 0;

  function findNestedScrollTarget(start: Element | null): HTMLElement | null {
    let el = start;
    while (el && el !== node) {
      if (el instanceof HTMLElement && isScrollableY(el)) return el;
      el = el.parentElement;
    }
    return null;
  }

  function findScrollTarget(e: WheelEvent): HTMLElement | null {
    // A prompt tag overlay can be the event target while the textarea below is
    // the scrollable under the cursor, so inspect the full hit-test stack.
    for (const el of document.elementsFromPoint(e.clientX, e.clientY)) {
      if (!(el instanceof Element)) continue;
      if (el !== node && !node.contains(el)) continue;

      const nestedTarget = findNestedScrollTarget(el);
      if (nestedTarget) return nestedTarget;
    }

    const eventTarget = e.target instanceof Element ? e.target : null;
    return findNestedScrollTarget(eventTarget) ?? (isScrollableY(node) ? node : null);
  }

  function onWheel(e: WheelEvent) {
    if (e.ctrlKey) return; // pinch-zoom / browser zoom gesture
    if (e.deltaY === 0 || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;

    const gestureStarting = e.timeStamp - lastWheelAt > GESTURE_PAUSE_MS;
    lastWheelAt = e.timeStamp;
    if (gestureStarting || !lockedTarget?.isConnected) {
      lockedTarget = findScrollTarget(e);
    }
    if (!lockedTarget) return;

    e.preventDefault();
    const delta =
      e.deltaMode === WheelEvent.DOM_DELTA_LINE
        ? e.deltaY * LINE_HEIGHT_PX
        : e.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? e.deltaY * lockedTarget.clientHeight
          : e.deltaY;
    lockedTarget.scrollTop += delta;
  }

  // Non-passive: we call preventDefault to stop the browser's own scroll
  // (and its hover-based target pick) once a gesture is locked.
  node.addEventListener("wheel", onWheel, { passive: false });

  return {
    destroy() {
      node.removeEventListener("wheel", onWheel);
    },
  };
}
