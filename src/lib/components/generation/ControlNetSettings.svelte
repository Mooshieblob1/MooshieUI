<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import {
    downloadModel,
    uploadImageBytes,
    checkNodeAvailable,
    isCustomNodeInstalled,
    installCustomNode,
    stopComfyui,
    startComfyui,
  } from "../../utils/api.js";
  import {
    CONTROLNET_PRESETS,
    getPreset,
    getPresetModel,
  } from "../../config/controlnet-presets.js";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  let preprocessorAvailable = $state<boolean | null>(null);
  let installing = $state(false);
  let installError = $state<string | null>(null);
  let installStep = $state("");
  let installMessage = $state("");
  let downloading = $state<string | null>(null);
  let downloadError = $state<string | null>(null);
  let dlBytes = $state(0);
  let dlTotal = $state(0);
  let uploadingImage = $state(false);
  let imagePreviewUrl = $state<string | null>(null);

  const dlPercent = $derived(dlTotal > 0 ? Math.round((dlBytes / dlTotal) * 100) : 0);

  function formatBytes(bytes: number): string {
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    if (bytes < 1024 * 1024 * 1024)
      return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  onMount(async () => {
    await listen("download:progress", (event: any) => {
      const data = event.payload as {
        filename: string;
        downloaded: number;
        total: number;
        done: boolean;
      };
      if (data.done) {
        dlBytes = 0;
        dlTotal = 0;
      } else {
        dlBytes = data.downloaded;
        dlTotal = data.total;
      }
    });
    await listen("install:progress", (event: any) => {
      const data = event.payload as {
        node_name: string;
        step: string;
        message: string;
        done: boolean;
      };
      installStep = data.step;
      installMessage = data.message;
    });
    // Check if preprocessor nodes are installed — first check filesystem (reliable),
    // then fall back to API check (may fail if ComfyUI hasn't loaded nodes yet)
    try {
      const installed = await isCustomNodeInstalled("comfyui_controlnet_aux");
      if (installed) {
        preprocessorAvailable = true;
      } else {
        preprocessorAvailable = await checkNodeAvailable("CannyEdgePreprocessor");
      }
    } catch {
      preprocessorAvailable = false;
    }
  });

  function isModelInstalled(filename: string): boolean {
    return models.controlnetModels.includes(filename);
  }

  async function selectPreset(presetId: string) {
    const preset = getPreset(presetId);
    if (!preset) return;

    generation.controlnetPreset = presetId;
    generation.controlnetPreprocessor = preset.preprocessor;

    const model = getPresetModel(presetId, generation.detectedArchitecture);
    if (model) {
      generation.controlnetModel = model.filename;

      if (!isModelInstalled(model.filename)) {
        downloading = model.filename;
        downloadError = null;
        try {
          await downloadModel(model.url, "controlnet", model.filename);
          await models.refresh();
        } catch (e) {
          downloadError = `Download failed: ${e}`;
          generation.controlnetModel = null;
        } finally {
          downloading = null;
        }
      }
    } else {
      generation.controlnetModel = null;
    }
  }

  function setPreview(file: File) {
    if (imagePreviewUrl) URL.revokeObjectURL(imagePreviewUrl);
    imagePreviewUrl = URL.createObjectURL(file);
  }

  function clearPreview() {
    if (imagePreviewUrl) {
      URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = null;
    }
  }

  async function handleImageUpload(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;

    uploadingImage = true;
    try {
      const buffer = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const result = await uploadImageBytes(bytes, file.name);
      generation.controlnetImage = result.name;
      setPreview(file);
    } catch (e) {
      console.error("Failed to upload control image:", e);
    } finally {
      uploadingImage = false;
    }
  }

  async function handleImagePaste() {
    try {
      const items = await navigator.clipboard.read();
      for (const item of items) {
        const imageType = item.types.find((t) => t.startsWith("image/"));
        if (imageType) {
          const blob = await item.getType(imageType);
          const ext = imageType.split("/")[1] || "png";
          const file = new File([blob], `pasted_image.${ext}`, { type: imageType });
          uploadingImage = true;
          const buffer = await file.arrayBuffer();
          const bytes = Array.from(new Uint8Array(buffer));
          const result = await uploadImageBytes(bytes, file.name);
          generation.controlnetImage = result.name;
          setPreview(file);
          uploadingImage = false;
          return;
        }
      }
    } catch (e) {
      console.error("Failed to paste image:", e);
    } finally {
      uploadingImage = false;
    }
  }

  async function handleImageDrop(event: DragEvent) {
    event.preventDefault();
    const file = event.dataTransfer?.files?.[0];
    if (!file) return;

    uploadingImage = true;
    try {
      const buffer = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const result = await uploadImageBytes(bytes, file.name);
      generation.controlnetImage = result.name;
      setPreview(file);
    } catch (e) {
      console.error("Failed to upload control image:", e);
    } finally {
      uploadingImage = false;
    }
  }

  async function installPreprocessors() {
    installing = true;
    installError = null;
    installStep = "clone";
    installMessage = "Starting installation...";
    try {
      await installCustomNode(
        "https://github.com/Fannovel16/comfyui_controlnet_aux.git",
        "comfyui_controlnet_aux"
      );

      // Restart ComfyUI to load new nodes
      installStep = "restart";
      installMessage = "Stopping ComfyUI...";
      connection.connected = false;
      await stopComfyui();

      installMessage = "Starting ComfyUI with new nodes...";
      await startComfyui();

      // Wait for the server to actually be ready via the event system
      installMessage = "Waiting for ComfyUI to become ready...";
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => {
          reject(new Error("ComfyUI did not become ready within 120 seconds"));
        }, 120_000);

        const unlistenReady = listen("comfyui:server_ready", () => {
          clearTimeout(timeout);
          unlistenReady.then((fn) => fn());
          unlistenError.then((fn) => fn());
          resolve();
        });

        const unlistenError = listen("comfyui:server_error", (event: any) => {
          clearTimeout(timeout);
          unlistenReady.then((fn) => fn());
          unlistenError.then((fn) => fn());
          reject(new Error(event.payload?.error || "ComfyUI failed to start"));
        });
      });

      // Server is ready — check if the node is now available
      installStep = "verify";
      installMessage = "Verifying preprocessor nodes...";
      try {
        preprocessorAvailable = await checkNodeAvailable("CannyEdgePreprocessor");
      } catch {
        preprocessorAvailable = false;
      }

      installing = false;
      installStep = "";
      installMessage = "";
    } catch (e) {
      installError = `Install failed: ${e}`;
      installing = false;
      installStep = "";
      installMessage = "";
    }
  }

  function presetAvailable(presetId: string): boolean {
    return getPresetModel(presetId, generation.detectedArchitecture) !== null;
  }
