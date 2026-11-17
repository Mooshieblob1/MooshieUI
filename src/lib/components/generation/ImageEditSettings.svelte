<script lang="ts">
  import { generation, EDIT_REFERENCE_SLOTS } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { uploadImageBytes, readClipboardImageSafe, downloadModel } from "../../utils/api.js";
  import { ipcListen } from "../../utils/ipc.js";
  import { onMount } from "svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import CameraAnglePicker from "./CameraAnglePicker.svelte";
  import { architectures } from "../../stores/architectures.svelte.js";
  import { findFamily, registeredMode, registeredSampling } from "../../utils/architectures.js";
  import {
    KREA2_EDIT_LORAS,
    KREA2_EDIT_METHODS,
    fitKrea2EditSize,
    installedKrea2EditLora,
    type Krea2EditMethod,
  } from "../../utils/krea2Edit.js";

  // Local object-URL previews, indexed by slot. Reference filenames live in the
  // store (generation.editReferenceImages); previews are lost across reloads, in
  // which case a stored filename still renders as a "set" slot without a thumbnail.
  let previews = $state<(string | null)[]>(Array(EDIT_REFERENCE_SLOTS).fill(null));
  let uploadingSlot = $state<number | null>(null);
  let dropSlot = $state<number | null>(null);
  let pasteSlot = $state<number | null>(null);
  let dropZone = $state<HTMLElement | null>(null);

  const slotCount = $derived(generation.editReferenceSlotCount);
  // Up to three slots are always shown. Beyond that (FLUX 3 Klein's ten), show
  // the filled ones plus one empty slot, so the panel grows as images are added.
  const visibleSlotCount = $derived.by(() => {
    if (slotCount <= 3) return slotCount;
    let lastFilled = -1;
    for (let i = 0; i < slotCount; i++) {
      if (generation.editReferenceImages[i] || previews[i]) lastFilled = i;
    }
    return Math.min(slotCount, Math.max(3, lastFilled + 2));
  });
  // A registered architecture's edit mode (src-tauri/src/architectures).
  const registeredEdit = $derived.by(() => {
    void architectures.version;
    const found = findFamily(generation.modelFamily);
    const mode = registeredMode(generation.modelFamily, "image_edit");
    return found && mode ? { label: found.family.label, arch: found.arch, mode } : null;
  });
  const familyBadge = $derived(
    registeredEdit
      ? registeredEdit.label
      : generation.isFlux3
      ? locale.t("generation.image_edit.family_badge.flux3klein")
      : generation.isQwenEditPlus
      ? locale.t("generation.image_edit.family_badge.qwen_edit_plus")
      : generation.isFluxKontext
        ? locale.t("generation.image_edit.family_badge.flux1kontext")
        : generation.isQwenEdit
          ? locale.t("generation.image_edit.family_badge.qwen_edit")
          : generation.isAnima
            ? locale.t("generation.image_edit.family_badge.anima")
            : generation.isKrea2
              ? locale.t("generation.image_edit.family_badge.krea2")
              : null,
  );

  // Anima ReStyler needs the Anima Edit LoRA; the workflow loads it by this exact
  // filename (ANIMA_EDIT_LORA_FILENAME in src-tauri/src/comfyui/nodes.rs).
  const animaEditLora = {
    url: "https://civitai.com/api/download/models/3089149",
    category: "loras",
    filename: "AnimeEditV2.safetensors",
  };
  // endsWith so a file nested in a subfolder still counts as installed.
  const animaLoraInstalled = $derived(
    models.loras.some((m) => m.endsWith(animaEditLora.filename)),
  );

  // Files the registered edit mode loads by fixed name (an edit LoRA) that
  // are not installed yet. endsWith so a file in a subfolder counts.
  const missingRequiredFiles = $derived(
    (registeredEdit?.mode.required_files ?? []).filter((file) => {
      const installed = file.category === "loras" ? models.loras : (models.serverModels[file.category] ?? []);
      return !installed.some((m) => m.split(/[\\/]/).pop()?.toLowerCase() === file.filename.toLowerCase());
    }),
  );
    async function downloadRequiredFile(file: { url: string; category: string; filename: string }) {
    loraDownloadError = null;
    loraDownloading = true;
    downloadingFilename = file.filename;
    try {
      await downloadModel(file.url, file.category, file.filename);
      await models.refresh();
    } catch (e) {
      loraDownloadError = locale.t("generation.image_edit.camera.download_failed", { error: String(e) });
    } finally {
      loraDownloading = false;
    }
  }

  // Krea 2 edits through the selected method's edit LoRA (utils/krea2Edit.ts).
  const krea2Lora = $derived(KREA2_EDIT_LORAS[generation.krea2EditMethod]);
  const krea2LoraInstalled = $derived(
    installedKrea2EditLora(models.loras, generation.krea2EditMethod) !== null,
  );
  /** Identity and General Edit run on the comfyui-krea2edit nodes and take the fidelity dials. */
  const krea2UsesPatch = $derived(generation.krea2EditMethod !== "style");
  let downloadingFilename = $state("");

  let loraDownloading = $state(false);
  let loraDownloadError = $state<string | null>(null);
  let dlBytes = $state(0);
  let dlTotal = $state(0);
  const dlPercent = $derived(dlTotal > 0 ? Math.round((dlBytes / dlTotal) * 100) : 0);

  async function downloadAnimaLora() {
    loraDownloadError = null;
    loraDownloading = true;
    downloadingFilename = animaEditLora.filename;
    try {
      await downloadModel(animaEditLora.url, animaEditLora.category, animaEditLora.filename);
      await models.refresh();
    } catch (e) {
      loraDownloadError = `Download failed: ${e}`;
    } finally {
      loraDownloading = false;
    }
  }

  async function downloadKrea2Lora() {
    const lora = krea2Lora;
    loraDownloadError = null;
    loraDownloading = true;
    downloadingFilename = lora.filename;
    try {
      await downloadModel(lora.url, "loras", lora.filename, undefined, lora.sha256);
      await models.refresh();
    } catch (e) {
      loraDownloadError = `Download failed: ${e}`;
    } finally {
      loraDownloading = false;
    }
  }

  onMount(async () => {
    await ipcListen("download:progress", (event: any) => {
      const data = event.payload as { downloaded: number; total: number; done: boolean };
      if (data.done) {
        dlBytes = 0;
        dlTotal = 0;
      } else {
        dlBytes = data.downloaded;
        dlTotal = data.total;
      }
    });
  });

  $effect(() => {
    const el = dropZone;
    if (!el) return;
    const onPaste = async (event: ClipboardEvent) => {
      if (pasteSlot === null) return;
      const target = event.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)
        return;
      event.preventDefault();
      event.stopImmediatePropagation();
      await handlePaste(pasteSlot);
    };
    window.addEventListener("paste", onPaste, { capture: true });
    return () => window.removeEventListener("paste", onPaste, { capture: true });
  });

  function setPreview(slot: number, url: string | null) {
    const current = previews[slot];
    if (current && current !== url) URL.revokeObjectURL(current);
    previews = previews.map((p, i) => (i === slot ? url : p));
  }

  /** Decode, optionally downscale to a sane max dimension, and report final size. */
  async function prepareImage(
    file: File,
    maxDimension = 1536,
  ): Promise<{ file: File; width: number; height: number }> {
    if (!file.type.startsWith("image/")) return { file, width: 0, height: 0 };

    const sourceUrl = URL.createObjectURL(file);
    try {
      const image = await new Promise<HTMLImageElement>((resolve, reject) => {
        const img = new Image();
        img.onload = () => resolve(img);
        img.onerror = () => reject(new Error("Failed to decode reference image"));
        img.src = sourceUrl;
      });

      const { width, height } = image;
      const longestSide = Math.max(width, height);
      if (!longestSide || longestSide <= maxDimension) {
        return { file, width, height };
      }

      const scale = maxDimension / longestSide;
      const targetWidth = Math.max(1, Math.round(width * scale));
      const targetHeight = Math.max(1, Math.round(height * scale));

      const canvasEl = document.createElement("canvas");
      canvasEl.width = targetWidth;
      canvasEl.height = targetHeight;
      const ctx = canvasEl.getContext("2d");
      if (!ctx) return { file, width, height };
      ctx.drawImage(image, 0, 0, targetWidth, targetHeight);

      const outputType = file.type === "image/png" ? "image/png" : "image/jpeg";
      const blob = await new Promise<Blob | null>((resolve) =>
        canvasEl.toBlob(resolve, outputType, outputType === "image/jpeg" ? 0.92 : undefined),
      );
      if (!blob) return { file, width, height };

      return {
        file: new File([blob], file.name, { type: outputType }),
        width: targetWidth,
        height: targetHeight,
      };
    } catch {
      return { file, width: 0, height: 0 };
    } finally {
      URL.revokeObjectURL(sourceUrl);
    }
  }

  /** Snap to a multiple of 16, which Qwen's SD3-style latent expects cleanly. */
  function snap16(value: number): number {
    return Math.max(16, Math.round(value / 16) * 16);
  }

  /**
   * A registered edit model samples at the reference's aspect, no larger than
   * the mode's own base size in pixels (MOSS-Image: 1 MP, as Comfy-Org's edit
   * template scales it), rounded to the model's size multiple.
   */
  function fitRegisteredEditSize(width: number, height: number): { width: number; height: number } | null {
    if (!registeredEdit) return null;
    const sampling = registeredSampling(generation.modelFamily, "image_edit");
    const step = Math.max(8, registeredEdit.arch.resolution_multiple || 16);
    const maxPixels = sampling ? sampling.width * sampling.height : width * height;
    const scale = Math.min(1, Math.sqrt(maxPixels / (width * height)));
    const snap = (v: number) => Math.max(step, Math.round((v * scale) / step) * step);
    return { width: snap(width), height: snap(height) };
  }

  async function uploadToSlot(slot: number, file: File) {
    if (!file.type.startsWith("image/")) return;
    const prepared = await prepareImage(file);
    setPreview(slot, URL.createObjectURL(prepared.file));
    uploadingSlot = slot;
    try {
      const buffer = await prepared.file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const result = await uploadImageBytes(bytes, prepared.file.name);
      generation.editReferenceImages = generation.editReferenceImages.map((v, i) =>
        i === slot ? result.name : v,
      );
      // Slot 0 owns the output geometry for Qwen (Kontext re-scales internally),
      // unless Lock resolution is on, which keeps the user's own size. Krea 2
      // edits stay at or under 2 MP, its trained range.
      if (slot === 0 && !generation.resolutionLocked && prepared.width > 0 && prepared.height > 0) {
        const size = registeredEdit
          ? (fitRegisteredEditSize(prepared.width, prepared.height) ?? {
              width: snap16(prepared.width),
              height: snap16(prepared.height),
            })
          : generation.isKrea2
            ? fitKrea2EditSize(prepared.width, prepared.height)
            : { width: snap16(prepared.width), height: snap16(prepared.height) };
        generation.width = size.width;
        generation.height = size.height;
      }
      generation.saveSettings();
    } catch (e) {
      console.error("Failed to upload reference image:", e);
      setPreview(slot, null);
      generation.editReferenceImages = generation.editReferenceImages.map((v, i) =>
        i === slot ? null : v,
      );
    } finally {
      uploadingSlot = null;
    }
  }

  async function handlePaste(slot: number) {
    const bytes = await readClipboardImageSafe();
    if (!bytes) return;
    const blob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
    const file = new File([blob], `reference_${slot + 1}.png`, { type: "image/png" });
    await uploadToSlot(slot, file);
  }

  function clearSlot(slot: number) {
    setPreview(slot, null);
    generation.editReferenceImages = generation.editReferenceImages.map((v, i) =>
      i === slot ? null : v,
    );
    generation.saveSettings();
  }

  // How Image Edit works, plus the selected family's own rules, behind one tip.
  const referenceTip = $derived(
    [
      locale.t("generation.image_edit.hint"),
      generation.isFlux3 ? locale.t("generation.image_edit.flux3_note") : null,
      registeredEdit && registeredEdit.mode.max_references > 1
        ? locale.t("generation.image_edit.registered_note", {
            model: registeredEdit.label,
            n: registeredEdit.mode.max_references,
          })
        : null,
      generation.isAnima ? locale.t("generation.image_edit.anima_note") : null,
      generation.isKrea2 && krea2UsesPatch
        ? locale.t("generation.image_edit.krea2_refs_note")
        : null,
      generation.isKrea2 && !krea2UsesPatch
        ? locale.t("generation.image_edit.krea2_style_note")
        : null,
    ]
      .filter(Boolean)
      .join(" "),
  );
