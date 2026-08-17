<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { isTauri, ipcListen } from "../../utils/ipc.js";
  import {
    exportVideoAnimation,
    probeVideoExport,
    copyFileToClipboard,
    copyFileTo,
    exportDownloadUrl,
    type VideoExportResult,
  } from "../../utils/api.js";
  import {
    presetsFor,
    qualityRange,
    supportsLoopCount,
    supportsAudio,
    extFor,
    offeredFps,
    presetFps,
    outputDimensions,
    outputFrameCount,
    crossfadeAvailable,
    estimateBytes,
    formatBytes,
    overSizeLimit,
    defaultCrossfadeFrames,
    type ExportFormat,
    type LoopMode,
    type SizeTarget,
  } from "../../utils/videoExport.js";

  interface Props {
    filename: string;
    sourceFps: number;
    sourceWidth: number;
    sourceHeight: number;
    frameCount: number;
    onClose: () => void;
  }

  let { filename, sourceFps, sourceWidth, sourceHeight, frameCount, onClose }: Props = $props();

  // AVIF leads because it is the smallest by a wide margin (roughly 6x smaller than WebP and
  // 110x smaller than GIF at comparable quality on typical video-like content). WebP and GIF
  // remain first-class options because Discord, Slack, Teams, and Signal will not animate an
  // AVIF inline - they post it as a file attachment.
  let format = $state<ExportFormat>("avif");
  let presetIndex = $state(1);
  let advanced = $state(false);
  let fps = $state(24);
  let width = $state(640);
  let quality = $state(63);
  let loopCount = $state(0);
  let loopMode = $state<LoopMode>("auto");
  // Proportional to this clip's own length rather than a flat constant - see
  // defaultCrossfadeFrames - so a short clip does not lose a disproportionate
  // slice of itself to the crossfade.
  let crossfadeFrames = $state(defaultCrossfadeFrames(frameCount));
  let sizeTarget = $state<SizeTarget>("discord");
  // MP4 is the only format that can carry the clip's audio, and H3 always writes
  // one, so the default is to keep it. `audioOn` folds in the cases where the
  // track cannot survive at all - format and loop mode both veto it.
  let keepAudio = $state(true);

  let capability = $state<{ available: boolean; reason: string | null; mp4: boolean } | null>(
    null
  );
  let running = $state(false);
  let progressLabel = $state("");
  let result = $state<VideoExportResult | null>(null);
  let errorText = $state<string | null>(null);
  let errorDetail = $state<string | null>(null);

  const ALL_FORMATS: ExportFormat[] = ["avif", "webp", "gif", "mp4"];
  const LOOP_MODES: LoopMode[] = ["auto", "none", "trim", "crossfade", "pingpong"];

  const presets = $derived(presetsFor(format));
  const fpsChoices = $derived(offeredFps(sourceFps));
  const dims = $derived(outputDimensions(sourceWidth, sourceHeight, width));
  // "auto" is estimated at the optimistic "trim" branch here because the popover has no
  // measured seam delta (that lives inside VideoPlayer and adding a seventh prop would
  // change the declared Produces interface). The real resolve_auto runs in Python at
  // export time with the actual measured seam delta.
  const outFrames = $derived(
    outputFrameCount(loopMode === "auto" ? "trim" : loopMode, frameCount, crossfadeFrames)
  );
  const qRange = $derived(qualityRange(format));
  const canCrossfade = $derived(crossfadeAvailable(frameCount, crossfadeFrames));
  // Whether the checkbox can do anything at all, as opposed to whether it is ticked.
  const audioPossible = $derived(supportsAudio(format, loopMode));
  const audioOn = $derived(keepAudio && audioPossible);
  // The probe reports libx264 separately from the venv itself: a venv can be
  // perfectly able to write AVIF and still have no H.264 encoder.
  const mp4Blocked = $derived(capability !== null && capability.available && !capability.mp4);

  function sizeFor(f: ExportFormat): number {
    // Real bytes replace the estimate only for the format actually encoded.
    if (result && f === format) return result.size_bytes;
    // The audio term only applies to the format that would actually carry one,
    // so each column of the comparison row stays honest about itself.
    return estimateBytes(
      f,
      outFrames,
      dims.width,
      dims.height,
      fps,
      keepAudio && supportsAudio(f, loopMode)
    );
  }

  const shownBytes = $derived(sizeFor(format));
  const overLimit = $derived(overSizeLimit(shownBytes, sizeTarget));

  function applyPreset(i: number) {
    presetIndex = i;
    const p = presetsFor(format)[i];
    fps = presetFps(p, sourceFps);
    // A preset never renders wider than the source can deliver.
    width = Math.min(p.width, sourceWidth || p.width);
    quality = p.quality;
    // Presets deliberately do not set loop mode - it stays where the user left it.
  }

  function setFormat(f: ExportFormat) {
    if (f === format) return;
    format = f;
    // The ladders are the same length but not the same meaning, so re-apply
    // the slot the user picked rather than carrying raw fps/width/quality
    // across a format change.
    applyPreset(Math.min(presetIndex, presetsFor(f).length - 1));
  }

  // Seed fps/width/quality from the default preset. Runs once, at init.
  applyPreset(presetIndex);

  $effect(() => {
    probeVideoExport()
      .then((c) => {
        capability = c;
        // The probe is async, so MP4 is selectable for the moment before it
        // answers. If it comes back without an H.264 encoder, step off the tab
        // rather than leaving the user on one that cannot run.
        if (!c.mp4 && format === "mp4") setFormat("avif");
      })
      .catch((e) => (capability = { available: false, reason: String(e), mp4: false }));
  });

  $effect(() => {
    const un = ipcListen("export:progress", (event: any) => {
      const d = event.payload;
      // Lines without a positive total (the seam measurement) carry no
      // progress; leave the last label standing rather than resetting to 0%.
      if (!d || typeof d.total !== "number" || d.total <= 0) return;
      const pct = Math.round((d.done / d.total) * 100);
      progressLabel = `${locale.t("video.export.exporting")} ${pct}%`;
    });
    return () => {
      un.then((f: any) => f?.()).catch(() => {});
    };
  });

  $effect(() => {
    // Any of these invalidates a previous encode: the next Save as / Copy
    // must re-run rather than reuse a file produced from other settings.
    void format;
    void fps;
    void width;
    void quality;
    void loopCount;
    void loopMode;
    void crossfadeFrames;
    void keepAudio;
    result = null;
  });

  async function runExport(): Promise<VideoExportResult | null> {
    errorText = null;
    errorDetail = null;
    running = true;
    progressLabel = locale.t("video.export.exporting");
    try {
      const r = await exportVideoAnimation({
        filename,
        format,
        fps,
        width: dims.width,
        quality,
        loopCount,
        loopMode,
        crossfadeFrames,
        keepAudio: audioOn,
      });
      result = r;
      return r;
    } catch (e) {
      const msg = String(e);
      errorText = locale.t("video.export.err_transcode_failed", { reason: msg.split("\n")[0] });
      errorDetail = msg;
      return null;
    } finally {
      running = false;
      progressLabel = "";
    }
  }

  async function onSaveAs() {
    // Export always writes the temp file first, so Save as and Copy share one
    // encode and Copy after Save as is instant.
    const r = result ?? (await runExport());
    if (!r) return;
    const ext = extFor(format);
    const base = filename.replace(/\.[^.]+$/, "");
    if (isTauri) {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const dest = await save({
        defaultPath: `${base}.${ext}`,
        filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
      });
      if (!dest) return;
      await copyFileTo(r.path, dest);
      gallery.showToast(locale.t("video.export.done_saved"), "success");
    } else {
      const a = document.createElement("a");
      a.href = exportDownloadUrl(r.path);
      a.download = `${base}.${ext}`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
    }
  }

  async function onCopy() {
    const r = result ?? (await runExport());
    if (!r) return;
    if (!isTauri) {
      // Browser mode has no clipboard command at all: Copy became Download.
      await onSaveAs();
      return;
    }
    try {
      await copyFileToClipboard(r.path);
      gallery.showToast(locale.t("video.export.done_copied"), "success");
    } catch (e) {
      errorText = locale.t("video.export.err_clipboard_failed", { path: r.path });
      errorDetail = String(e);
    }
  }

  async function copyDetails() {
    if (!errorDetail) return;
    await navigator.clipboard.writeText(errorDetail).catch(() => {});
  }