</script>

<div class="space-y-3">
  <!-- Enable toggle -->
  <div class="flex items-center justify-between">
    <label class="text-xs text-neutral-400"
      >ControlNet<InfoTip
        text="Use a reference image to guide the generation. ControlNet can preserve edges, depth, pose, and more from a source image."
      /></label
    >
    <button
      title="Toggle ControlNet"
      class="relative w-10 h-5 rounded-full transition-colors {generation.controlnetEnabled
        ? 'bg-indigo-600'
        : 'bg-neutral-700'}"
      onclick={() =>
        (generation.controlnetEnabled = !generation.controlnetEnabled)}
      role="switch"
      aria-checked={generation.controlnetEnabled}
    >
      <span
        class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {generation.controlnetEnabled
          ? 'translate-x-5'
          : ''}"
      ></span>
    </button>
  </div>

  {#if generation.controlnetEnabled}
    <!-- Preprocessor warning / install progress -->
    {#if preprocessorAvailable === false && generation.controlnetMode === "preset"}
      <div
        class="bg-amber-900/30 border border-amber-700/50 rounded-lg px-3 py-2 text-xs text-amber-300"
      >
        {#if installing}
          <div class="space-y-2">
            <div class="flex items-center gap-2">
              <div class="w-3.5 h-3.5 shrink-0 border-2 border-amber-400 border-t-transparent rounded-full animate-spin"></div>
              <span class="font-medium">
                {#if installStep === "clone"}
                  Cloning repository...
                {:else if installStep === "pip"}
                  Installing dependencies...
                {:else if installStep === "restart"}
                  Restarting ComfyUI...
                {:else if installStep === "verify"}
                  Verifying installation...
                {:else}
                  Installing...
                {/if}
              </span>
            </div>
            {#if installMessage}
              <div class="bg-neutral-900/60 rounded px-2 py-1.5 font-mono text-[10px] text-neutral-400 max-h-20 overflow-y-auto break-all">
                {installMessage}
              </div>
            {/if}
            <div class="w-full bg-amber-900/50 rounded-full h-1.5 overflow-hidden">
              <div
                class="bg-amber-400 h-full rounded-full transition-all duration-500"
                style="width: {installStep === 'clone' ? '25' : installStep === 'pip' ? '55' : installStep === 'restart' ? '80' : installStep === 'verify' ? '95' : '10'}%"
              ></div>
            </div>
          </div>
        {:else}
          <p class="mb-1.5">
            Preprocessors require <strong>comfyui_controlnet_aux</strong>. Install
            it to enable automatic edge/depth/pose detection.
          </p>
          <button
            onclick={installPreprocessors}
            class="px-3 py-1 rounded bg-amber-700 hover:bg-amber-600 text-white text-xs transition-colors"
          >
            Install & Restart
          </button>
        {/if}
        {#if installError}
          <p class="text-red-400 mt-1">{installError}</p>
        {/if}
      </div>
    {/if}

    <!-- Mode tabs -->
    <div class="flex rounded-lg bg-neutral-800 p-0.5">
      <button
        class="flex-1 text-xs py-1.5 rounded-md transition-colors {generation.controlnetMode ===
        'preset'
          ? 'bg-neutral-700 text-white'
          : 'text-neutral-400 hover:text-neutral-300'}"
        onclick={() => (generation.controlnetMode = "preset")}
      >
        Presets
      </button>
      <button
        class="flex-1 text-xs py-1.5 rounded-md transition-colors {generation.controlnetMode ===
        'custom'
          ? 'bg-neutral-700 text-white'
          : 'text-neutral-400 hover:text-neutral-300'}"
        onclick={() => (generation.controlnetMode = "custom")}
      >
        Custom
      </button>
    </div>

    {#if generation.controlnetMode === "preset"}
      <!-- Preset grid -->
      <div class="grid grid-cols-2 gap-1.5">
        {#each CONTROLNET_PRESETS as preset}
          {@const available = presetAvailable(preset.id)}
          {@const selected = generation.controlnetPreset === preset.id}
          <button
            onclick={() => available && selectPreset(preset.id)}
            disabled={!available || downloading !== null}
            class="text-left p-2 rounded-lg border transition-colors {selected
              ? 'border-indigo-500 bg-indigo-500/10'
              : available
                ? 'border-neutral-700 bg-neutral-800/50 hover:border-neutral-600'
                : 'border-neutral-800 bg-neutral-900/30 opacity-40 cursor-not-allowed'}"
          >
            <div class="text-xs font-medium {selected ? 'text-indigo-300' : 'text-neutral-200'}">
              {preset.label}
            </div>
            <div class="text-[10px] text-neutral-500 mt-0.5 leading-tight">
              {available ? preset.description : "Not available for this model"}
            </div>
          </button>
        {/each}
      </div>

      <!-- Download progress -->
      {#if downloading}
        <div class="bg-neutral-800/80 rounded-lg px-3 py-2">
          <div
            class="flex items-center justify-between text-[11px] text-neutral-400 mb-1"
          >
            <span class="truncate mr-2">Downloading {downloading}...</span>
            {#if dlTotal > 0}
              <span class="shrink-0 tabular-nums"
                >{formatBytes(dlBytes)} / {formatBytes(dlTotal)} ({dlPercent}%)</span
              >
            {/if}
          </div>
          {#if dlTotal > 0}
            <div
              class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden"
            >
              <div
                class="bg-indigo-400 h-full rounded-full transition-all duration-300 ease-out"
                style="width: {dlPercent}%"
              ></div>
            </div>
          {:else}
            <div
              class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden"
            >
              <div
                class="bg-indigo-400 h-full rounded-full w-1/3 animate-pulse"
              ></div>
            </div>
          {/if}
        </div>
      {/if}
      {#if downloadError}
        <p class="text-xs text-red-400">{downloadError}</p>
      {/if}
    {:else}
      <!-- Custom mode -->
      <div>
        <label class="block text-xs text-neutral-400 mb-1"
          >ControlNet Model<InfoTip
            text="The ControlNet model file from your models/controlnet/ folder. Download models from the Model Hub or place them manually."
          /></label
        >
        <select
          bind:value={generation.controlnetModel}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
        >
          <option value={null}>Select model...</option>
          {#each models.controlnetModels as model}
            <option value={model}>{model}</option>
          {/each}
        </select>
      </div>

      <div class="flex items-center gap-2">
        <input
          type="checkbox"
          id="cn-use-preprocessor"
          checked={!!generation.controlnetPreprocessor}
          onchange={(e) => {
            generation.controlnetPreprocessor = (e.target as HTMLInputElement).checked
              ? "CannyEdgePreprocessor"
              : null;
          }}
          class="w-4 h-4 accent-indigo-500 rounded"
        />
        <label for="cn-use-preprocessor" class="text-xs text-neutral-400">
          Use preprocessor
        </label>
      </div>

      {#if generation.controlnetPreprocessor !== null}
        <div>
          <label class="block text-xs text-neutral-400 mb-1">Preprocessor</label>
          <input
            type="text"
            bind:value={generation.controlnetPreprocessor}
            placeholder="e.g. CannyEdgePreprocessor"
            class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
          />
        </div>
      {/if}
    {/if}

    <!-- Control image upload -->
    <div>
      <label class="block text-xs text-neutral-400 mb-1"
        >Control Image<InfoTip
          text="The reference image for ControlNet. In preset mode, this image will be processed (e.g. edge detection) before being used as guidance."
        /></label
      >
      {#if generation.controlnetImage}
        <div class="space-y-2">
          {#if imagePreviewUrl}
            <div class="relative rounded-lg overflow-hidden bg-neutral-800 border border-neutral-700">
              <img
                src={imagePreviewUrl}
                alt="Control image"
                class="w-full max-h-48 object-contain"
              />
              <div class="absolute top-1.5 right-1.5">
                <button
                  onclick={() => { generation.controlnetImage = null; clearPreview(); }}
                  class="p-1 rounded bg-neutral-900/80 text-neutral-400 hover:text-red-400 transition-colors"
                  title="Remove image"
                >
                  <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                </button>
              </div>
            </div>
          {/if}
          <div class="flex items-center gap-2 bg-neutral-800 rounded-lg px-3 py-2">
            <span class="text-xs text-neutral-300 truncate flex-1">{generation.controlnetImage}</span>
            {#if !imagePreviewUrl}
              <button
                onclick={() => { generation.controlnetImage = null; clearPreview(); }}
                class="text-xs text-red-400 hover:text-red-300 shrink-0"
              >
                Remove
              </button>
            {/if}
            <label class="text-xs text-indigo-400 hover:text-indigo-300 cursor-pointer shrink-0">
              Replace
              <input
                type="file"
                accept="image/*"
                onchange={handleImageUpload}
                class="hidden"
              />
            </label>
          </div>
        </div>
      {:else}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="border-2 border-dashed border-neutral-700 rounded-lg p-4 text-center hover:border-neutral-600 transition-colors"
          ondragover={(e) => e.preventDefault()}
          ondrop={handleImageDrop}
        >
          {#if uploadingImage}
            <p class="text-xs text-neutral-500">Uploading...</p>
          {:else}
            <div class="flex items-center justify-center gap-3">
              <label class="cursor-pointer text-xs text-neutral-500 hover:text-neutral-300 transition-colors">
                <input
                  type="file"
                  accept="image/*"
                  onchange={handleImageUpload}
                  class="hidden"
                />
                Browse or drop image
              </label>
              <span class="text-neutral-700">|</span>
              <button
                type="button"
                onclick={handleImagePaste}
                class="text-xs text-neutral-500 hover:text-neutral-300 transition-colors flex items-center gap-1"
              >
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" /></svg>
                Paste
              </button>
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <!-- Strength & range sliders -->
    <div>
      <label
        class="flex items-center justify-between text-xs text-neutral-400 mb-1"
      >
        <span>Strength<InfoTip
          text="How strongly the ControlNet guides the generation. Higher values follow the control image more closely but may reduce creativity."
        /></span>
        <span class="text-neutral-300"
          >{generation.controlnetStrength.toFixed(2)}</span
        >
      </label>
      <input
        type="range"
        bind:value={generation.controlnetStrength}
        min="0"
        max="2"
        step="0.05"
        class="w-full accent-indigo-500"
      />
    </div>

    <div class="grid grid-cols-2 gap-3">
      <div>
        <label
          class="flex items-center justify-between text-xs text-neutral-400 mb-1"
        >
          <span>Start %<InfoTip
            text="When ControlNet starts influencing the generation (0% = from the very beginning). Delaying the start can add more variation."
          /></span>
          <span class="text-neutral-300"
            >{(generation.controlnetStartPercent * 100).toFixed(0)}%</span
          >
        </label>
        <input
          type="range"
          bind:value={generation.controlnetStartPercent}
          min="0"
          max="1"
          step="0.05"
          class="w-full accent-indigo-500"
        />
      </div>
      <div>
        <label
          class="flex items-center justify-between text-xs text-neutral-400 mb-1"
        >
          <span>End %<InfoTip
            text="When ControlNet stops influencing the generation (100% = until the very end). Ending early lets the model refine details freely."
          /></span>
          <span class="text-neutral-300"
            >{(generation.controlnetEndPercent * 100).toFixed(0)}%</span
          >
        </label>
        <input
          type="range"
          bind:value={generation.controlnetEndPercent}
          min="0"
          max="1"
          step="0.05"
          class="w-full accent-indigo-500"
        />
      </div>
    </div>
  {/if}
</div>
