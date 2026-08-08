<script lang="ts">
  /**
   * Shot-list editor for the vendored MiniMax H3 Director node.
   *
   * Everything here drives the `videoTimeline` store, which compiles the whole
   * thing into the node's `timeline_data` JSON at generation time. The strip is
   * measured in frames at H3's fixed 24 fps, the same unit the node uses.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { videoTimeline } from "../../stores/videoTimeline.svelte.js";
  import type {
    TimelineAudioSegment,
    TimelineMotionSegment,
    TimelinePromptFormat,
    TimelineSegment,
  } from "../../stores/videoTimeline.svelte.js";
  import { uploadImageBytes } from "../../utils/api.js";
  import {
    H3_FPS,
    H3_MAX_REF_AUDIOS,
    H3_MAX_REF_VIDEOS,
    H3_MAX_TIMELINE_CHARACTERS,
  } from "../../utils/videoParams.js";

  type TrackId = "shots" | "motion" | "audio";
  /** The fields every track shares, which is all the strip itself needs. */
  type AnySegment = TimelineSegment | TimelineMotionSegment | TimelineAudioSegment;

  /** Strip scale. A 15 s timeline is ~960 px wide, so it scrolls rather than squeezes. */
  const PX_PER_SECOND = 64;
  const PX_PER_FRAME = PX_PER_SECOND / H3_FPS;

  let selected = $state<{ track: TrackId; id: string } | null>(null);
  let previews = $state<Record<string, string | null>>({});
  let uploadingKey = $state<string | null>(null);
  let dropKey = $state<string | null>(null);
  let uploadError = $state<string | null>(null);

  /** Frames the backend will actually render, from the duration slider. */
  const windowFrames = $derived(generation.videoFrameLength);
  const stripFrames = $derived(Math.max(windowFrames, videoTimeline.totalFrames, H3_FPS));
  const stripWidth = $derived(Math.round(stripFrames * PX_PER_FRAME));
  const overflows = $derived(videoTimeline.totalFrames > windowFrames);

  const selectedSegment = $derived.by<AnySegment | null>(() => {
    const sel = selected;
    if (!sel) return null;
    return segmentsOf(sel.track).find((s) => s.id === sel.id) ?? null;
  });
  const selectedShot = $derived(
    selected?.track === "shots" ? (selectedSegment as TimelineSegment | null) : null,
  );

  function segmentsOf(track: TrackId): AnySegment[] {
    if (track === "shots") return videoTimeline.segments;
    if (track === "motion") return videoTimeline.motionSegments;
    return videoTimeline.audioSegments;
  }

  function patchSegment(track: TrackId, id: string, patch: Partial<AnySegment>) {
    if (track === "shots") videoTimeline.updateSegment(id, patch as Partial<TimelineSegment>);
    else if (track === "motion")
      videoTimeline.updateMotionSegment(id, patch as Partial<TimelineMotionSegment>);
    else videoTimeline.updateAudioSegment(id, patch as Partial<TimelineAudioSegment>);
  }

  function dropSegment(track: TrackId, id: string) {
    if (track === "shots") videoTimeline.removeSegment(id);
    else if (track === "motion") videoTimeline.removeMotionSegment(id);
    else videoTimeline.removeAudioSegment(id);
    if (selected?.id === id) selected = null;
    videoTimeline.flush();
  }

  function seconds(frameCount: number): string {
    return (frameCount / H3_FPS).toFixed(1);
  }

  // --- Dragging ------------------------------------------------------------

  type DragState = {
    track: TrackId;
    id: string;
    mode: "move" | "resize";
    originX: number;
    originStart: number;
    originLength: number;
  };
  let drag: DragState | null = null;

  function beginDrag(event: PointerEvent, track: TrackId, segment: AnySegment, mode: DragState["mode"]) {
    event.preventDefault();
    event.stopPropagation();
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    selected = { track, id: segment.id };
    drag = {
      track,
      id: segment.id,
      mode,
      originX: event.clientX,
      originStart: segment.start,
      originLength: segment.length,
    };
  }

  function moveDrag(event: PointerEvent) {
    if (!drag) return;
    const delta = Math.round((event.clientX - drag.originX) / PX_PER_FRAME);
    if (drag.mode === "move") {
      patchSegment(drag.track, drag.id, { start: Math.max(0, drag.originStart + delta) });
    } else {
      patchSegment(drag.track, drag.id, { length: Math.max(1, drag.originLength + delta) });
    }
  }

  function endDrag(event: PointerEvent) {
    if (!drag) return;
    drag = null;
    (event.currentTarget as HTMLElement).releasePointerCapture?.(event.pointerId);
    videoTimeline.flush();
  }

  /** Keyboard equivalent of dragging: arrows nudge, shift+arrows resize. */
  function nudge(event: KeyboardEvent, track: TrackId, segment: AnySegment) {
    const step = event.altKey ? 1 : H3_FPS;
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      const delta = event.key === "ArrowLeft" ? -step : step;
      if (event.shiftKey) {
        patchSegment(track, segment.id, { length: Math.max(1, segment.length + delta) });
      } else {
        patchSegment(track, segment.id, { start: Math.max(0, segment.start + delta) });
      }
      videoTimeline.flush();
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      selected = { track, id: segment.id };
    }
  }

  // --- Uploads -------------------------------------------------------------

  function setPreview(key: string, url: string | null) {
    const current = previews[key];
    if (current && current !== url) URL.revokeObjectURL(current);
    previews = { ...previews, [key]: url };
  }

  /**
   * Downscale stills so multi-megapixel drops stay off the wire. Clips and
   * audio travel untouched: ComfyUI's upload endpoint writes raw bytes under
   * the filename, and the Director resolves plain input-folder names.
   */
  async function prepareUpload(file: File, maxDimension = 1536): Promise<File> {
    if (!file.type.startsWith("image/")) return file;
    const bitmap = await createImageBitmap(file).catch(() => null);
    if (!bitmap) return file;
    const scale = Math.min(1, maxDimension / Math.max(bitmap.width, bitmap.height));
    if (scale >= 1) {
      bitmap.close();
      return file;
    }
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(bitmap.width * scale);
    canvas.height = Math.round(bitmap.height * scale);
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      bitmap.close();
      return file;
    }
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    bitmap.close();
    const blob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob((b) => resolve(b), "image/png"),
    );
    if (!blob) return file;
    return new File([blob], file.name.replace(/\.[^.]+$/, "") + ".png", { type: "image/png" });
  }

  /**
   * Upload one file and hand the caller the ComfyUI input-folder name to store.
   * `key` scopes the spinner and the local preview to a single slot.
   */
  async function uploadTo(key: string, file: File, assign: (name: string, original: string) => void) {
    uploadError = null;
    const prepared = await prepareUpload(file);
    if (prepared.type.startsWith("image/")) {
      setPreview(key, URL.createObjectURL(prepared));
    }
    uploadingKey = key;
    try {
      const buffer = await prepared.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const result = await uploadImageBytes(bytes, prepared.name);
      assign(result.name, file.name);
      videoTimeline.flush();
    } catch (e) {
      console.error("Failed to upload timeline media:", e);
      uploadError = String(e);
      setPreview(key, null);
    } finally {
      uploadingKey = null;
    }
  }

  function addShot() {
    const shot = videoTimeline.addSegment();
    selected = { track: "shots", id: shot.id };
    videoTimeline.flush();
  }

  function addMotion() {
    const shot = videoTimeline.addMotionSegment();
    if (shot) selected = { track: "motion", id: shot.id };
    videoTimeline.flush();
  }

  function addAudio() {
    const shot = videoTimeline.addAudioSegment();
    if (shot) selected = { track: "audio", id: shot.id };
    videoTimeline.flush();
  }

  function clearAll() {
    videoTimeline.clear();
    selected = null;
    previews = {};
    videoTimeline.flush();
  }

  function removeCastPhoto(characterId: string, file: string) {
    const character = videoTimeline.characters.find((c) => c.id === characterId);
    if (!character) return;
    videoTimeline.updateCharacter(characterId, {
      files: character.files.filter((f) => f !== file),
    });
    videoTimeline.flush();
  }

  const trackLabelKeys: Record<TrackId, string> = {
    shots: "video_timeline.track.shots",
    motion: "video_timeline.track.motion",
    audio: "video_timeline.track.audio",
  };
