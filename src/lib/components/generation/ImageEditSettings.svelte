<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { uploadImageBytes, readClipboardImageSafe } from "../../utils/api.js";
  import InfoTip from "../ui/InfoTip.svelte";

  // Local object-URL previews, indexed by slot. Reference filenames live in the
  // store (generation.editReferenceImages); previews are lost across reloads, in
  // which case a stored filename still renders as a "set" slot without a thumbnail.
  let previews = $state<(string | null)[]>([null, null, null]);
  let uploadingSlot = $state<number | null>(null);
  let dropSlot = $state<number | null>(null);
  let pasteSlot = $state<number | null>(null);
  let dropZone = $state<HTMLElement | null>(null);

  const slotCount = $derived(generation.editReferenceSlotCount);
  const familyBadge = $derived(
    generation.isQwenEditPlus
      ? locale.t("generation.image_edit.family_badge.qwen_edit_plus")
      : generation.isFluxKontext
        ? locale.t("generation.image_edit.family_badge.flux1kontext")
        : generation.isQwenEdit
          ? locale.t("generation.image_edit.family_badge.qwen_edit")
          : null,
  );

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
      // Slot 0 owns the output geometry for Qwen (Kontext re-scales internally).
      if (slot === 0 && prepared.width > 0 && prepared.height > 0) {
        generation.width = snap16(prepared.width);
        generation.height = snap16(prepared.height);
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
    <p class="text-[11px] text-neutral-500">{locale.t("generation.image_edit.hint")}</p>

    {#each Array(slotCount) as _, slot (slot)}
      {@const filename = generation.editReferenceImages[slot]}
      {@const preview = previews[slot]}
      <div>
        {#if slotCount > 1}
          <span class="text-xs text-neutral-400 block mb-1.5">
            {locale.t("generation.image_edit.reference_image_n", { n: slot + 1 })}
          </span>
        {:else}
          <span class="text-xs text-neutral-400 block mb-1.5">
            {locale.t("generation.image_edit.reference_image")}
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
  {/if}
</div>
