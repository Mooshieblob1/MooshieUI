<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import {
    uploadImageBytes,
    readClipboardImageSafe,
    checkNodeAvailable,
    isCustomNodeInstalled,
  } from "../../utils/api.js";
  import { onMount } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  let untwistingAvailable = $state<boolean | null>(null);
  let scaleImageAvailable = $state<boolean | null>(null);
  let uploadingImage = $state(false);
  let imagePreviewUrl = $state<string | null>(null);
  let advancedOpen = $state(false);
  let dropZone = $state<HTMLElement | null>(null);
  let pasteActive = $state(false);

  const nodesReady = $derived(untwistingAvailable === true && scaleImageAvailable === true);
  const ANCESTRAL_SAMPLERS = ["euler_ancestral", "euler_a"];

  $effect(() => {
    const el = dropZone;
    if (!el) return;
    const onPaste = async (event: ClipboardEvent) => {
      if (!pasteActive || generation.styleReferenceImage) return;
      const target = event.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      await handleImagePaste();
    };
    window.addEventListener("paste", onPaste, { capture: true });
    return () => window.removeEventListener("paste", onPaste, { capture: true });
  });

  async function nodePackageAvailable(packageName: string, verifyNode: string): Promise<boolean> {
    try {
      const installed = await isCustomNodeInstalled(packageName);
      if (installed) return true;
      return await checkNodeAvailable(verifyNode);
    } catch {
      return false;
    }
  }

  onMount(async () => {
    try {
      [untwistingAvailable, scaleImageAvailable] = await Promise.all([
        nodePackageAvailable("ComfyUi-Untwisting-RoPE", "RFInversion"),
        nodePackageAvailable(
          "ComfyUi-Scale-Image-to-Total-Pixels-Advanced",
          "ImageScaleToTotalPixelsX",
        ),
      ]);
    } catch {
      untwistingAvailable = false;
      scaleImageAvailable = false;
    }
  });

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

  async function downscaleStyleReference(file: File, maxDimension = 1024): Promise<File> {
    if (!file.type.startsWith("image/")) return file;

    const sourceUrl = URL.createObjectURL(file);
    try {
      const image = await new Promise<HTMLImageElement>((resolve, reject) => {
        const img = new Image();
        img.onload = () => resolve(img);
        img.onerror = () => reject(new Error("Failed to decode style reference image"));
        img.src = sourceUrl;
      });

      const { width, height } = image;
      const longestSide = Math.max(width, height);
      if (!longestSide || longestSide <= maxDimension) {
        return file;
      }

      const scale = maxDimension / longestSide;
      const targetWidth = Math.max(1, Math.round(width * scale));
      const targetHeight = Math.max(1, Math.round(height * scale));

      const canvas = document.createElement("canvas");
      canvas.width = targetWidth;
      canvas.height = targetHeight;
      const ctx = canvas.getContext("2d");
      if (!ctx) return file;
      ctx.drawImage(image, 0, 0, targetWidth, targetHeight);

      const outputType = file.type === "image/png" ? "image/png" : "image/jpeg";
      const blob = await new Promise<Blob | null>((resolve) =>
        canvas.toBlob(resolve, outputType, outputType === "image/jpeg" ? 0.9 : undefined),
      );
      if (!blob) return file;

      return new File([blob], file.name, { type: outputType });
    } catch {
      return file;
    } finally {
      URL.revokeObjectURL(sourceUrl);
    }
  }

  async function handleImageFile(file: File) {
    if (!file.type.startsWith("image/")) return;
    const downscaled = await downscaleStyleReference(file);
    setPreview(downscaled);
    uploadingImage = true;
    try {
      const buffer = await downscaled.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const result = await uploadImageBytes(bytes, downscaled.name);
      generation.styleReferenceImage = result.name;
      generation.saveSettings();
    } catch (e) {
      console.error("Failed to upload style reference:", e);
      clearPreview();
      generation.styleReferenceImage = null;
    } finally {
      uploadingImage = false;
    }
  }

  async function handleImagePaste() {
    const bytes = await readClipboardImageSafe();
    if (!bytes) return;
    const originalBlob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
    const originalFile = new File([originalBlob], "style_reference.png", { type: "image/png" });
    const downscaled = await downscaleStyleReference(originalFile);

    uploadingImage = true;
    try {
      const buffer = await downscaled.arrayBuffer();
      const resizedBytes = Array.from(new Uint8Array(buffer));
      const result = await uploadImageBytes(resizedBytes, downscaled.name);
      generation.styleReferenceImage = result.name;
      setPreview(downscaled);
      generation.saveSettings();
    } catch (e) {
      console.error("Failed to upload style reference from paste:", e);
    } finally {
      uploadingImage = false;
    }
  }

  function clearReference() {
    generation.styleReferenceImage = null;
    clearPreview();
    generation.saveSettings();
  }

  function onToggleEnabled() {
    const next = !generation.styleTransferEnabled;
    generation.styleTransferEnabled = next;
    if (next) {
      generation.controlnetEnabled = false;
      generation.upscaleEnabled = false;
      generation.facefixEnabled = false;
      if (!ANCESTRAL_SAMPLERS.includes(generation.samplerName)) {
        generation.samplerName = "euler_ancestral";
      }
    }
    generation.saveSettings();
  }

  $effect(() => {
    if (generation.styleTransferEnabled && generation.controlnetEnabled) {
      generation.controlnetEnabled = false;
    }
  });