</script>

{#snippet mediaSlot(
  key: string,
  filename: string,
  label: string,
  accept: string,
  assign: (name: string, original: string) => void,
  clear: (() => void) | null,
)}
  {@const preview = previews[key]}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="relative rounded-lg border border-dashed bg-neutral-800/40 min-h-20 flex flex-col items-center justify-center p-2 transition-colors {dropKey ===
    key
      ? 'border-indigo-500 bg-indigo-500/10'
      : 'border-neutral-600 hover:border-neutral-500'}"
    ondragenter={(e) => {
      e.preventDefault();
      dropKey = key;
    }}
    ondragover={(e) => e.preventDefault()}
    ondragleave={() => {
      if (dropKey === key) dropKey = null;
    }}
    ondrop={async (e) => {
      e.preventDefault();
      dropKey = null;
      const file = e.dataTransfer?.files?.[0];
      if (file) await uploadTo(key, file, assign);
    }}
  >
    {#if preview}
      <img src={preview} alt="" class="max-h-20 rounded object-contain mb-1.5" />
    {:else if filename}
      <p class="text-[11px] text-neutral-400 mb-1.5 text-center break-all">{filename}</p>
    {/if}
    {#if preview || filename}
      {#if clear}
        <button
          type="button"
          class="text-[11px] text-red-400 hover:text-red-300"
          onclick={() => {
            setPreview(key, null);
            clear();
          }}
        >
          {locale.t("common.remove")}
        </button>
      {/if}
    {:else}
      <p class="text-[11px] text-neutral-500 text-center">
        {label}
        <label class="text-indigo-400 hover:text-indigo-300 cursor-pointer ml-1">
          {locale.t("generation.image_edit.upload_prompt")}
          <input
            type="file"
            {accept}
            class="hidden"
            onchange={async (e) => {
              const input = e.currentTarget as HTMLInputElement;
              const file = input.files?.[0];
              input.value = "";
              if (file) await uploadTo(key, file, assign);
            }}
          />
        </label>
      </p>
    {/if}
    {#if uploadingKey === key}
      <div class="absolute inset-0 flex items-center justify-center bg-neutral-900/70 rounded-lg">
        <div
          class="w-5 h-5 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"
        ></div>
      </div>
    {/if}
  </div>
{/snippet}

{#snippet track(id: TrackId, items: AnySegment[])}
  <div class="flex items-stretch gap-2">
    <span class="w-24 shrink-0 pt-3 text-[11px] text-neutral-500">{locale.t(trackLabelKeys[id])}</span>
    <div class="relative h-16 rounded-md bg-neutral-800/40" style="width: {stripWidth}px">
      {#each items as segment (segment.id)}
        {@const active = selected?.track === id && selected?.id === segment.id}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div
          role="button"
          tabindex="0"
          aria-label={locale.t(trackLabelKeys[id])}
          class="absolute top-1.5 bottom-1.5 rounded border px-1.5 py-1.5 overflow-hidden cursor-grab select-none touch-none transition-colors {active
            ? 'border-indigo-400 bg-indigo-500/25'
            : 'border-neutral-600 bg-neutral-700/60 hover:border-neutral-500'}"
          style="left: {Math.round(segment.start * PX_PER_FRAME)}px; width: {Math.max(
            18,
            Math.round(segment.length * PX_PER_FRAME),
          )}px"
          onpointerdown={(e) => beginDrag(e, id, segment, "move")}
          onpointermove={moveDrag}
          onpointerup={endDrag}
          onpointercancel={endDrag}
          onkeydown={(e) => nudge(e, id, segment)}
        >
          <span class="block text-[11px] leading-snug text-neutral-200 truncate pointer-events-none">
            {#if id === "shots"}
              {(segment as TimelineSegment).prompt.trim() ||
                segment.fileName ||
                locale.t("video_timeline.untitled_shot")}
            {:else}
              {segment.fileName || locale.t("video_timeline.no_file")}
            {/if}
          </span>
          <span class="block text-[11px] leading-snug text-neutral-400 pointer-events-none">
            {locale.t("video_timeline.segment_span", {
              start: seconds(segment.start),
              length: seconds(segment.length),
            })}
          </span>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="absolute inset-y-0 right-0 w-2 cursor-ew-resize bg-neutral-500/50 hover:bg-indigo-400"
            onpointerdown={(e) => beginDrag(e, id, segment, "resize")}
            onpointermove={moveDrag}
            onpointerup={endDrag}
            onpointercancel={endDrag}
          ></div>
        </div>
      {/each}
    </div>
  </div>
{/snippet}

<div class="flex h-full flex-col overflow-y-auto px-3 py-2 text-neutral-200">
  <div class="mb-3">
    <h2 class="text-sm font-semibold text-neutral-100">{locale.t("video_timeline.title")}</h2>
    <p class="text-[11px] text-neutral-500">{locale.t("video_timeline.intro")}</p>
  </div>

  <!-- Header controls -->
  <div class="flex flex-wrap items-center gap-3 mb-3">
    <label class="flex items-center gap-2 text-xs text-neutral-300">
      <input
        type="checkbox"
        checked={videoTimeline.enabled}
        onchange={(e) => {
          videoTimeline.setEnabled((e.currentTarget as HTMLInputElement).checked);
          videoTimeline.flush();
        }}
        class="accent-indigo-500"
      />
      {locale.t("video_timeline.enable")}
    </label>

    <label class="flex items-center gap-2 text-xs text-neutral-400">
      {locale.t("video_timeline.prompt_format")}
      <select
        value={videoTimeline.promptFormat}
        onchange={(e) => {
          videoTimeline.setPromptFormat(
            (e.currentTarget as HTMLSelectElement).value as TimelinePromptFormat,
          );
          videoTimeline.flush();
        }}
        class="bg-neutral-800 border border-neutral-700 rounded-md px-2 py-1 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500"
      >
        <option value="minimax">{locale.t("video_timeline.format.minimax")}</option>
        <option value="comfyui">{locale.t("video_timeline.format.comfyui")}</option>
      </select>
    </label>

    <span class="text-[11px] text-neutral-500">
      {locale.t("video_timeline.length_summary", {
        timeline: seconds(videoTimeline.totalFrames),
        window: seconds(windowFrames),
      })}
    </span>

    <div class="ml-auto flex items-center gap-2">
      <button
        type="button"
        onclick={addShot}
        class="px-2.5 py-1 text-[11px] rounded-md bg-indigo-600 hover:bg-indigo-500 text-white transition-colors"
      >
        {locale.t("video_timeline.add_shot")}
      </button>
      <button
        type="button"
        onclick={clearAll}
        class="px-2.5 py-1 text-[11px] rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-300 transition-colors"
      >
        {locale.t("video_timeline.clear")}
      </button>
    </div>
  </div>

  {#if !videoTimeline.enabled}
    <p class="text-[11px] text-amber-400/90 mb-3">{locale.t("video_timeline.disabled_hint")}</p>
  {/if}
  {#if overflows}
    <p class="text-[11px] text-amber-400/90 mb-3">{locale.t("video_timeline.overflow_hint")}</p>
  {/if}
  {#if uploadError}
    <p class="text-[11px] text-red-400 mb-3">{uploadError}</p>
  {/if}

  <!-- Strip -->
  <div class="shrink-0 overflow-x-auto overflow-y-hidden pb-2 mb-4">
    <div class="min-w-max space-y-2">
      <!-- Ruler -->
      <div class="flex items-end gap-2">
        <span class="w-24 shrink-0"></span>
        <div class="relative h-5" style="width: {stripWidth}px">
          {#each Array.from({ length: Math.ceil(stripFrames / H3_FPS) + 1 }, (_, i) => i) as tick (tick)}
            <span
              class="absolute top-0 text-[10px] text-neutral-500 border-l border-neutral-700 pl-0.5"
              style="left: {Math.round(tick * H3_FPS * PX_PER_FRAME)}px">{tick}</span
            >
          {/each}
          <span
            class="absolute inset-y-0 border-l border-indigo-500/60"
            style="left: {Math.round(windowFrames * PX_PER_FRAME)}px"
          ></span>
        </div>
      </div>

      {@render track("shots", videoTimeline.segments)}
      {@render track("motion", videoTimeline.motionSegments)}
      {@render track("audio", videoTimeline.audioSegments)}
    </div>
  </div>

  {#if videoTimeline.segments.length === 0}
    <p class="text-[11px] text-neutral-500 mb-4">{locale.t("video_timeline.empty")}</p>
  {/if}

  <div class="flex flex-wrap items-center gap-2 mb-4">
    <button
      type="button"
      onclick={addMotion}
      disabled={videoTimeline.motionSegments.length >= H3_MAX_REF_VIDEOS}
      class="px-2.5 py-1 text-[11px] rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-300 disabled:opacity-40 disabled:hover:bg-neutral-800 transition-colors"
    >
      {locale.t("video_timeline.add_motion")}
    </button>
    <button
      type="button"
      onclick={addAudio}
      disabled={videoTimeline.audioSegments.length >= H3_MAX_REF_AUDIOS}
      class="px-2.5 py-1 text-[11px] rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-300 disabled:opacity-40 disabled:hover:bg-neutral-800 transition-colors"
    >
      {locale.t("video_timeline.add_audio")}
    </button>
    <span class="text-[11px] text-neutral-600">{locale.t("video_timeline.reference_hint")}</span>
  </div>

  <!-- Inspector -->
  <div class="rounded-lg border border-neutral-800 bg-neutral-900/40 p-3 mb-4">
    <h3 class="text-xs font-semibold text-neutral-300 mb-2">
      {locale.t("video_timeline.inspector.title")}
    </h3>
    {#if !selected || !selectedSegment}
      <p class="text-[11px] text-neutral-500">{locale.t("video_timeline.inspector.none")}</p>
    {:else}
      {@const segment = selectedSegment}
      {@const trackId = selected.track}
      <div class="grid gap-3 sm:grid-cols-2">
        <div>
          {#if trackId === "shots" && selectedShot}
            {@render mediaSlot(
              `shot-${segment.id}`,
              segment.fileName,
              selectedShot.kind === "video"
                ? locale.t("video_timeline.drop_clip")
                : locale.t("video_timeline.drop_still"),
              selectedShot.kind === "video" ? "video/*" : "image/*",
              (name, original) =>
                videoTimeline.updateSegment(segment.id, { file: name, fileName: original }),
              () => videoTimeline.updateSegment(segment.id, { file: "", fileName: "" }),
            )}
          {:else if trackId === "motion"}
            {@render mediaSlot(
              `motion-${segment.id}`,
              segment.fileName,
              locale.t("video_timeline.drop_clip"),
              "video/*",
              (name, original) =>
                videoTimeline.updateMotionSegment(segment.id, { file: name, fileName: original }),
              () => videoTimeline.updateMotionSegment(segment.id, { file: "", fileName: "" }),
            )}
          {:else}
            {@render mediaSlot(
              `audio-${segment.id}`,
              segment.fileName,
              locale.t("video_timeline.drop_audio"),
              "audio/*",
              (name, original) =>
                videoTimeline.updateAudioSegment(segment.id, { file: name, fileName: original }),
              () => videoTimeline.updateAudioSegment(segment.id, { file: "", fileName: "" }),
            )}
          {/if}
        </div>

        <div class="space-y-2">
          {#if selectedShot}
            <div>
              <span class="block text-[11px] text-neutral-500 mb-1"
                >{locale.t("video_timeline.field.prompt")}</span
              >
              <textarea
                rows="3"
                value={selectedShot.prompt}
                placeholder={locale.t("video_timeline.field.prompt_placeholder")}
                oninput={(e) =>
                  videoTimeline.updateSegment(segment.id, {
                    prompt: (e.currentTarget as HTMLTextAreaElement).value,
                  })}
                onchange={() => videoTimeline.flush()}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-md px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 resize-y"
              ></textarea>
            </div>
            <div class="flex flex-wrap items-center gap-3">
              <label class="flex items-center gap-1.5 text-[11px] text-neutral-400">
                {locale.t("video_timeline.field.kind")}
                <select
                  value={selectedShot.kind}
                  onchange={(e) => {
                    videoTimeline.updateSegment(segment.id, {
                      kind: (e.currentTarget as HTMLSelectElement).value as "image" | "video",
                    });
                    videoTimeline.flush();
                  }}
                  class="bg-neutral-800 border border-neutral-700 rounded-md px-2 py-1 text-[11px] text-neutral-100 focus:outline-none focus:border-indigo-500"
                >
                  <option value="image">{locale.t("video_timeline.kind.image")}</option>
                  <option value="video">{locale.t("video_timeline.kind.video")}</option>
                </select>
              </label>
              {#if selectedShot.kind === "image"}
                <label class="flex items-center gap-1.5 text-[11px] text-neutral-400">
                  <input
                    type="checkbox"
                    checked={selectedShot.isEndFrame}
                    onchange={(e) => {
                      videoTimeline.updateSegment(segment.id, {
                        isEndFrame: (e.currentTarget as HTMLInputElement).checked,
                      });
                      videoTimeline.flush();
                    }}
                    class="accent-indigo-500"
                  />
                  {locale.t("video_timeline.field.end_frame")}
                </label>
              {/if}
            </div>
          {/if}

          <div class="grid grid-cols-3 gap-2">
            <label class="text-[11px] text-neutral-500">
              {locale.t("video_timeline.field.start")}
              <input
                type="number"
                min="0"
                step={H3_FPS}
                value={segment.start}
                oninput={(e) =>
                  patchSegment(trackId, segment.id, {
                    start: Math.max(0, Math.round(Number((e.currentTarget as HTMLInputElement).value) || 0)),
                  })}
                onchange={() => videoTimeline.flush()}
                class="mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-md px-2 py-1 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500"
              />
            </label>
            <label class="text-[11px] text-neutral-500">
              {locale.t("video_timeline.field.length")}
              <input
                type="number"
                min="1"
                step={H3_FPS}
                value={segment.length}
                oninput={(e) =>
                  patchSegment(trackId, segment.id, {
                    length: Math.max(1, Math.round(Number((e.currentTarget as HTMLInputElement).value) || 1)),
                  })}
                onchange={() => videoTimeline.flush()}
                class="mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-md px-2 py-1 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500"
              />
            </label>
            <label class="text-[11px] text-neutral-500">
              {locale.t("video_timeline.field.trim_start")}
              <input
                type="number"
                min="0"
                step={H3_FPS}
                value={segment.trimStart}
                oninput={(e) =>
                  patchSegment(trackId, segment.id, {
                    trimStart: Math.max(
                      0,
                      Math.round(Number((e.currentTarget as HTMLInputElement).value) || 0),
                    ),
                  })}
                onchange={() => videoTimeline.flush()}
                class="mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-md px-2 py-1 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500"
              />
            </label>
          </div>
          <p class="text-[11px] text-neutral-600">{locale.t("video_timeline.frames_hint")}</p>

          <button
            type="button"
            onclick={() => dropSegment(trackId, segment.id)}
            class="text-[11px] text-red-400 hover:text-red-300"
          >
            {locale.t("video_timeline.remove_segment")}
          </button>
        </div>
      </div>
    {/if}
  </div>

  <!-- Cast -->
  <div class="mb-4">
    <div class="flex items-center gap-2 mb-1">
      <h3 class="text-xs font-semibold text-neutral-300">{locale.t("video_timeline.cast.title")}</h3>
      <button
        type="button"
        onclick={() => {
          videoTimeline.addCharacter();
          videoTimeline.flush();
        }}
        disabled={videoTimeline.characters.length >= H3_MAX_TIMELINE_CHARACTERS}
        class="ml-auto px-2.5 py-1 text-[11px] rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-300 disabled:opacity-40 disabled:hover:bg-neutral-800 transition-colors"
      >
        {locale.t("video_timeline.cast.add")}
      </button>
    </div>
    <p class="text-[11px] text-neutral-500 mb-2">{locale.t("video_timeline.cast.intro")}</p>

    {#if videoTimeline.characters.length === 0}
      <p class="text-[11px] text-neutral-600">{locale.t("video_timeline.cast.empty")}</p>
    {/if}

    <div class="space-y-3">
      {#each videoTimeline.characters as character, index (character.id)}
        <div class="rounded-lg border border-neutral-800 bg-neutral-900/40 p-3">
          <div class="flex items-center gap-2 mb-2">
            <span class="text-[11px] font-mono text-indigo-300"
              >{locale.t("video_timeline.cast.tag", { index: String(index + 1) })}</span
            >
            <button
              type="button"
              onclick={() => {
                videoTimeline.removeCharacter(character.id);
                videoTimeline.flush();
              }}
              class="ml-auto text-[11px] text-red-400 hover:text-red-300"
            >
              {locale.t("common.remove")}
            </button>
          </div>
          <textarea
            rows="2"
            value={character.description}
            placeholder={locale.t("video_timeline.cast.description_placeholder")}
            oninput={(e) =>
              videoTimeline.updateCharacter(character.id, {
                description: (e.currentTarget as HTMLTextAreaElement).value,
              })}
            onchange={() => videoTimeline.flush()}
            class="w-full bg-neutral-800 border border-neutral-700 rounded-md px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 resize-y mb-2"
          ></textarea>
          <div class="grid grid-cols-2 sm:grid-cols-4 gap-2">
            {#each character.files as file (file)}
              <div
                class="relative rounded-lg border border-neutral-700 bg-neutral-800/40 p-2 flex flex-col items-center justify-center min-h-20"
              >
                <p class="text-[11px] text-neutral-400 text-center break-all mb-1.5">{file}</p>
                <button
                  type="button"
                  class="text-[11px] text-red-400 hover:text-red-300"
                  onclick={() => removeCastPhoto(character.id, file)}
                >
                  {locale.t("common.remove")}
                </button>
              </div>
            {/each}
            {@render mediaSlot(
              `cast-${character.id}`,
              "",
              locale.t("video_timeline.cast.drop_photo"),
              "image/*",
              (name) => {
                const current = videoTimeline.characters.find((c) => c.id === character.id);
                if (!current) return;
                videoTimeline.updateCharacter(character.id, { files: [...current.files, name] });
                setPreview(`cast-${character.id}`, null);
              },
              null,
            )}
          </div>
        </div>
      {/each}
    </div>
  </div>

  <!-- Sound -->
  <div class="mb-2">
    <h3 class="text-xs font-semibold text-neutral-300 mb-2">
      {locale.t("video_timeline.sound.title")}
    </h3>
    <div class="grid gap-3 sm:grid-cols-2">
      <label class="text-[11px] text-neutral-500">
        {locale.t("video_timeline.sound.soundscape")}
        <textarea
          rows="2"
          value={videoTimeline.overallSoundscape}
          placeholder={locale.t("video_timeline.sound.soundscape_placeholder")}
          oninput={(e) =>
            videoTimeline.setOverallSoundscape((e.currentTarget as HTMLTextAreaElement).value)}
          onchange={() => videoTimeline.flush()}
          class="mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-md px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 resize-y"
        ></textarea>
      </label>
      <label class="text-[11px] text-neutral-500">
        {locale.t("video_timeline.sound.music")}
        <textarea
          rows="2"
          value={videoTimeline.nonDiegeticMusic}
          placeholder={locale.t("video_timeline.sound.music_placeholder")}
          oninput={(e) =>
            videoTimeline.setNonDiegeticMusic((e.currentTarget as HTMLTextAreaElement).value)}
          onchange={() => videoTimeline.flush()}
          class="mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-md px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 resize-y"
        ></textarea>
      </label>
    </div>
  </div>
</div>