</script>

<div
  class="absolute bottom-full right-0 mb-2 w-80 rounded-xl bg-neutral-900 border border-neutral-700 shadow-xl p-3 flex flex-col gap-3 text-neutral-100"
>
  <div class="flex items-center justify-between">
    <h3 class="text-sm font-medium">{locale.t("video.export.title")}</h3>
    <button class="text-neutral-400 hover:text-neutral-100" onclick={onClose}>&#x2715;</button>
  </div>

  {#if capability && !capability.available}
    <p class="text-xs text-amber-400">
      {capability.reason ?? locale.t("video.export.err_no_venv")}
    </p>
  {/if}

  <!-- Format tabs -->
  <div class="flex gap-1">
    {#each ALL_FORMATS as f (f)}
      <button
        class="flex-1 min-w-0 px-2 py-1 rounded-lg text-xs flex flex-col items-center leading-tight disabled:opacity-40"
        class:bg-neutral-700={format === f}
        class:bg-neutral-800={format !== f}
        disabled={f === "mp4" && mp4Blocked}
        title={f === "mp4" && mp4Blocked ? locale.t("video.export.mp4_unavailable") : undefined}
        onclick={() => setFormat(f)}
      >
        <span>{locale.t(`video.export.format_${f}`)}</span>
        {#if f === "avif"}
          <span class="w-full truncate text-[9px] text-[var(--theme-accent-500)]">
            {locale.t("video.export.badge_recommended")}
          </span>
        {/if}
      </button>
    {/each}
  </div>

  <!-- Selected format compatibility note -->
  <p class="text-[11px] text-neutral-400">
    {locale.t(`video.export.compat_${format}`)}
  </p>

  {#if mp4Blocked}
    <p class="text-[11px] text-amber-400">{locale.t("video.export.mp4_unavailable")}</p>
  {/if}

  <!-- Presets -->
  <div class="flex gap-1">
    {#each presets as p, i (p.id)}
      <button
        class="flex-1 px-2 py-1 rounded-lg text-xs"
        class:bg-neutral-700={presetIndex === i}
        class:bg-neutral-800={presetIndex !== i}
        onclick={() => applyPreset(i)}
      >
        {locale.t(`video.export.${p.id}`)}
      </button>
    {/each}
  </div>

  <!-- Loop mode -->
  <div class="flex flex-col gap-1">
    <span class="text-[11px] text-neutral-400">{locale.t("video.export.loop_mode")}</span>
    <div class="flex flex-wrap gap-1">
      {#each LOOP_MODES as m (m)}
        <!-- A clip too short to crossfade simply does not offer the chip.
             Not an error, and not a greyed-out chip. -->
        {#if m !== "crossfade" || canCrossfade}
          <button
            class="px-2 py-1 rounded-lg text-[11px]"
            class:bg-neutral-700={loopMode === m}
            class:bg-neutral-800={loopMode !== m}
            title={locale.t(`video.export.loop_${m}_tip`)}
            onclick={() => (loopMode = m)}
          >
            {locale.t(`video.export.loop_${m}`)}
          </button>
        {/if}
      {/each}
    </div>
  </div>

  <!-- Audio. MP4 only, and hidden rather than disabled on the others: an animated
       image has no audio track to keep, so the control would be meaningless there
       rather than merely unavailable. It sits next to the loop-mode chips because
       ping-pong is what takes it away. -->
  {#if format === "mp4"}
    <div class="flex flex-col gap-1">
      <label class="flex items-center gap-2 text-xs">
        <input type="checkbox" bind:checked={keepAudio} disabled={!audioPossible} />
        <span class="text-neutral-400">{locale.t("video.export.keep_audio")}</span>
      </label>
      {#if !audioPossible}
        <p class="text-[11px] text-neutral-400">{locale.t("video.export.audio_pingpong")}</p>
      {/if}
    </div>
  {/if}

  <!-- Disclosure. The chevron is an inline SVG that rotates rather than two
       glyphs swapped by state: entities inside a {} expression are not decoded
       by Svelte, so the character forms rendered as literal "&#x25B8;" text. -->
  <button
    class="self-start flex items-center gap-1.5 rounded-lg border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 hover:bg-neutral-700 hover:text-neutral-100 transition-colors"
    aria-expanded={advanced}
    onclick={() => (advanced = !advanced)}
  >
    <svg
      xmlns="http://www.w3.org/2000/svg"
      class="w-3 h-3 shrink-0 transition-transform {advanced ? 'rotate-90' : ''}"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2.5"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      <path d="m9 18 6-6-6-6" />
    </svg>
    {locale.t("video.export.advanced")}
  </button>

  {#if advanced}
    <div class="flex flex-col gap-2 text-xs">
      <div class="flex flex-col gap-1">
        <span class="text-neutral-400">{locale.t("video.export.fps")}</span>
        <!-- A discrete picker, not a slider: only integer divisors of the
             source resample cleanly, and rates above the source are absent
             rather than greyed out. -->
        <div class="flex flex-wrap gap-1">
          {#each fpsChoices as f (f)}
            <button
              class="px-2 py-0.5 rounded-lg"
              class:bg-neutral-700={fps === f}
              class:bg-neutral-800={fps !== f}
              onclick={() => (fps = f)}
            >
              {f}
            </button>
          {/each}
        </div>
      </div>

      <label class="flex flex-col gap-1">
        <span class="text-neutral-400">
          {locale.t("video.export.width")} - {dims.width}x{dims.height}
        </span>
        <input
          type="range"
          min="160"
          max={sourceWidth || 832}
          step="2"
          bind:value={width}
          class="w-full"
        />
      </label>

      <label class="flex flex-col gap-1">
        <span class="text-neutral-400">
          {format === "gif"
            ? locale.t("video.export.colors")
            : locale.t("video.export.quality")} - {quality}
        </span>
        <input
          type="range"
          min={qRange.min}
          max={qRange.max}
          bind:value={quality}
          class="w-full"
        />
      </label>

      {#if loopMode === "crossfade"}
        <label class="flex flex-col gap-1">
          <span class="text-neutral-400">
            {locale.t("video.export.crossfade_frames")} - {crossfadeFrames}
          </span>
          <input type="range" min="1" max="16" bind:value={crossfadeFrames} class="w-full" />
        </label>
      {/if}

      <!-- Repeat count: disabled on AVIF because the container ignores it. Disabled rather than
           hidden so the control does not appear to vanish on a format switch. -->
      <div class="flex flex-col gap-1">
        <label class="flex items-center gap-2">
          <input
            type="checkbox"
            checked={loopCount === 0}
            disabled={!supportsLoopCount(format)}
            onchange={(e) => (loopCount = (e.currentTarget as HTMLInputElement).checked ? 0 : 1)}
          />
          <span class="text-neutral-400">
            {locale.t("video.export.loop_count")}: {locale.t("video.export.loop_count_infinite")}
          </span>
        </label>
        {#if !supportsLoopCount(format)}
          <p class="text-[11px] text-neutral-400">{locale.t("video.export.loop_count_unsupported")}</p>
        {/if}
      </div>

      <label class="flex flex-col gap-1">
        <span class="text-neutral-400">{locale.t("video.export.target_platform")}</span>
        <select
          bind:value={sizeTarget}
          class="bg-neutral-800 rounded-lg px-1.5 py-1 border border-neutral-700"
        >
          <option value="discord">{locale.t("video.export.target_discord")}</option>
          <option value="nitro">{locale.t("video.export.target_nitro")}</option>
          <option value="none">{locale.t("video.export.target_none")}</option>
        </select>
      </label>
    </div>
  {/if}

  <!-- Size comparison across all four formats: apples to apples, same frame count and
       dimensions, only the per-format coefficient differs (plus MP4's audio term).
       Wraps because four columns do not fit one 320px row. -->
  <div class="flex flex-col gap-1">
    <span class="text-[11px] text-neutral-400">{locale.t("video.export.size_compare")}</span>
    <div class="flex flex-wrap gap-x-2 gap-y-0.5 text-[11px] tabular-nums">
      {#each ALL_FORMATS as f (f)}
        <span
          class:text-neutral-100={format === f}
          class:text-neutral-500={format !== f}
        >
          {locale.t(`video.export.format_${f}`)}
          {formatBytes(sizeFor(f))}
        </span>
      {/each}
    </div>
  </div>

  <div class="flex items-center justify-between text-xs">
    <span class:text-amber-400={overLimit} class="tabular-nums">
      {result
        ? locale.t("video.export.size_actual", { size: formatBytes(shownBytes) })
        : locale.t("video.export.size_estimate", { size: formatBytes(shownBytes) })}
    </span>
    <span class="text-neutral-400">
      {locale.t("video.export.frames", { count: String(outFrames) })}
    </span>
  </div>

  {#if overLimit}
    <p class="text-[11px] text-amber-400">
      {locale.t("video.export.over_limit", {
        target: locale.t(`video.export.target_${sizeTarget}`),
      })}
    </p>
  {/if}

  <!-- Asking for audio is not the same as getting it: a source with no track, or
       one the mp4 container will not hold, degrades to a silent export rather than
       failing. Say so here instead of letting the user find out on playback. -->
  {#if result && audioOn && !result.has_audio}
    <p class="text-[11px] text-amber-400">{locale.t("video.export.audio_dropped")}</p>
  {/if}

  {#if errorText}
    <div class="flex flex-col gap-1">
      <p class="text-[11px] text-red-400">{errorText}</p>
      <button class="text-[11px] text-neutral-400 hover:text-neutral-200 text-left" onclick={copyDetails}>
        {locale.t("video.export.err_copy_details")}
      </button>
    </div>
  {/if}

  <div class="flex items-center gap-2">
    <button
      class="flex-1 px-2 py-1.5 rounded-lg text-xs bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40"
      disabled={running || capability?.available === false}
      onclick={onSaveAs}
    >
      {locale.t("video.export.save_as")}
    </button>
    <button
      class="flex-1 px-2 py-1.5 rounded-lg text-xs bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40"
      disabled={running || capability?.available === false}
      title={locale.t("video.export.copy_tip_linux")}
      onclick={onCopy}
    >
      {isTauri ? locale.t("video.export.copy") : locale.t("video.export.download")}
    </button>
  </div>

  {#if running}
    <p class="text-[11px] text-neutral-400 tabular-nums">{progressLabel}</p>
  {/if}
</div>
