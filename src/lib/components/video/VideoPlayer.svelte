<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";

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

  // Twelve component-local fields. No store: nothing outside this component
  // reads any of it.
  let videoEl = $state<HTMLVideoElement | null>(null);
  let rootEl = $state<HTMLDivElement | null>(null);
  let playing = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let bufferedEnd = $state(0);
  let volume = $state(1);
  let muted = $state(false);
  let rate = $state(1);
  let looping = $state(true);
  let seamMode = $state(false);
  let controlsVisible = $state(true);
  let decodeFailed = $state(false);
  let overflowOpen = $state(false);

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
    if (videoEl) videoEl.currentTime = (value / 1000) * duration;
  }

  function toggleSeam() {
    seamMode = !seamMode;
    if (!videoEl || duration <= 0) return;
    if (seamMode) {
      // Loop the 1.2 s straddling the wrap. Watching the same window over and
      // over is how you actually judge whether a loop is seamless.
      videoEl.currentTime = Math.max(0, duration - SEAM_HALF);
      videoEl.play().catch(() => {});
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
    decodeFailed = false;
    if (videoEl) videoEl.load();
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
          <div class="relative flex-1 h-3 flex items-center">
            <div class="absolute inset-x-0 h-1 rounded-full bg-neutral-600"></div>
            <div
              class="absolute left-0 h-1 rounded-full bg-neutral-500"
              style="width: {bufferedPct}%"
            ></div>
            <div
              class="absolute left-0 h-1 rounded-full"
              style="width: {progressPct}%; background: var(--theme-accent-500)"
            ></div>
            <input
              type="range"
              min="0"
              max="1000"
              value={progressPct * 10}
              oninput={onScrub}
              aria-label={locale.t("video.player.scrubber")}
              class="absolute inset-0 w-full appearance-none bg-transparent cursor-pointer
                     [&::-webkit-slider-thumb]:appearance-none
                     [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:w-3
                     [&::-webkit-slider-thumb]:rounded-full
                     [&::-webkit-slider-thumb]:bg-white
                     [&::-moz-range-thumb]:h-3 [&::-moz-range-thumb]:w-3
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

            <!-- Task 9 inserts the seam-delta readout here. -->
            <!-- Task 10 inserts the export button here, gated on `filename`. -->
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>
