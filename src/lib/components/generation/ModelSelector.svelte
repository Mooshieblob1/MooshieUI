<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { autocomplete } from "../../stores/autocomplete.svelte.js";
  import { downloadModel, findModelByHash, hashModelFile, readModelSpec, type ModelSpec } from "../../utils/api.js";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  interface ModelFile {
    filename: string;
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
    };
  }

  const recommendedModels: RecommendedModel[] = [
    {
      label: "SIH-1.5",
      size: "~7.5 GB",
      checkpoint: {
        filename: "SIH-1.5.safetensors",
        url: "https://huggingface.co/Enferlain/juice/resolve/main/noob/%CE%A3%CE%99%CE%97-1.5.safetensors",
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
      label: "Anima Preview 2",
      size: "~13 GB",
      splitModel: {
        diffusionModel: {
          filename: "anima-preview2.safetensors",
          url: "https://huggingface.co/circlestone-labs/Anima/resolve/main/split_files/diffusion_models/anima-preview2.safetensors",
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
  ];

  let modelSpec = $state<ModelSpec | null>(null);
  let modelSpecLoading = $state(false);
  let modelSpecFilename = $state("");
  let showModelInfo = $state(false);

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

  let modelSpecUnavailable = $state(false);

  async function loadModelSpec(category: string, filename: string) {
    if (!filename || !filename.endsWith(".safetensors")) {
      modelSpec = null;
      modelSpecUnavailable = false;
      modelSpecFilename = "";
      return;
    }
    if (filename === modelSpecFilename) return;
    modelSpecFilename = filename;
    modelSpecLoading = true;
    modelSpecUnavailable = false;
    try {
      const spec = await readModelSpec(category, filename);
      if (spec && Object.keys(spec).length > 0) {
        modelSpec = spec;
        // Update generation store with authoritative architecture from modelspec
        if (spec.architecture) {
          generation.modelspecArchitecture = spec.architecture;
        } else {
          generation.modelspecArchitecture = null;
        }
      } else {
        modelSpec = null;
        modelSpecUnavailable = true;
        generation.modelspecArchitecture = null;
      }
    } catch {
      modelSpec = null;
      modelSpecUnavailable = true;
      generation.modelspecArchitecture = null;
    } finally {
      modelSpecLoading = false;
    }
  }

  $effect(() => {
    if (generation.useSplitModel && generation.diffusionModel) {
      loadModelSpec("diffusion_models", generation.diffusionModel);
    } else if (generation.checkpoint) {
      loadModelSpec("checkpoints", generation.checkpoint);
    }
  });

  let checkpointSearch = $state("");
  let showCheckpointDropdown = $state(false);
  let showLoraDropdown = $state<number | null>(null);
  let loraSearches = $state<Record<number, string>>({});
  let downloading = $state<string | null>(null);
  let downloadProgress = $state("");

  // Download progress tracking
  let dlFilename = $state("");
  let dlBytes = $state(0);
  let dlTotal = $state(0);

  // Hash-based model detection: maps "category::hash" -> resolved filename on disk
  let hashResolved = $state<Record<string, string>>({});

  function formatBytes(bytes: number): string {
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  const dlPercent = $derived(dlTotal > 0 ? Math.round((dlBytes / dlTotal) * 100) : 0);

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

  onMount(async () => {
    unlistenDownload = await listen("download:progress", (event: any) => {
      const data = event.payload as {
        filename: string;
        downloaded: number;
        total: number;
        done: boolean;
      };
      if (data.done) {
        dlFilename = "";
        dlBytes = 0;
        dlTotal = 0;
      } else {
        dlFilename = data.filename;
        dlBytes = data.downloaded;
        dlTotal = data.total;
      }
    });

    // Resolve model hashes in background
    resolveModelHashes();
  });

  onDestroy(() => {
    unlistenDownload?.();
  });

  const activeLoraCount = $derived(
    generation.loras.filter((l) => l.enabled && l.name).length
  );

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
    showLoraDropdown = null;
    loraSearches = { ...loraSearches, [index]: "" };
  }

  function displayLoraName(fullPath: string): string {
    if (!fullPath) return "Select LoRA...";
    const parts = fullPath.replace(/\\/g, "/").split("/");
    return parts[parts.length - 1];
  }

  /** Check if a recommended model is already installed (hash-first, filename fallback) */
  function isRecommendedInstalled(rec: RecommendedModel): boolean {
    if (rec.splitModel) {
      return isModelFileInstalled(rec.splitModel.diffusionModel, models.diffusionModels);
    }
    if (rec.checkpoint) {
      return isModelFileInstalled(rec.checkpoint, models.checkpoints);
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

  /** Combine installed checkpoints + recommended models into a single filtered list */
  const filteredItems = $derived(() => {
    const q = checkpointSearch.toLowerCase();
    const items: { type: "checkpoint" | "recommended"; label: string; value: string; rec?: RecommendedModel; installed: boolean; size?: string }[] = [];

    // Add recommended models first
    for (const rec of recommendedModels) {
      const installed = isRecommendedInstalled(rec);
      if (!q || rec.label.toLowerCase().includes(q)) {
        items.push({
          type: "recommended",
          label: installed ? rec.label : `⬇ ${rec.label}`,
          value: rec.label,
          rec,
          installed,
          size: rec.size,
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

    return items;
  });

  function selectCheckpoint(name: string) {
    // Clear split model state when selecting a normal checkpoint
    generation.useSplitModel = false;
    generation.diffusionModel = null;
    generation.clipModel = null;
    generation.clipType = null;
    generation.checkpoint = name;
    generation.applyModelSpecificPreset(name);
    checkpointSearch = "";
    showCheckpointDropdown = false;
  }

  async function selectRecommended(rec: RecommendedModel) {
    showCheckpointDropdown = false;
    checkpointSearch = "";

    const installed = isRecommendedInstalled(rec);

    if (!installed) {
      downloading = rec.label;
      try {
        if (rec.splitModel) {
          const sm = rec.splitModel;
          downloadProgress = "Downloading diffusion model...";
          await downloadModel(sm.diffusionModel.url, sm.diffusionModel.category, sm.diffusionModel.filename);
          await cacheHashAfterDownload(sm.diffusionModel);
          downloadProgress = "Downloading text encoder...";
          await downloadModel(sm.clipModel.url, sm.clipModel.category, sm.clipModel.filename);
          await cacheHashAfterDownload(sm.clipModel);
          downloadProgress = "Downloading VAE...";
          await downloadModel(sm.vaeModel.url, sm.vaeModel.category, sm.vaeModel.filename);
          await cacheHashAfterDownload(sm.vaeModel);
        } else if (rec.checkpoint) {
          downloadProgress = "Downloading checkpoint...";
          await downloadModel(rec.checkpoint.url, rec.checkpoint.category, rec.checkpoint.filename);
          await cacheHashAfterDownload(rec.checkpoint);
          if (rec.vaeModel) {
            downloadProgress = "Downloading VAE...";
            await downloadModel(rec.vaeModel.url, rec.vaeModel.category, rec.vaeModel.filename);
            await cacheHashAfterDownload(rec.vaeModel);
          }
        }
        await models.refresh();
      } catch (e) {
        console.error("Failed to download model:", e);
        downloadProgress = `Download failed: ${e}`;
        setTimeout(() => { downloading = null; downloadProgress = ""; }, 3000);
        return;
      } finally {
        downloading = null;
        downloadProgress = "";
      }
    }

    // Use resolved filenames (handles renamed files detected by hash)
    if (rec.splitModel) {
      const sm = rec.splitModel;
      generation.useSplitModel = true;
      generation.diffusionModel = resolvedFilename(sm.diffusionModel);
      generation.clipModel = resolvedFilename(sm.clipModel);
      generation.clipType = sm.clipModel.clipType;
      generation.vae = resolvedFilename(sm.vaeModel);
      generation.checkpoint = rec.label;
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
      generation.applyModelSpecificPreset(generation.useSplitModel ? generation.diffusionModel : generation.checkpoint);
    }
  }

  /** Display name for the current model */
  const displayCheckpoint = $derived(() => {
    if (generation.useSplitModel && generation.diffusionModel) {
      const match = recommendedModels.find((r) => {
        if (!r.splitModel) return false;
        const expected = r.splitModel.diffusionModel.filename;
        const resolved = resolvedFilename(r.splitModel.diffusionModel);
        return generation.diffusionModel === expected || generation.diffusionModel === resolved;
      });
      return match?.label ?? generation.diffusionModel;
    }
    // Check if current checkpoint matches a recommended model (by filename or hash-resolved name)
    const recMatch = recommendedModels.find((r) => {
      if (!r.checkpoint) return false;
      const expected = r.checkpoint.filename;
      const resolved = resolvedFilename(r.checkpoint);
      return generation.checkpoint === expected || generation.checkpoint === resolved;
    });
    if (recMatch) return recMatch.label;
    return generation.checkpoint || "Select checkpoint...";
  });
</script>

<div class="space-y-3">
  <!-- Checkpoint -->
  <div class="relative">
    <label class="block text-xs text-neutral-400 mb-1">Checkpoint<InfoTip text="The AI model that generates your images. Different checkpoints are trained on different styles — anime, photorealism, illustration, etc." /></label>
    <button
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-left text-neutral-100 hover:border-neutral-600 focus:outline-none focus:border-indigo-500 transition-colors truncate flex items-center gap-2"
      onclick={() => (showCheckpointDropdown = !showCheckpointDropdown)}
      disabled={downloading !== null}
    >
      <span class="truncate">{displayCheckpoint()}</span>
    </button>
    {#if downloading}
      <div class="mt-2 bg-neutral-800/80 rounded-lg px-3 py-2">
        <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
          <span class="truncate mr-2">{downloadProgress || `Downloading ${downloading}...`}</span>
          {#if dlTotal > 0}
            <span class="shrink-0 tabular-nums">{formatBytes(dlBytes)} / {formatBytes(dlTotal)} ({dlPercent}%)</span>
          {/if}
        </div>
        {#if dlTotal > 0}
          <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
            <div
              class="bg-indigo-400 h-full rounded-full transition-[width] duration-300 ease-out"
              style="width: {dlPercent}%"
            ></div>
          </div>
        {:else}
          <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
            <div class="bg-indigo-400 h-full rounded-full w-1/3 animate-pulse"></div>
          </div>
        {/if}
      </div>
    {/if}
    {#if showCheckpointDropdown}
      <div
        class="absolute z-50 mt-1 w-full bg-neutral-800 border border-neutral-700 rounded-lg shadow-xl max-h-60 overflow-hidden"
      >
        <input
          type="text"
          bind:value={checkpointSearch}
          placeholder="Search..."
          class="w-full bg-neutral-750 border-b border-neutral-700 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none"
        />
        <div class="overflow-y-auto max-h-48">
          {#each filteredItems() as item}
            {#if item.type === "recommended"}
              <button
                class="w-full text-left px-3 py-1.5 hover:bg-neutral-700 flex items-center justify-between gap-2 {item.installed ? 'text-indigo-300' : 'text-indigo-400'}"
                onclick={() => item.rec && selectRecommended(item.rec)}
              >
                <span class="text-sm truncate">
                  {item.label}
                  {#if !item.installed}
                    <span class="text-[10px] text-neutral-500 ml-1">(auto-download)</span>
                  {/if}
                </span>
                {#if item.size}
                  <span class="text-[10px] text-neutral-500 shrink-0">{item.size}</span>
                {/if}
              </button>
            {:else}
              <button
                class="w-full text-left px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-700 truncate"
                onclick={() => selectCheckpoint(item.value)}
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
      <div class="mt-1.5 text-[11px] text-neutral-600">No ModelSpec metadata</div>
    {:else if modelSpec}
      <button
        class="mt-1.5 w-full flex items-center gap-1.5 text-[11px] text-indigo-400 hover:text-indigo-300 transition-colors"
        onclick={() => (showModelInfo = !showModelInfo)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a1 1 0 000 2v3a1 1 0 001 1h1a1 1 0 100-2v-3a1 1 0 00-1-1H9z" clip-rule="evenodd"/></svg>
        {showModelInfo ? "Hide" : "Show"} model info
        {#if modelSpec.title}
          <span class="text-neutral-500 truncate">— {modelSpec.title}</span>
        {/if}
        <span class="ml-auto px-1 py-0.5 rounded bg-emerald-900/30 text-emerald-400 text-[9px]">ModelSpec</span>
      </button>
      {#if showModelInfo}
        <div class="mt-1.5 bg-neutral-800/60 border border-neutral-700/50 rounded-lg p-2.5 space-y-1.5 text-xs">
          {#if modelSpec.title}
            <div class="font-medium text-neutral-200">{modelSpec.title}</div>
          {/if}
          {#if modelSpec.author}
            <div class="text-neutral-500">by {modelSpec.author}</div>
          {/if}
          {#if modelSpec.description}
            <div class="text-neutral-400 text-[11px] whitespace-pre-line max-h-32 overflow-y-auto">{stripHtml(modelSpec.description)}</div>
          {/if}
          {#if modelSpec.architecture}
            <div class="flex gap-2">
              <span class="text-neutral-500">Architecture:</span>
              <span class="text-neutral-300">{modelSpec.architecture}</span>
            </div>
          {/if}
          {#if modelSpec.resolution}
            <div class="flex gap-2">
              <span class="text-neutral-500">Resolution:</span>
              <span class="text-neutral-300">{modelSpec.resolution}</span>
            </div>
          {/if}
          {#if modelSpec.prediction_type}
            <div class="flex gap-2">
              <span class="text-neutral-500">Prediction:</span>
              <span class="text-neutral-300">{modelSpec.prediction_type}</span>
            </div>
          {/if}
          {#if modelSpec.trigger_phrase}
            <div>
              <span class="text-neutral-500">Trigger phrase:</span>
              <button
                class="ml-1.5 text-indigo-400 hover:text-indigo-300 transition-colors"
                title="Copy trigger phrase to prompt"
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
          {#if modelSpec.tags}
            <div class="flex flex-wrap gap-1 mt-1">
              {#each modelSpec.tags.split(",").map(t => t.trim()).filter(Boolean) as tag}
                <span class="px-1.5 py-0.5 bg-neutral-700/50 text-neutral-400 rounded text-[10px]">{tag}</span>
              {/each}
            </div>
          {/if}
          {#if modelSpec.license}
            <div class="flex gap-2 text-[10px]">
              <span class="text-neutral-600">License:</span>
              <span class="text-neutral-500">{modelSpec.license}</span>
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  </div>

  <!-- VAE -->
  <div>
    <label class="block text-xs text-neutral-400 mb-1">VAE<InfoTip text="Variational Auto-Encoder — converts between pixel images and the latent space the AI works in. 'Automatic' uses the one built into your checkpoint, which is usually best." /></label>
    <select
      bind:value={generation.vae}
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
    >
      <option value="">Automatic (from checkpoint)</option>
      {#each models.vaes as vae}
        <option value={vae}>{vae}</option>
      {/each}
    </select>
  </div>

  <!-- LoRAs -->
  <div>
    <div class="flex items-center justify-between mb-1.5">
      <div class="flex items-center gap-2">
        <label class="text-xs text-neutral-400">LoRAs<InfoTip text="Low-Rank Adaptations — small add-on models that modify the checkpoint's style or teach it new concepts (characters, styles, objects) without replacing the whole model." /></label>
        {#if activeLoraCount > 0}
          <span class="text-[10px] px-1.5 py-0.5 rounded-full bg-indigo-600/20 text-indigo-400">
            {activeLoraCount} active
          </span>
        {/if}
      </div>
      <button
        onclick={() => generation.addLora()}
        class="text-xs text-indigo-400 hover:text-indigo-300 transition-colors"
      >
        + Add LoRA
      </button>
    </div>
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
            onclick={() => generation.toggleLora(i)}
            role="switch"
            aria-checked={lora.enabled}
            title={lora.enabled ? "Disable" : "Enable"}
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
              onclick={() =>
                (showLoraDropdown = showLoraDropdown === i ? null : i)}
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
                  placeholder="Search LoRAs..."
                  class="w-full bg-neutral-750 border-b border-neutral-700 px-2 py-1.5 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none"
                />
                <div class="overflow-y-auto max-h-36">
                  {#each filteredLorasForIndex(i) as l}
                    <button
                      class="w-full text-left px-2 py-1 text-xs text-neutral-200 hover:bg-neutral-700 truncate"
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
            onclick={() => generation.removeLora(i)}
            class="text-neutral-500 hover:text-red-400 transition-colors text-sm leading-none shrink-0"
            title="Remove"
          >
            &times;
          </button>
        </div>

        <!-- Strength sliders -->
        {#if lora.name}
          <div class="space-y-1.5">
            <div>
              <div class="flex items-center justify-between text-xs mb-0.5">
                <span class="text-neutral-500">Model<InfoTip text="How strongly this LoRA affects the image generation model. Higher values = stronger effect, but too high can distort the image." /></span>
                <span class="text-neutral-300 tabular-nums">{lora.strength_model.toFixed(2)}</span>
              </div>
              <input
                type="range"
                bind:value={lora.strength_model}
                min="0"
                max="2"
                step="0.05"
                class="w-full accent-indigo-500"
              />
            </div>
            <div>
              <div class="flex items-center justify-between text-xs mb-0.5">
                <span class="text-neutral-500">CLIP<InfoTip text="How strongly this LoRA affects text understanding. Controls how much the LoRA changes what the AI 'sees' in your prompt." /></span>
                <span class="text-neutral-300 tabular-nums">{lora.strength_clip.toFixed(2)}</span>
              </div>
              <input
                type="range"
                bind:value={lora.strength_clip}
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
  </div>
</div>