</script>

<div class="space-y-3">
  <div class="flex items-center justify-between">
    <label class="text-xs text-neutral-400">
      {locale.t("generation.style_transfer.title")}
      <InfoTip text={locale.t("generation.style_transfer.tip")} />
    </label>
    <button
      title={locale.t("generation.style_transfer.toggle")}
      class="relative w-10 h-5 rounded-full transition-colors {generation.styleTransferEnabled
        ? 'bg-indigo-600'
        : 'bg-neutral-700'}"
      onclick={onToggleEnabled}
      role="switch"
      aria-checked={generation.styleTransferEnabled}
      disabled={generation.mode !== "txt2img"}
    >
      <span
        class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {generation.styleTransferEnabled
          ? 'translate-x-5'
          : ''}"
      ></span>
    </button>
  </div>

  {#if generation.mode !== "txt2img"}
    <p class="text-[11px] text-amber-400/90">{locale.t("generation.style_transfer.txt2img_only")}</p>
  {/if}

  {#if generation.styleTransferEnabled && generation.mode === "txt2img"}
    <div class="rounded-lg border border-amber-700/50 bg-amber-900/20 px-3 py-2 text-xs text-amber-200">
      {locale.t("generation.style_transfer.wip_notice")}
    </div>

    {#if !nodesReady}
      <div
        class="bg-amber-900/30 border border-amber-700/50 rounded-lg px-3 py-2 text-xs text-amber-300 space-y-1"
      >
        <p>{locale.t("generation.style_transfer.nodes_install")}</p>
        <p>
          Missing nodes are installed automatically during backend startup. Wait a bit, then
          restart ComfyUI if they still do not appear.
        </p>
      </div>
    {/if}

    {#if generation.controlnetEnabled}
      <p class="text-[11px] text-amber-400/90">
        {locale.t("generation.style_transfer.conflicts_controlnet")}
      </p>
    {/if}
    {#if generation.upscaleEnabled}
      <p class="text-[11px] text-amber-400/90">
        {locale.t("generation.style_transfer.conflicts_upscale")}
      </p>
    {/if}

    {#if !ANCESTRAL_SAMPLERS.includes(generation.samplerName)}
      <p class="text-[11px] text-neutral-500">
        {locale.t("generation.style_transfer.ancestral_hint")}
      </p>
    {/if}

    <div>
      <span class="text-xs text-neutral-400 block mb-1.5">
        {locale.t("generation.style_transfer.reference_image")}
      </span>
      <div
        bind:this={dropZone}
        role="button"
        tabindex="0"
        class="relative rounded-lg border border-dashed border-neutral-600 bg-neutral-800/40 min-h-[100px] flex flex-col items-center justify-center p-3 transition-colors hover:border-neutral-500"
        onmouseenter={() => (pasteActive = true)}
        onmouseleave={() => (pasteActive = false)}
        ondragover={(e) => {
          e.preventDefault();
        }}
        ondrop={async (e) => {
          e.preventDefault();
          const file = e.dataTransfer?.files?.[0];
          if (file) await handleImageFile(file);
        }}
      >
        {#if imagePreviewUrl || generation.styleReferenceImage}
          {#if imagePreviewUrl}
            <img
              src={imagePreviewUrl}
              alt=""
              class="max-h-32 rounded object-contain mb-2"
            />
          {/if}
          <button
            type="button"
            class="text-xs text-red-400 hover:text-red-300"
            onclick={clearReference}
          >
            {locale.t("common.remove")}
          </button>
        {:else}
          <p class="text-xs text-neutral-500 text-center">
            {locale.t("generation.style_transfer.drop_image")}
            <label class="text-indigo-400 hover:text-indigo-300 cursor-pointer ml-1">
              {locale.t("generation.style_transfer.upload")}
              <input
                type="file"
                accept="image/*"
                class="hidden"
                onchange={async (e) => {
                  const file = (e.currentTarget as HTMLInputElement).files?.[0];
                  if (file) await handleImageFile(file);
                }}
              />
            </label>
          </p>
        {/if}
        {#if uploadingImage}
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

    <div>
      <label class="text-xs text-neutral-400 flex items-center gap-1 mb-1">
        {locale.t("generation.style_transfer.style_strength")}
        <InfoTip text={locale.t("generation.style_transfer.style_strength_tip")} />
        <span class="ml-auto tabular-nums text-neutral-500"
          >{generation.styleTransferLowScaleEnd.toFixed(2)}</span
        >
      </label>
      <input
        type="range"
        min="1"
        max="1.5"
        step="0.01"
        bind:value={generation.styleTransferLowScaleEnd}
        onchange={() => generation.saveSettings()}
        class="w-full accent-indigo-500"
      />
    </div>

    <div>
      <label class="text-xs text-neutral-400 flex items-center gap-1 mb-1">
        {locale.t("generation.style_transfer.structure_match")}
        <InfoTip text={locale.t("generation.style_transfer.structure_match_tip")} />
        <span class="ml-auto tabular-nums text-neutral-500"
          >{generation.styleTransferHighScaleStart.toFixed(2)}</span
        >
      </label>
      <input
        type="range"
        min="1"
        max="1.15"
        step="0.01"
        bind:value={generation.styleTransferHighScaleStart}
        onchange={() => generation.saveSettings()}
        class="w-full accent-indigo-500"
      />
    </div>

    <button
      type="button"
      class="text-xs text-neutral-400 hover:text-neutral-200 flex items-center gap-1"
      onclick={() => (advancedOpen = !advancedOpen)}
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-3 h-3 transition-transform {advancedOpen ? '' : '-rotate-90'}"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        ><polyline points="6 9 12 15 18 9" /></svg
      >
      {locale.t("generation.style_transfer.advanced")}
    </button>

    {#if advancedOpen}
      <div class="space-y-2 pl-1 border-l border-neutral-700">
        <div>
          <label class="text-xs text-neutral-400 flex items-center gap-1 mb-1">
            {locale.t("generation.style_transfer.megapixels")}
            <InfoTip text={locale.t("generation.style_transfer.megapixels_tip")} />
          </label>
          <input
            type="number"
            min="0.5"
            max="4"
            step="0.05"
            bind:value={generation.styleTransferMegapixels}
            onchange={() => generation.saveSettings()}
            class="w-full bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-xs text-neutral-200"
          />
        </div>
        <div>
          <label class="text-xs text-neutral-500 block mb-1" for="style-transfer-rf-mode">RF mode</label>
          <select
            id="style-transfer-rf-mode"
            bind:value={generation.styleTransferRfMode}
            onchange={() => generation.saveSettings()}
            class="w-full bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-xs text-neutral-200"
          >
            <option value="rf_gamma_rk2">rf_gamma_rk2</option>
            <option value="rf_gamma">rf_gamma</option>
            <option value="fireflow">fireflow</option>
            <option value="linear">linear</option>
          </select>
        </div>
        <div class="grid grid-cols-2 gap-2">
          <div>
            <label class="text-[10px] text-neutral-500" for="style-transfer-gamma">Gamma</label>
            <input
              id="style-transfer-gamma"
              type="number"
              min="0"
              max="1"
              step="0.05"
              bind:value={generation.styleTransferGamma}
              onchange={() => generation.saveSettings()}
              class="w-full bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-xs"
            />
          </div>
          <div>
            <label class="text-[10px] text-neutral-500" for="style-transfer-beta">Beta</label>
            <input
              id="style-transfer-beta"
              type="number"
              min="0.01"
              max="100"
              step="1"
              bind:value={generation.styleTransferBeta}
              onchange={() => generation.saveSettings()}
              class="w-full bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-xs"
            />
          </div>
        </div>
      </div>
    {/if}
  {/if}
</div>
