<script lang="ts">
  import { onMount } from "svelte";
  import { generation } from "../../stores/generation.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import {
    checkNodeAvailable,
    detectLlmHardware,
    downloadModel,
    getComputeCapability,
    getConfig,
    installH3Teacache,
    installH3Turbo,
    isH3TeacacheInstalled,
    isH3TurboInstalled,
    readClipboardImageSafe,
    uploadImageBytes,
  } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import { rifeInstall } from "../../stores/rifeInstall.svelte.js";
  import { restartComfyuiAndWait } from "../../utils/comfyuiRestart.js";
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
    h3TierFiles,
    h3TierForDiffusionModel,
  } from "../../utils/h3Models.js";
  import type { H3ModelCategory, H3ModelFile, H3TierId } from "../../utils/h3Models.js";
  import { RIFE_MULTIPLIERS, RIFE_SCALE_FACTORS, interpolatedFps } from "../../utils/rife.js";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import type { OutputImage, VideoAspectRatio, VideoVariant } from "../../types/index.js";
  import EditableValue from "../ui/EditableValue.svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import { uploadForVideo, videoReferenceSlotsFree } from "../../utils/galleryActions.js";

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

  /** Same contract for the Turbo pack: `node_name` on `install:progress`. */
  const TURBO_PACKAGE_NAME = "ComfyUI-MiniMax-H3-Turbo";
  const TURBO_NODE_CLASS = "MiniMaxH3TurboSampler";

  /** Same contract for the TeaCache pack: `node_name` on `install:progress`. */
  const TEACACHE_PACKAGE_NAME = "ComfyUI-MiniMaxH3-TeaCache";
  const TEACACHE_NODE_CLASS = "MiniMaxH3TeaCache";


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
  /** Filename of the single file a per-row button is currently fetching. */
  let downloadingFile = $state<string | null>(null);
  let stackError = $state<string | null>(null);

  let dlEntries = $state<Record<string, DlEntry>>({});
  let dlOrder = $state<string[]>([]);

  /** Same lazy-install shape for the Turbo node pack + its ~744 MB LoRA. */
  let turboInstalled = $state<boolean | null>(null);
  let turboInstalling = $state(false);
  let turboInstallStep = $state("");
  let turboInstallMessage = $state("");
  let turboInstallError = $state<string | null>(null);

  /** Same lazy-install shape as Turbo, minus the LoRA half - the pack alone is the install. */
  let teacacheInstalled = $state<boolean | null>(null);
  let teacacheInstalling = $state(false);
  let teacacheInstallStep = $state("");
  let teacacheInstallMessage = $state("");
  let teacacheInstallError = $state<string | null>(null);

  let previews = $state<Record<string, string | null>>({});
  let uploadingSlot = $state<string | null>(null);
  let dropSlot = $state<string | null>(null);
  let pasteSlot = $state<string | null>(null);
  let uploadError = $state<string | null>(null);
  let dropZone = $state<HTMLElement | null>(null);
  /** Frame slot key the single-pick modal is currently targeting, or null. */
  let pickerSlot = $state<string | null>(null);
  let refPickerOpen = $state(false);
  const refSlotsFree = $derived(videoReferenceSlotsFree());

  const dimensions = $derived(generation.videoDimensions);
  let randomSeed = $derived(generation.seed === "-1");

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
  const tierFiles = $derived(h3TierFiles(selectedTier));
  const missingTierFiles = $derived(
    tierFiles.filter((entry) => installedName(entry.file) === null).map((entry) => entry.file),
  );
  const tierDownloadBytes = $derived(
    missingTierFiles.reduce((total, file) => total + file.sizeBytes, 0),
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
  /** No adapter file for TeaCache - the node pack alone gates the toggle. */
  const teacacheReady = $derived(teacacheInstalled === true);

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
  const turboInstallPercent = $derived(
    { clone: 25, done: 40, lora: 55, restart: 80, verify: 95 }[turboInstallStep] ?? 10,
  );
  /** No LoRA-download stage here, so the weights compress to three steps. */
  const teacacheInstallPercent = $derived(
    { clone: 30, restart: 65, verify: 90 }[teacacheInstallStep] ?? 10,
  );

  function dlPercent(entry: DlEntry): number {
    return entry.total > 0 ? Math.round((entry.downloaded / entry.total) * 100) : 0;
  }

  onMount(() => {
    if (models.diffusionModels.length === 0 && !models.loading) void models.refresh();
    void loadHardware();
    rifeInstall.listen();
    void rifeInstall.refresh();
    void loadTurboState();
    void loadTeacacheState();
    const unlistenInstall = ipcListen("install:progress", (event: any) => {
      const data = event.payload as { node_name: string; step: string; message: string };
      if (data.node_name === TURBO_PACKAGE_NAME) {
        turboInstallStep = data.step;
        turboInstallMessage = data.message;
      } else if (data.node_name === TEACACHE_PACKAGE_NAME) {
        teacacheInstallStep = data.step;
        teacacheInstallMessage = data.message;
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

  async function loadTurboState() {
    try {
      turboInstalled = await isH3TurboInstalled();
    } catch {
      turboInstalled = null;
    }
  }

  async function loadTeacacheState() {
    try {
      teacacheInstalled = await isH3TeacacheInstalled();
    } catch {
      teacacheInstalled = null;
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
    if (downloadingStack || missingTierFiles.length === 0) return;
    downloadingStack = true;
    stackError = null;
    try {
      await runDownloads(missingTierFiles);
      applyStack();
    } catch (e) {
      stackError = String(e);
    } finally {
      downloadingStack = false;
    }
  }

  /** Fetch one row's file. Same plumbing as the bulk button, one entry deep. */
  async function downloadOne(file: H3ModelFile) {
    if (downloadingStack || downloadingFile) return;
    downloadingFile = file.filename;
    stackError = null;
    try {
      await runDownloads([file]);
      applyStack();
    } catch (e) {
      stackError = String(e);
    } finally {
      downloadingFile = null;
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
    rifeInstall.error = null;
    if (!next) {
      generation.videoRifeEnabled = false;
      generation.saveSettings();
    } else if (rifeInstall.installed) {
      generation.videoRifeEnabled = true;
      generation.saveSettings();
    } else {
      // Install runs in the background; the checkbox snaps back until it lands.
      void rifeInstall.install().then((ok) => {
        if (!ok) return;
        generation.videoRifeEnabled = true;
        generation.saveSettings();
      });
    }
    input.checked = generation.videoRifeEnabled;
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
        connection.connected = false;
        await restartComfyuiAndWait(
          locale.t("generation.video.rife_install_timeout"),
          locale.t("generation.video.rife_install_failed_start"),
        );

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

  /** Same contract as `toggleTurbo`: the store flag only turns on once usable. */
  function toggleTeacache(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const next = input.checked;
    teacacheInstallError = null;
    if (!next) {
      generation.videoTeacacheEnabled = false;
      generation.saveSettings();
    } else if (teacacheReady) {
      generation.videoTeacacheEnabled = true;
      generation.saveSettings();
    } else {
      void installTeacache();
    }
    input.checked = generation.videoTeacacheEnabled;
  }

  /**
   * Simpler than `installTurbo`: no adapter file to fetch and no model list to
   * refresh afterwards - cloning the pack and restarting ComfyUI is the whole job.
   */
  async function installTeacache() {
    teacacheInstalling = true;
    teacacheInstallError = null;
    try {
      teacacheInstallStep = "clone";
      teacacheInstallMessage = locale.t("generation.video.teacache_install_starting");
      await installH3Teacache();

      teacacheInstallStep = "restart";
      teacacheInstallMessage = locale.t("generation.video.turbo_install_restarting");
      connection.connected = false;
      await restartComfyuiAndWait(
        locale.t("generation.video.rife_install_timeout"),
        locale.t("generation.video.rife_install_failed_start"),
      );

      teacacheInstallStep = "verify";
      teacacheInstallMessage = locale.t("generation.video.teacache_install_verifying");
      const available = await checkNodeAvailable(TEACACHE_NODE_CLASS).catch(() => false);
      if (!available) throw new Error(locale.t("generation.video.teacache_install_not_loaded"));

      teacacheInstalled = true;
      generation.videoTeacacheEnabled = true;
      generation.saveSettings();
    } catch (e) {
      teacacheInstallError = String(e);
      teacacheInstalled = await isH3TeacacheInstalled().catch(() => false);
    } finally {
      teacacheInstalling = false;
      teacacheInstallStep = "";
      teacacheInstallMessage = "";
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

  /**
   * Put a gallery image into a frame slot. Deliberately mirrors `uploadToSlot`
   * so the preview thumbnail, aspect match, spinner and error surface behave
   * identically to a dragged file. `setPreview` takes ownership of the object
   * URL and revokes whatever it replaces.
   *
   * The catch is the one place it diverges: `uploadToSlot` previews the local
   * file optimistically before uploading, so it has to undo that on failure.
   * Here nothing is written until the upload succeeds, so a failed pick must
   * leave whatever the slot already held alone.
   */
  async function assignFromGallery(key: string, image: OutputImage) {
    const slot = findSlot(key);
    if (!slot) return;
    uploadError = null;
    uploadingSlot = key;
    try {
      const { name, aspect, previewUrl } = await uploadForVideo(image, "video_frame.png");
      setPreview(key, previewUrl);
      slot.assign(name);
      slot.assignAspect?.(aspect);
      if (slot.assignAspect && aspect) generation.videoAspectRatio = "auto";
      generation.saveSettings();
    } catch (e) {
      console.error("Failed to assign gallery image to video frame:", e);
      uploadError = String(e);
    } finally {
      uploadingSlot = null;
    }
  }

  /** Fill the free reference slots in order from a multi-pick. */
  async function assignRefsFromGallery(images: OutputImage[]) {
    uploadError = null;
    for (const image of images) {
      const index = generation.videoRefImages.findIndex((slot) => !slot);
      if (index === -1) break;
      const key = `ref${index}`;
      uploadingSlot = key;
      try {
        const { name, previewUrl } = await uploadForVideo(image, "video_reference.png");
        setPreview(key, previewUrl);
        generation.videoRefImages = generation.videoRefImages.map((current, i) =>
          i === index ? name : current,
        );
      } catch (e) {
        console.error("Failed to assign gallery image to video reference:", e);
        uploadError = String(e);
        break;
      } finally {
        uploadingSlot = null;
      }
    }
    generation.saveSettings();
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

  <!-- Seed. Video shares `generation.seed` with image generation, but had no
       control here to see or reset it, so a fixed seed picked up elsewhere
       (typed in, "Use Last", a PNG metadata import) silently stuck to every
       video generated afterwards with no way to tell why (#minimal repro: any
       non "-1" value in the store never surfaces in this panel). -->
  <div>
    <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
      <span>{locale.t('generation.sampler.seed')}<InfoTip text={locale.t('generation.sampler.seed_tip')} /></span>
      <div class="flex items-center gap-1">
        {#if progress.lastCompletedSeed != null}
          <button
            class="text-[10px] px-1.5 py-0.5 rounded bg-neutral-700 text-neutral-300 hover:bg-neutral-600 transition-colors"
            onclick={() => {
              generation.seed = progress.lastCompletedSeed!;
              generation.saveSettings();
            }}
            title={locale.t('generation.sampler.seed_use_last_tip')}
          >
            {locale.t('generation.sampler.seed_use_last')}
          </button>
        {/if}
        <button
          class="text-[10px] px-1.5 py-0.5 rounded {randomSeed
            ? 'bg-indigo-600 text-white'
            : 'bg-neutral-700 text-neutral-300'} transition-colors"
          onclick={() => {
            generation.seed = randomSeed ? (progress.lastCompletedSeed ?? "0") : "-1";
            generation.saveSettings();
          }}
        >
          {locale.t('generation.sampler.seed_random')}
        </button>
      </div>
    </label>
    <input
      type="text"
      inputmode="numeric"
      value={randomSeed ? '' : generation.seed}
      placeholder={locale.t('generation.sampler.random_display')}
      oninput={(e) => {
        const digits = e.currentTarget.value.replace(/\D/g, '');
        if (e.currentTarget.value !== digits) e.currentTarget.value = digits;
        generation.seed = digits === '' ? "-1" : digits;
        generation.saveSettings();
      }}
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
    />
  </div>

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

    {#if generation.videoVariant === "ref2va" && refSlotsFree > 0}
      <button
        type="button"
        class="w-full px-3 py-1.5 rounded-lg border border-neutral-700 text-[11px] text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
        onclick={() => (refPickerOpen = true)}
      >
        {locale.t("generation.video.choose_from_gallery")}
      </button>
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
                <button
                  type="button"
                  class="text-indigo-400 hover:text-indigo-300 ml-1"
                  onclick={(e) => {
                    e.stopPropagation();
                    pickerSlot = slot.key;
                  }}
                >
                  {locale.t("generation.video.choose_from_gallery")}
                </button>
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
        disabled={rifeInstall.installing}
        class="w-4 h-4 accent-indigo-500 rounded disabled:opacity-50"
        onchange={toggleRife}
      />
      <label for="video-rife-enabled" class="text-xs text-neutral-400">
        {locale.t("generation.video.rife")}<InfoTip text={locale.t("generation.video.rife_tip")} />
      </label>
    </div>

    {#if generation.videoRifeEnabled}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.video.rife_on_hint", {
          fps: String(interpolatedFps(24, generation.videoRifeMultiplier)),
        })}
      </p>

      <div class="mt-2 flex items-center gap-2">
        <span class="text-xs text-neutral-400" title={locale.t("generation.video.rife_multiplier_tip")}>
          {locale.t("generation.video.rife_multiplier")}
        </span>
        <div class="flex rounded-lg overflow-hidden border border-neutral-700">
          {#each RIFE_MULTIPLIERS as factor (factor)}
            <button
              type="button"
              class="px-2 py-1 text-xs"
              class:bg-neutral-700={generation.videoRifeMultiplier === factor}
              class:text-neutral-100={generation.videoRifeMultiplier === factor}
              class:text-neutral-400={generation.videoRifeMultiplier !== factor}
              onclick={() => {
                generation.videoRifeMultiplier = factor;
                generation.saveSettings();
              }}
            >
              {factor}x
            </button>
          {/each}
        </div>
      </div>

      <details class="mt-2">
        <summary class="text-xs text-neutral-400 cursor-pointer select-none">
          {locale.t("generation.video.rife_advanced")}
        </summary>
        <div class="mt-2 flex flex-col gap-2 pl-1">
          <label class="flex items-center gap-2 text-xs text-neutral-400">
            <span title={locale.t("generation.video.rife_scale_tip")}>
              {locale.t("generation.video.rife_scale")}
            </span>
            <select
              class="bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1 text-xs text-neutral-100"
              value={generation.videoRifeScaleFactor}
              onchange={(e) => {
                generation.videoRifeScaleFactor = Number(e.currentTarget.value);
                generation.saveSettings();
              }}
            >
              {#each RIFE_SCALE_FACTORS as scale (scale)}
                <option value={scale}>{scale}</option>
              {/each}
            </select>
          </label>

          <label class="flex items-center gap-2 text-xs text-neutral-400">
            <input
              type="checkbox"
              class="accent-[var(--theme-accent-500)]"
              checked={generation.videoRifeFastMode}
              onchange={(e) => {
                generation.videoRifeFastMode = e.currentTarget.checked;
                generation.saveSettings();
              }}
            />
            <span title={locale.t("generation.video.rife_fast_mode_tip")}>
              {locale.t("generation.video.rife_fast_mode")}
            </span>
          </label>

          <label class="flex items-center gap-2 text-xs text-neutral-400">
            <input
              type="checkbox"
              class="accent-[var(--theme-accent-500)]"
              checked={generation.videoRifeEnsemble}
              onchange={(e) => {
                generation.videoRifeEnsemble = e.currentTarget.checked;
                generation.saveSettings();
              }}
            />
            <span title={locale.t("generation.video.rife_ensemble_tip")}>
              {locale.t("generation.video.rife_ensemble")}
            </span>
          </label>
        </div>
      </details>
    {:else}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.video.rife_off_hint", { fps: H3_FPS })}
      </p>
    {/if}

    {#if rifeInstall.installed === false && !rifeInstall.installing && !generation.videoRifeEnabled}
      <p class="text-[11px] text-neutral-500">{locale.t("generation.video.rife_install_hint")}</p>
    {/if}

    {#if rifeInstall.installing}
      <div class="rounded-lg border border-amber-600/60 bg-amber-900/25 px-3 py-2 space-y-1.5">
        <div class="flex items-center gap-2 text-xs text-amber-200">
          <div
            class="w-3.5 h-3.5 border-2 border-amber-300 border-t-transparent rounded-full animate-spin shrink-0"
          ></div>
          <span>
            {#if rifeInstall.step === "restart"}
              {locale.t("generation.video.rife_install_restarting")}
            {:else if rifeInstall.step === "verify"}
              {locale.t("generation.video.rife_install_verifying")}
            {:else if rifeInstall.step === "download"}
              {locale.t("generation.video.rife_install_downloading")}
            {:else}
              {locale.t("generation.video.rife_install_starting")}
            {/if}
          </span>
        </div>
        {#if rifeInstall.message}
          <p class="text-[10px] font-mono text-amber-300/80 break-all">{rifeInstall.message}</p>
        {/if}
        <div class="h-1 rounded-full bg-amber-950/60 overflow-hidden">
          <div
            class="h-full bg-amber-400 transition-all duration-300"
            style="width: {rifeInstall.percent}%"
          ></div>
        </div>
      </div>
    {/if}

    {#if rifeInstall.error}
      <p class="text-[11px] text-red-400">
        {locale.t("generation.video.rife_install_failed", { error: rifeInstall.error })}
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

  <!-- TeaCache -->
  <div class="space-y-2">
    <div class="flex items-center gap-2">
      <input
        type="checkbox"
        id="video-teacache-enabled"
        checked={generation.videoTeacacheEnabled}
        disabled={teacacheInstalling}
        class="w-4 h-4 accent-indigo-500 rounded disabled:opacity-50"
        onchange={toggleTeacache}
      />
      <label for="video-teacache-enabled" class="text-xs text-neutral-400">
        {locale.t("generation.video.teacache")}<InfoTip
          text={locale.t("generation.video.teacache_tip")}
        />
      </label>
    </div>

    {#if !teacacheReady && !teacacheInstalling && !generation.videoTeacacheEnabled}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.video.teacache_install_hint")}
      </p>
    {/if}

    {#if teacacheInstalling}
      <div class="rounded-lg border border-amber-600/60 bg-amber-900/25 px-3 py-2 space-y-1.5">
        <div class="flex items-center gap-2 text-xs text-amber-200">
          <div
            class="w-3.5 h-3.5 border-2 border-amber-300 border-t-transparent rounded-full animate-spin shrink-0"
          ></div>
          <span>
            {#if teacacheInstallStep === "restart"}
              {locale.t("generation.video.turbo_install_restarting")}
            {:else if teacacheInstallStep === "verify"}
              {locale.t("generation.video.teacache_install_verifying")}
            {:else}
              {locale.t("generation.video.teacache_install_starting")}
            {/if}
          </span>
        </div>
        {#if teacacheInstallMessage}
          <p class="text-[10px] font-mono text-amber-300/80 break-all">{teacacheInstallMessage}</p>
        {/if}
        <div class="h-1 rounded-full bg-amber-950/60 overflow-hidden">
          <div
            class="h-full bg-amber-400 transition-all duration-300"
            style="width: {teacacheInstallPercent}%"
          ></div>
        </div>
      </div>
    {/if}

    {#if teacacheInstallError}
      <p class="text-[11px] text-red-400">
        {locale.t("generation.video.teacache_install_failed", { error: teacacheInstallError })}
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

    <div class="space-y-1">
      {#each tierFiles as entry (entry.file.filename)}
        {@const installed = installedName(entry.file) !== null}
        <div class="flex items-center gap-2 text-[11px]">
          <div class="min-w-0 flex-1">
            <p class="text-neutral-300 truncate">
              {locale.t(entry.labelKey)}
              {#if entry.role === generation.videoVariant}
                <span class="text-indigo-400">({locale.t("generation.video.role_in_use")})</span>
              {/if}
            </p>
            <p class="text-neutral-500 truncate" title={entry.file.filename}>
              {entry.file.filename}
            </p>
          </div>
          <span class="shrink-0 font-mono text-neutral-500">
            {locale.formatBytes(entry.file.sizeBytes)}
          </span>
          {#if installed}
            <svg
              class="w-3.5 h-3.5 shrink-0 text-emerald-400"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
              aria-label={locale.t("generation.video.stack_file_installed")}
              role="img"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="3"
                d="M5 13l4 4L19 7"
              />
            </svg>
          {:else}
            <button
              type="button"
              onclick={() => downloadOne(entry.file)}
              disabled={downloadingStack || downloadingFile !== null || turboInstalling}
              class="shrink-0 px-2 py-1 rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 disabled:opacity-50 disabled:hover:border-neutral-700 disabled:hover:text-neutral-300 transition-colors"
            >
              {locale.t("generation.video.stack_download_one")}
            </button>
          {/if}
        </div>
      {/each}
    </div>

    {#if missingTierFiles.length === 0}
      <p class="text-[11px] text-neutral-500">{locale.t("generation.video.stack_ready")}</p>
    {:else}
      <p class="text-[11px] text-amber-300">
        {locale.t("generation.video.stack_missing", {
          count: missingTierFiles.length,
          total: tierFiles.length,
          size: locale.formatBytes(tierDownloadBytes),
        })}
      </p>
      <button
        type="button"
        onclick={downloadStack}
        disabled={downloadingStack || downloadingFile !== null || turboInstalling}
        class="w-full px-3 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:hover:bg-indigo-600 text-sm text-white transition-colors"
      >
        {downloadingStack
          ? locale.t("generation.video.stack_downloading")
          : locale.t("generation.video.stack_download", {
              count: missingTierFiles.length,
              size: locale.formatBytes(tierDownloadBytes),
            })}
      </button>
    {/if}

    {#if (downloadingStack || downloadingFile !== null) && dlOrder.length > 0}
      {@render downloadRows()}
    {/if}

    {#if stackError}
      <p class="text-[11px] text-red-400">
        {locale.t("generation.video.stack_download_failed", { error: stackError })}
      </p>
    {/if}
  </div>
</div>

<GalleryPickerModal
  open={pickerSlot !== null}
  title={locale.t("generation.video.pick_for_slot", {
    slot: findSlot(pickerSlot ?? "")?.label ?? "",
  })}
  onselect={(images) => {
    const key = pickerSlot;
    const image = images[0];
    if (key && image) void assignFromGallery(key, image);
  }}
  onclose={() => (pickerSlot = null)}
/>

<GalleryPickerModal
  open={refPickerOpen}
  multiple
  max={refSlotsFree}
  title={locale.t("generation.video.pick_refs_title")}
  onselect={(images) => void assignRefsFromGallery(images)}
  onclose={() => (refPickerOpen = false)}
/>

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
