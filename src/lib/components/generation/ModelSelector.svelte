<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { autocomplete } from "../../stores/autocomplete.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { downloadModel, findModelByHash, hashModelFile, getComputeCapability } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import { onMount, onDestroy, tick } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import { MODEL_FAMILIES, familyIsSdxlLike } from "../../utils/modelFamily.js";
  import type { ModelFamily } from "../../utils/modelFamily.js";

  interface ModelFile {
    filename: string;
    /** Download URL. Empty string means detection-only — no download attempted. */
    url: string;
    category: string;
    /** AutoV2 hash (first 10 chars of full SHA256, uppercase) — CivitAI-compatible */
    hash?: string;
    clipType?: string;
  }

  interface RecommendedModel {
    label: string;
    /** Total download size (human-readable) shown in the dropdown */
    size: string;
    /** Translation key for computed/semantic size labels such as local-only. */
    sizeKey?: string;
    /** Regular checkpoint model (single file) */
    checkpoint?: ModelFile;
    /** VAE to download alongside the checkpoint */
    vaeModel?: ModelFile;
    /** Split model loading (UNETLoader + CLIPLoader + VAELoader) */
    splitModel?: {
      diffusionModel: ModelFile;
      clipModel: ModelFile & { clipType: string };
      vaeModel: ModelFile;
    };
    /** Auto-apply these settings when selected */
    autoSettings?: {
      steps?: number;
      cfg?: number;
      samplerName?: string;
      scheduler?: string;
      upscaleSteps?: number;
      upscaleDenoise?: number;
      facefixSteps?: number;
    };
    /**
     * Minimum NVIDIA compute capability required (e.g. 8.9 for Ada / Blackwell
     * FP8 native tensor cores). Hides the entry on lower-tier GPUs where the
     * model would either fail to load or run dramatically slower than the
     * non-quantised alternative. `null`/absent = available everywhere.
     */
    minComputeCapability?: number;
    /** Hint shown next to the size in the dropdown to explain the gate. */
    gateHint?: string;
    /**
     * When true, the entry is shown only if all components are detected
     * locally (by hash or filename). Used for models we can't redistribute
     * but want to recognise / auto-pair when the user has them on disk.
     */
    detectionOnly?: boolean;
  }

  const recommendedModels: RecommendedModel[] = [
    {
      label: "Juice",
      size: "~6.9 GB",
      checkpoint: {
        filename: "Juice.safetensors",
        url: "https://huggingface.co/Enferlain/juice/resolve/main/noob/2048/v2/21862-seele_pop3.safetensors",
        category: "checkpoints",
      },
      vaeModel: {
        filename: "sdxl_vae.safetensors",
        url: "https://huggingface.co/stabilityai/sdxl-vae/resolve/main/sdxl_vae.safetensors",
        category: "vae",
      },
      autoSettings: {
        steps: 20,
        cfg: 1.4,
        samplerName: "euler_cfg_pp",
        scheduler: "sgm_uniform",
      },
    },
    {
      label: "Anima Base v1.0",
      size: "~13 GB",
      splitModel: {
        diffusionModel: {
          filename: "anima-base-v1.0.safetensors",
          url: "https://huggingface.co/circlestone-labs/Anima/resolve/main/split_files/diffusion_models/anima-base-v1.0.safetensors",
          category: "diffusion_models",
        },
        clipModel: {
          filename: "qwen_3_06b_base.safetensors",
          url: "https://huggingface.co/circlestone-labs/Anima/resolve/main/split_files/text_encoders/qwen_3_06b_base.safetensors",
          category: "text_encoders",
          clipType: "wan",
        },
        vaeModel: {
          filename: "qwen_image_vae.safetensors",
          url: "https://huggingface.co/circlestone-labs/Anima/resolve/main/split_files/vae/qwen_image_vae.safetensors",
          category: "vae",
        },
      },
      autoSettings: {
        steps: 30,
        cfg: 4,
        samplerName: "er_sde",
        upscaleSteps: 10,
        upscaleDenoise: 0.3,
        facefixSteps: 10,
      },
    },
    {
      label: "Anima Base v1.0 (FP8)",
      size: "",
      sizeKey: "common.local",
      detectionOnly: true,
      minComputeCapability: 8.9,
      gateHint: "Ada / Blackwell only",
      splitModel: {
        diffusionModel: {
          filename: "anima-base-v1.0-fp8.safetensors",
          url: "",
          category: "diffusion_models",
        },
        clipModel: {
          filename: "qwen_3_06b_base.safetensors",
          url: "",
          category: "text_encoders",
          clipType: "wan",
        },
        vaeModel: {
          filename: "qwen_image_vae.safetensors",
          url: "",
          category: "vae",
        },
      },
      autoSettings: {
        steps: 30,
        cfg: 4,
        samplerName: "er_sde",
        upscaleSteps: 10,
        upscaleDenoise: 0.3,
        facefixSteps: 10,
      },
    },
    {
      label: "Anima Preview 3",
      size: "",
      sizeKey: "common.local",
      detectionOnly: true,
      splitModel: {
        diffusionModel: {
          filename: "anima-preview3-base.safetensors",
          url: "",
          category: "diffusion_models",
        },
        clipModel: {
          filename: "qwen_3_06b_base.safetensors",
          url: "",
          category: "text_encoders",
          clipType: "wan",
        },
        vaeModel: {
          filename: "qwen_image_vae.safetensors",
          url: "",
          category: "vae",
        },
      },
      autoSettings: {
        steps: 30,
        cfg: 4,
        samplerName: "er_sde",
        upscaleSteps: 10,
        upscaleDenoise: 0.3,
        facefixSteps: 10,
      },
    },
    {
      label: "Anima Preview 3 (FP8)",
      size: "",
      sizeKey: "common.local",
      detectionOnly: true,
      // FP8 native tensor-core compute lands on Ada Lovelace (8.9) and
      // Blackwell (10.0+ datacenter / 12.0 consumer). Earlier GPUs would
      // fall back to BF16 emulation and lose the speed/VRAM advantage that
      // makes this build worthwhile, so we hide the entry on those tiers.
      minComputeCapability: 8.9,
      gateHint: "Ada / Blackwell only",
      splitModel: {
        diffusionModel: {
          filename: "anima-preview3-base-fp8.safetensors",
          url: "",
          category: "diffusion_models",
        },
        clipModel: {
          filename: "qwen_3_06b_base.safetensors",
          url: "",
          category: "text_encoders",
          clipType: "wan",
        },
        vaeModel: {
          filename: "qwen_image_vae.safetensors",
          url: "",
          category: "vae",
        },
      },
      autoSettings: {
        steps: 30,
        cfg: 4,
        samplerName: "er_sde",
        upscaleSteps: 10,
        upscaleDenoise: 0.3,
        facefixSteps: 10,
      },
    },
    {
      label: "Anima Preview 2",
      size: "",
      sizeKey: "common.local",
      detectionOnly: true,
      splitModel: {
        diffusionModel: {
          filename: "anima-preview2.safetensors",
          url: "",
          category: "diffusion_models",
        },
        clipModel: {
          filename: "qwen_3_06b_base.safetensors",
          url: "",
          category: "text_encoders",
          clipType: "wan",
        },
        vaeModel: {
          filename: "qwen_image_vae.safetensors",
          url: "",
          category: "vae",
        },
      },
      autoSettings: {
        steps: 30,
        cfg: 4,
        samplerName: "er_sde",
        upscaleSteps: 10,
        upscaleDenoise: 0.3,
        facefixSteps: 10,
      },
    },
    {
      // Flux 2 Klein 9B (NVFP4) — detection-only entry. We can't redistribute
      // BFL's weights, but if the user has dropped the three components into
      // their ComfyUI models tree we recognise them and auto-wire the split
      // model + Qwen 3 text encoder + Flux VAE. Resolution by hash via
      // `findModelByHash` handles renamed files, and the unified text-encoder
      // listing in `models.svelte.ts` picks up encoders living under the
      // legacy `clip/` directory (where the user's `qwen_3_8b_fp4mixed`
      // happens to live).
      label: "Flux 2 Klein 9B (NVFP4)",
      size: "",
      sizeKey: "common.local",
      detectionOnly: true,
      splitModel: {
        diffusionModel: {
          filename: "flux-2-klein-9b-nvfp4.safetensors",
          url: "",
          category: "diffusion_models",
        },
        clipModel: {
          filename: "qwen_3_8b_fp4mixed.safetensors",
          url: "",
          // Listed under text_encoders so the picker resolves correctly;
          // backend hash lookup also falls through to `clip/`.
          category: "text_encoders",
          // Flux 2 Klein ships with Qwen 3 as its text encoder.
          clipType: "qwen_image",
        },
        vaeModel: {
          filename: "flux-vae.safetensors",
          url: "",
          category: "vae",
        },
      },
      autoSettings: {
        steps: 20,
        cfg: 1.0,
        samplerName: "euler",
        scheduler: "simple",
      },
    },
  ];

  let showArchitecturePicker = $state(false);
  let showModelInfo = $state(false);

  // Read-only views of the store state that App.svelte's detection effect fills in.
  const modelSpec = $derived(generation.modelSpec);
  const modelSpecUnavailable = $derived(generation.modelSpecUnavailable);

  /** ModelSpec fields the info panel gives a dedicated row to. */
  const RENDERED_MODEL_SPEC_FIELDS = new Set([
    "thumbnail",
    "title",
    "author",
    "date",
    "description",
    "architecture",
    "implementation",
    "sai_model_spec",
    "resolution",
    "prediction_type",
    "trigger_phrase",
    "usage_hint",
    "preprocessor",
    "encoder_layer",
    "merged_from",
    "hash_sha256",
    "tags",
    "license",
  ]);

  /**
   * Fields the file declared that have no dedicated row — rendered verbatim so a
   * non-standard or newly added ModelSpec key is never silently dropped. Driven
   * by `modelspec_keys` so derived backend keys (family, recommended_vae, ...)
   * stay out of the list.
   */
  const extraModelSpecFields = $derived.by(() => {
    const declared = modelSpec?.modelspec_keys;
    if (!declared) return [];
    return declared
      .split(",")
      .map((field) => field.trim())
      .filter((field) => field && !RENDERED_MODEL_SPEC_FIELDS.has(field))
      .map((field) => ({ field, value: modelSpec?.[field] ?? "" }))
      .filter((entry) => entry.value !== "");
  });

  /** A ModelSpec thumbnail is a base64 data URI; ignore anything else. */
  const modelSpecThumbnail = $derived(
    modelSpec?.thumbnail?.startsWith("data:image/") ? modelSpec.thumbnail : null
  );

  /** Strip HTML tags and convert to readable plain text. */
  function stripHtml(html: string): string {
    return html
      .replace(/<br\s*\/?>/gi, "\n")
      .replace(/<\/p>/gi, "\n")
      .replace(/<hr\s*\/?>/gi, "\n---\n")
      .replace(/<a[^>]+href="([^"]*)"[^>]*>[^<]*<\/a>/gi, "$1")
      .replace(/<[^>]+>/g, "")
      .replace(/&amp;/g, "&")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&quot;/g, '"')
      .replace(/&#39;/g, "'")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
  }

  function currentFamilyOverride(): ModelFamily | null {
    const modelKey = generation.currentModelMetadataKey();
    return modelKey ? generation.modelFamilyOverrides[modelKey] ?? null : null;
  }

  function architectureBadgeLabel(): string {
    if (!generation.currentModelMetadataKey()) return "";
    const manualOverride = currentFamilyOverride();
    if (manualOverride) return manualOverride;
    if (generation.isModelMetadataLoading) return locale.t("generation.model.architecture_detecting");
    return generation.modelFamily === "unknown" ? "undefined" : generation.modelFamily;
  }

  function applyCurrentModelFamilyOverride(family: ModelFamily | null): void {
    const modelKey = generation.currentModelMetadataKey();
    if (!modelKey) return;

    showArchitecturePicker = false;
    generation.setModelFamilyOverride(modelKey, family);

    if (family) {
      generation.prepareManualOverride(modelKey);
      generation.applyModelMetadata({
        modelspecPredictionType: null,
        modelspecPredictKey: null,
        modelspecHeaderVPred: false,
        modelFamily: family,
        modelIsSdxlLike: familyIsSdxlLike(family),
        modelTurboVariant: "none",
        modelRecommendedVae: null,
        modelRecommendedClipModel: null,
        modelRecommendedClipType: null,
      });
      generation.applyModelSpecificPreset();
      return;
    }

    // Clearing the override changes generation.modelFamilyOverrides, which the
    // App-level detection effect tracks — it re-runs the backend fetch for us.
    generation.invalidateModelMetadataCache();
  }

  // Krea 2 only works with the Qwen3-VL 4B text encoder (12x2560 = 30720-dim
  // conditioning); any other encoder fails deep inside ComfyUI sampling with a
  // cryptic feature-count error. Marker list mirrors KREA2_TEXT_ENCODER_MARKERS
  // in src-tauri/src/commands/api.rs.
  const KREA2_ENCODER_MARKERS = ["qwen3vl-4b", "qwen3vl_4b", "qwen3-vl-4b", "qwen3_vl_4b", "qwen3vl4b"];
  const KREA2_ENCODER_FILE: ModelFile = {
    filename: "qwen3vl_4b_fp8_scaled.safetensors",
    url: "https://huggingface.co/Comfy-Org/Krea-2/resolve/main/text_encoders/qwen3vl_4b_fp8_scaled.safetensors",
    category: "text_encoders",
  };
  const KREA2_VAE_FILE: ModelFile = {
    filename: "qwen_image_vae.safetensors",
    url: "https://huggingface.co/Comfy-Org/Krea-2/resolve/main/vae/qwen_image_vae.safetensors",
    category: "vae",
  };
  let krea2EnsureRunning = false;

  function isKrea2Encoder(filename: string | null | undefined): boolean {
    if (!filename) return false;
    const lower = filename.toLowerCase();
    return KREA2_ENCODER_MARKERS.some((marker) => lower.includes(marker));
  }

  /**
   * Runs after model metadata is applied for a Krea 2 model. Ensures a
   * Qwen3-VL 4B encoder is selected — preferring one already on disk and
   * auto-downloading the FP8 build from Comfy-Org/Krea-2 otherwise (plus the
   * qwen_image VAE when no qwen VAE is installed).
   */
  async function ensureKrea2Encoder() {
    if (krea2EnsureRunning || !generation.useSplitModel) return;
    if (isKrea2Encoder(generation.clipModel)) {
      if (generation.clipType !== "krea2") {
        generation.clipType = "krea2";
        generation.saveSettings();
      }
      return;
    }

    const existing = models.textEncoders.find((f) => isKrea2Encoder(f));
    if (existing) {
      generation.clipModel = existing;
      generation.clipType = "krea2";
      generation.saveSettings();
      return;
    }

    // Nothing suitable installed — auto-download. Skip if another batch
    // download already owns the progress UI; the generate-time guard still
    // catches the misconfiguration with an actionable message.
    if (downloading !== null) return;

    krea2EnsureRunning = true;
    const files: { file: ModelFile; label: string }[] = [
      { file: KREA2_ENCODER_FILE, label: locale.t('generation.model.downloading_text_encoder') },
    ];
    if (!models.vaes.some((v) => v.toLowerCase().includes("qwen"))) {
      files.push({ file: KREA2_VAE_FILE, label: locale.t('generation.model.downloading_vae') });
    }
    downloading = "Krea 2";
    downloadError = "";
    const seeded: Record<string, DlEntry> = {};
    const order: string[] = [];
    for (const { file, label } of files) {
      seeded[file.filename] = { filename: file.filename, label, downloaded: 0, total: 0, done: false };
      order.push(file.filename);
    }
    dlEntries = seeded;
    dlOrder = order;
    try {
      await Promise.all(
        files.map(async ({ file }) => {
          await downloadModel(file.url, file.category, file.filename);
          await cacheHashAfterDownload(file);
        }),
      );
      await models.refresh();
      generation.clipModel = KREA2_ENCODER_FILE.filename;
      generation.clipType = "krea2";
      if (
        files.some(({ file }) => file === KREA2_VAE_FILE) &&
        !generation.vae.toLowerCase().includes("qwen")
      ) {
        generation.vae = KREA2_VAE_FILE.filename;
      }
      generation.saveSettings();
      downloading = null;
      dlEntries = {};
      dlOrder = [];
    } catch (e) {
      console.error("Failed to download Krea 2 text encoder:", e);
      downloadError = `Download failed: ${e}`;
      setTimeout(() => {
        downloading = null;
        downloadError = "";
        dlEntries = {};
        dlOrder = [];
      }, 4000);
    } finally {
      krea2EnsureRunning = false;
    }
  }

  // Family detection itself lives in an App.svelte $effect (it has to run while
  // this panel is collapsed). The encoder download stays here because it drives
  // this component's progress rows, so it only runs while the panel is open.
  $effect(() => {
    if (generation.modelFamily === "krea2" && generation.useSplitModel) {
      void ensureKrea2Encoder();
    }
  });

  let checkpointSearch = $state("");
  let showCheckpointDropdown = $state(false);
  let showLoraDropdown = $state<number | null>(null);
  let loraSearches = $state<Record<number, string>>({});
  let activeLoraDropdownListEl = $state<HTMLDivElement | null>(null);
  let downloading = $state<string | null>(null);
  let downloadError = $state("");
  let modelSelectorRootEl = $state<HTMLDivElement | null>(null);
  let checkpointDropdownListEl = $state<HTMLDivElement | null>(null);
  let architecturePickerEl = $state<HTMLDivElement | null>(null);

  /**
   * Detected NVIDIA compute capability. `null` until probed; remains `null`
   * on non-NVIDIA systems / when nvidia-smi is unavailable. We surface gated
   * recommended models (e.g. FP8-only builds) only when this is known to
   * meet or exceed the entry's `minComputeCapability` — never optimistically
   * (so AMD/Intel/CPU users don't see entries they can't run).
   */
  let computeCapability = $state<number | null>(null);

  // NVFP4 weights only run natively on Blackwell (compute capability 10.0+).
  // Warn owners of older NVIDIA GPUs that they need the FP8 build instead.
  const showNvfp4Warning = $derived(
    generation.useSplitModel &&
      (generation.diffusionModel ?? "").toLowerCase().includes("nvfp4") &&
      computeCapability !== null &&
      computeCapability < 10.0
  );

  // Shown under the text-encoder picker when a Krea 2 model is paired with an
  // encoder that will fail at generation time (wrong conditioning size).
  const showKrea2EncoderWarning = $derived(
    generation.useSplitModel &&
      generation.modelFamily === "krea2" &&
      !isKrea2Encoder(generation.clipModel)
  );

  // Per-file download progress. Keyed by filename so parallel downloads of
  // different components (diffusion model / text encoder / VAE) each have
  // their own tracked row that stays visible until the whole batch completes.
  interface DlEntry {
    filename: string;
    label: string;
    downloaded: number;
    total: number;
    done: boolean;
  }
  let dlEntries = $state<Record<string, DlEntry>>({});
  // Preserve a stable render order (the order downloads were started).
  let dlOrder = $state<string[]>([]);

  // Hash-based model detection: maps "category::hash" -> resolved filename on disk
  let hashResolved = $state<Record<string, string>>({});


  function dlPercent(e: DlEntry): number {
    return e.total > 0 ? Math.round((e.downloaded / e.total) * 100) : 0;
  }

  /** Load cached model hashes from localStorage */
  function loadCachedHashes(): Record<string, string> {
    try {
      return JSON.parse(localStorage.getItem("modelHashes") || "{}");
    } catch { return {}; }
  }

  /** Save a hash mapping to localStorage */
  function cacheHash(category: string, filename: string, hash: string) {
    const cached = loadCachedHashes();
    cached[`${category}::${hash}`] = filename;
    localStorage.setItem("modelHashes", JSON.stringify(cached));
  }

  /** Resolve recommended models by hash on mount */
  async function resolveModelHashes() {
    const allFiles: ModelFile[] = [];
    for (const rec of recommendedModels) {
      if (rec.checkpoint?.hash) allFiles.push(rec.checkpoint);
      if (rec.vaeModel?.hash) allFiles.push(rec.vaeModel);
      if (rec.splitModel) {
        if (rec.splitModel.diffusionModel.hash) allFiles.push(rec.splitModel.diffusionModel);
        if (rec.splitModel.clipModel.hash) allFiles.push(rec.splitModel.clipModel);
        if (rec.splitModel.vaeModel.hash) allFiles.push(rec.splitModel.vaeModel);
      }
    }

    // Also check locally cached hashes (from previous downloads)
    const cached = loadCachedHashes();
    const resolved: Record<string, string> = {};

    const lookups = allFiles.map(async (f) => {
      if (!f.hash) return;
      const key = `${f.category}::${f.hash}`;

      // First check cached mapping
      if (cached[key]) {
        resolved[key] = cached[key];
        return;
      }

      // Otherwise scan the directory by hash
      try {
        const found = await findModelByHash(f.category, f.hash);
        if (found) {
          resolved[key] = found;
          cacheHash(f.category, found, f.hash);
        }
      } catch (e) {
        console.warn(`Hash lookup failed for ${f.filename}:`, e);
      }
    });

    await Promise.all(lookups);
    hashResolved = resolved;
  }

  /** Check if a model file is installed (by hash first, then filename fallback) */
  function isModelFileInstalled(f: ModelFile, modelList: string[]): boolean {
    if (f.hash) {
      const key = `${f.category}::${f.hash}`;
      if (hashResolved[key]) return true;
    }
    return modelList.includes(f.filename);
  }

  /** Get the actual filename on disk for a model file (may differ from expected if renamed) */
  function resolvedFilename(f: ModelFile): string {
    if (f.hash) {
      const key = `${f.category}::${f.hash}`;
      if (hashResolved[key]) return hashResolved[key];
    }
    return f.filename;
  }

  /** After downloading a model file, compute its hash and cache it */
  async function cacheHashAfterDownload(f: ModelFile) {
    try {
      const result = await hashModelFile(f.category, f.filename);
      // Cache the AutoV2 hash (CivitAI-compatible, first 10 chars of SHA256)
      cacheHash(f.category, f.filename, result.autov2);
      hashResolved = { ...hashResolved, [`${f.category}::${result.autov2}`]: f.filename };
    } catch (e) {
      console.warn(`Failed to hash ${f.filename} after download:`, e);
    }
  }

  let unlistenDownload: (() => void) | null = null;

  function handleDocumentPointerDown(event: PointerEvent) {
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (showArchitecturePicker && architecturePickerEl && !architecturePickerEl.contains(target)) {
      showArchitecturePicker = false;
    }
    if (modelSelectorRootEl?.contains(target)) return;
    showCheckpointDropdown = false;
    showLoraDropdown = null;
  }

  function handleDocumentKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    showArchitecturePicker = false;
    showCheckpointDropdown = false;
    showLoraDropdown = null;
  }

  onMount(async () => {
    document.addEventListener("pointerdown", handleDocumentPointerDown);
    document.addEventListener("keydown", handleDocumentKeydown);

    unlistenDownload = await ipcListen("download:progress", (event: any) => {
      const data = event.payload as {
        filename: string;
        downloaded: number;
        total: number;
        done: boolean;
      };
      // Only update entries we initiated. Ignore bleed-through from other
      // download:progress emitters (setup wizard, ControlNet, etc.).
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

    // Resolve model hashes in background
    resolveModelHashes();

    // Probe NVIDIA compute capability so we can gate FP8-only recommended
    // entries. Cheap (single nvidia-smi shell-out) and fire-and-forget — a
    // failure just leaves the gate closed, which is the safe default.
    try {
      computeCapability = await getComputeCapability();
      console.debug("[ModelSelector] detected compute capability:", computeCapability);
    } catch (err) {
      console.warn("[ModelSelector] getComputeCapability failed:", err);
      computeCapability = null;
    }
  });

  onDestroy(() => {
    document.removeEventListener("pointerdown", handleDocumentPointerDown);
    document.removeEventListener("keydown", handleDocumentKeydown);
    unlistenDownload?.();
  });

  const activeLoraCount = $derived(
    generation.loras.filter((l) => l.enabled && l.name).length
  );

  const LORAS_COLLAPSE_KEY = "mooshieui.generation.lorasCollapsed.v1";
  let lorasOpen = $state(localStorage.getItem(LORAS_COLLAPSE_KEY) !== "true");
  let lorasSaveTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const collapsed = String(!lorasOpen);
    if (lorasSaveTimer) clearTimeout(lorasSaveTimer);
    lorasSaveTimer = setTimeout(() => {
      try { localStorage.setItem(LORAS_COLLAPSE_KEY, collapsed); } catch {}
    }, 300);
  });

  function filteredLorasForIndex(index: number) {
    const search = loraSearches[index] ?? "";
    return models.loras.filter((l) =>
      l.toLowerCase().includes(search.toLowerCase())
    );
  }

  function selectLora(index: number, name: string) {
    generation.loras = generation.loras.map((l, i) =>
      i === index ? { ...l, name } : l
    );
    generation.saveSettings();
    showLoraDropdown = null;
    loraSearches = { ...loraSearches, [index]: "" };
  }

  function isLoraSelected(index: number, name: string): boolean {
    return generation.loras[index]?.name === name;
  }

  function displayLoraName(fullPath: string): string {
    if (!fullPath) return locale.t('generation.model.select_lora');
    const parts = fullPath.replace(/\\/g, "/").split("/");
    return parts[parts.length - 1];
  }

  /** Get the correct model list for a given category */
  function modelListForCategory(category: string): string[] {
    switch (category) {
      case "checkpoints": return models.checkpoints;
      case "vae": return models.vaes;
      case "loras": return models.loras;
      case "diffusion_models": return models.diffusionModels;
      case "text_encoders": return models.textEncoders;
      case "controlnet": return models.controlnetModels;
      case "upscale_models": return models.upscaleModels;
      default: return [];
    }
  }

  /** Check if ALL components of a recommended model are installed */
  function isRecommendedInstalled(rec: RecommendedModel): boolean {
    if (rec.splitModel) {
      const sm = rec.splitModel;
      return (
        isModelFileInstalled(sm.diffusionModel, models.diffusionModels) &&
        isModelFileInstalled(sm.clipModel, models.textEncoders) &&
        isModelFileInstalled(sm.vaeModel, models.vaes)
      );
    }
    if (rec.checkpoint) {
      if (!isModelFileInstalled(rec.checkpoint, models.checkpoints)) return false;
      if (rec.vaeModel && !isModelFileInstalled(rec.vaeModel, models.vaes)) return false;
      return true;
    }
    return false;
  }

  /** Set of filenames belonging to recommended models — rebuilt only when hash resolution changes */
  const recommendedFilenames = $derived(() => {
    return new Set(
      recommendedModels
        .filter((r) => r.checkpoint)
        .flatMap((r) => {
          const names = [r.checkpoint!.filename];
          if (r.checkpoint!.hash) {
            const resolved = hashResolved[`${r.checkpoint!.category}::${r.checkpoint!.hash}`];
            if (resolved) names.push(resolved);
          }
          return names;
        })
    );
  });

  /** Diffusion/UNET files already covered by a curated recommended entry */
  const recommendedDiffusionFilenames = $derived(() => {
    return new Set(
      recommendedModels
        .filter((r) => r.splitModel)
        .flatMap((r) => {
          const f = r.splitModel!.diffusionModel;
          const names = [f.filename];
          if (f.hash) {
            const resolved = hashResolved[`${f.category}::${f.hash}`];
            if (resolved) names.push(resolved);
          }
          return names;
        })
    );
  });

  interface DropdownItem {
    type: "checkpoint" | "recommended" | "diffusion";
    label: string;
    value: string;
    rec?: RecommendedModel;
    installed: boolean;
    size?: string;
    gateHint?: string;
  }

  function matchesRecommendedModel(rec: RecommendedModel): boolean {
    if (rec.splitModel) {
      if (!generation.useSplitModel || !generation.diffusionModel) return false;
      const expected = rec.splitModel.diffusionModel.filename;
      const resolved = resolvedFilename(rec.splitModel.diffusionModel);
      return generation.diffusionModel === expected || generation.diffusionModel === resolved;
    }
    if (rec.checkpoint) {
      if (generation.useSplitModel || !generation.checkpoint) return false;
      const expected = rec.checkpoint.filename;
      const resolved = resolvedFilename(rec.checkpoint);
      return generation.checkpoint === expected || generation.checkpoint === resolved;
    }
    return false;
  }

  function isDropdownItemSelected(item: DropdownItem): boolean {
    if (item.type === "recommended" && item.rec) {
      return matchesRecommendedModel(item.rec);
    }
    if (item.type === "diffusion") {
      return generation.useSplitModel && generation.diffusionModel === item.value;
    }
    return !generation.useSplitModel && generation.checkpoint === item.value;
  }

  /** Combine installed checkpoints + recommended models into a single filtered list */
  const filteredItems = $derived(() => {
    const q = checkpointSearch.toLowerCase();
    const items: DropdownItem[] = [];

    // Add recommended models first
    for (const rec of recommendedModels) {
      const installed = isRecommendedInstalled(rec);
      // Hide entries gated behind a compute-capability we haven't met unless
      // all split/checkpoint components already exist locally. That keeps
      // retired or hardware-specific models visible for users who have them,
      // without advertising them as fresh downloads.
      if (rec.minComputeCapability !== undefined && !installed) {
        if (computeCapability === null || computeCapability < rec.minComputeCapability) continue;
      }
      // Detection-only entries (no download URLs) are hidden until every
      // component is present on disk — otherwise the user would see an entry
      // they can't action.
      if (rec.detectionOnly && !installed) continue;
      if (!q || rec.label.toLowerCase().includes(q)) {
        items.push({
          type: "recommended",
          label: installed ? rec.label : `⬇ ${rec.label}`,
          value: rec.label,
          rec,
          installed,
          size: rec.sizeKey ? locale.t(rec.sizeKey) : rec.size,
          gateHint: rec.gateHint,
        });
      }
    }

    // Add regular checkpoints (skip ones that match a recommended model by filename or hash)
    const excluded = recommendedFilenames();
    for (const ckpt of models.checkpoints) {
      if (excluded.has(ckpt)) continue;
      if (!q || ckpt.toLowerCase().includes(q)) {
        items.push({
          type: "checkpoint",
          label: ckpt,
          value: ckpt,
          installed: true,
        });
      }
    }

    // Locally installed diffusion weights not in the curated list (e.g. custom Anima fine-tunes)
    const excludedDiffusion = recommendedDiffusionFilenames();
    for (const dm of models.diffusionModels) {
      if (excludedDiffusion.has(dm)) continue;
      if (!q || dm.toLowerCase().includes(q)) {
        items.push({
          type: "diffusion",
          label: dm,
          value: dm,
          installed: true,
        });
      }
    }

    return items;
  });

  // Model dropdown controls. On open, scroll the list to the selected item
  async function openCheckpointDropdown() {
    showLoraDropdown = null;
    showCheckpointDropdown = true;
    await tick();
    const container = checkpointDropdownListEl;
    const selectedRow = container?.querySelector<HTMLElement>('[data-selected="true"]');
    if (!container || !selectedRow) return;

    const containerRect = container.getBoundingClientRect();
    const rowRect = selectedRow.getBoundingClientRect();
    const rowTop = rowRect.top - containerRect.top + container.scrollTop;
    container.scrollTop = Math.max(0, rowTop - 8);
  }
  function closeCheckpointDropdown() {
    showCheckpointDropdown = false;
  }
  function toggleCheckpointDropdown() {
    if (showCheckpointDropdown) {
      closeCheckpointDropdown();
      return;
    }
    void openCheckpointDropdown();
  }

  async function openLoraDropdown(index: number) {
    showCheckpointDropdown = false;
    showLoraDropdown = index;
    await tick();
    const container = activeLoraDropdownListEl;
    const selectedRow = container?.querySelector<HTMLElement>('[data-selected="true"]');
    if (!container || !selectedRow) return;

    const containerRect = container.getBoundingClientRect();
    const rowRect = selectedRow.getBoundingClientRect();
    const rowTop = rowRect.top - containerRect.top + container.scrollTop;
    container.scrollTop = Math.max(0, rowTop - 8);
  }
  function toggleLoraDropdown(index: number) {
    if (showLoraDropdown === index) {
      showLoraDropdown = null;
      return;
    }
    void openLoraDropdown(index);
  }

  function selectCheckpoint(name: string) {
    // Clear split model state when selecting a normal checkpoint. Detection may
    // flip this back (and re-set modelSourceCategory) if the file turns out to be
    // a split-file model that just happens to live in checkpoints/.
    generation.useSplitModel = false;
    generation.diffusionModel = null;
    generation.modelSourceCategory = null;
    generation.clipModel = null;
    generation.clipType = null;
    // generation.vae = "";  // Reset selected vae for checkpoint
    generation.checkpoint = name;
    generation.applyModelMetadata({
      modelspecPredictionType: null,
      modelspecPredictKey: null,
      modelspecHeaderVPred: false,
      modelFamily: "unknown",
      modelIsSdxlLike: false,
      modelTurboVariant: "none",
      modelRecommendedVae: null,
      modelRecommendedClipModel: null,
      modelRecommendedClipType: null,
    });
    generation.applyModelSpecificPreset();
    checkpointSearch = "";
    closeCheckpointDropdown();
  }

  /** Use a diffusion model file discovered on disk (not in the curated recommended list). */
  async function selectCustomDiffusion(filename: string) {
    closeCheckpointDropdown();
    checkpointSearch = "";
    generation.useSplitModel = true;
    generation.diffusionModel = filename;
    generation.checkpoint = filename;
    generation.modelSourceCategory = null;
    await generation.fetchAndApplyModelMetadata("diffusion_models", filename);
    generation.applyModelSpecificPreset();
  }

  async function selectRecommended(rec: RecommendedModel) {
    closeCheckpointDropdown();
    checkpointSearch = "";

    // Check each component individually and download only what's missing
    const missingFiles: { file: ModelFile; label: string }[] = [];
    if (rec.splitModel) {
      const sm = rec.splitModel;
      if (!isModelFileInstalled(sm.diffusionModel, modelListForCategory(sm.diffusionModel.category)))
        missingFiles.push({ file: sm.diffusionModel, label: locale.t('generation.model.downloading_diffusion') });
      if (!isModelFileInstalled(sm.clipModel, modelListForCategory(sm.clipModel.category)))
        missingFiles.push({ file: sm.clipModel, label: locale.t('generation.model.downloading_text_encoder') });
      if (!isModelFileInstalled(sm.vaeModel, modelListForCategory(sm.vaeModel.category)))
        missingFiles.push({ file: sm.vaeModel, label: locale.t('generation.model.downloading_vae') });
    } else if (rec.checkpoint) {
      if (!isModelFileInstalled(rec.checkpoint, modelListForCategory(rec.checkpoint.category)))
        missingFiles.push({ file: rec.checkpoint, label: locale.t('generation.model.downloading_checkpoint') });
      if (rec.vaeModel && !isModelFileInstalled(rec.vaeModel, modelListForCategory(rec.vaeModel.category)))
        missingFiles.push({ file: rec.vaeModel, label: locale.t('generation.model.downloading_vae') });
    }

    if (missingFiles.length > 0) {
      // Detection-only entries have no download URLs — surface a clear error
      // rather than letting the empty URL hit the backend.
      const undownloadable = missingFiles.filter((m) => !m.file.url);
      if (undownloadable.length > 0) {
        downloadError = `Missing local file(s): ${undownloadable.map((m) => m.file.filename).join(", ")}`;
        downloading = rec.label;
        setTimeout(() => {
          downloading = null;
          downloadError = "";
        }, 4000);
        return;
      }
      downloading = rec.label;
      downloadError = "";
      // Seed a progress row for every file up-front so all three bars are
      // visible from the moment the download starts — even before their first
      // progress event arrives.
      const seeded: Record<string, DlEntry> = {};
      const order: string[] = [];
      for (const { file, label } of missingFiles) {
        seeded[file.filename] = {
          filename: file.filename,
          label,
          downloaded: 0,
          total: 0,
          done: false,
        };
        order.push(file.filename);
      }
      dlEntries = seeded;
      dlOrder = order;

      try {
        // Run all downloads in parallel. Each call emits its own
        // download:progress events keyed by filename, so the UI tracks them
        // independently.
        await Promise.all(
          missingFiles.map(async ({ file }) => {
            await downloadModel(file.url, file.category, file.filename);
            await cacheHashAfterDownload(file);
          }),
        );
        await models.refresh();
      } catch (e) {
        console.error("Failed to download model:", e);
        downloadError = `Download failed: ${e}`;
        setTimeout(() => {
          downloading = null;
          downloadError = "";
          dlEntries = {};
          dlOrder = [];
        }, 3000);
        return;
      } finally {
        if (!downloadError) {
          downloading = null;
          dlEntries = {};
          dlOrder = [];
        }
      }
    }

    // Use resolved filenames (handles renamed files detected by hash). Curated
    // entries always download into the canonical folder, so no source-category
    // override applies.
    generation.modelSourceCategory = null;
    if (rec.splitModel) {
      const sm = rec.splitModel;
      generation.useSplitModel = true;
      generation.diffusionModel = resolvedFilename(sm.diffusionModel);
      generation.clipModel = resolvedFilename(sm.clipModel);
      generation.clipType = sm.clipModel.clipType;
      generation.vae = resolvedFilename(sm.vaeModel);
      generation.checkpoint = rec.label;

      // For detection-only entries we never went through the download path,
      // so cache hashes the first time the user activates the entry. This
      // makes future detections survive renames / directory moves without
      // requiring the user to relink.
      if (rec.detectionOnly) {
        // Fire-and-forget — hashing GBs of weights can take a while.
        void Promise.all([
          cacheHashAfterDownload({ ...sm.diffusionModel, filename: generation.diffusionModel! }),
          cacheHashAfterDownload({ ...sm.clipModel, filename: generation.clipModel! }),
          cacheHashAfterDownload({ ...sm.vaeModel, filename: generation.vae }),
        ]).catch((e) => console.warn("[ModelSelector] hash caching failed:", e));
      }
    } else if (rec.checkpoint) {
      generation.useSplitModel = false;
      generation.diffusionModel = null;
      generation.clipModel = null;
      generation.clipType = null;
      generation.checkpoint = resolvedFilename(rec.checkpoint);
      generation.vae = rec.vaeModel ? resolvedFilename(rec.vaeModel) : "";
    }

    // Apply auto-settings
    if (rec.autoSettings) {
      if (rec.autoSettings.steps !== undefined) generation.steps = rec.autoSettings.steps;
      if (rec.autoSettings.cfg !== undefined) generation.cfg = rec.autoSettings.cfg;
      if (rec.autoSettings.samplerName !== undefined) generation.samplerName = rec.autoSettings.samplerName;
      if (rec.autoSettings.scheduler !== undefined) generation.scheduler = rec.autoSettings.scheduler;
      if (rec.autoSettings.upscaleSteps !== undefined) generation.upscaleSteps = rec.autoSettings.upscaleSteps;
      if (rec.autoSettings.upscaleDenoise !== undefined) generation.upscaleDenoise = rec.autoSettings.upscaleDenoise;
      if (rec.autoSettings.facefixSteps !== undefined) generation.facefixSteps = rec.autoSettings.facefixSteps;
      // Still notify autocomplete about model change (applyModelSpecificPreset won't run)
      autocomplete.notifyModelChanged(generation.isAnima);
    } else {
      generation.applyModelSpecificPreset();
    }
  }

  /** Display name for the current model */
  const displayCheckpoint = $derived(() => {
    if (generation.useSplitModel && generation.diffusionModel) {
      const match = recommendedModels.find((r) => r.splitModel && matchesRecommendedModel(r));
      return match?.label ?? generation.diffusionModel;
    }
    // Check if current checkpoint matches a recommended model (by filename or hash-resolved name)
    const recMatch = recommendedModels.find((r) => r.checkpoint && matchesRecommendedModel(r));
    if (recMatch) return recMatch.label;
    return generation.checkpoint || locale.t('generation.model.select_checkpoint');
  });
