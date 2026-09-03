<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import {
    uploadImage,
    uploadImageBytes,
    readClipboardImageSafe,
    checkNodeAvailable,
    installCustomNode,
  } from "../../utils/api.js";
  import { onMount } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  const IPADAPTER_CLASS = "IPAdapterUnifiedLoader";
  const IPADAPTER_GIT_URL = "https://github.com/cubiq/ComfyUI_IPAdapter_plus.git";
  const IPADAPTER_PACK_NAME = "ComfyUI_IPAdapter_plus";

  const SIGCLIP_URL =
    "https://huggingface.co/Comfy-Org/sigclip_vision_384/resolve/main/sigclip_vision_patch14_384.safetensors";
  const CLIP_VIT_H_URL =
    "https://huggingface.co/h94/IP-Adapter/resolve/main/models/image_encoder/CLIP-ViT-H-14-laion2B-s32B-b79K.safetensors";
  const IPA_SD15_URL =
    "https://huggingface.co/h94/IP-Adapter/resolve/main/models/ip-adapter-plus_sd15.safetensors";
  const IPA_SDXL_URL =
    "https://huggingface.co/h94/IP-Adapter/resolve/main/sdxl_models/ip-adapter-plus_sdxl_vit-h.safetensors";

  let ipadapterAvailable = $state<boolean | null>(null);
  let installing = $state(false);
  let installError = $state<string | null>(null);
  let uploadingImage = $state(false);
  let imagePreviewUrl = $state<string | null>(null);
  let dropZone = $state<HTMLElement | null>(null);
  let pasteActive = $state(false);

  const isRedux = $derived(generation.styleRefIsRedux);
  const isIPAdapter = $derived(generation.styleRefIsIPAdapter);
  const supported = $derived(generation.supportsStyleRef);

  const ipadapterWeightTypes = [
    "linear",
    "style transfer",
    "composition",
    "strong style transfer",
  ];
  const reduxWeightTypes = ["multiply", "attn_bias", "average"];

  async function probeIpadapter() {
    if (!isIPAdapter) {
      ipadapterAvailable = null;
      return;
    }
    ipadapterAvailable = await checkNodeAvailable(IPADAPTER_CLASS).catch(() => null);
  }

  onMount(() => {
    probeIpadapter();
  });

  $effect(() => {
    // Re-probe when the model family changes
    void generation.modelFamily;
    probeIpadapter();
  });

  $effect(() => {
    const el = dropZone;
    if (!el) return;
    const handler = (event: Event) => {
      const e = event as CustomEvent<{ paths: string[] }>;
      const path = e.detail?.paths?.[0];
      if (!path) return;
      handleTauriDrop(path);
    };
    el.addEventListener("tauri-file-drop", handler);
    return () => el.removeEventListener("tauri-file-drop", handler);
  });

  $effect(() => {
    const handler = async (event: ClipboardEvent) => {
      if (!pasteActive || generation.styleRefImage) return;
      const target = event.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable
      )
        return;
      event.preventDefault();
      event.stopImmediatePropagation();
      await pasteImage();
    };
    window.addEventListener("paste", handler, { capture: true });
    return () => window.removeEventListener("paste", handler, { capture: true });
  });

  async function handleTauriDrop(path: string) {
    uploadingImage = true;
    try {
      const result = await uploadImage(path);
      generation.styleRefImage = result.name;
      imagePreviewUrl = null;
    } catch (e) {
      console.error("style ref tauri drop failed:", e);
    } finally {
      uploadingImage = false;
    }
  }

  async function pasteImage() {
    const bytes = await readClipboardImageSafe().catch(() => null);
    if (!bytes || !bytes.length) return;
    uploadingImage = true;
    try {
      const result = await uploadImageBytes(bytes, "pasted_style_ref.png");
      generation.styleRefImage = result.name;
    } catch (e) {
      console.error("style ref paste failed:", e);
    } finally {
      uploadingImage = false;
    }
  }

  async function handleFileBrowse() {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      await uploadFile(file);
    };
    input.click();
  }

  async function uploadFile(file: File) {
    uploadingImage = true;
    try {
      const buf = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      const result = await uploadImageBytes(bytes, file.name);
      generation.styleRefImage = result.name;
      imagePreviewUrl = URL.createObjectURL(file);
    } catch (e) {
      console.error("style ref upload failed:", e);
    } finally {
      uploadingImage = false;
    }
  }

  function handleWebDrop(event: DragEvent) {
    event.preventDefault();
    const file = event.dataTransfer?.files?.[0];
    if (file && file.type.startsWith("image/")) {
      uploadFile(file);
    }
  }

  function clearImage() {
    generation.styleRefImage = null;
    imagePreviewUrl = null;
  }

  async function handleInstall() {
    installing = true;
    installError = null;
    try {
      await installCustomNode(IPADAPTER_GIT_URL, IPADAPTER_PACK_NAME);
      ipadapterAvailable = await checkNodeAvailable(IPADAPTER_CLASS).catch(() => null);
    } catch (e) {
      installError = `${e}`;
    } finally {
      installing = false;
    }
  }
