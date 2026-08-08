<script lang="ts">
  import { onMount } from "svelte";
  import { generation } from "../../stores/generation.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import {
    checkNodeAvailable,
    detectLlmHardware,
    getConfig,
    installRife,
    isRifeInstalled,
    readClipboardImageSafe,
    startComfyui,
    stopComfyui,
    uploadImageBytes,
  } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import {
    H3_ASPECT_RATIOS,
    H3_FPS,
    H3_MAX_DURATION_SECONDS,
    H3_MAX_REF_IMAGES,
    H3_MEGAPIXEL_OPTIONS,
    H3_MIN_DURATION_SECONDS,
    estimateH3ModelGb,
    estimateH3VramGb,
    isH3HighVramHarmful,
  } from "../../utils/videoParams.js";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import type { VideoAspectRatio, VideoVariant } from "../../types/index.js";
  import EditableValue from "../ui/EditableValue.svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  /**
   * One upload target. `fl2va` exposes two (first frame / last frame), `ref2va`
   * exposes a growing list of reference slots. Keys are stable per variant so a
   * preview URL survives collapsing and re-expanding the section.
   */
  interface FrameSlot {
    key: string;
    label: string;
    filename: string | null;
    assign: (value: string | null) => void;
    /** Present on the fl2va frame slots only: records the uploaded image's own
     *  `"W:H"` so the "match image" aspect ratio has something to match. */
    assignAspect?: (value: string | null) => void;
  }

  const variants: { id: VideoVariant; labelKey: string; descKey: string }[] = [
    { id: "fl2va", labelKey: "generation.video.variant_fl2va", descKey: "generation.video.variant_fl2va_desc" },
    { id: "ref2va", labelKey: "generation.video.variant_ref2va", descKey: "generation.video.variant_ref2va_desc" },
  ];

  /** Matches the `node_name` the Rust installer stamps on `install:progress`. */
  const RIFE_PACKAGE_NAME = "ComfyUI-Frame-Interpolation";
  /** ComfyUI class the pack registers. The space is part of the name. */
  const RIFE_NODE_CLASS = "RIFE VFI";

  /**
   * Read straight off the shared store rather than fetching once on mount: the
   * lazy RIFE install restarts ComfyUI, and a local one-shot copy would still be
   * holding the pre-restart lists (or an empty one) afterwards. `App.svelte`
   * refreshes the store on every connection / server-ready event.
   */
  const diffusionModels = $derived(models.diffusionModels);
  const clipModels = $derived(models.textEncoders);
  const vaeModels = $derived(models.vaes);
  let detectedVramGb = $state<number | null>(null);
  /** `config.vram_mode`; "high" maps to ComfyUI's `--highvram`. `null` until read. */
  let vramMode = $state<string | null>(null);

  /**
   * `null` until the disk check answers. The frame-interpolation pack is a
   * lazy install: it only matters in video mode and drags a ~60 MB checkpoint
   * behind it, so nothing is fetched until the user asks for interpolation.
   */
  let rifeInstalled = $state<boolean | null>(null);
  let rifeInstalling = $state(false);
  let rifeInstallStep = $state("");
  let rifeInstallMessage = $state("");
  let rifeInstallError = $state<string | null>(null);

  let previews = $state<Record<string, string | null>>({});
  let uploadingSlot = $state<string | null>(null);
  let dropSlot = $state<string | null>(null);
  let pasteSlot = $state<string | null>(null);
  let uploadError = $state<string | null>(null);
  let dropZone = $state<HTMLElement | null>(null);

  const dimensions = $derived(generation.videoDimensions);

  /** Highest filled reference slot, so the list grows one empty slot at a time. */
  const visibleRefSlots = $derived(
    Math.min(
      H3_MAX_REF_IMAGES,
      generation.videoRefImages.reduce((last, value, index) => (value ? index + 1 : last), 0) + 1,
    ),
  );

  const slots = $derived<FrameSlot[]>(
    generation.videoVariant === "fl2va"
      ? [
          {
            key: "first",
            label: locale.t("generation.video.first_frame"),
            filename: generation.videoFirstFrame,
            assign: (value) => (generation.videoFirstFrame = value),
            assignAspect: (value) => (generation.videoFirstFrameAspect = value),
          },
          // Hidden rather than overwritten while the first frame doubles as the
          // last one, so a separately uploaded last frame comes back on untick.
          ...(generation.videoFirstFrameAsLast
            ? []
            : [
                {
                  key: "last",
                  label: locale.t("generation.video.last_frame"),
                  filename: generation.videoLastFrame,
                  assign: (value: string | null) => (generation.videoLastFrame = value),
                  assignAspect: (value: string | null) =>
                    (generation.videoLastFrameAspect = value),
                },
              ]),
        ]
      : Array.from({ length: visibleRefSlots }, (_, index) => ({
          key: `ref${index}`,
          label: locale.t("generation.video.ref_slot", { index: index + 1 }),
          filename: generation.videoRefImages[index] ?? null,
          assign: (value: string | null) => {
            generation.videoRefImages = generation.videoRefImages.map((current, i) =>
              i === index ? value : current,
            );
          },
        })),
  );

  const modelGb = $derived(estimateH3ModelGb(generation.videoDiffusionModel));
  const requiredVramGb = $derived(
    estimateH3VramGb(dimensions.width, dimensions.height, generation.videoFrameLength, modelGb),
  );
  const vramWarning = $derived(detectedVramGb !== null && requiredVramGb > detectedVramGb);

  /**
   * VRAM Mode "high" force-loads the whole DiT instead of staging it, which on a
   * card this model barely fits leaves nothing for activations and drags every
   * sampler step over PCIe. It never raises an out-of-memory error, so without
   * this banner the only symptom is a generation that takes an hour.
   */
  const highVramWarning = $derived(
    vramMode === "high" && isH3HighVramHarmful(modelGb, detectedVramGb),
  );

  /** Coarse stage weights - the install has no single measurable total. */
  const rifeInstallPercent = $derived(
    { clone: 20, download: 50, done: 65, restart: 80, verify: 95 }[rifeInstallStep] ?? 10,
  );

  onMount(() => {
    if (models.diffusionModels.length === 0 && !models.loading) void models.refresh();
    void loadHardware();
    void loadRifeState();
    const unlisten = ipcListen("install:progress", (event: any) => {
      const data = event.payload as { node_name: string; step: string; message: string };
      if (data.node_name !== RIFE_PACKAGE_NAME) return;
      rifeInstallStep = data.step;
      rifeInstallMessage = data.message;
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  });

  async function loadHardware() {
    try {
      const hardware = await detectLlmHardware();
      const maxVramMb = hardware.gpus.reduce((max, gpu) => Math.max(max, gpu.vram_mb), 0);
      detectedVramGb = maxVramMb > 0 ? maxVramMb / 1024 : null;
    } catch {
      detectedVramGb = null;
    }
    try {
      vramMode = (await getConfig()).vram_mode;
    } catch {
      vramMode = null;
    }
  }

  async function loadRifeState() {
    try {
      rifeInstalled = await isRifeInstalled();
    } catch {
      rifeInstalled = null;
    }
  }

  function toggleFirstFrameAsLast(event: Event) {
    generation.videoFirstFrameAsLast = (event.currentTarget as HTMLInputElement).checked;
    generation.saveSettings();
  }

  /**
   * Toggling interpolation on with the pack missing does not flip the switch —
   * it reveals the install prompt. The store flag only ever turns on once the
   * nodes are actually loadable, so a queued video can never reference a node
   * class ComfyUI does not have.
   */
  function toggleRife(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const next = input.checked;
    rifeInstallError = null;
    if (!next) {
      generation.videoRifeEnabled = false;
      generation.saveSettings();
    } else if (rifeInstalled) {
      generation.videoRifeEnabled = true;
      generation.saveSettings();
    } else {
      void installRifeNodes();
    }
    // The browser already flipped the box on click; the store is the only
    // thing allowed to decide what it shows, and it stays off until the
    // nodes exist. Svelte re-applies `checked` when the store does change.
    input.checked = generation.videoRifeEnabled;
  }

  /**
   * Clone + checkpoint download, then a ComfyUI restart: a freshly cloned pack
   * is not importable until the server re-scans `custom_nodes`.
   */
  async function installRifeNodes() {
    rifeInstalling = true;
    rifeInstallError = null;
    rifeInstallStep = "clone";
    rifeInstallMessage = locale.t("generation.video.rife_install_starting");
    try {
      await installRife();

      rifeInstallStep = "restart";
      rifeInstallMessage = locale.t("generation.video.rife_install_restarting");
      connection.connected = false;
      await stopComfyui();
      await startComfyui();

      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(
          () => reject(new Error(locale.t("generation.video.rife_install_timeout"))),
          120_000,
        );
        const unlistenReady = ipcListen("comfyui:server_ready", () => {
          clearTimeout(timeout);
          void unlistenReady.then((fn) => fn());
          void unlistenError.then((fn) => fn());
          resolve();
        });
        const unlistenError = ipcListen("comfyui:server_error", (event: any) => {
          clearTimeout(timeout);
          void unlistenReady.then((fn) => fn());
          void unlistenError.then((fn) => fn());
          reject(new Error(event.payload?.error || locale.t("generation.video.rife_install_failed_start")));
        });
      });

      rifeInstallStep = "verify";
      rifeInstallMessage = locale.t("generation.video.rife_install_verifying");
      const available = await checkNodeAvailable(RIFE_NODE_CLASS).catch(() => false);
      if (!available) throw new Error(locale.t("generation.video.rife_install_not_loaded"));

      rifeInstalled = true;
      generation.videoRifeEnabled = true;
      generation.saveSettings();
    } catch (e) {
      rifeInstallError = String(e);
      rifeInstalled = await isRifeInstalled().catch(() => false);
    } finally {
      // The restart invalidates whatever the store cached, whether or not the
      // install itself succeeded. `refresh()` swallows its own errors.
      await models.refresh();
      rifeInstalling = false;
      rifeInstallStep = "";
      rifeInstallMessage = "";
    }
  }

  // Paste into whichever slot the pointer is over, matching ImageEditSettings.
  $effect(() => {
    const el = dropZone;
    if (!el) return;
    const onPaste = async (event: ClipboardEvent) => {
      const slot = pasteSlot;
      if (slot === null) return;
      const target = event.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)
        return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const bytes = await readClipboardImageSafe();
      if (!bytes) return;
      const blob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
      await uploadToSlot(slot, new File([blob], `${slot}.png`, { type: "image/png" }));
    };
    window.addEventListener("paste", onPaste, { capture: true });
    return () => window.removeEventListener("paste", onPaste, { capture: true });
  });

  function setPreview(key: string, url: string | null) {
    const current = previews[key];
    if (current && current !== url) URL.revokeObjectURL(current);
    previews = { ...previews, [key]: url };
  }

  /**
   * Decode and downscale to keep multi-megapixel drops off the wire, reporting
   * the source image's own pixel size as `"W:H"`. The downscale is uniform, so
   * that string still describes the frame's shape and is what the "match image"
   * aspect ratio matches. `null` when the image could not be decoded.
   */
  async function prepareImage(
    file: File,
    maxDimension = 1536,
  ): Promise<{ file: File; aspect: string | null }> {
    const bitmap = await createImageBitmap(file).catch(() => null);
    if (!bitmap) return { file, aspect: null };
    const aspect = `${bitmap.width}:${bitmap.height}`;
    const scale = Math.min(1, maxDimension / Math.max(bitmap.width, bitmap.height));
    if (scale >= 1) {
      bitmap.close();
      return { file, aspect };
    }
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(bitmap.width * scale);
    canvas.height = Math.round(bitmap.height * scale);
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      bitmap.close();
      return { file, aspect };
    }
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    bitmap.close();
    const blob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob((b) => resolve(b), "image/png"),
    );
    if (!blob) return { file, aspect };
    return {
      file: new File([blob], file.name.replace(/\.[^.]+$/, "") + ".png", { type: "image/png" }),
      aspect,
    };
  }

  function findSlot(key: string): FrameSlot | undefined {
    return slots.find((slot) => slot.key === key);
  }

  async function uploadToSlot(key: string, file: File) {
    if (!file.type.startsWith("image/")) return;
    const slot = findSlot(key);
    if (!slot) return;
    uploadError = null;
    const { file: prepared, aspect } = await prepareImage(file);
    setPreview(key, URL.createObjectURL(prepared));
    uploadingSlot = key;
    try {
      const buffer = await prepared.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const result = await uploadImageBytes(bytes, prepared.name);
      slot.assign(result.name);
      slot.assignAspect?.(aspect);
      // A frame the user just supplied is a strong statement about the framing
      // they want, so match it. They can still pick a fixed ratio afterwards.
      if (slot.assignAspect && aspect) generation.videoAspectRatio = "auto";
      generation.saveSettings();
    } catch (e) {
      console.error("Failed to upload video frame:", e);
      uploadError = String(e);
      setPreview(key, null);
      slot.assign(null);
      slot.assignAspect?.(null);
    } finally {
      uploadingSlot = null;
    }
  }

  function clearSlot(key: string) {
    setPreview(key, null);
    const slot = findSlot(key);
    slot?.assign(null);
    slot?.assignAspect?.(null);
    generation.saveSettings();
  }

  function setVariant(variant: VideoVariant) {
    generation.videoVariant = variant;
    // ref2va sends no first/last frame, so there is nothing left to match.
    if (variant !== "fl2va" && generation.videoAspectRatio === "auto")
      generation.videoAspectRatio = "16:9";
    generation.saveSettings();
  }