</script>

<div bind:this={modelSelectorRootEl} class="space-y-3">
  <!-- Checkpoint -->
  <div class="relative">
    <div class="mb-1 flex items-center justify-between gap-2">
      <label class="block text-xs text-neutral-400">{locale.t('generation.model.checkpoint')}<InfoTip text={locale.t('generation.model.checkpoint_tip')} /></label>
      {#if generation.currentModelMetadataKey()}
        <div bind:this={architecturePickerEl} class="relative shrink-0">
          <button
            type="button"
            class="shrink-0 text-[10px] px-2 py-0.5 rounded-full border transition-colors cursor-pointer {generation.isModelMetadataLoading
              ? 'bg-amber-600/15 text-amber-300 border-amber-600/30 hover:bg-amber-600/25'
              : currentFamilyOverride()
                ? 'bg-indigo-600/20 text-indigo-300 border-indigo-600/30 hover:bg-indigo-600/30'
                : generation.modelFamily === 'unknown'
                  ? 'bg-red-600/15 text-red-300 border-red-600/30 hover:bg-red-600/25'
                  : 'bg-emerald-600/20 text-emerald-400 border-emerald-600/30 hover:bg-emerald-600/30'}"
            title={locale.t("generation.model.architecture_picker_title")}
            onclick={() => showArchitecturePicker = !showArchitecturePicker}
          >
            {architectureBadgeLabel()}
          </button>
          {#if showArchitecturePicker}
            <div class="absolute right-0 top-[calc(100%+6px)] z-20 min-w-40 rounded-lg border border-neutral-700 bg-neutral-900 p-2 shadow-xl">
              <button
                type="button"
                class="mb-1 w-full rounded px-2 py-1 text-left text-xs text-neutral-300 hover:bg-neutral-800"
                onclick={() => applyCurrentModelFamilyOverride(null)}
              >
                {locale.t("generation.model.architecture_auto")}
              </button>
              {#each MODEL_FAMILIES.filter((family) => family !== "unknown") as family}
                <button
                  type="button"
                  class="block w-full rounded px-2 py-1 text-left text-xs text-neutral-300 hover:bg-neutral-800"
                  onclick={() => applyCurrentModelFamilyOverride(family)}
                >
                  {family}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>
    <button
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-left text-neutral-100 hover:border-neutral-600 focus:outline-none focus:border-indigo-500 transition-colors truncate flex items-center gap-2"
      onclick={toggleCheckpointDropdown}
      disabled={downloading !== null}
    >
      <span class="truncate">{displayCheckpoint()}</span>
    </button>
    {#if downloading}
      <div class="mt-2 bg-neutral-800/80 rounded-lg px-3 py-2 space-y-2">
        {#if downloadError}
          <div class="text-[11px] text-red-400">{downloadError}</div>
        {/if}
        {#each dlOrder as filename (filename)}
          {@const entry = dlEntries[filename]}
          {#if entry}
            <div>
              <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
                <span class="truncate mr-2 flex items-center gap-1.5">
                  {#if entry.done}
                    <svg class="w-3 h-3 text-emerald-400 shrink-0" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
                      <path fill-rule="evenodd" d="M16.704 5.29a1 1 0 010 1.42l-7.5 7.5a1 1 0 01-1.42 0l-3.5-3.5a1 1 0 111.42-1.42L8.5 12.08l6.79-6.79a1 1 0 011.414 0z" clip-rule="evenodd" />
                    </svg>
                  {/if}
                  <span class="truncate">{entry.label}</span>
                </span>
                {#if entry.total > 0}
                  <span class="shrink-0 tabular-nums">
                    {locale.formatBytes(entry.downloaded)} / {locale.formatBytes(entry.total)} ({dlPercent(entry)}%)
                  </span>
                {/if}
              </div>
              {#if entry.total > 0}
                <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
                  <div
                    class="h-full rounded-full transition-[width] duration-300 ease-out {entry.done ? 'bg-emerald-400' : 'bg-indigo-400'}"
                    style="width: {dlPercent(entry)}%"
                  ></div>
                </div>
              {:else}
                <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
                  <div class="bg-indigo-400 h-full rounded-full w-1/3 animate-pulse"></div>
                </div>
              {/if}
            </div>
          {/if}
        {/each}
      </div>
    {/if}
    {#if showNvfp4Warning}
      <div class="mt-2 rounded-lg border border-amber-600/30 bg-amber-600/10 px-3 py-2 text-[11px] text-amber-300">
        {locale.t('generation.model.nvfp4_warning')}
      </div>
    {/if}
    {#if showCheckpointDropdown}
      <div
        class="absolute z-50 mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-lg shadow-xl max-h-60 overflow-hidden"
      >
        <input
          type="text"
          bind:value={checkpointSearch}
          placeholder={locale.t('generation.model.search_placeholder')}
          class="w-full bg-neutral-750 border-b border-neutral-700 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none"
        />
        <div bind:this={checkpointDropdownListEl} class="overflow-y-auto max-h-48">
          {#each filteredItems() as item}
            {#if item.type === "recommended"}
              <button
                data-selected={isDropdownItemSelected(item) ? "true" : undefined}
                class="w-full text-left px-3 py-1.5 flex items-center justify-between gap-2 transition-colors {isDropdownItemSelected(item) ? 'bg-indigo-500/15 ring-1 ring-inset ring-indigo-500/40' : 'hover:bg-neutral-700'} {item.installed ? 'text-indigo-300' : 'text-indigo-400'}"
                onclick={() => item.rec && selectRecommended(item.rec)}
                title={item.label}
              >
                <span class="min-w-0 flex-1 text-sm whitespace-normal break-words leading-snug">
                  {item.label}
                  {#if item.gateHint}
                    <span class="ml-1 rounded border border-emerald-500/40 bg-emerald-500/10 px-1 py-0.5 align-middle text-[9px] uppercase tracking-wide text-emerald-300">{item.gateHint}</span>
                  {/if}
                  {#if !item.installed}
                    <span class="text-[10px] text-neutral-500 ml-1">({locale.t('generation.model.auto_download')})</span>
                  {/if}
                </span>
                {#if item.size}
                  <span class="text-[10px] text-neutral-500 shrink-0">{item.size}</span>
                {/if}
              </button>
            {:else if item.type === "diffusion"}
              <button
                data-selected={isDropdownItemSelected(item) ? "true" : undefined}
                class="w-full text-left px-3 py-1.5 text-sm whitespace-normal break-words leading-snug transition-colors {isDropdownItemSelected(item) ? 'bg-indigo-500/15 ring-1 ring-inset ring-indigo-500/40 text-indigo-200' : 'text-neutral-200 hover:bg-neutral-700'}"
                onclick={() => selectCustomDiffusion(item.value)}
                title={item.label}
              >
                {item.label}
              </button>
            {:else}
              <button
                data-selected={isDropdownItemSelected(item) ? "true" : undefined}
                class="w-full text-left px-3 py-1.5 text-sm whitespace-normal break-words leading-snug transition-colors {isDropdownItemSelected(item) ? 'bg-indigo-500/15 ring-1 ring-inset ring-indigo-500/40 text-indigo-200' : 'text-neutral-200 hover:bg-neutral-700'}"
                onclick={() => selectCheckpoint(item.value)}
                title={item.label}
              >
                {item.label}
              </button>
            {/if}
          {/each}
        </div>
      </div>
    {/if}

    <!-- ModelSpec info -->
    {#if modelSpecUnavailable && !modelSpec}
      <div class="mt-1.5 text-[11px] text-neutral-600">{locale.t('generation.model.no_modelspec')}</div>
    {:else if modelSpec}
      <button
        class="mt-1.5 w-full flex items-center gap-1.5 text-[11px] text-indigo-400 hover:text-indigo-300 transition-colors"
        onclick={() => (showModelInfo = !showModelInfo)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a1 1 0 000 2v3a1 1 0 001 1h1a1 1 0 100-2v-3a1 1 0 00-1-1H9z" clip-rule="evenodd"/></svg>
        {showModelInfo ? locale.t('generation.model.hide_model_info') : locale.t('generation.model.show_model_info')}
        {#if modelSpec.title}
          <span class="text-neutral-500 truncate">— {modelSpec.title}</span>
        {/if}
        <span class="ml-auto px-1 py-0.5 rounded bg-emerald-900/30 text-emerald-400 text-[9px]">{locale.t("generation.model_spec_badge")}</span>
      </button>
      {#if showModelInfo}
        <div class="mt-1.5 bg-neutral-800/60 border border-neutral-700/50 rounded-lg p-2.5 space-y-1.5 text-xs">
          {#if modelSpecThumbnail}
            <img
              src={modelSpecThumbnail}
              alt={locale.t('generation.model.thumbnail_alt')}
              class="w-full max-h-48 object-contain rounded-md bg-neutral-900/60"
            />
          {/if}
          {#if modelSpec.title}
            <div class="font-medium text-neutral-200">{modelSpec.title}</div>
          {/if}
          {#if modelSpec.author}
            <div class="text-neutral-500">by {modelSpec.author}</div>
          {/if}
          {#if modelSpec.date}
            <div class="flex gap-2">
              <span class="text-neutral-500">{locale.t('generation.model.date_label')}</span>
              <span class="text-neutral-300">{modelSpec.date}</span>
            </div>
          {/if}
          {#if modelSpec.description}
            <div class="text-neutral-400 text-[11px] whitespace-pre-line max-h-32 overflow-y-auto">{stripHtml(modelSpec.description)}</div>
          {/if}
          {#if modelSpec.architecture}
            <div class="flex gap-2">
              <span class="text-neutral-500">{locale.t('generation.model.architecture_label')}</span>
              <span class="text-neutral-300">
                {modelSpec.architecture}
                {#if modelSpec.architecture_inferred === "true"}
                  <span class="text-neutral-500">({locale.t('generation.model.architecture_inferred')})</span>
                {/if}
              </span>
            </div>
          {/if}
          {#if modelSpec.implementation}
            <div class="flex gap-2">
              <span class="text-neutral-500 shrink-0">{locale.t('generation.model.implementation_label')}</span>
              <span class="text-neutral-300 break-all">{modelSpec.implementation}</span>
            </div>
          {/if}
          {#if modelSpec.hash}
            <div class="flex gap-2 items-center">
              <span class="text-neutral-500">{locale.t('generation.model.hash_label')}</span>
              <span class="text-neutral-300 font-mono text-[10px]">{modelSpec.hash}</span>
              <button
                class="text-neutral-500 hover:text-neutral-300 transition-colors"
                title={locale.t('generation.model.copy_hash')}
                onclick={() => { if (modelSpec?.hash) navigator.clipboard.writeText(modelSpec.hash); }}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 20 20" fill="currentColor"><path d="M8 2a1 1 0 000 2h2a1 1 0 100-2H8z"/><path d="M3 5a2 2 0 012-2 3 3 0 003 3h2a3 3 0 003-3 2 2 0 012 2v6h-4.586l1.293-1.293a1 1 0 00-1.414-1.414l-3 3a1 1 0 000 1.414l3 3a1 1 0 001.414-1.414L10.414 13H15v3a2 2 0 01-2 2H5a2 2 0 01-2-2V5z"/></svg>
              </button>
            </div>
          {/if}
          {#if modelSpec.hash_sha256}
            <div class="flex gap-2 items-center">
              <span class="text-neutral-500 shrink-0">{locale.t('generation.model.hash_sha256_label')}</span>
              <span class="text-neutral-300 font-mono text-[10px] truncate">{modelSpec.hash_sha256}</span>
              <button
                class="text-neutral-500 hover:text-neutral-300 transition-colors shrink-0"
                title={locale.t('generation.model.copy_hash')}
                onclick={() => { if (modelSpec?.hash_sha256) navigator.clipboard.writeText(modelSpec.hash_sha256); }}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 20 20" fill="currentColor"><path d="M8 2a1 1 0 000 2h2a1 1 0 100-2H8z"/><path d="M3 5a2 2 0 012-2 3 3 0 003 3h2a3 3 0 003-3 2 2 0 012 2v6h-4.586l1.293-1.293a1 1 0 00-1.414-1.414l-3 3a1 1 0 000 1.414l3 3a1 1 0 001.414-1.414L10.414 13H15v3a2 2 0 01-2 2H5a2 2 0 01-2-2V5z"/></svg>
              </button>
            </div>
          {/if}
          {#if modelSpec.resolution}
            <div class="flex gap-2">
              <span class="text-neutral-500">{locale.t('generation.model.resolution_label')}</span>
              <span class="text-neutral-300">{modelSpec.resolution}</span>
            </div>
          {/if}
          {#if modelSpec.prediction_type}
            <div class="flex gap-2">
              <span class="text-neutral-500">{locale.t('generation.model.prediction_label')}</span>
              <span class="text-neutral-300">{modelSpec.prediction_type}</span>
            </div>
          {/if}
          {#if modelSpec.trigger_phrase}
            <div>
              <span class="text-neutral-500">{locale.t('generation.model.trigger_phrase_label')}</span>
              <button
                class="ml-1.5 text-indigo-400 hover:text-indigo-300 transition-colors"
                title={locale.t('generation.model.copy_trigger')}
                onclick={() => {
                  if (modelSpec?.trigger_phrase && !generation.positivePrompt.includes(modelSpec.trigger_phrase)) {
                    generation.positivePrompt = generation.positivePrompt
                      ? `${modelSpec.trigger_phrase}, ${generation.positivePrompt}`
                      : modelSpec.trigger_phrase;
                  }
                }}
              >
                {modelSpec.trigger_phrase}
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 inline ml-0.5" viewBox="0 0 20 20" fill="currentColor"><path d="M8 2a1 1 0 000 2h2a1 1 0 100-2H8z"/><path d="M3 5a2 2 0 012-2 3 3 0 003 3h2a3 3 0 003-3 2 2 0 012 2v6h-4.586l1.293-1.293a1 1 0 00-1.414-1.414l-3 3a1 1 0 000 1.414l3 3a1 1 0 001.414-1.414L10.414 13H15v3a2 2 0 01-2 2H5a2 2 0 01-2-2V5z"/></svg>
              </button>
            </div>
          {/if}
          {#if modelSpec.usage_hint}
            <div class="text-neutral-400 text-[11px] italic whitespace-pre-line">{stripHtml(modelSpec.usage_hint)}</div>
          {/if}
          {#if modelSpec.preprocessor}
            <div class="flex gap-2">
              <span class="text-neutral-500 shrink-0">{locale.t('generation.model.preprocessor_label')}</span>
              <span class="text-neutral-300 break-all">{modelSpec.preprocessor}</span>
            </div>
          {/if}
          {#if modelSpec.encoder_layer}
            <div class="flex gap-2">
              <span class="text-neutral-500 shrink-0">{locale.t('generation.model.encoder_layer_label')}</span>
              <span class="text-neutral-300">{modelSpec.encoder_layer}</span>
            </div>
          {/if}
          {#if modelSpec.merged_from}
            <div class="flex gap-2">
              <span class="text-neutral-500 shrink-0">{locale.t('generation.model.merged_from_label')}</span>
              <span class="text-neutral-300 break-words">{modelSpec.merged_from}</span>
            </div>
          {/if}
          {#if modelSpec.tags}
            <div class="flex flex-wrap gap-1 mt-1">
              {#each modelSpec.tags.split(",").map(t => t.trim()).filter(Boolean) as tag}
                <span class="px-1.5 py-0.5 bg-neutral-700/50 text-neutral-400 rounded text-[10px]">{tag}</span>
              {/each}
            </div>
          {/if}
          {#if extraModelSpecFields.length}
            <div class="pt-1.5 border-t border-neutral-700/50 space-y-1">
              <div class="text-neutral-500 text-[10px] uppercase tracking-wide">{locale.t('generation.model.other_fields_label')}</div>
              {#each extraModelSpecFields as entry (entry.field)}
                <div class="flex gap-2 text-[10px]">
                  <span class="text-neutral-600 shrink-0 font-mono">{entry.field}</span>
                  <span class="text-neutral-400 break-all whitespace-pre-line max-h-24 overflow-y-auto">{entry.value}</span>
                </div>
              {/each}
            </div>
          {/if}
          {#if modelSpec.license}
            <div class="flex gap-2 text-[10px]">
              <span class="text-neutral-600">{locale.t('generation.model.license_label')}</span>
              <span class="text-neutral-500">{modelSpec.license}</span>
            </div>
          {/if}
          {#if modelSpec.sai_model_spec}
            <div class="flex gap-2 text-[10px]">
              <span class="text-neutral-600">{locale.t('generation.model.spec_version_label')}</span>
              <span class="text-neutral-500">{modelSpec.sai_model_spec}</span>
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  </div>

  <!-- VAE -->
  <div>
    <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.model.vae')}<InfoTip text={locale.t('generation.model.vae_tip')} /></label>
    <select
      bind:value={generation.vae}
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
    >
      <option value="">{locale.t('generation.model.auto_vae')}</option>
      {#each models.vaes as vae}
        <option value={vae}>{vae}</option>
      {/each}
    </select>
  </div>

  <!-- Text encoder (split models only). Lists files from both the
       text_encoders/ and clip/ folders (models.textEncoders merges them). -->
  {#if generation.useSplitModel}
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.model.text_encoder')}<InfoTip text={locale.t('generation.model.text_encoder_tip')} /></label>
      <select
        bind:value={generation.clipModel}
        onchange={() => generation.saveSettings()}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        <option value={null}>{locale.t('generation.model.text_encoder_none')}</option>
        {#each models.textEncoders as encoder}
          <option value={encoder}>{encoder}</option>
        {/each}
      </select>
      {#if showKrea2EncoderWarning}
        <div class="mt-2 rounded-lg border border-amber-600/30 bg-amber-600/10 px-3 py-2 text-[11px] text-amber-300">
          {locale.t('generation.model.krea2_encoder_warning')}
        </div>
      {/if}
    </div>
  {/if}

  <!-- LoRAs -->
  <div>
    <div class="flex items-center justify-between mb-1.5">
      <div class="flex items-center gap-2">
        <label class="text-xs text-neutral-400">{locale.t('generation.model.lora_title')}<InfoTip text={locale.t('generation.model.lora_tip')} /></label>
        {#if activeLoraCount > 0}
          <span class="text-[10px] px-1.5 py-0.5 rounded-full bg-indigo-600/20 text-indigo-400">
            {activeLoraCount} active
          </span>
        {/if}
      </div>
      <div class="flex items-center gap-2">
        <button
          onclick={() => {
            generation.addLora();
            generation.saveSettings();
          }}
          class="text-xs text-indigo-400 hover:text-indigo-300 transition-colors"
        >
          {locale.t('generation.model.add_lora')}
        </button>
        {#if generation.loras.length > 0}
          <button
            class="text-neutral-400 hover:text-neutral-200 focus:outline-none"
            onclick={() => (lorasOpen = !lorasOpen)}
            title={lorasOpen ? locale.t('common.collapse', { section: locale.t('generation.model.lora_title') }) : locale.t('common.expand', { section: locale.t('generation.model.lora_title') })}
            aria-label={lorasOpen ? locale.t('common.collapse', { section: locale.t('generation.model.lora_title') }) : locale.t('common.expand', { section: locale.t('generation.model.lora_title') })}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {lorasOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>
        {/if}
      </div>
    </div>
    {#if lorasOpen}
    {#each generation.loras as lora, i}
      <div
        class="mb-2 rounded-lg border p-2.5 transition-opacity {lora.enabled
          ? 'bg-neutral-800 border-neutral-700'
          : 'bg-neutral-800/50 border-neutral-700/50 opacity-50'}"
      >
        <!-- Header row: toggle + name + remove -->
        <div class="flex items-center gap-2 mb-2">
          <button
            class="relative w-8 h-4 rounded-full transition-colors shrink-0 {lora.enabled
              ? 'bg-indigo-600'
              : 'bg-neutral-700'}"
            onclick={() => {
              generation.toggleLora(i);
              generation.saveSettings();
            }}
            role="switch"
            aria-checked={lora.enabled}
            title={lora.enabled ? locale.t("generation.lora.disable") : locale.t("generation.lora.enable")}
          >
            <span
              class="absolute top-0.5 left-0.5 w-3 h-3 rounded-full bg-white transition-transform {lora.enabled
                ? 'translate-x-4'
                : ''}"
            ></span>
          </button>

          <!-- Searchable LoRA selector -->
          <div class="relative flex-1 min-w-0">
            <button
              class="w-full bg-neutral-750 border border-neutral-600 rounded px-2 py-1 text-xs text-left truncate transition-colors {lora.enabled
                ? 'text-neutral-100 hover:border-neutral-500'
                : 'text-neutral-500'}"
              onclick={() => toggleLoraDropdown(i)}
            >
              {displayLoraName(lora.name)}
            </button>
            {#if showLoraDropdown === i}
              <div
                class="absolute z-50 mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-lg shadow-xl max-h-48 overflow-hidden"
              >
                <input
                  type="text"
                  bind:value={loraSearches[i]}
                  placeholder={locale.t('generation.model.search_loras')}
                  class="w-full bg-neutral-750 border-b border-neutral-700 px-2 py-1.5 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none"
                />
                <div bind:this={activeLoraDropdownListEl} class="overflow-y-auto max-h-36">
                  {#each filteredLorasForIndex(i) as l}
                    <button
                      data-selected={isLoraSelected(i, l) ? "true" : undefined}
                      class="w-full text-left px-2 py-1 text-xs whitespace-normal break-words leading-snug transition-colors {isLoraSelected(i, l) ? 'bg-indigo-500/15 ring-1 ring-inset ring-indigo-500/40 text-indigo-200' : 'text-neutral-200 hover:bg-neutral-700'}"
                      onclick={() => selectLora(i, l)}
                    >
                      {l}
                    </button>
                  {/each}
                </div>
              </div>
            {/if}
          </div>

          <button
            onclick={() => {
              generation.removeLora(i);
              generation.saveSettings();
            }}
            class="text-neutral-500 hover:text-red-400 transition-colors text-sm leading-none shrink-0"
            title={locale.t('common.remove')}
          >
            &times;
          </button>
        </div>

        <!-- Strength sliders -->
        {#if lora.name}
          <div class="space-y-1.5">
            <div use:scrollCapture>
              <div class="flex items-center justify-between text-xs mb-0.5">
                <span class="text-neutral-500">{locale.t('generation.model.lora_strength_model')}<InfoTip text={locale.t('generation.model.lora_strength_model_tip')} /></span>
                <span class="text-neutral-300 tabular-nums">{locale.formatDecimal(lora.strength_model, 2)}</span>
              </div>
              <input
                type="range"
                bind:value={lora.strength_model}
                oninput={() => generation.saveSettings()}
                min="0"
                max="2"
                step="0.05"
                class="w-full accent-indigo-500"
              />
            </div>
            <div use:scrollCapture>
              <div class="flex items-center justify-between text-xs mb-0.5">
                <span class="text-neutral-500">{locale.t('generation.model.lora_strength_clip')}<InfoTip text={locale.t('generation.model.lora_strength_clip_tip')} /></span>
                <span class="text-neutral-300 tabular-nums">{locale.formatDecimal(lora.strength_clip, 2)}</span>
              </div>
              <input
                type="range"
                bind:value={lora.strength_clip}
                oninput={() => generation.saveSettings()}
                min="0"
                max="2"
                step="0.05"
                class="w-full accent-indigo-500"
              />
            </div>
          </div>
        {/if}
      </div>
    {/each}
    {/if}
  </div>
</div>
