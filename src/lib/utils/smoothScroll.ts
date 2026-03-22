/**
 * Svelte action that adds Lenis-style lerp smooth scrolling to a container.
 * Intercepts wheel events and applies momentum-based scrolling with lerp.
 *
 * Usage: <div use:smoothScroll> or <div use:smoothScroll={{ lerp: 0.1, multiplier: 1.2 }}>
 */

export interface SmoothScrollOpts {
  /** Lerp factor — lower = smoother/slower (0–1). Default 0.1 */
  lerp?: number;
  /** Wheel delta multiplier. Default 1.2 */
  multiplier?: number;
}

export function smoothScroll(node: HTMLElement, opts?: SmoothScrollOpts) {

  let lerp = opts?.lerp ?? 0.1;
  let multiplier = opts?.multiplier ?? 1.2;

  let targetScroll = node.scrollTop;
  let currentScroll = node.scrollTop;
  let animating = false;
  let rafId = 0;

  function tick() {
    currentScroll += (targetScroll - currentScroll) * lerp;

    // Snap when close enough
    if (Math.abs(targetScroll - currentScroll) < 0.5) {
      currentScroll = targetScroll;
      animating = false;
    }

    node.scrollTop = currentScroll;

    if (animating) {
      rafId = requestAnimationFrame(tick);
    }
  }

  /** Check if an element between the event target and our node can scroll */
  function hasNestedScroll(target: EventTarget | null): boolean {
    let el = target as HTMLElement | null;
    while (el && el !== node) {
      if (el.scrollHeight > el.clientHeight + 1) {
        const style = getComputedStyle(el);
        const ov = style.overflowY;
        if (ov === "auto" || ov === "scroll" || ov === "overlay") {
          return true;
        }
      }
      el = el.parentElement;
    }
    return false;
  }

  function onWheel(e: WheelEvent) {
    // Don't intercept if a nested scrollable element should handle it
    if (hasNestedScroll(e.target)) return;

    e.preventDefault();

    // Sync target with actual scroll position if user scrolled via other means
    if (!animating) {
      targetScroll = node.scrollTop;
      currentScroll = node.scrollTop;
    }

    targetScroll += e.deltaY * multiplier;

    // Clamp to scroll bounds
    const maxScroll = node.scrollHeight - node.clientHeight;
    targetScroll = Math.max(0, Math.min(maxScroll, targetScroll));

    if (!animating) {
      animating = true;
      rafId = requestAnimationFrame(tick);
    }
  }

  node.addEventListener("wheel", onWheel, { passive: false });

  return {
    update(newOpts?: SmoothScrollOpts) {
      lerp = newOpts?.lerp ?? 0.1;
      multiplier = newOpts?.multiplier ?? 1.2;
    },
    destroy() {
      node.removeEventListener("wheel", onWheel);
      if (rafId) cancelAnimationFrame(rafId);
    },
  };
}
