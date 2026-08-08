<script module lang="ts">
  // Audio preferences outlive any one player instance. They are module-scoped,
  // not component-scoped, because the lightbox unmounts and remounts the player
  // on every open and on every clip change - component state would silently
  // reset the user's mute on each of those, which is exactly what it did.
  // Deliberately not persisted to disk: this is a per-session preference.
  let volume = $state(1);
  let muted = $state(false);
</script>

<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import VideoExportPopover from "./VideoExportPopover.svelte";
  import VideoInterpolatePopover from "./VideoInterpolatePopover.svelte";

  interface Props {
    /** Range-serving gallery URL. */
    src: string;
    /** Real clip frame rate; 24 only when the index does not know. */
    fps?: number;
    /** full = lightbox, compact = generation preview. */
    density?: "full" | "compact";
    /** Gallery filename; presence enables the export button (Task 10). */
    filename?: string;
    onContextMenu?: (e: MouseEvent) => void;
  }

  let {
    src,
    fps = 24,
    density = "full",
    filename = undefined,
    onContextMenu = undefined,
  }: Props = $props();

  // Component-local state. No store: nothing outside this component reads any of it.
  let videoEl = $state<HTMLVideoElement | null>(null);
  let rootEl = $state<HTMLDivElement | null>(null);
  let playing = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let bufferedEnd = $state(0);
  // volume and muted live in <script module> above: they survive remounts.
  let rate = $state(1);
  let looping = $state(true);
  let seamMode = $state(false);
  let controlsVisible = $state(true);
  let decodeFailed = $state(false);
  let overflowOpen = $state(false);
  // Scrubber drag guard: prevents timeupdate writes from snapping the thumb
  // back to the playhead while the user is mid-drag.
  let scrubbing = $state(false);
  let scrubPos = $state(0);
  /** 0-100 mean absolute pixel difference between first and last frame, or null if unmeasurable. */
  let seamDelta = $state<number | null>(null);
  /** True once measurement has been attempted for the current clip. */
  let seamDeltaComputed = false;
  /** Incremented on every clip change; lets in-flight measureSeam detect it is stale. */
  let measureGen = 0;

  let exportOpen = $state(false);
  let interpolateOpen = $state(false);
  let videoWidth = $state(0);
  let videoHeight = $state(0);

  const clipFrames = $derived(
    duration > 0 && fps > 0 ? Math.max(1, Math.round(duration * fps)) : 0
  );

  const SPEEDS = [0.25, 0.5, 1, 1.5, 2];
  /** Half-window around the wrap, in seconds. 1.2 s total. */
  const SEAM_HALF = 0.6;

  let idleTimer: ReturnType<typeof setTimeout> | null = null;

  const frameStep = $derived(fps > 0 ? 1 / fps : 1 / 24);
  const progressPct = $derived(duration > 0 ? (currentTime / duration) * 100 : 0);
  const bufferedPct = $derived(duration > 0 ? (bufferedEnd / duration) * 100 : 0);

  function fmt(t: number): string {
    if (!Number.isFinite(t) || t < 0) t = 0;
    const m = Math.floor(t / 60);
    const s = Math.floor(t % 60);
    return `${m}:${s.toString().padStart(2, "0")}`;
  }

  /** Draw videoEl into ctx at 64x64 and return the pixel data, or null on SecurityError. */
  function grab(ctx: CanvasRenderingContext2D): Uint8ClampedArray | null {
    if (!videoEl) return null;
    ctx.drawImage(videoEl, 0, 0, 64, 64);
    try {
      return ctx.getImageData(0, 0, 64, 64).data;
    } catch {
      // SecurityError: the gallery:// custom protocol tainted the canvas.
      // Treated as optional - hide the number, never surface an error, and
      // leave seam-check playback working exactly as it does without it.
      return null;
    }
  }

  async function seekTo(t: number): Promise<void> {
    if (!videoEl) return;
    // Capture a non-null ref so TypeScript can narrow inside the Promise callback.
    const el = videoEl;
    await new Promise<void>((resolve) => {
      let fallback: ReturnType<typeof setTimeout> | null = null;
      const done = () => {
        // Clear the fallback timer so it does not fire a second resolve() after
        // the seeked event already fired.
        if (fallback !== null) clearTimeout(fallback);
        el.removeEventListener("seeked", done);
        resolve();
      };
      el.addEventListener("seeked", done);
      el.currentTime = t;
      // A seek to where we already are fires no seeked event; do not hang.
      fallback = setTimeout(done, 400);
    });
  }

  /**
   * Measures the mean absolute pixel difference (0-100) between the first and
   * last frame of the clip at 64x64.
   *
   * 64x64 is deliberate: it captures whether the composition matches, not
   * whether individual pixels do, so encoder noise does not drown the signal.
   *
   * This is a convenience readout only. The authoritative seam delta that drives
   * Auto loop mode is measured in Python during export, where the pixels are
   * always reachable regardless of CORS/protocol tainting.
   *
   * Always called with seamMode already true, so onTimeUpdate clamps any
   * mid-clip position. We do not seek back to resumeAt on exit - that seek
   * would be immediately overridden. toggleSeam's .then() block handles the
   * final park at duration - SEAM_HALF and restores playback there.
   */
  async function measureSeam() {
    if (seamDeltaComputed || !videoEl || duration <= 0) return;
    seamDeltaComputed = true;
    // Snapshot the generation counter so we can detect if the clip changes
    // mid-measurement and discard the stale result without skipping the restore.
    const gen = ++measureGen;

    const wasPaused = videoEl.paused;
    videoEl.pause();

    const canvas = document.createElement("canvas");
    canvas.width = 64;
    canvas.height = 64;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });

    try {
      if (!ctx) return;

      await seekTo(0);
      const first = grab(ctx);
      if (!first) return;

      await seekTo(Math.max(0, duration - 0.5 / (fps > 0 ? fps : 24)));
      const last = grab(ctx);
      if (!last) return;

      let sum = 0;
      let n = 0;
      for (let i = 0; i < first.length; i += 4) {
        sum += Math.abs(first[i] - last[i]);
        sum += Math.abs(first[i + 1] - last[i + 1]);
        sum += Math.abs(first[i + 2] - last[i + 2]);
        n += 3;
      }
      // Guard against a clip change that happened while we were awaiting seeks.
      if (gen === measureGen) seamDelta = (sum / n / 255) * 100;
    } finally {
      // Do not seek back to a captured resumeAt: seamMode is active, so
      // onTimeUpdate would clamp any mid-clip restore immediately. The .then()
      // block in toggleSeam owns final positioning. Only restore play state here.
      // The finally runs unconditionally - a superseded run still owns restoring
      // the playback state it changed.
      if (!wasPaused) videoEl?.play().catch(() => {});
    }
  }

  function togglePlay() {
    if (!videoEl) return;
    if (videoEl.paused) videoEl.play().catch(() => {});
    else videoEl.pause();
  }

  function seekBy(delta: number) {
    if (!videoEl || !Number.isFinite(duration)) return;
    videoEl.currentTime = Math.max(0, Math.min(duration, videoEl.currentTime + delta));
  }

  function stepFrame(dir: 1 | -1) {
    if (!videoEl) return;
    videoEl.pause();
    seekBy(dir * frameStep);
  }

  function onScrub(e: Event) {
    const value = Number((e.currentTarget as HTMLInputElement).value);
    scrubPos = value;
    if (videoEl) videoEl.currentTime = (value / 1000) * duration;
  }

  function toggleSeam() {
    seamMode = !seamMode;
    if (!videoEl || duration <= 0) return;
    if (seamMode) {
      // Measure first (measureSeam restores play state in its finally block),
      // then park in the wrap window. The .then() seek is what actually lands;
      // measureSeam intentionally skips the mid-clip restore because seamMode is
      // active and onTimeUpdate would clamp it immediately anyway.
      void measureSeam().then(() => {
        if (!seamMode || !videoEl) return;
        videoEl.currentTime = Math.max(0, duration - SEAM_HALF);
        videoEl.play().catch(() => {});
      });
    }
    // Toggling off leaves playback exactly where it is.
  }

  function onTimeUpdate() {
    if (!videoEl) return;
    currentTime = videoEl.currentTime;
    if (seamMode && duration > 0 && currentTime > SEAM_HALF && currentTime < duration - SEAM_HALF) {
      videoEl.currentTime = duration - SEAM_HALF;
    }
  }

  function onProgress() {
    if (!videoEl || videoEl.buffered.length === 0) return;
    bufferedEnd = videoEl.buffered.end(videoEl.buffered.length - 1);
  }

  function toggleFullscreen() {
    if (!rootEl) return;
    if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
    else rootEl.requestFullscreen().catch(() => {});
  }

  function onKeydown(e: KeyboardEvent) {
    switch (e.key) {
      case " ":
        e.preventDefault();
        togglePlay();
        break;
      case "ArrowLeft":
        // The lightbox also binds arrows for gallery navigation. Stop it here
        // so focus in the player means seeking, not navigating away mid-clip.
        e.stopPropagation();
        e.preventDefault();
        seekBy(-5);
        break;
      case "ArrowRight":
        e.stopPropagation();
        e.preventDefault();
        seekBy(5);
        break;
      case ",":
        stepFrame(-1);
        break;
      case ".":
        stepFrame(1);
        break;
      case "m":
        muted = !muted;
        break;
      case "l":
        looping = !looping;
        break;
      case "f":
        toggleFullscreen();
        break;
    }
  }

  function wake() {
    controlsVisible = true;
    if (density === "compact") return; // the compact bar never hides
    if (idleTimer) clearTimeout(idleTimer);
    idleTimer = setTimeout(() => {
      controlsVisible = false;
    }, 2000);
  }

  function retry() {
    // Flipping decodeFailed re-mounts the <video> element; autoplay handles the rest.
    decodeFailed = false;
  }

  $effect(() => {
    if (videoEl) {
      videoEl.volume = volume;
      videoEl.muted = muted;
      videoEl.playbackRate = rate;
      videoEl.loop = looping;
    }
  });

  $effect(() => () => {
    if (idleTimer) clearTimeout(idleTimer);
  });

  $effect(() => {
    // `src` is the dependency: a new clip needs a new measurement, and a new
    // clip deserves a fresh attempt even if the previous one failed to decode.
    // Incrementing measureGen invalidates any in-flight measureSeam so it
    // cannot write a stale result after this reset completes.
    void src;
    seamDelta = null;
    seamDeltaComputed = false;
    decodeFailed = false;
    measureGen++;
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  bind:this={rootEl}
  class="relative w-full h-full flex items-center justify-center bg-black/60 rounded-xl overflow-hidden group"
  onmousemove={wake}
  onfocusin={wake}
  onkeydown={onKeydown}
  oncontextmenu={onContextMenu}
  role="region"
  aria-label={locale.t("preview.video_alt")}
  tabindex="-1"
>
  {#if decodeFailed}
    <div class="flex flex-col items-center gap-3 p-6 text-center">
      <p class="text-sm text-neutral-300">{locale.t("video.player.decode_error")}</p>
      <button
        class="px-3 py-1.5 rounded-lg text-sm bg-neutral-800 hover:bg-neutral-700 text-neutral-100"
        onclick={retry}
      >
        {locale.t("video.player.retry")}
      </button>
    </div>
  {:else}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
      bind:this={videoEl}
      {src}
      class="max-w-full max-h-full object-contain"
      autoplay
      playsinline
      onloadedmetadata={() => {
        duration = videoEl?.duration ?? 0;
        videoWidth = videoEl?.videoWidth ?? 0;
        videoHeight = videoEl?.videoHeight ?? 0;
      }}
      ontimeupdate={onTimeUpdate}
      onprogress={onProgress}
      onplay={() => (playing = true)}
      onpause={() => (playing = false)}
      onerror={() => (decodeFailed = true)}
    ></video>

    <div
      class="absolute inset-x-0 bottom-0 p-2 transition-opacity duration-200"
      class:opacity-0={!controlsVisible}
      class:pointer-events-none={!controlsVisible}
    >
      <div
        class="flex flex-col gap-1.5 rounded-xl bg-neutral-900/80 backdrop-blur px-2.5 py-2 border border-neutral-700/60"
      >
        <!-- Primary row -->
        <div class="flex items-center gap-2">
          <button
            class="p-1.5 rounded-lg hover:bg-neutral-700/70 text-neutral-100"
            onclick={togglePlay}
            aria-label={playing
              ? locale.t("video.player.pause")
              : locale.t("video.player.play")}
            title={playing ? locale.t("video.player.pause") : locale.t("video.player.play")}
          >
            {playing ? "❚❚" : "▶"}
          </button>

          <span class="text-xs tabular-nums text-neutral-300 shrink-0">
            {fmt(currentTime)} / {fmt(duration)}
          </span>

          <!-- Scrubber: a transparent native range over a painted div stack, so
               keyboard and screen-reader behaviour come for free. -->
          <div class="relative flex-1 h-6 flex items-center">
            <div class="absolute inset-x-0 h-2 rounded-full bg-neutral-600"></div>
            <div
              class="absolute left-0 h-2 rounded-full bg-neutral-500"
              style="width: {bufferedPct}%"
            ></div>
            <div
              class="absolute left-0 h-2 rounded-full"
              style="width: {progressPct}%; background: var(--theme-accent-500)"
            ></div>
            <input
              type="range"
              min="0"
              max="1000"
              value={scrubbing ? scrubPos : progressPct * 10}
              oninput={onScrub}
              onpointerdown={(e) => {
                scrubPos = Number((e.currentTarget as HTMLInputElement).value);
                scrubbing = true;
              }}
              onpointerup={() => (scrubbing = false)}
              onpointercancel={() => (scrubbing = false)}
              onchange={() => (scrubbing = false)}
              aria-label={locale.t("video.player.scrubber")}
              class="absolute inset-0 w-full appearance-none bg-transparent cursor-pointer
                     [&::-webkit-slider-thumb]:appearance-none
                     [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:w-4
                     [&::-webkit-slider-thumb]:rounded-full
                     [&::-webkit-slider-thumb]:bg-white
                     [&::-moz-range-thumb]:h-4 [&::-moz-range-thumb]:w-4
                     [&::-moz-range-thumb]:rounded-full
                     [&::-moz-range-thumb]:border-0 [&::-moz-range-thumb]:bg-white"
            />
          </div>

          <button
            class="p-1.5 rounded-lg hover:bg-neutral-700/70 text-neutral-100"
            onclick={() => (muted = !muted)}
            aria-label={muted
              ? locale.t("video.player.unmute")
              : locale.t("video.player.mute")}
            title={muted ? locale.t("video.player.unmute") : locale.t("video.player.mute")}
          >
            {muted ? "🔇" : "🔊"}
          </button>

          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            bind:value={volume}
            aria-label={locale.t("video.player.volume")}
            class="w-16 h-1 appearance-none rounded-full bg-neutral-600 cursor-pointer
                   [&::-webkit-slider-thumb]:appearance-none
                   [&::-webkit-slider-thumb]:h-2.5 [&::-webkit-slider-thumb]:w-2.5
                   [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white"
          />

          <button
            class="p-1.5 rounded-lg text-neutral-100"
            class:bg-neutral-700={looping}
            onclick={() => (looping = !looping)}
            aria-label={looping
              ? locale.t("video.player.loop_off")
              : locale.t("video.player.loop_on")}
            title={looping
              ? locale.t("video.player.loop_off")
              : locale.t("video.player.loop_on")}
          >
            ⟳
          </button>

          <button
            class="p-1.5 rounded-lg hover:bg-neutral-700/70 text-neutral-100"
            onclick={toggleFullscreen}
            aria-label={locale.t("video.player.fullscreen")}
            title={locale.t("video.player.fullscreen")}
          >
            ⛶
          </button>

          {#if density === "compact"}
            <button
              class="p-1.5 rounded-lg hover:bg-neutral-700/70 text-neutral-100"
              onclick={() => (overflowOpen = !overflowOpen)}
              aria-label={locale.t("video.player.more")}
              title={locale.t("video.player.more")}
            >
              ⋯
            </button>
          {/if}
        </div>

        <!-- Secondary row: generation-aware controls. Inline at full density;
             behind one overflow button in the preview, which is too narrow to
             hold ten controls without wrapping. -->
        {#if density === "full" || overflowOpen}
          <div class="flex items-center gap-2 flex-wrap">
            <button
              class="p-1.5 rounded-lg hover:bg-neutral-700/70 text-neutral-100 text-xs"
              onclick={() => stepFrame(-1)}
              aria-label={locale.t("video.player.frame_back")}
              title={locale.t("video.player.frame_back")}
            >
              ⟨|
            </button>
            <button
              class="p-1.5 rounded-lg hover:bg-neutral-700/70 text-neutral-100 text-xs"
              onclick={() => stepFrame(1)}
              aria-label={locale.t("video.player.frame_forward")}
              title={locale.t("video.player.frame_forward")}
            >
              |⟩
            </button>

            <select
              bind:value={rate}
              aria-label={locale.t("video.player.speed")}
              title={locale.t("video.player.speed")}
              class="text-xs bg-neutral-800 text-neutral-100 rounded-lg px-1.5 py-1 border border-neutral-700"
            >
              {#each SPEEDS as s (s)}
                <option value={s}>{s}x</option>
              {/each}
            </select>

            <button
              class="px-2 py-1 rounded-lg text-xs text-neutral-100"
              class:bg-neutral-700={seamMode}
              onclick={toggleSeam}
              title={locale.t("video.player.seam_check_tip")}
            >
              {locale.t("video.player.seam_check")}
            </button>

            {#if seamDelta !== null}
              <span
                class="text-xs tabular-nums px-1.5"
                class:text-green-400={seamDelta < 2}
                class:text-amber-400={seamDelta >= 2 && seamDelta <= 10}
                class:text-red-400={seamDelta > 10}
              >
                {locale.t("video.player.seam_delta", { value: seamDelta.toFixed(1) })}
              </span>
            {/if}
            {#if filename}
              <div class="ml-auto flex items-center gap-1">
                <div class="relative">
                  <button
                    class="px-2 py-1 rounded-lg text-xs text-neutral-100"
                    class:bg-neutral-700={interpolateOpen}
                    onclick={() => (interpolateOpen = !interpolateOpen)}
                  >
                    {locale.t("video.interpolate.open")}
                  </button>
                  {#if interpolateOpen}
                    <VideoInterpolatePopover
                      {filename}
                      sourceFps={fps}
                      sourceWidth={videoWidth}
                      sourceHeight={videoHeight}
                      frameCount={clipFrames}
                      onClose={() => (interpolateOpen = false)}
                    />
                  {/if}
                </div>
                <div class="relative">
                  <button
                    class="px-2 py-1 rounded-lg text-xs text-neutral-100"
                    class:bg-neutral-700={exportOpen}
                    onclick={() => (exportOpen = !exportOpen)}
                  >
                    {locale.t("video.export.open")}
                  </button>
                  {#if exportOpen}
                    <VideoExportPopover
                      {filename}
                      sourceFps={fps}
                      sourceWidth={videoWidth}
                      sourceHeight={videoHeight}
                      frameCount={clipFrames}
                      onClose={() => (exportOpen = false)}
                    />
                  {/if}
                </div>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>
