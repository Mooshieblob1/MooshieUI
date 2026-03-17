<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { downloadModel } from "../../utils/api.js";
  import InfoTip from "../ui/InfoTip.svelte";

  interface RecommendedModel {
    label: string;
    /** If true, uses split model loading (UNETLoader + CLIPLoader + VAELoader) */
    splitModel?: {
      diffusionModel: { filename: string; url: string; category: string };
      clipModel: { filename: string; url: string; category: string; clipType: string };
      vaeModel: { filename: string; url: string; category: string };
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
      label: "Anima Preview 2",
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
      },
    },
  ];

  let checkpointSearch = $state("");
  let showCheckpointDropdown = $state(false);
  let showLoraDropdown = $state<number | null>(null);
  let loraSearches = $state<Record<number, string>>({});
  let downloading = $state<string | null>(null);
  let downloadProgress = $state("");

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

  /** Check if a recommended split model's diffusion file is already installed */
  function isSplitModelInstalled(rec: RecommendedModel): boolean {
    if (!rec.splitModel) return false;
    return models.diffusionModels.includes(rec.splitModel.diffusionModel.filename);
  }

  /** Combine installed checkpoints + recommended models into a single filtered list */
  const filteredItems = $derived(() => {
    const q = checkpointSearch.toLowerCase();
    const items: { type: "checkpoint" | "recommended"; label: string; value: string; rec?: RecommendedModel; installed: boolean }[] = [];

    // Add recommended models first
    for (const rec of recommendedModels) {
      const installed = rec.splitModel ? isSplitModelInstalled(rec) : false;
      if (!q || rec.label.toLowerCase().includes(q)) {
        items.push({
          type: "recommended",
          label: installed ? rec.label : `⬇ ${rec.label}`,
          value: rec.label,
          rec,
          installed,
        });
      }
    }

    // Add regular checkpoints
    for (const ckpt of models.checkpoints) {
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
    checkpointSearch = "";
    showCheckpointDropdown = false;
  }

  async function selectRecommended(rec: RecommendedModel) {
    showCheckpointDropdown = false;
    checkpointSearch = "";

    if (rec.splitModel) {
      const sm = rec.splitModel;
      const installed = isSplitModelInstalled(rec);

      if (!installed) {
        // Download all three files
        downloading = rec.label;
        try {
          downloadProgress = "Downloading diffusion model...";
          await downloadModel(sm.diffusionModel.url, sm.diffusionModel.category, sm.diffusionModel.filename);
          downloadProgress = "Downloading text encoder...";
          await downloadModel(sm.clipModel.url, sm.clipModel.category, sm.clipModel.filename);
          downloadProgress = "Downloading VAE...";
          await downloadModel(sm.vaeModel.url, sm.vaeModel.category, sm.vaeModel.filename);
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

      // Configure split model loading
      generation.useSplitModel = true;
      generation.diffusionModel = sm.diffusionModel.filename;
      generation.clipModel = sm.clipModel.filename;
      generation.clipType = sm.clipModel.clipType;
      generation.vae = sm.vaeModel.filename;
      generation.checkpoint = rec.label; // Display name
    }

    // Apply auto-settings
    if (rec.autoSettings) {
      if (rec.autoSettings.steps !== undefined) generation.steps = rec.autoSettings.steps;
      if (rec.autoSettings.cfg !== undefined) generation.cfg = rec.autoSettings.cfg;
      if (rec.autoSettings.samplerName !== undefined) generation.samplerName = rec.autoSettings.samplerName;
      if (rec.autoSettings.scheduler !== undefined) generation.scheduler = rec.autoSettings.scheduler;
      if (rec.autoSettings.upscaleSteps !== undefined) generation.upscaleSteps = rec.autoSettings.upscaleSteps;
      if (rec.autoSettings.upscaleDenoise !== undefined) generation.upscaleDenoise = rec.autoSettings.upscaleDenoise;
    }
  }

  /** Display name for the current model */
  const displayCheckpoint = $derived(
    generation.useSplitModel && generation.diffusionModel
      ? recommendedModels.find((r) => r.splitModel?.diffusionModel.filename === generation.diffusionModel)?.label ?? generation.diffusionModel
      : generation.checkpoint || "Select checkpoint..."
  );
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
      <span class="truncate">{displayCheckpoint}</span>
      {#if generation.isAnima}
        <span class="shrink-0 text-[9px] px-1.5 py-0.5 rounded-full bg-emerald-600/20 text-emerald-400 border border-emerald-600/30">Quality prompts applied</span>
      {/if}
    </button>
    {#if downloading}
      <p class="text-xs text-indigo-400 mt-1 animate-pulse">{downloadProgress || `Downloading ${downloading}...`}</p>
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
                class="w-full text-left px-3 py-1.5 text-sm hover:bg-neutral-700 truncate {item.installed ? 'text-indigo-300' : 'text-indigo-400'}"
                onclick={() => item.rec && selectRecommended(item.rec)}
              >
                {item.label}
                {#if !item.installed}
                  <span class="text-[10px] text-neutral-500 ml-1">(auto-download)</span>
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