</script>

<div class="space-y-3" bind:this={dropZone}>
  <div class="flex items-center justify-between gap-2">
    <label class="text-xs text-neutral-400">
      {locale.t("generation.image_edit.title")}
      <InfoTip text={locale.t("generation.image_edit.hint")} />
    </label>
    {#if familyBadge}
      <span
        class="shrink-0 rounded-full border border-indigo-700/60 bg-indigo-900/30 px-2 py-0.5 text-[10px] text-indigo-300"
      >
        {familyBadge}
      </span>
    {/if}
  </div>

  {#if !generation.supportsImageEditMode}
    <div
      class="rounded-lg border border-amber-700/50 bg-amber-900/20 px-3 py-2 text-xs text-amber-200"
    >
      {locale.t("generation.image_edit.wrong_model")}
    </div>
  {:else}
    {#each missingRequiredFiles as file (file.filename)}
      <button
        class="w-full rounded-lg bg-neutral-800 border border-neutral-700 px-3 py-2 text-xs text-neutral-100 hover:border-indigo-500 transition-colors disabled:opacity-50"
        disabled={loraDownloading}
        onclick={() => downloadRequiredFile(file)}
      >
        {locale.t("generation.image_edit.required_download", { file: file.filename })}{#if file.size}
          <span class="text-neutral-400"> ({file.size})</span>{/if}
      </button>
    {/each}
    {#if registeredEdit && loraDownloading}
      <div class="bg-neutral-800/80 rounded-lg px-3 py-2">
        <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
          <span class="truncate mr-2">
            {locale.t("generation.upscale.downloading", { model: downloadingFilename })}
          </span>
          {#if dlTotal > 0}
            <span class="shrink-0 tabular-nums">
              {locale.formatBytes(dlBytes)} / {locale.formatBytes(dlTotal)} ({dlPercent}%)
            </span>
          {/if}
        </div>
        {#if dlTotal > 0}
          <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
            <div
              class="bg-indigo-400 h-full rounded-full transition-[width] duration-300 ease-out"
              style="width: {dlPercent}%"
            ></div>
          </div>
        {/if}
      </div>
    {/if}
    {#if registeredEdit && loraDownloadError}
      <p class="text-[11px] text-red-400">{loraDownloadError}</p>
    {/if}
    {#if generation.isAnima}
      {#if !animaLoraInstalled}
        <button
          class="w-full rounded-lg bg-neutral-800 border border-neutral-700 px-3 py-2 text-xs text-neutral-100 hover:border-indigo-500 transition-colors disabled:opacity-50"
          disabled={loraDownloading}
          onclick={downloadAnimaLora}
        >
          {locale.t("generation.image_edit.anima_lora_download")}
        </button>
      {/if}
      {#if loraDownloading}
        <div class="bg-neutral-800/80 rounded-lg px-3 py-2">
          <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
            <span class="truncate mr-2">
              {locale.t("generation.upscale.downloading", { model: animaEditLora.filename })}
            </span>
            {#if dlTotal > 0}
              <span class="shrink-0 tabular-nums">
                {locale.formatBytes(dlBytes)} / {locale.formatBytes(dlTotal)} ({dlPercent}%)
              </span>
            {/if}
          </div>
          {#if dlTotal > 0}
            <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
              <div
                class="bg-indigo-400 h-full rounded-full transition-[width] duration-300 ease-out"
                style="width: {dlPercent}%"
              ></div>
            </div>
          {/if}
        </div>
      {/if}
      {#if loraDownloadError}
        <p class="text-[11px] text-red-400">{loraDownloadError}</p>
      {/if}

      <div>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>
            {locale.t("generation.image_edit.reference_strength")}
            <InfoTip text={locale.t("generation.image_edit.reference_strength_tip")} />
          </span>
          <EditableValue
            value={generation.editReferenceStrength}
            min={0}
            max={1}
            step={0.05}
            decimals={2}
            onchange={(v) => {
              generation.editReferenceStrength = v;
              generation.saveSettings();
            }}
          />
        </label>
        <input
          type="range"
          bind:value={generation.editReferenceStrength}
          onchange={() => generation.saveSettings()}
          min="0"
          max="1"
          step="0.05"
          class="w-full accent-indigo-500"
        />
      </div>

      <label class="flex items-center justify-between gap-3 text-xs text-neutral-300">
        <span class="leading-tight">
          {locale.t("generation.image_edit.split_screen")}
          <InfoTip text={locale.t("generation.image_edit.split_screen_tip")} />
        </span>
        <input
          type="checkbox"
          bind:checked={generation.editSplitScreen}
          onchange={() => generation.saveSettings()}
          class="accent-indigo-500 w-4 h-4 shrink-0"
        />
      </label>
    {/if}

    {#if generation.isKrea2}
      <label class="block text-xs text-neutral-400">
        <span class="flex items-center mb-1">
          {locale.t("generation.image_edit.krea2_method")}
          <InfoTip text={locale.t(`generation.image_edit.krea2_method_tip.${generation.krea2EditMethod}`)} />
        </span>
        <select
          class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-neutral-100 transition-colors focus:border-indigo-500 focus:outline-none"
          aria-label={locale.t("generation.image_edit.krea2_method")}
          value={generation.krea2EditMethod}
          onchange={(e) =>
            generation.setKrea2EditMethod(
              (e.currentTarget as HTMLSelectElement).value as Krea2EditMethod,
            )}
        >
          {#each KREA2_EDIT_METHODS as method (method)}
            <option value={method}>{locale.t(`generation.image_edit.krea2_method_${method}`)}</option>
          {/each}
        </select>
      </label>

      {#if !krea2LoraInstalled}
        <button
          class="w-full rounded-lg bg-neutral-800 border border-neutral-700 px-3 py-2 text-xs text-neutral-100 hover:border-indigo-500 transition-colors disabled:opacity-50"
          disabled={loraDownloading}
          onclick={downloadKrea2Lora}
        >
          {locale.t("generation.image_edit.krea2_lora_download", {
            size: locale.formatBytes(krea2Lora.bytes),
          })}
        </button>
      {/if}

      {#if krea2UsesPatch}
        <div>
          <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
            <span>
              {locale.t("generation.image_edit.krea2_ref_boost")}
              <InfoTip text={locale.t("generation.image_edit.krea2_ref_boost_tip")} />
            </span>
            <EditableValue
              value={generation.krea2EditRefBoostEffective}
              min={0.5}
              max={8}
              step={0.25}
              decimals={2}
              onchange={(v) => {
                generation.krea2EditRefBoost = v;
                generation.saveSettings();
              }}
            />
          </label>
          <input
            type="range"
            value={generation.krea2EditRefBoostEffective}
            oninput={(e) =>
              (generation.krea2EditRefBoost = Number((e.currentTarget as HTMLInputElement).value))}
            onchange={() => generation.saveSettings()}
            min="0.5"
            max="8"
            step="0.25"
            class="w-full accent-indigo-500"
          />
        </div>

        <label class="flex items-center justify-between gap-3 text-xs text-neutral-300">
          <span class="leading-tight">
            {locale.t("generation.image_edit.krea2_beside")}
            <InfoTip text={locale.t("generation.image_edit.krea2_beside_tip")} />
          </span>
          <input
            type="checkbox"
            bind:checked={generation.krea2EditBeside}
            onchange={() => generation.saveSettings()}
            class="accent-indigo-500 w-4 h-4 shrink-0"
          />
        </label>
      {/if}
    {/if}

    {#if loraDownloading && generation.isKrea2}
      <div class="bg-neutral-800/80 rounded-lg px-3 py-2">
        <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
          <span class="truncate mr-2">
            {locale.t("generation.upscale.downloading", { model: downloadingFilename })}
          </span>
          {#if dlTotal > 0}
            <span class="shrink-0 tabular-nums">
              {locale.formatBytes(dlBytes)} / {locale.formatBytes(dlTotal)} ({dlPercent}%)
            </span>
          {/if}
        </div>
        {#if dlTotal > 0}
          <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
            <div
              class="bg-indigo-400 h-full rounded-full transition-[width] duration-300 ease-out"
              style="width: {dlPercent}%"
            ></div>
          </div>
        {/if}
      </div>
    {/if}
    {#if loraDownloadError && generation.isKrea2}
      <p class="text-[11px] text-red-400">{loraDownloadError}</p>
    {/if}

    {#each Array(visibleSlotCount) as _, slot (slot)}
      {@const filename = generation.editReferenceImages[slot]}
      {@const preview = previews[slot]}
      <div>
        {#if slotCount > 1}
          <span class="text-xs text-neutral-400 block mb-1.5">
            {locale.t("generation.image_edit.reference_image_n", { n: slot + 1 })}{#if slot === 0}<InfoTip text={referenceTip} />{/if}
          </span>
        {:else}
          <span class="text-xs text-neutral-400 block mb-1.5">
            {locale.t("generation.image_edit.reference_image")}<InfoTip text={referenceTip} />
          </span>
        {/if}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          role="button"
          tabindex="0"
          class="relative rounded-lg border border-dashed bg-neutral-800/40 min-h-[100px] flex flex-col items-center justify-center p-3 transition-colors {dropSlot ===
          slot
            ? 'border-indigo-500 bg-indigo-500/10'
            : 'border-neutral-600 hover:border-neutral-500'}"
          onmouseenter={() => (pasteSlot = slot)}
          onmouseleave={() => {
            if (pasteSlot === slot) pasteSlot = null;
          }}
          ondragenter={(e) => {
            e.preventDefault();
            dropSlot = slot;
          }}
          ondragover={(e) => e.preventDefault()}
          ondragleave={() => {
            if (dropSlot === slot) dropSlot = null;
          }}
          ondrop={async (e) => {
            e.preventDefault();
            dropSlot = null;
            const file = e.dataTransfer?.files?.[0];
            if (file) await uploadToSlot(slot, file);
          }}
        >
          {#if preview || filename}
            {#if preview}
              <img src={preview} alt="" class="max-h-32 rounded object-contain mb-2" />
            {:else}
              <p class="text-[11px] text-neutral-500 mb-2">
                {locale.t("generation.image_edit.reference_set")}
              </p>
            {/if}
            <button
              type="button"
              class="text-xs text-red-400 hover:text-red-300"
              onclick={() => clearSlot(slot)}
            >
              {locale.t("common.remove")}
            </button>
          {:else}
            <p class="text-xs text-neutral-500 text-center">
              {locale.t("generation.image_edit.drop_hint")}
              <label class="text-indigo-400 hover:text-indigo-300 cursor-pointer ml-1">
                {locale.t("generation.image_edit.upload_prompt")}
                <input
                  type="file"
                  accept="image/*"
                  class="hidden"
                  onchange={async (e) => {
                    const file = (e.currentTarget as HTMLInputElement).files?.[0];
                    if (file) await uploadToSlot(slot, file);
                  }}
                />
              </label>
            </p>
          {/if}
          {#if uploadingSlot === slot}
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

    {#if generation.isQwenEditPlus}
      <CameraAnglePicker />
    {/if}
  {/if}
</div>
