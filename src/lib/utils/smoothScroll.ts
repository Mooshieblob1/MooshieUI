/**
 * Svelte action that adds Lenis-style lerp smooth scrolling to a container.
 * Only activates on Windows (WebView2/Chromium). WebKitGTK on Linux already
 * has its own smooth scrolling, and intercepting wheel events there causes jank.
 *
 * Usage: <div use:smoothScroll> or <div use:smoothScroll={{ lerp: 0.1, multiplier: 1.2 }}>
 */

export interface SmoothScrollOpts {
  /** Lerp factor — lower = smoother/slower (0–1). Default 0.1 */
  lerp?: number;
  /** Wheel delta multiplier. Default 1.2 */
  multiplier?: number;
}

const isWindows = navigator.userAgent.includes("Windows");

export function smoothScroll(node: HTMLElement, opts?: SmoothScrollOpts) {
  if (!isWindows) return {};

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

  function onWheel(e: WheelEvent) {
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
