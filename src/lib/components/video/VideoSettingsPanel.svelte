<script lang="ts">
  import { onMount } from "svelte";
  import { generation } from "../../stores/generation.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import {
    checkNodeAvailable,
    detectLlmHardware,
    downloadModel,
    getComputeCapability,
    getConfig,
    installH3Turbo,
    installRife,
    isH3TurboInstalled,
    isRifeInstalled,
    readClipboardImageSafe,
    startComfyui,
    stopComfyui,
    uploadImageBytes,
  } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import {
    H3_ASPECT_RATIOS,
    H3_DEFAULT_STEPS,
    H3_FPS,
    H3_MAX_DURATION_SECONDS,
    H3_BLACKWELL_COMPUTE_CAPABILITY,
    H3_MAX_MEGAPIXELS,
    H3_MAX_REF_IMAGES,
    H3_MEGAPIXEL_STEP,
    H3_MIN_DURATION_SECONDS,
    H3_MIN_MEGAPIXELS,
    H3_TURBO_MAX_STEPS,
    H3_TURBO_MIN_STEPS,
    assessH3Vram,
    clampH3Megapixels,
    estimateH3ModelGb,
    estimateH3VramGb,
    h3UsableVramGb,
    isH3HighVramHarmful,
    isH3Nvfp4Emulated,
    suggestH3Megapixels,
  } from "../../utils/videoParams.js";
  import {
    H3_DEFAULT_TIER,
    H3_TIERS,
    H3_TURBO_LORA,
    h3Stack,
    h3StackFiles,
    h3TierForDiffusionModel,
  } from "../../utils/h3Models.js";
  import type { H3ModelCategory, H3ModelFile, H3TierId } from "../../utils/h3Models.js";
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

  /** Same contract for the Turbo pack: `node_name` on `install:progress`. */
  const TURBO_PACKAGE_NAME = "ComfyUI-MiniMax-H3-Turbo";
  const TURBO_NODE_CLASS = "MiniMaxH3TurboSampler";


  /** One row in the download progress list, mirroring `ModelSelector`. */
  interface DlEntry {
    filename: string;
    label: string;
    downloaded: number;
    total: number;
    done: boolean;
  }

  /**
   * Read straight off the shared store rather than fetching once on mount: the
   * lazy RIFE install restarts ComfyUI, and a local one-shot copy would still be
   * holding the pre-restart lists (or an empty one) afterwards. `App.svelte`
   * refreshes the store on every connection / server-ready event.
   */
  let detectedVramGb = $state<number | null>(null);
  /** `config.vram_mode`; "high" maps to ComfyUI's `--highvram`. `null` until read. */
  let vramMode = $state<string | null>(null);
  /** `null` when the GPU probe fails or reports no CUDA device. */
  let computeCapability = $state<number | null>(null);
  /** Gate for the tier-resolving effect: probing decides the default tier. */
  let hardwareProbed = $state(false);

  /** The quality tier driving all four model files. */
  let selectedTier = $state<H3TierId>(H3_DEFAULT_TIER);
  /** One-shot latch so the auto-resolve only overrides the user's pick once. */
  let tierResolved = $state(false);
  let downloadingStack = $state(false);
  let stackError = $state<string | null>(null);

  let dlEntries = $state<Record<string, DlEntry>>({});
  let dlOrder = $state<string[]>([]);

  /**
   * `null` until the disk check answers. The frame-interpolation pack is a
   * lazy install: it only matters in video mode and drags a ~20 MB checkpoint
   * behind it, so nothing is fetched until the user asks for interpolation.
   */
  let rifeInstalled = $state<boolean | null>(null);
  let rifeInstalling = $state(false);
  let rifeInstallStep = $state("");
  let rifeInstallMessage = $state("");
  let rifeInstallError = $state<string | null>(null);

  /** Same lazy-install shape for the Turbo node pack + its ~744 MB LoRA. */
  let turboInstalled = $state<boolean | null>(null);
  let turboInstalling = $state(false);
  let turboInstallStep = $state("");
  let turboInstallMessage = $state("");
  let turboInstallError = $state<string | null>(null);

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

  /** The store list ComfyUI scans for a given H3 category. */
  function listFor(category: H3ModelCategory): string[] {
    if (category === "diffusion_models") return models.diffusionModels;
    if (category === "text_encoders") return models.textEncoders;
    if (category === "loras") return models.loras;
    return models.vaes;
  }

  /**
   * The name ComfyUI knows a stack file by, or `null` when it is not on disk.
   * Entries can carry a subdirectory prefix (`h3/minimax_....safetensors`), and
   * the workflow needs that exact string, so match on the basename and return
   * whatever the scan reported.
   */
  function installedName(file: H3ModelFile): string | null {
    const wanted = file.filename.toLowerCase();
    return (
      listFor(file.category).find((entry) => {
        const base = entry.replace(/\\/g, "/").split("/").pop() ?? entry;
        return base.toLowerCase() === wanted;
      }) ?? null
    );
  }

  const stack = $derived(h3Stack(selectedTier, generation.videoVariant));
  const stackFiles = $derived(stack ? h3StackFiles(stack) : []);
  const missingStackFiles = $derived(stackFiles.filter((file) => installedName(file) === null));
  const stackDownloadBytes = $derived(
    missingStackFiles.reduce((total, file) => total + file.sizeBytes, 0),
  );
  /** NVFP4 runs anywhere but is only worth picking on Blackwell. */
  const tierUnderpowered = $derived(
    selectedTier === "nvfp4" &&
      computeCapability !== null &&
      computeCapability < H3_BLACKWELL_COMPUTE_CAPABILITY,
  );

  const turboLoraName = $derived(installedName(H3_TURBO_LORA));
  /** Both halves have to be present before the workflow can reference them. */
  const turboReady = $derived(turboInstalled === true && turboLoraName !== null);

  const modelGb = $derived(estimateH3ModelGb(generation.videoDiffusionModel));
  /**
   * An NVFP4 DiT on a pre-Blackwell card pays a transient-memory penalty the
   * file size does not show, so the same settings need a bigger card there.
   */
  const nvfp4Emulated = $derived(
    isH3Nvfp4Emulated(generation.videoDiffusionModel, computeCapability),
  );
  const vramOptions = $derived({ nvfp4Emulated });
  /**
   * Compared against usable VRAM, not the sticker capacity: the desktop holds a
   * slice of every card, and on a small one that slice decides whether a pass
   * fits. `null` while the hardware probe has not answered.
   */
  const usableVramGb = $derived(h3UsableVramGb(detectedVramGb));
  const requiredVramGb = $derived(
    estimateH3VramGb(
      dimensions.width,
      dimensions.height,
      generation.videoFrameLength,
      modelGb,
      vramOptions,
    ),
  );
  const vramVerdict = $derived(assessH3Vram(requiredVramGb, usableVramGb));
  /**
   * The pixel budget that would fit comfortably, offered only when it is not
   * the one already selected - repeating the current value back reads as a
   * broken suggestion rather than a reassurance.
   */
  const suggestedMegapixels = $derived(
    suggestH3Megapixels(
      generation.resolvedVideoAspectRatio,
      generation.videoFrameLength,
      modelGb,
      usableVramGb,
      vramOptions,
    ),
  );
  const showSuggestion = $derived(
    suggestedMegapixels !== null && suggestedMegapixels !== generation.videoMegapixels,
  );
  /**
   * Written out rather than toggled per class so the /20 tints survive. Both
   * tiers are advisory, not alarms: the estimate is a rough one, generation is
   * never blocked, and a red banner over a setup that would have worked fine
   * costs more trust than it saves.
   */
  const vramBannerClass = $derived(
    vramVerdict === "over"
      ? "border-amber-600/50 bg-amber-900/20 text-amber-200"
      : "border-sky-700/50 bg-sky-900/20 text-sky-200",
  );

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
  const turboInstallPercent = $derived(
    { clone: 25, done: 40, lora: 55, restart: 80, verify: 95 }[turboInstallStep] ?? 10,
  );

  function dlPercent(entry: DlEntry): number {
    return entry.total > 0 ? Math.round((entry.downloaded / entry.total) * 100) : 0;
  }

  onMount(() => {
    if (models.diffusionModels.length === 0 && !models.loading) void models.refresh();
    void loadHardware();
    void loadRifeState();
    void loadTurboState();
    const unlistenInstall = ipcListen("install:progress", (event: any) => {
      const data = event.payload as { node_name: string; step: string; message: string };
      if (data.node_name === RIFE_PACKAGE_NAME) {
        rifeInstallStep = data.step;
        rifeInstallMessage = data.message;
      } else if (data.node_name === TURBO_PACKAGE_NAME) {
        turboInstallStep = data.step;
        turboInstallMessage = data.message;
      }
    });
    // Other download sources (setup wizard, ModelSelector) share this event, so
    // only rows this panel seeded are ever touched.
    const unlistenDownload = ipcListen("download:progress", (event: any) => {
      const data = event.payload as {
        filename: string;
        downloaded: number;
        total: number;
        done: boolean;
      };
      const existing = dlEntries[data.filename];
      if (!existing) return;
      dlEntries = {
        ...dlEntries,
        [data.filename]: {
          ...existing,
          downloaded: data.downloaded,
          total: data.total || existing.total,
          done: data.done,
        },
      };
    });
    return () => {
      void unlistenInstall.then((fn) => fn());
      void unlistenDownload.then((fn) => fn());
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
    try {
      computeCapability = await getComputeCapability();
    } catch {
      computeCapability = null;
    }
    // Releases the tier-resolving effect, whether or not the probes answered.
    hardwareProbed = true;
  }

  async function loadRifeState() {
    try {
      rifeInstalled = await isRifeInstalled();
    } catch {
      rifeInstalled = null;
    }
  }

  async function loadTurboState() {
    try {
      turboInstalled = await isH3TurboInstalled();
    } catch {
      turboInstalled = null;
    }
  }

  /**
   * Point the store's four model fields at the selected tier. Missing files are
   * assigned `null` on purpose: that is what makes `videoModelsReady` false and
   * surfaces the download button, instead of leaving a stale name from the
   * previous tier pointing at a file the workflow would then fail to load.
   */
  function applyStack() {
    if (!stack) return;
    const next = {
      videoDiffusionModel: installedName(stack.diffusion),
      videoClipModel: installedName(stack.textEncoder),
      videoVaeModel: installedName(stack.videoVae),
      videoAudioVaeModel: installedName(stack.audioVae),
    };
    let changed = false;
    for (const [key, value] of Object.entries(next) as [
      keyof typeof next,
      string | null,
    ][]) {
      if (generation[key] !== value) {
        generation[key] = value;
        changed = true;
      }
    }
    if (changed) generation.saveSettings();
  }

  /**
   * Resolve the tier once per session, then keep the store's model fields in
   * step with what is on disk. Settles after one extra pass because the writes
   * `applyStack()` makes match what it just read.
   */
  $effect(() => {
    // Tracked so a `models.refresh()` (post-download, post-restart) re-runs this.
    void models.diffusionModels;
    void models.textEncoders;
    void models.vaes;
    if (models.loading || !hardwareProbed) return;

    if (!tierResolved) {
      selectedTier = resolveTier();
      tierResolved = true;
    }
    applyStack();
  });

  /**
   * Best tier for this machine: whatever the store already points at, else
   * whatever is installed, else NVFP4 on Blackwell and the int8 default below.
   */
  function resolveTier(): H3TierId {
    const fromStore = h3TierForDiffusionModel(generation.videoDiffusionModel);
    if (fromStore) return fromStore;
    for (const tier of H3_TIERS) {
      const files = [tier.diffusion.fl2va, tier.diffusion.ref2va];
      if (files.some((file) => installedName(file) !== null)) return tier.id;
    }
    if (computeCapability !== null && computeCapability >= H3_BLACKWELL_COMPUTE_CAPABILITY) {
      return "nvfp4";
    }
    return H3_DEFAULT_TIER;
  }

  function selectTier(event: Event) {
    selectedTier = (event.currentTarget as HTMLSelectElement).value as H3TierId;
    tierResolved = true;
    stackError = null;
    applyStack();
  }

  /**
   * Fetch a set of model files in parallel with per-file progress, the same way
   * `ModelSelector.selectRecommended()` does. Sizes come from the registry so
   * the bars are meaningful before the first `download:progress` lands.
   */
  async function runDownloads(files: H3ModelFile[]) {
    const seeded: Record<string, DlEntry> = {};
    for (const file of files) {
      seeded[file.filename] = {
        filename: file.filename,
        label: file.filename,
        downloaded: 0,
        total: file.sizeBytes,
        done: false,
      };
    }
    dlEntries = seeded;
    dlOrder = files.map((file) => file.filename);
    try {
      await Promise.all(
        files.map((file) => downloadModel(file.url, file.category, file.filename)),
      );
    } finally {
      await models.refresh();
      dlEntries = {};
      dlOrder = [];
    }
  }

  async function downloadStack() {
    if (downloadingStack || missingStackFiles.length === 0) return;
    downloadingStack = true;
    stackError = null;
    try {
      await runDownloads(missingStackFiles);
      applyStack();
    } catch (e) {
      stackError = String(e);
    } finally {
      downloadingStack = false;
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
   * Bounce ComfyUI and wait for it to come back. A freshly cloned pack is not
   * importable until the server re-scans `custom_nodes`, so every lazy node
   * install ends here.
   */
  async function restartComfyuiAndWait() {
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
        reject(
          new Error(event.payload?.error || locale.t("generation.video.rife_install_failed_start")),
        );
      });
    });
  }

  /** Clone + checkpoint download, then a restart to load the new node classes. */
  async function installRifeNodes() {
    rifeInstalling = true;
    rifeInstallError = null;
    rifeInstallStep = "clone";
    rifeInstallMessage = locale.t("generation.video.rife_install_starting");
    try {
      await installRife();

      rifeInstallStep = "restart";
      rifeInstallMessage = locale.t("generation.video.rife_install_restarting");
      await restartComfyuiAndWait();

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

  /** Same contract as `toggleRife`: the store flag only turns on once usable. */
  function toggleTurbo(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const next = input.checked;
    turboInstallError = null;
    if (!next) {
      generation.videoTurboEnabled = false;
      generation.saveSettings();
    } else if (turboReady) {
      generation.videoTurboEnabled = true;
      generation.saveSettings();
    } else {
      void installTurbo();
    }
    input.checked = generation.videoTurboEnabled;
  }

  /**
   * The node pack and the LoRA install independently. Only the pack needs a
   * ComfyUI restart (new Python classes); a LoRA file is picked up by a plain
   * `models.refresh()`, so an already-cloned pack skips the restart entirely.
   */
  async function installTurbo() {
    turboInstalling = true;
    turboInstallError = null;
    const needsPack = turboInstalled !== true;
    try {
      if (needsPack) {
        turboInstallStep = "clone";
        turboInstallMessage = locale.t("generation.video.turbo_install_starting");
        await installH3Turbo();
      }

      if (installedName(H3_TURBO_LORA) === null) {
        turboInstallStep = "lora";
        turboInstallMessage = locale.t("generation.video.turbo_install_downloading");
        await runDownloads([H3_TURBO_LORA]);
      }

      if (needsPack) {
        turboInstallStep = "restart";
        turboInstallMessage = locale.t("generation.video.rife_install_restarting");
        await restartComfyuiAndWait();

        turboInstallStep = "verify";
        turboInstallMessage = locale.t("generation.video.turbo_install_verifying");
        const available = await checkNodeAvailable(TURBO_NODE_CLASS).catch(() => false);
        if (!available) throw new Error(locale.t("generation.video.turbo_install_not_loaded"));
      }

      turboInstalled = true;
      generation.videoTurboEnabled = true;
      generation.saveSettings();
    } catch (e) {
      turboInstallError = String(e);
      turboInstalled = await isH3TurboInstalled().catch(() => false);
    } finally {
      await models.refresh();
      turboInstalling = false;
      turboInstallStep = "";
      turboInstallMessage = "";
    }
  }

  function setTurboSteps(value: number) {
    const clamped = Math.min(H3_TURBO_MAX_STEPS, Math.max(H3_TURBO_MIN_STEPS, Math.round(value)));
    if (clamped === generation.videoTurboSteps) return;
    generation.videoTurboSteps = clamped;
    generation.saveSettings();
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

  <!-- Pixel budget. A slider rather than a preset list: every 0.1 MP step is a
       different resolution once snapped to 32, and the budget a given card can
       carry sits wherever the VRAM assessment below puts it. -->
  <div use:scrollCapture>
    <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
      <span>
        {locale.t("generation.video.megapixels")}
        <InfoTip text={locale.t("generation.video.megapixels_tip")} />
      </span>
      <EditableValue
        value={generation.videoMegapixels}
        min={H3_MIN_MEGAPIXELS}
        max={H3_MAX_MEGAPIXELS}
        step={H3_MEGAPIXEL_STEP}
        decimals={1}
        suffix=" MP"
        onchange={(v) => {
          generation.videoMegapixels = clampH3Megapixels(v);
          generation.saveSettings();
        }}
      />
    </label>
    <input
      type="range"
      bind:value={generation.videoMegapixels}
      onchange={() => generation.saveSettings()}
      min={H3_MIN_MEGAPIXELS}
      max={H3_MAX_MEGAPIXELS}
      step={H3_MEGAPIXEL_STEP}
      class="w-full accent-indigo-500"
    />
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

  <!-- VRAM assessment. Two tiers off one estimate: sky when the pass fits with
       little headroom left (slower, because weights start moving over PCIe),
       amber when it likely does not fit. Both are advice, not warnings - the
       estimate is a model, the user's card is the authority, and generation is
       never blocked either way. -->
  {#if vramVerdict === "tight" || vramVerdict === "over"}
    <div class="flex items-start gap-2 rounded-lg border px-3 py-2 text-xs {vramBannerClass}">
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
        <line x1="12" y1="16" x2="12" y2="12" />
        <line x1="12" y1="8" x2="12.01" y2="8" />
      </svg>
      <div class="flex flex-col gap-1">
        <span>
          {locale.t(
            vramVerdict === "over"
              ? "generation.video.vram_warning"
              : "generation.video.vram_warning_tight",
            {
              detected: (detectedVramGb ?? 0).toFixed(1),
              usable: (usableVramGb ?? 0).toFixed(1),
              required: requiredVramGb.toFixed(1),
            },
          )}
        </span>
        {#if nvfp4Emulated}
          <span>{locale.t("generation.video.vram_nvfp4_emulated")}</span>
        {/if}
        {#if showSuggestion}
          <button
            class="self-start underline underline-offset-2 hover:no-underline"
            onclick={() => {
              generation.videoMegapixels = suggestedMegapixels ?? generation.videoMegapixels;
              generation.saveSettings();
            }}
          >
            {locale.t("generation.video.vram_suggest", {
              megapixels: (suggestedMegapixels ?? 0).toFixed(1),
            })}
          </button>
        {/if}
      </div>
    </div>
  {/if}

  <!-- VRAM Mode "high" note - amber rather than red: the slowdown is measured,
       but it is a setting the user can undo in one click, not a failure -->
  {#if highVramWarning}
    <div
      class="flex items-start gap-2 rounded-lg border border-amber-600/50 bg-amber-900/20 px-3 py-2 text-xs text-amber-200"
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
        <line x1="12" y1="16" x2="12" y2="12" />
        <line x1="12" y1="8" x2="12.01" y2="8" />
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

  <!-- Turbo LoRA -->
  <div class="space-y-2">
    <div class="flex items-center gap-2">
      <input
        type="checkbox"
        id="video-turbo-enabled"
        checked={generation.videoTurboEnabled}
        disabled={turboInstalling}
        class="w-4 h-4 accent-indigo-500 rounded disabled:opacity-50"
        onchange={toggleTurbo}
      />
      <label for="video-turbo-enabled" class="text-xs text-neutral-400">
        {locale.t("generation.video.turbo")}<InfoTip
          text={locale.t("generation.video.turbo_tip")}
        />
      </label>
    </div>

    <p class="text-[11px] text-neutral-500">
      {generation.videoTurboEnabled
        ? locale.t("generation.video.turbo_on_hint", { steps: generation.videoTurboSteps })
        : locale.t("generation.video.turbo_off_hint", { steps: H3_DEFAULT_STEPS })}
    </p>

    {#if generation.videoTurboEnabled}
      <div use:scrollCapture>
        <label
          class="flex items-center justify-between text-[11px] text-neutral-500 mb-1"
          for="video-turbo-steps"
        >
          <span>
            {locale.t("generation.video.turbo_steps")}
            <InfoTip text={locale.t("generation.video.turbo_steps_tip")} />
          </span>
          <EditableValue
            value={generation.videoTurboSteps}
            min={H3_TURBO_MIN_STEPS}
            max={H3_TURBO_MAX_STEPS}
            step={1}
            onchange={setTurboSteps}
          />
        </label>
        <input
          type="range"
          id="video-turbo-steps"
          min={H3_TURBO_MIN_STEPS}
          max={H3_TURBO_MAX_STEPS}
          step="1"
          value={generation.videoTurboSteps}
          oninput={(e) => setTurboSteps(Number((e.currentTarget as HTMLInputElement).value))}
          class="w-full accent-indigo-500"
        />
      </div>
    {/if}

    {#if !turboReady && !turboInstalling && !generation.videoTurboEnabled}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.video.turbo_install_hint", {
          size: locale.formatBytes(H3_TURBO_LORA.sizeBytes),
        })}
      </p>
    {/if}

    {#if turboInstalling}
      <div class="rounded-lg border border-amber-600/60 bg-amber-900/25 px-3 py-2 space-y-1.5">
        <div class="flex items-center gap-2 text-xs text-amber-200">
          <div
            class="w-3.5 h-3.5 border-2 border-amber-300 border-t-transparent rounded-full animate-spin shrink-0"
          ></div>
          <span>
            {#if turboInstallStep === "restart"}
              {locale.t("generation.video.turbo_install_restarting")}
            {:else if turboInstallStep === "verify"}
              {locale.t("generation.video.turbo_install_verifying")}
            {:else if turboInstallStep === "lora"}
              {locale.t("generation.video.turbo_install_downloading")}
            {:else}
              {locale.t("generation.video.turbo_install_starting")}
            {/if}
          </span>
        </div>
        {#if turboInstallMessage}
          <p class="text-[10px] font-mono text-amber-300/80 break-all">{turboInstallMessage}</p>
        {/if}
        <div class="h-1 rounded-full bg-amber-950/60 overflow-hidden">
          <div
            class="h-full bg-amber-400 transition-all duration-300"
            style="width: {turboInstallPercent}%"
          ></div>
        </div>
      </div>

      {#if dlOrder.length > 0}
        {@render downloadRows()}
      {/if}
    {/if}

    {#if turboInstallError}
      <p class="text-[11px] text-red-400">
        {locale.t("generation.video.turbo_install_failed", { error: turboInstallError })}
      </p>
    {/if}
  </div>

  <!-- Models -->
  <div class="space-y-2 pt-1 border-t border-neutral-800">
    <span class="block text-xs text-neutral-400 pt-2">
      {locale.t("generation.video.models")}
      <InfoTip text={locale.t("generation.video.models_tip")} />
    </span>

    <select
      id="video-model-stack"
      value={selectedTier}
      onchange={selectTier}
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
    >
      {#each H3_TIERS as tier (tier.id)}
        <option value={tier.id}>{locale.t(tier.labelKey)}</option>
      {/each}
    </select>

    {#if tierUnderpowered}
      <p class="text-[11px] text-amber-300">
        {locale.t("generation.video.stack_requires_blackwell")}
      </p>
    {/if}

    {#if missingStackFiles.length === 0}
      <p class="text-[11px] text-neutral-500">{locale.t("generation.video.stack_ready")}</p>
    {:else}
      <p class="text-[11px] text-amber-300">
        {locale.t("generation.video.stack_missing", {
          count: missingStackFiles.length,
          size: locale.formatBytes(stackDownloadBytes),
        })}
      </p>
      <button
        type="button"
        onclick={downloadStack}
        disabled={downloadingStack || turboInstalling}
        class="w-full px-3 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:hover:bg-indigo-600 text-sm text-white transition-colors"
      >
        {downloadingStack
          ? locale.t("generation.video.stack_downloading")
          : locale.t("generation.video.stack_download", {
              size: locale.formatBytes(stackDownloadBytes),
            })}
      </button>
    {/if}

    {#if downloadingStack && dlOrder.length > 0}
      {@render downloadRows()}
    {/if}

    {#if stackError}
      <p class="text-[11px] text-red-400">
        {locale.t("generation.video.stack_download_failed", { error: stackError })}
      </p>
    {/if}
  </div>
</div>

{#snippet downloadRows()}
  <div class="space-y-1.5">
    {#each dlOrder as filename (filename)}
      {@const entry = dlEntries[filename]}
      {#if entry}
        <div class="space-y-1">
          <div class="flex items-center gap-1.5 text-[10px] text-neutral-400">
            {#if entry.done}
              <svg
                class="w-3 h-3 text-emerald-400 shrink-0"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="3"
                  d="M5 13l4 4L19 7"
                />
              </svg>
            {/if}
            <span class="truncate">{entry.label}</span>
            <span class="ml-auto shrink-0 font-mono">
              {locale.formatBytes(entry.downloaded)} / {locale.formatBytes(entry.total)} ({dlPercent(
                entry,
              )}%)
            </span>
          </div>
          <div class="h-1 rounded-full bg-neutral-800 overflow-hidden">
            {#if entry.total > 0}
              <div
                class="h-full rounded-full transition-[width] duration-300 ease-out {entry.done
                  ? 'bg-emerald-400'
                  : 'bg-indigo-400'}"
                style="width: {dlPercent(entry)}%"
              ></div>
            {:else}
              <div class="bg-indigo-400 h-full rounded-full w-1/3 animate-pulse"></div>
            {/if}
          </div>
        </div>
      {/if}
    {/each}
  </div>
{/snippet}