</script>

<div class="space-y-3 text-xs">
  <!-- Enable toggle -->
  <label class="flex items-center gap-2 text-neutral-400 select-none cursor-pointer">
    <input
      type="checkbox"
      class="accent-indigo-500"
      checked={generation.styleRefEnabled}
      onchange={(e) => {
        generation.styleRefEnabled = (e.target as HTMLInputElement).checked;
        generation.saveSettings();
      }}
      title={locale.t("generation.style_ref.toggle")}
    />
    {locale.t("generation.style_ref.title")}
    <InfoTip text={locale.t("generation.style_ref.tip")} />
  </label>

  {#if generation.styleRefEnabled}
    {#if !supported}
      <!-- Unsupported family hint -->
      <div
        class="rounded-lg border border-amber-600/30 bg-amber-600/10 px-3 py-2 text-[11px] text-amber-300"
      >
        {locale.t("generation.style_ref.unsupported")}
      </div>
    {:else}
      <!-- Family badge -->
      <div class="text-[10px] font-medium uppercase tracking-wide text-neutral-500">
        {isRedux
          ? locale.t("generation.style_ref.redux_mode")
          : locale.t("generation.style_ref.ipadapter_mode")}
      </div>

      <!-- IP-Adapter install prompt (SD1.5 / SDXL only) -->
      {#if isIPAdapter && ipadapterAvailable === false}
        <div
          class="rounded-lg border border-amber-600/30 bg-amber-600/10 px-3 py-2 text-[11px] text-amber-300"
        >
          {locale.t("generation.style_ref.nodes_install")}
        </div>
        <div>
          <button
            class="text-indigo-400 transition-colors hover:text-indigo-300 disabled:opacity-50"
            disabled={installing}
            onclick={handleInstall}
          >
            {installing
              ? locale.t("generation.style_ref.installing")
              : locale.t("generation.style_ref.install")}
          </button>
          {#if installError}
            <div class="mt-1 text-[11px] text-red-400">
              {locale.t("generation.style_ref.install_error", { error: installError })}
            </div>
          {/if}
        </div>
      {/if}

      <!-- Image picker -->
      <div>
        <div class="mb-1 text-neutral-400">{locale.t("generation.style_ref.image")}</div>
        {#if generation.styleRefImage && imagePreviewUrl}
          <div class="relative mb-2">
            <img
              src={imagePreviewUrl}
              alt="Style reference"
              class="max-h-32 w-full rounded-lg object-cover"
            />
            <button
              class="absolute right-1 top-1 rounded bg-neutral-900/80 px-1.5 py-0.5 text-[10px] text-neutral-300 hover:text-white"
              onclick={clearImage}
            >
              Clear
            </button>
          </div>
        {:else if generation.styleRefImage}
          <div class="mb-2 truncate text-[11px] text-neutral-400">
            {generation.styleRefImage}
            <button
              class="ml-1 text-neutral-500 hover:text-neutral-300"
              onclick={clearImage}
            >
              [x]
            </button>
          </div>
        {/if}

        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          bind:this={dropZone}
          class="cursor-pointer select-none rounded-lg border border-dashed border-neutral-700 px-3 py-3 text-center text-neutral-500 transition-colors hover:border-neutral-500"
          onmouseenter={() => (pasteActive = true)}
          onmouseleave={() => (pasteActive = false)}
          ondragover={(e) => e.preventDefault()}
          ondrop={handleWebDrop}
          onclick={handleFileBrowse}
        >
          {#if uploadingImage}
            <span class="text-indigo-400">Uploading...</span>
          {:else}
            {locale.t("generation.style_ref.drop_image")}
            <span class="cursor-pointer text-indigo-400 underline"
              >{locale.t("generation.style_ref.upload")}</span
            >
          {/if}
        </div>
      </div>

      <!-- Strength -->
      <div>
        <div class="mb-1 text-neutral-400">
          {locale.t("generation.style_ref.strength")}
          <InfoTip text={locale.t("generation.style_ref.strength_tip")} />
          <span class="ml-1 text-neutral-300">{generation.styleRefStrength.toFixed(2)}</span>
        </div>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={generation.styleRefStrength}
          oninput={(e) => {
            generation.styleRefStrength = parseFloat((e.target as HTMLInputElement).value);
          }}
          onchange={() => generation.saveSettings()}
          class="w-full"
        />
      </div>

      <!-- Weight type -->
      <div>
        <div class="mb-1 text-neutral-400">
          {locale.t("generation.style_ref.weight_type")}
          <InfoTip
            text={isRedux
              ? locale.t("generation.style_ref.weight_type_tip_redux")
              : locale.t("generation.style_ref.weight_type_tip_ipadapter")}
          />
        </div>
        <select
          value={generation.styleRefWeightType}
          onchange={(e) => {
            generation.styleRefWeightType = (e.target as HTMLSelectElement).value;
            generation.saveSettings();
          }}
          class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-neutral-100 transition-colors focus:border-indigo-500 focus:outline-none"
        >
          {#each isRedux ? reduxWeightTypes : ipadapterWeightTypes as wt}
            <option value={wt}>{wt}</option>
          {/each}
        </select>
      </div>

      <!-- Start / End (IP-Adapter only) -->
      {#if isIPAdapter}
        <div class="grid grid-cols-2 gap-2">
          <div>
            <div class="mb-1 text-neutral-400">
              {locale.t("generation.style_ref.start")}
              <InfoTip text={locale.t("generation.style_ref.start_end_tip")} />
              <span class="ml-1 text-neutral-300">{generation.styleRefStart.toFixed(2)}</span>
            </div>
            <input
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={generation.styleRefStart}
              oninput={(e) => {
                generation.styleRefStart = parseFloat((e.target as HTMLInputElement).value);
              }}
              onchange={() => generation.saveSettings()}
              class="w-full"
            />
          </div>
          <div>
            <div class="mb-1 text-neutral-400">
              {locale.t("generation.style_ref.end")}
              <span class="ml-1 text-neutral-300">{generation.styleRefEnd.toFixed(2)}</span>
            </div>
            <input
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={generation.styleRefEnd}
              oninput={(e) => {
                generation.styleRefEnd = parseFloat((e.target as HTMLInputElement).value);
              }}
              onchange={() => generation.saveSettings()}
              class="w-full"
            />
          </div>
        </div>
      {/if}

      <!-- Flux Redux model hints -->
      {#if isRedux}
        <div class="space-y-1.5 text-[11px] text-neutral-500">
          <div>
            {locale.t("generation.style_ref.redux_hf_gated")}
          </div>
          <div>
            {locale.t("generation.style_ref.model_clip_vision_missing_redux")}
            <a href={SIGCLIP_URL} target="_blank" rel="noreferrer" class="ml-1 text-indigo-400 underline">
              {locale.t("generation.style_ref.download_clip_vision_redux")}
            </a>
          </div>
        </div>
      {/if}

      <!-- IP-Adapter model hints -->
      {#if isIPAdapter}
        <div class="space-y-1.5 text-[11px] text-neutral-500">
          <div>
            {locale.t(
              generation.modelFamily === "sd15"
                ? "generation.style_ref.model_ipadapter_sd15_missing"
                : "generation.style_ref.model_ipadapter_sdxl_missing",
            )}
            <a
              href={generation.modelFamily === "sd15" ? IPA_SD15_URL : IPA_SDXL_URL}
              target="_blank"
              rel="noreferrer"
              class="ml-1 text-indigo-400 underline"
            >
              {locale.t(
                generation.modelFamily === "sd15"
                  ? "generation.style_ref.download_ipadapter_sd15"
                  : "generation.style_ref.download_ipadapter_sdxl",
              )}
            </a>
          </div>
          <div>
            {locale.t("generation.style_ref.model_clip_vision_missing_ipadapter")}
            <a href={CLIP_VIT_H_URL} target="_blank" rel="noreferrer" class="ml-1 text-indigo-400 underline">
              {locale.t("generation.style_ref.download_clip_vision_ipadapter")}
            </a>
          </div>
        </div>
      {/if}
    {/if}
  {/if}
</div>
