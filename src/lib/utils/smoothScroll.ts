/**
 * Svelte action that adds Lenis-style smooth scrolling to a container.
 * Uses time-based interpolation with exponential decay for consistent
 * smoothness regardless of frame rate.
 *
 * Usage: <div use:smoothScroll> or <div use:smoothScroll={{ duration: 1.2, multiplier: 1.8 }}>
 */

export interface SmoothScrollOpts {
  /** Scroll duration in seconds — higher = smoother/slower. Default 1.2 */
  duration?: number;
  /** Wheel delta multiplier — amplifies scroll distance. Default 1.8 */
  multiplier?: number;
}

export function smoothScroll(node: HTMLElement, opts?: SmoothScrollOpts) {

  let duration = opts?.duration ?? 1.2;
  let multiplier = opts?.multiplier ?? 1.8;

  let targetScroll = node.scrollTop;
  let currentScroll = node.scrollTop;
  let animating = false;
  let rafId = 0;
  let lastTime = 0;

  /**
   * Time-based exponential decay interpolation.
   * Uses 1 - e^(-dt/tau) so the animation converges at the same
   * rate regardless of frame timing. tau is derived from duration.
   */
  function tick(now: number) {
    if (!lastTime) lastTime = now;
    const dt = Math.min((now - lastTime) / 1000, 0.1); // cap dt to avoid jumps
    lastTime = now;

    // tau controls decay speed; smaller = snappier. Lenis-like feel at ~duration/6.
    const tau = duration / 6;
    const factor = 1 - Math.exp(-dt / tau);

    currentScroll += (targetScroll - currentScroll) * factor;

    // Snap when close enough (sub-pixel)
    if (Math.abs(targetScroll - currentScroll) < 0.5) {
      currentScroll = targetScroll;
      node.scrollTop = currentScroll;
      animating = false;
      lastTime = 0;
      return;
    }

    node.scrollTop = currentScroll;
    rafId = requestAnimationFrame(tick);
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

    // Sync with actual scroll position if animation was idle
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
      lastTime = 0;
      rafId = requestAnimationFrame(tick);
    }
  }

  node.addEventListener("wheel", onWheel, { passive: false });

  return {
    update(newOpts?: SmoothScrollOpts) {
      duration = newOpts?.duration ?? 1.2;
      multiplier = newOpts?.multiplier ?? 1.8;
    },
    destroy() {
      node.removeEventListener("wheel", onWheel);
      if (rafId) cancelAnimationFrame(rafId);
    },
  };
}