</script>

<div class="space-y-3" bind:this={dropZone}>
  <!-- Variant -->
  <div>
    <span class="block text-xs text-neutral-400 mb-1.5">
      {locale.t("generation.video.variant")}
      <InfoTip text={locale.t("generation.video.variant_tip")} />
    </span>
    <div class="grid grid-cols-2 gap-2">
      {#each variants as variant (variant.id)}
        <button
          type="button"
          class="rounded-lg border px-3 py-2 text-left transition-colors {generation.videoVariant ===
          variant.id
            ? 'border-indigo-500 bg-indigo-500/10'
            : 'border-neutral-700 bg-neutral-800/40 hover:border-neutral-600'}"
          onclick={() => setVariant(variant.id)}
        >
          <span class="block text-xs font-medium text-neutral-100">{locale.t(variant.labelKey)}</span>
          <span class="block text-[11px] leading-tight text-neutral-500 mt-0.5">
            {locale.t(variant.descKey)}
          </span>
        </button>
      {/each}
    </div>
  </div>

  <!-- Duration -->
  <div use:scrollCapture>
    <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
      <span>
        {locale.t("generation.video.duration")}
        <InfoTip text={locale.t("generation.video.duration_tip")} />
      </span>
      <EditableValue
        value={generation.videoDurationSeconds}
        min={H3_MIN_DURATION_SECONDS}
        max={H3_MAX_DURATION_SECONDS}
        step={0.5}
        decimals={1}
        suffix="s"
        onchange={(v) => {
          generation.videoDurationSeconds = v;
          generation.saveSettings();
        }}
      />
    </label>
    <input
      type="range"
      bind:value={generation.videoDurationSeconds}
      onchange={() => generation.saveSettings()}
      min={H3_MIN_DURATION_SECONDS}
      max={H3_MAX_DURATION_SECONDS}
      step="0.5"
      class="w-full accent-indigo-500"
    />
    <p class="text-[11px] text-neutral-500 mt-1">
      {locale.t("generation.video.frame_count", {
        frames: generation.videoFrameLength,
        fps: H3_FPS,
      })}
    </p>
  </div>

  <!-- Geometry -->
  <div class="grid grid-cols-2 gap-3">
    <div>
      <label class="block text-xs text-neutral-400 mb-1" for="video-aspect-ratio">
        {locale.t("generation.video.aspect_ratio")}
      </label>
      <select
        id="video-aspect-ratio"
        value={generation.videoAspectRatio}
        onchange={(e) => {
          generation.videoAspectRatio = (e.currentTarget as HTMLSelectElement)
            .value as VideoAspectRatio;
          generation.saveSettings();
        }}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#if generation.videoVariant === "fl2va"}
          <option value="auto">{locale.t("generation.video.aspect_ratio_auto")}</option>
        {/if}
        {#each H3_ASPECT_RATIOS as ratio (ratio)}
          <option value={ratio}>{ratio}</option>
        {/each}
      </select>
    </div>
    <div>
      <label class="block text-xs text-neutral-400 mb-1" for="video-megapixels">
        {locale.t("generation.video.megapixels")}
        <InfoTip text={locale.t("generation.video.megapixels_tip")} />
      </label>
      <select
        id="video-megapixels"
        value={generation.videoMegapixels}
        onchange={(e) => {
          generation.videoMegapixels = Number((e.currentTarget as HTMLSelectElement).value);
          generation.saveSettings();
        }}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each H3_MEGAPIXEL_OPTIONS as option (option)}
          <option value={option}>{option} MP</option>
        {/each}
      </select>
    </div>
  </div>
  <p class="text-[11px] text-neutral-500 -mt-1">
    {locale.t("generation.video.resolution", {
      width: dimensions.width,
      height: dimensions.height,
    })}
  </p>
  {#if generation.videoAspectRatio === "auto"}
    <p class="text-[11px] text-neutral-500 -mt-1">
      {generation.videoFrameAspect
        ? locale.t("generation.video.aspect_ratio_auto_hint", {
            source: generation.videoFrameAspect.replace(":", " x "),
          })
        : locale.t("generation.video.aspect_ratio_auto_empty")}
    </p>
  {/if}

  <!-- VRAM warning -->
  {#if vramWarning}
    <div
      class="flex items-start gap-2 rounded-lg border border-amber-600/60 bg-amber-900/25 px-3 py-2 text-xs text-amber-200"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-4 h-4 shrink-0 mt-px"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0Z" />
        <line x1="12" y1="9" x2="12" y2="13" />
        <line x1="12" y1="17" x2="12.01" y2="17" />
      </svg>
      <span>
        {locale.t("generation.video.vram_warning", {
          detected: (detectedVramGb ?? 0).toFixed(1),
          required: requiredVramGb.toFixed(1),
        })}
      </span>
    </div>
  {/if}

  <!-- VRAM Mode "high" warning - red, because this is a measured certainty rather than a risk -->
  {#if highVramWarning}
    <div
      class="flex items-start gap-2 rounded-lg border border-red-600/60 bg-red-900/25 px-3 py-2 text-xs text-red-200"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-4 h-4 shrink-0 mt-px"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <circle cx="12" cy="12" r="10" />
        <line x1="12" y1="8" x2="12" y2="12" />
        <line x1="12" y1="16" x2="12.01" y2="16" />
      </svg>
      <span>
        {locale.t("generation.video.highvram_warning", {
          model: modelGb.toFixed(1),
          detected: (detectedVramGb ?? 0).toFixed(1),
        })}
      </span>
    </div>
  {/if}

  <!-- Frame / reference slots -->
  <div class="space-y-2">
    <span class="block text-xs text-neutral-400">
      {generation.videoVariant === "fl2va"
        ? locale.t("generation.video.frames")
        : locale.t("generation.video.ref_images")}
      <InfoTip
        text={generation.videoVariant === "fl2va"
          ? locale.t("generation.video.frames_tip")
          : locale.t("generation.video.ref_images_tip")}
      />
    </span>

    {#if generation.videoVariant === "ref2va" && generation.videoRefImageFilenames.length === 0}
      <p class="text-[11px] text-amber-300">{locale.t("generation.video.ref_required")}</p>
    {/if}

    <div class="grid grid-cols-2 gap-2">
      {#each slots as slot (slot.key)}
        {@const preview = previews[slot.key]}
        <div>
          <span class="text-[11px] text-neutral-500 block mb-1">{slot.label}</span>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            role="button"
            tabindex="0"
            class="relative rounded-lg border border-dashed bg-neutral-800/40 min-h-[88px] flex flex-col items-center justify-center p-2 transition-colors {dropSlot ===
            slot.key
              ? 'border-indigo-500 bg-indigo-500/10'
              : 'border-neutral-600 hover:border-neutral-500'}"
            onmouseenter={() => (pasteSlot = slot.key)}
            onmouseleave={() => {
              if (pasteSlot === slot.key) pasteSlot = null;
            }}
            ondragenter={(e) => {
              e.preventDefault();
              dropSlot = slot.key;
            }}
            ondragover={(e) => e.preventDefault()}
            ondragleave={() => {
              if (dropSlot === slot.key) dropSlot = null;
            }}
            ondrop={async (e) => {
              e.preventDefault();
              dropSlot = null;
              const file = e.dataTransfer?.files?.[0];
              if (file) await uploadToSlot(slot.key, file);
            }}
          >
            {#if preview || slot.filename}
              {#if preview}
                <img src={preview} alt="" class="max-h-24 rounded object-contain mb-1.5" />
              {:else}
                <p class="text-[11px] text-neutral-500 mb-1.5 text-center break-all">
                  {slot.filename}
                </p>
              {/if}
              <button
                type="button"
                class="text-[11px] text-red-400 hover:text-red-300"
                onclick={() => clearSlot(slot.key)}
              >
                {locale.t("common.remove")}
              </button>
            {:else}
              <p class="text-[11px] text-neutral-500 text-center">
                {locale.t("generation.image_edit.drop_hint")}
                <label class="text-indigo-400 hover:text-indigo-300 cursor-pointer ml-1">
                  {locale.t("generation.image_edit.upload_prompt")}
                  <input
                    type="file"
                    accept="image/*"
                    class="hidden"
                    onchange={async (e) => {
                      const file = (e.currentTarget as HTMLInputElement).files?.[0];
                      if (file) await uploadToSlot(slot.key, file);
                    }}
                  />
                </label>
              </p>
            {/if}
            {#if uploadingSlot === slot.key}
              <div
                class="absolute inset-0 flex items-center justify-center bg-neutral-900/70 rounded-lg"
              >
                <div
                  class="w-5 h-5 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"
                ></div>
              </div>
            {/if}
          </div>
        </div>
      {/each}
    </div>

    {#if uploadError}
      <p class="text-[11px] text-red-400">{uploadError}</p>
    {/if}

    {#if generation.videoVariant === "fl2va"}
      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          id="video-first-frame-as-last"
          checked={generation.videoFirstFrameAsLast}
          class="w-4 h-4 accent-indigo-500 rounded"
          onchange={toggleFirstFrameAsLast}
        />
        <label for="video-first-frame-as-last" class="text-xs text-neutral-400">
          {locale.t("generation.video.first_frame_as_last")}<InfoTip
            text={locale.t("generation.video.first_frame_as_last_tip")}
          />
        </label>
      </div>
      {#if generation.videoFirstFrameAsLast}
        <p class="text-[11px] {generation.videoFirstFrame ? 'text-neutral-500' : 'text-amber-300'}">
          {generation.videoFirstFrame
            ? locale.t("generation.video.first_frame_as_last_hint")
            : locale.t("generation.video.first_frame_as_last_needs_first")}
        </p>
      {/if}
    {/if}
  </div>

  <!-- Frame interpolation (RIFE) -->
  <div class="space-y-2">
    <div class="flex items-center gap-2">
      <input
        type="checkbox"
        id="video-rife-enabled"
        checked={generation.videoRifeEnabled}
        disabled={rifeInstalling}
        class="w-4 h-4 accent-indigo-500 rounded disabled:opacity-50"
        onchange={toggleRife}
      />
      <label for="video-rife-enabled" class="text-xs text-neutral-400">
        {locale.t("generation.video.rife")}<InfoTip text={locale.t("generation.video.rife_tip")} />
      </label>
    </div>

    <p class="text-[11px] text-neutral-500">
      {generation.videoRifeEnabled
        ? locale.t("generation.video.rife_on_hint", { fps: H3_FPS * 2 })
        : locale.t("generation.video.rife_off_hint", { fps: H3_FPS })}
    </p>

    {#if rifeInstalled === false && !rifeInstalling && !generation.videoRifeEnabled}
      <p class="text-[11px] text-neutral-500">{locale.t("generation.video.rife_install_hint")}</p>
    {/if}

    {#if rifeInstalling}
      <div class="rounded-lg border border-amber-600/60 bg-amber-900/25 px-3 py-2 space-y-1.5">
        <div class="flex items-center gap-2 text-xs text-amber-200">
          <div
            class="w-3.5 h-3.5 border-2 border-amber-300 border-t-transparent rounded-full animate-spin shrink-0"
          ></div>
          <span>
            {#if rifeInstallStep === "restart"}
              {locale.t("generation.video.rife_install_restarting")}
            {:else if rifeInstallStep === "verify"}
              {locale.t("generation.video.rife_install_verifying")}
            {:else if rifeInstallStep === "download"}
              {locale.t("generation.video.rife_install_downloading")}
            {:else}
              {locale.t("generation.video.rife_install_starting")}
            {/if}
          </span>
        </div>
        {#if rifeInstallMessage}
          <p class="text-[10px] font-mono text-amber-300/80 break-all">{rifeInstallMessage}</p>
        {/if}
        <div class="h-1 rounded-full bg-amber-950/60 overflow-hidden">
          <div
            class="h-full bg-amber-400 transition-all duration-300"
            style="width: {rifeInstallPercent}%"
          ></div>
        </div>
      </div>
    {/if}

    {#if rifeInstallError}
      <p class="text-[11px] text-red-400">
        {locale.t("generation.video.rife_install_failed", { error: rifeInstallError })}
      </p>
    {/if}
  </div>

  <!-- Models -->
  <div class="space-y-2 pt-1 border-t border-neutral-800">
    <span class="block text-xs text-neutral-400 pt-2">
      {locale.t("generation.video.models")}
      <InfoTip text={locale.t("generation.video.models_tip")} />
    </span>

    <div>
      <label class="block text-[11px] text-neutral-500 mb-1" for="video-diffusion-model">
        {locale.t("generation.video.diffusion_model")}
      </label>
      <select
        id="video-diffusion-model"
        value={generation.videoDiffusionModel ?? ""}
        onchange={(e) => {
          const value = (e.currentTarget as HTMLSelectElement).value;
          generation.videoDiffusionModel = value || null;
          generation.saveSettings();
        }}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        <option value="">{locale.t("generation.video.select_model")}</option>
        {#each diffusionModels as model (model)}
          <option value={model}>{model}</option>
        {/each}
      </select>
      {#if !generation.videoDiffusionModelLooksLikeH3}
        <p class="text-[11px] text-amber-300 mt-1">{locale.t("generation.video.not_h3_warning")}</p>
      {:else if !generation.videoDiffusionModelMatchesVariant}
        <p class="text-[11px] text-amber-300 mt-1">
          {locale.t("generation.video.variant_mismatch_warning")}
        </p>
      {/if}
    </div>

    <div>
      <label class="block text-[11px] text-neutral-500 mb-1" for="video-clip-model">
        {locale.t("generation.video.clip_model")}
      </label>
      <select
        id="video-clip-model"
        value={generation.videoClipModel ?? ""}
        onchange={(e) => {
          const value = (e.currentTarget as HTMLSelectElement).value;
          generation.videoClipModel = value || null;
          generation.saveSettings();
        }}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        <option value="">{locale.t("generation.video.select_model")}</option>
        {#each clipModels as model (model)}
          <option value={model}>{model}</option>
        {/each}
      </select>
    </div>

    <div class="grid grid-cols-2 gap-2">
      <div>
        <label class="block text-[11px] text-neutral-500 mb-1" for="video-vae-model">
          {locale.t("generation.video.vae_model")}
        </label>
        <select
          id="video-vae-model"
          value={generation.videoVaeModel ?? ""}
          onchange={(e) => {
            const value = (e.currentTarget as HTMLSelectElement).value;
            generation.videoVaeModel = value || null;
            generation.saveSettings();
          }}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
        >
          <option value="">{locale.t("generation.video.select_model")}</option>
          {#each vaeModels as model (model)}
            <option value={model}>{model}</option>
          {/each}
        </select>
      </div>
      <div>
        <label class="block text-[11px] text-neutral-500 mb-1" for="video-audio-vae-model">
          {locale.t("generation.video.audio_vae_model")}
        </label>
        <select
          id="video-audio-vae-model"
          value={generation.videoAudioVaeModel ?? ""}
          onchange={(e) => {
            const value = (e.currentTarget as HTMLSelectElement).value;
            generation.videoAudioVaeModel = value || null;
            generation.saveSettings();
          }}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
        >
          <option value="">{locale.t("generation.video.select_model")}</option>
          {#each vaeModels as model (model)}
            <option value={model}>{model}</option>
          {/each}
        </select>
      </div>
    </div>

    {#if !generation.videoModelsReady}
      <p class="text-[11px] text-amber-300">{locale.t("generation.video.models_missing")}</p>
    {/if}
  </div>
</div>
