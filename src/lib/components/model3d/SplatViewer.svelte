<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { locale } from "../../stores/locale.svelte.js";
  import { splatBounds, type SplatData } from "../../utils/splatPly.js";
  import { SplatRenderer, type OrbitCamera } from "./splatRenderer.js";

  interface Props {
    data: SplatData | null;
    /** Slow turntable spin until the user grabs the model. */
    autoRotate?: boolean;
  }

  let { data, autoRotate = $bindable(true) }: Props = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let renderer: SplatRenderer | null = null;
  let error = $state("");
  let camera: OrbitCamera | null = null;
  let home: OrbitCamera | null = null;
  let frame = 0;
  let lastTime = 0;
  const pointers = new Map<number, { x: number; y: number }>();
  let pinchDistance = 0;
  let panning = false;

  const PITCH_LIMIT = Math.PI / 2 - 0.05;

  function requestDraw() {
    if (!frame) frame = requestAnimationFrame(draw);
  }

  function draw(time: number) {
    frame = 0;
    if (!renderer) return;
    if (camera && autoRotate && pointers.size === 0) {
      const dt = lastTime ? Math.min(0.1, (time - lastTime) / 1000) : 0;
      camera.yaw += dt * 0.45;
      lastTime = time;
      frame = requestAnimationFrame(draw);
    } else {
      lastTime = 0;
    }
    renderer.render(camera);
  }

  function frameData(next: SplatData | null) {
    if (!renderer) return;
    if (!next) {
      camera = null;
      home = null;
      renderer.clear();
      return;
    }
    const { center, radius } = splatBounds(next);
    renderer.setData(next, radius);
    const fov = (40 * Math.PI) / 180;
    home = { yaw: 0, pitch: 0.18, distance: (radius / Math.sin(fov / 2)) * 1.1, target: center, fov };
    camera = { ...home, target: [...home.target] as [number, number, number] };
    requestDraw();
  }

  export function resetView() {
    if (!home) return;
    camera = { ...home, target: [...home.target] as [number, number, number] };
    requestDraw();
  }

  /** PNG snapshot of the current view, for a library thumbnail. */
  export function snapshot(): string | null {
    try { return canvas?.toDataURL("image/png") ?? null; } catch { return null; }
  }

  onMount(() => {
    if (!canvas) return;
    try {
      renderer = new SplatRenderer(canvas);
      renderer.onsorted = requestDraw;
    } catch (e) {
      error = String(e instanceof Error ? e.message : e);
      return;
    }
    const observer = new ResizeObserver(requestDraw);
    observer.observe(canvas);
    frameData(data);
    return () => observer.disconnect();
  });

  onDestroy(() => {
    if (frame) cancelAnimationFrame(frame);
    renderer?.dispose();
    renderer = null;
  });

  let shown: SplatData | null = null;
  $effect(() => {
    const next = data;
    if (next === shown) return;
    shown = next;
    frameData(next);
  });

  $effect(() => {
    if (autoRotate) requestDraw();
  });

  function orbit(dx: number, dy: number) {
    if (!camera) return;
    camera.yaw += dx * 0.008;
    camera.pitch = Math.max(-PITCH_LIMIT, Math.min(PITCH_LIMIT, camera.pitch + dy * 0.008));
  }

  function pan(dx: number, dy: number) {
    if (!camera || !canvas) return;
    const scale = (2 * camera.distance * Math.tan(camera.fov / 2)) / Math.max(1, canvas.clientHeight);
    const cy = Math.cos(camera.yaw), sy = Math.sin(camera.yaw);
    // Screen right and screen down in the splat frame (up is -Y).
    const right = [cy, 0, sy];
    const sp = Math.sin(camera.pitch), cp = Math.cos(camera.pitch);
    const down = [sp * sy, cp, -sp * cy];
    for (let k = 0; k < 3; k++) camera.target[k] -= (dx * right[k] + dy * down[k]) * scale;
  }

  function zoom(factor: number) {
    if (!camera || !home) return;
    camera.distance = Math.max(home.distance * 0.05, Math.min(home.distance * 6, camera.distance * factor));
  }

  function onpointerdown(event: PointerEvent) {
    if (!camera) return;
    canvas?.setPointerCapture(event.pointerId);
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    panning = event.button === 2 || event.button === 1 || event.shiftKey;
    autoRotate = false;
    if (pointers.size === 2) {
      const [a, b] = [...pointers.values()];
      pinchDistance = Math.hypot(a.x - b.x, a.y - b.y);
    }
  }

  function onpointermove(event: PointerEvent) {
    const previous = pointers.get(event.pointerId);
    if (!previous || !camera) return;
    const dx = event.clientX - previous.x;
    const dy = event.clientY - previous.y;
    pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (pointers.size === 2) {
      const [a, b] = [...pointers.values()];
      const distance = Math.hypot(a.x - b.x, a.y - b.y);
      if (pinchDistance > 0 && distance > 0) zoom(pinchDistance / distance);
      pinchDistance = distance;
      pan(dx / 2, dy / 2);
    } else if (panning) {
      pan(dx, dy);
    } else {
      orbit(dx, dy);
    }
    requestDraw();
  }

  function onpointerup(event: PointerEvent) {
    pointers.delete(event.pointerId);
    if (pointers.size < 2) pinchDistance = 0;
  }

  function onwheel(event: WheelEvent) {
    if (!camera) return;
    event.preventDefault();
    zoom(Math.exp(event.deltaY * 0.0015));
    requestDraw();
  }

  function onkeydown(event: KeyboardEvent) {
    if (!camera) return;
    const step = event.shiftKey ? 30 : 12;
    if (event.key === "ArrowLeft") orbit(-step, 0);
    else if (event.key === "ArrowRight") orbit(step, 0);
    else if (event.key === "ArrowUp") orbit(0, -step);
    else if (event.key === "ArrowDown") orbit(0, step);
    else if (event.key === "+" || event.key === "=") zoom(0.85);
    else if (event.key === "-") zoom(1 / 0.85);
    else if (event.key === "0" || event.key === "Home") { resetView(); return; }
    else return;
    event.preventDefault();
    autoRotate = false;
    requestDraw();
  }
</script>

<div class="relative h-full w-full overflow-hidden rounded-md bg-[radial-gradient(circle_at_50%_40%,rgb(38_38_38),rgb(10_10_10))]">
  <canvas
    bind:this={canvas}
    class="block h-full w-full touch-none select-none outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 {data ? 'cursor-grab active:cursor-grabbing' : ''}"
    tabindex="0"
    aria-label={locale.t("model3d.viewer_label")}
    {onpointerdown}
    {onpointermove}
    {onpointerup}
    onpointercancel={onpointerup}
    {onwheel}
    {onkeydown}
    oncontextmenu={(event) => event.preventDefault()}
    ondblclick={resetView}
  ></canvas>
  {#if error}
    <p class="absolute inset-x-4 top-1/2 -translate-y-1/2 text-center text-sm text-red-300" role="alert">{locale.t("model3d.viewer_unavailable")} {error}</p>
  {/if}
</div>
