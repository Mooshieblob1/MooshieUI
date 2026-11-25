<script lang="ts">
  /**
   * Vibe Transfer and Precise Reference.
   *
   * NovelAI rejects a request carrying both systems, so the store clears one
   * when the other gains an image and the payload builder enforces the same
   * precedence. This panel makes that visible rather than letting a populated
   * section be silently dropped: whichever list lost out is dimmed, with the
   * reason spelled out above both.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { savedCharacters } from "../../stores/savedCharacters.svelte.js";
  import { readClipboardImageSafe } from "../../utils/api.js";
  import { loadOutputImageForGenerationInput } from "../../utils/galleryActions.js";
  import {
    NAI_REF_OUTFIT,
    NOVELAI_MAX_VIBES,
    NOVELAI_MAX_DIRECTOR_REFERENCES,
    NOVELAI_OUTFIT_REFERENCE_OPTION,
    NOVELAI_REFERENCE_DESCRIPTIONS,
    novelAiReferenceBaseType,
    novelAiReferencePreferSameOutfit,
    novelAiReferenceSupportsSameOutfit,
    novelAiSetPreferSameOutfit,
  } from "../../utils/novelaiModels.js";
  import { fileToNovelAiBase64, novelAiBase64ToSrc } from "../../utils/novelaiImage.js";
  import { portal } from "../../utils/portal.js";
  import { VIBE_ENCODE_COST } from "../../utils/novelaiCost.js";
  import { CHECKERBOARD_CLASS } from "../../utils/transparentBackdrop.js";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import type { NovelAiVibe, OutputImage } from "../../types/index.js";

  const uid = $props.id();
  let busy = $state(false);
  let dragOverTarget = $state<"vibe" | "reference" | null>(null);
  let pasteTarget = $state<"vibe" | "reference" | null>(null);
  let pickerTarget = $state<"vibe" | "reference" | null>(null);
  let zoomedSrc = $state<string | null>(null);
  let refFileInput = $state<HTMLInputElement | null>(null);
  let vibeFileInput = $state<HTMLInputElement | null>(null);

  const vibes = $derived(generation.novelaiSettings.vibes);
  /**
   * Whether this vibe already has a token NovelAI will accept as is.
   *
   * A token is minted for one model at one extraction level, so moving
   * either invalidates it and the next generation pays for a fresh encode.
   * A vibe that arrived as a bare token has no image to re-encode from and
   * is always good.
   */
  const isEncoded = (vibe: NovelAiVibe) =>
    !!vibe.encoding &&
    (!vibe.image ||
      (vibe.encoded_model === generation.checkpoint &&
        vibe.encoded_information_extracted === vibe.information_extracted));
  const references = $derived(generation.novelaiSettings.director_references);
  const novelAiModel = $derived(generation.novelAiModel);
  const supportsOutfits = $derived(novelAiModel?.outfitReferences ?? false);
  const showFidelity = $derived(novelAiModel?.preciseReferenceFidelity ?? false);
  const strengthMax = $derived(novelAiModel?.preciseReferenceStrengthMax ?? 1);
  const inpaintUnsupported = $derived(
    references.some((r) => r.enabled !== false) &&
      generation.isNovelAiInfill &&
      !(novelAiModel?.preciseReferenceInpainting ?? false),
  );
  const hasUnsupportedOutfitRef = $derived(
    !supportsOutfits && references.some((r) => r.enabled !== false && r.description === NAI_REF_OUTFIT),
  );
  const hasActiveReference = $derived(references.some((r) => r.enabled !== false));
  const anySupported = $derived(
    generation.supportsNovelAiVibeTransfer || generation.supportsNovelAiPreciseReference,
  );
  const referenceFull = $derived(references.length >= NOVELAI_MAX_DIRECTOR_REFERENCES || busy);
  const vibeFull = $derived(vibes.length >= NOVELAI_MAX_VIBES || busy);
  const pickerMax = $derived(
    pickerTarget === "reference"
      ? Math.max(1, NOVELAI_MAX_DIRECTOR_REFERENCES - references.length)
      : Math.max(1, NOVELAI_MAX_VIBES - vibes.length),
  );

  async function addBlobs(blobs: readonly Blob[], target: "vibe" | "reference") {
    if (blobs.length === 0 || busy) return;
    busy = true;
    try {
      let addedCount = 0;
      let hadDecodeFailure = false;
      let duplicateCount = 0;
      let hitCap = false;
      for (const blob of blobs) {
        if (target === "vibe" && generation.novelaiSettings.vibes.length >= NOVELAI_MAX_VIBES) {
          hitCap = true;
          break;
        }
        if (
          target === "reference" &&
          generation.novelaiSettings.director_references.length >= NOVELAI_MAX_DIRECTOR_REFERENCES
        ) {
          hitCap = true;
          break;
        }
        const base64 = await fileToNovelAiBase64(blob);
        if (!base64) {
          hadDecodeFailure = true;
          continue;
        }
        if (target === "vibe") {
          generation.addNovelAiVibe(base64);
          addedCount += 1;
        } else {
          if (generation.novelaiSettings.director_references.some((r) => r.image === base64)) {
            duplicateCount += 1;
            continue;
          }
          generation.addNovelAiDirectorReference(base64);
          addedCount += 1;
        }
      }
      if (hitCap && target === "reference") {
        gallery.showToast(
          locale.t("generation.novelai.reference.toast_full", {
            max: String(NOVELAI_MAX_DIRECTOR_REFERENCES),
          }),
          "info",
        );
      } else if (duplicateCount > 0 && addedCount === 0) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_duplicate"), "info");
      } else if (hadDecodeFailure && addedCount === 0 && duplicateCount === 0 && target === "reference") {
        gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
      }
    } finally {
      busy = false;
    }
  }

  async function addFromFiles(files: FileList | null, target: "vibe" | "reference") {
    if (!files || files.length === 0) return;
    await addBlobs(Array.from(files), target);
  }

  async function addFromClipboard(target: "vibe" | "reference", clipboardItems?: DataTransferItemList) {
    if (clipboardItems) {
      for (const item of Array.from(clipboardItems)) {
        if (item.type.startsWith("image/")) {
          const file = item.getAsFile();
          if (file) {
            await addBlobs([file], target);
            return;
          }
        }
      }
    }
    try {
      const bytes = await readClipboardImageSafe();
      if (!bytes || bytes.length === 0) {
        gallery.showToast(locale.t("common.no_clipboard_image"), "info");
        return;
      }
      await addBlobs([new Blob([new Uint8Array(bytes)], { type: "image/png" })], target);
    } catch {
      gallery.showToast(locale.t("common.no_clipboard_image"), "info");
    }
  }

  async function addFromGallery(images: OutputImage[]) {
    const target = pickerTarget;
    pickerTarget = null;
    if (!target || images.length === 0 || busy) return;
    busy = true;
    try {
      const blobs: Blob[] = [];
      for (const image of images) {
        try {
          const { bytes } = await loadOutputImageForGenerationInput(image, "reference.png");
          blobs.push(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
        } catch (err) {
          console.error("Failed to load gallery image for NovelAI reference:", err);
        }
      }
      busy = false;
      if (blobs.length > 0) {
        await addBlobs(blobs, target);
      } else if (target === "reference") {
        gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
      }
    } finally {
      busy = false;
    }
  }

  async function handleTauriDrop(paths: string[], target: "vibe" | "reference") {
    if (paths.length === 0) return;
    try {
      const { readFile } = await import("@tauri-apps/plugin-fs");
      const files: File[] = [];
      for (const p of paths) {
        const bytes = await readFile(p);
        const name = p.split(/[\\/]/).pop() || "reference.png";
        files.push(new File([bytes], name));
      }
      await addBlobs(files, target);
    } catch (err) {
      console.error("NovelAI reference drop failed:", err);
      if (target === "reference") {
        gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
      }
    }
  }

  function tauriDropTarget(el: HTMLElement, target: "vibe" | "reference") {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<{ path?: string; paths?: string[] }>).detail;
      const paths = detail?.paths?.length ? detail.paths : detail?.path ? [detail.path] : [];
      if (paths.length > 0) void handleTauriDrop(paths, target);
    };
    el.addEventListener("tauri-file-drop", handler);
    return { destroy: () => el.removeEventListener("tauri-file-drop", handler) };
  }

  $effect(() => {
    const handler = async (event: ClipboardEvent) => {
      const target = pasteTarget;
      if (!target) return;
      if (target === "reference" && referenceFull) return;
      if (target === "vibe" && vibeFull) return;
      const el = event.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) {
        return;
      }
      event.preventDefault();
      event.stopImmediatePropagation();
      await addFromClipboard(target, event.clipboardData?.items);
    };
    window.addEventListener("paste", handler, { capture: true });
    return () => window.removeEventListener("paste", handler, { capture: true });
  });

  // Vibe Transfer and Precise Reference exclude each other; both tips say so
  // when the model offers both.
  function withExclusive(tip: string): string {
    const both =
      generation.supportsNovelAiVibeTransfer && generation.supportsNovelAiPreciseReference;
    return both ? `${tip}\n\n${locale.t("generation.novelai.references.exclusive")}` : tip;
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (zoomedSrc && e.key === "Escape") {
      e.preventDefault();
      zoomedSrc = null;
    }
  }}
/>

{#snippet uploader(
  target: "vibe" | "reference",
  full: boolean,
  onClickBrowse: () => void,
)}
  {@const isOver = dragOverTarget === target}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    data-drop-zone={target === "reference" ? "novelai-precise-ref" : "novelai-vibe"}
    use:tauriDropTarget={target}
    class="border border-dashed rounded-lg px-2.5 py-2 text-center transition-colors {isOver
      ? 'border-indigo-500 bg-indigo-500/10'
      : 'border-neutral-700 hover:border-neutral-600 bg-neutral-950/40'} {full
      ? 'opacity-40 pointer-events-none'
      : ''}"
    onmouseenter={() => { pasteTarget = target; }}
    onmouseleave={() => { if (pasteTarget === target) pasteTarget = null; }}
    onfocusin={() => { pasteTarget = target; }}
    onfocusout={() => { if (pasteTarget === target) pasteTarget = null; }}
    ondragenter={(e) => { e.preventDefault(); e.stopPropagation(); dragOverTarget = target; }}
    ondragover={(e) => { e.preventDefault(); e.stopPropagation(); dragOverTarget = target; }}
    ondragleave={() => { if (dragOverTarget === target) dragOverTarget = null; }}
    ondrop={(e) => {
      e.preventDefault();
      e.stopPropagation();
      dragOverTarget = null;
      void addFromFiles(e.dataTransfer?.files ?? null, target);
    }}
  >
    <div class="flex items-center justify-center gap-2 flex-wrap">
      <span class="text-[10px] text-neutral-500">{locale.t("generation.image.drag_drop_or")}</span>
      <button
        type="button"
        class="text-[11px] text-emerald-400/80 hover:text-emerald-300 transition-colors"
        onclick={() => void addFromClipboard(target)}
      >
        {locale.t("generation.image.ctrl_v_paste")}
      </button>
      <span class="text-neutral-700">|</span>
      <button
        type="button"
        class="text-[11px] text-neutral-300 hover:text-white transition-colors"
        onclick={onClickBrowse}
      >
        {locale.t("generation.image.select_image")}
      </button>
      <span class="text-neutral-700">|</span>
      <button
        type="button"
        class="text-[11px] text-indigo-400 hover:text-indigo-300 transition-colors"
        onclick={() => { pickerTarget = target; }}
      >
        {locale.t("generation.novelai.reference.from_gallery")}
      </button>
    </div>
  </div>
{/snippet}

<div class="space-y-4">
  {#if !anySupported}
    <div class="space-y-2">
      <span class="text-xs text-neutral-400">{locale.t("generation.novelai.references.title")}</span>
      <p class="text-[11px] text-amber-400/90">
        {locale.t("generation.novelai.references.unsupported")}
      </p>
    </div>
  {:else}
    {#if generation.supportsNovelAiPreciseReference}
      <div class="space-y-2 {vibes.length > 0 || inpaintUnsupported ? 'opacity-50' : ''}">
        <div class="flex items-center justify-between gap-2">
          <span class="text-xs text-neutral-400 flex items-center gap-1.5">
            {locale.t("generation.novelai.reference.title")}
            <InfoTip text={withExclusive(locale.t("generation.novelai.reference.tip"))} />
            {#if references.length > 0}
              <span class="text-[10px] tabular-nums text-neutral-500">
                {references.length}/{NOVELAI_MAX_DIRECTOR_REFERENCES}
              </span>
            {/if}
          </span>
          {#if references.length >= 2}
            <button
              type="button"
              class="px-1.5 py-0.5 text-[10px] rounded text-neutral-400 hover:text-red-300 hover:bg-neutral-800"
              onclick={() => generation.clearNovelAiDirectorReferences()}
            >
              {locale.t("generation.novelai.reference.clear_all")}
            </button>
          {/if}
        </div>

        {#if inpaintUnsupported}
          <p class="text-[11px] text-amber-400/90">
            {locale.t("generation.novelai.reference.inpaint_unsupported")}
          </p>
        {/if}
        {#if hasUnsupportedOutfitRef}
          <p class="text-[11px] text-amber-400/90">
            {locale.t("generation.novelai.reference.outfit_unsupported")}
          </p>
        {/if}

        {#if references.length < NOVELAI_MAX_DIRECTOR_REFERENCES}
          {@render uploader("reference", referenceFull, () => refFileInput?.click())}
        {/if}
        <input
          bind:this={refFileInput}
          type="file"
          accept="image/*"
          multiple
          class="hidden"
          onchange={(e) => {
            void addFromFiles(e.currentTarget.files, "reference");
            e.currentTarget.value = "";
          }}
        />

        {#each references as reference, index (index)}
          {@const baseDescription = novelAiReferenceBaseType(reference.description)}
          {@const refEnabled = reference.enabled !== false}
          {@const outfitSkipped = !supportsOutfits && reference.description === NAI_REF_OUTFIT}
          {@const sourceChar = reference.source_character_id
            ? savedCharacters.characters.find((c) => c.id === reference.source_character_id) ?? null
            : null}
          {@const strengthInputId = `${uid}-ref-strength-${index}`}
          {@const fidelityInputId = `${uid}-ref-fidelity-${index}`}
          <div
            class="rounded-lg border border-neutral-800 bg-neutral-900/60 p-2 flex gap-3 {!refEnabled || outfitSkipped
              ? 'opacity-50'
              : ''}"
          >
            <button
              type="button"
              class="shrink-0 w-16 h-16 rounded overflow-hidden border border-neutral-800 hover:border-indigo-500/70 transition-colors cursor-zoom-in {CHECKERBOARD_CLASS}"
              title={locale.t("generation.novelai.reference.zoom")}
              aria-label={locale.t("generation.novelai.reference.zoom")}
              onclick={() => { zoomedSrc = novelAiBase64ToSrc(reference.image); }}
            >
              <img
                src={novelAiBase64ToSrc(reference.image)}
                alt=""
                class="w-full h-full object-cover"
              />
            </button>
            <div class="flex-1 space-y-1.5 min-w-0">
              <div class="flex items-center gap-1.5">
                <input
                  type="checkbox"
                  class="accent-indigo-500 shrink-0"
                  checked={refEnabled}
                  aria-label={locale.t("generation.novelai.reference.enable")}
                  title={locale.t("generation.novelai.reference.enable")}
                  onchange={(e) =>
                    generation.updateNovelAiDirectorReference(index, {
                      enabled: e.currentTarget.checked,
                    })}
                />
                <select
                  class="flex-1 min-w-0 px-2 py-1 text-[11px] rounded-md bg-neutral-950 border border-neutral-800 text-neutral-200 focus:outline-none focus:border-indigo-600"
                  value={baseDescription}
                  onchange={(e) => {
                    const next = e.currentTarget.value;
                    if (next !== baseDescription) {
                      generation.updateNovelAiDirectorReference(index, { description: next });
                    }
                  }}
                >
                  {#each NOVELAI_REFERENCE_DESCRIPTIONS as option (option.value)}
                    <option value={option.value}>{locale.t(option.labelKey)}</option>
                  {/each}
                  {#if supportsOutfits || reference.description === NAI_REF_OUTFIT}
                    <option value={NOVELAI_OUTFIT_REFERENCE_OPTION.value}>
                      {locale.t(NOVELAI_OUTFIT_REFERENCE_OPTION.labelKey)}
                    </option>
                  {/if}
                </select>
                <button
                  type="button"
                  class="px-2 py-0.5 text-[11px] rounded-md text-neutral-400 hover:text-red-400 hover:bg-neutral-800 shrink-0"
                  onclick={() => generation.removeNovelAiDirectorReference(index)}
                >
                  {locale.t("generation.novelai.reference.remove")}
                </button>
              </div>
              {#if sourceChar}
                <div class="flex items-center gap-1.5">
                  <span
                    class="inline-block max-w-[12rem] truncate rounded border border-indigo-500/40 bg-indigo-500/15 px-1.5 py-px text-[10px] text-indigo-200"
                    title={locale.t("generation.novelai.reference.from_character_tip", {
                      name: sourceChar.name,
                    })}
                  >
                    {sourceChar.name}
                  </span>
                </div>
              {/if}
              {#if supportsOutfits && novelAiReferenceSupportsSameOutfit(reference.description)}
                <label class="flex items-center gap-1.5 text-[11px] text-neutral-300">
                  <input
                    type="checkbox"
                    class="accent-indigo-500"
                    checked={novelAiReferencePreferSameOutfit(reference.description)}
                    onchange={(e) =>
                      generation.updateNovelAiDirectorReference(index, {
                        description: novelAiSetPreferSameOutfit(
                          reference.description,
                          e.currentTarget.checked,
                        ),
                      })}
                  />
                  {locale.t("generation.novelai.reference.prefer_same_outfit")}
                  <InfoTip text={locale.t("generation.novelai.reference.prefer_same_outfit_tip")} />
                </label>
              {/if}
              <div class="space-y-0.5">
                <div class="flex items-center justify-between text-[11px] text-neutral-500">
                  <label for={strengthInputId}>
                    {locale.t("generation.novelai.reference.strength")}
                  </label>
                  <EditableValue
                    value={reference.strength}
                    min={-5}
                    max={10}
                    step={0.05}
                    decimals={2}
                    onchange={(v) =>
                      generation.updateNovelAiDirectorReference(index, { strength: v })}
                  />
                </div>
                <input
                  id={strengthInputId}
                  type="range"
                  min="0"
                  max={strengthMax}
                  step="0.05"
                  class="w-full accent-indigo-500"
                  value={Math.min(strengthMax, Math.max(0, reference.strength))}
                  oninput={(e) =>
                    generation.updateNovelAiDirectorReference(index, {
                      strength: Number(e.currentTarget.value),
                    })}
                />
              </div>
              {#if showFidelity}
                <div class="space-y-0.5">
                  <div class="flex items-center justify-between text-[11px] text-neutral-500">
                    <span>
                      <label for={fidelityInputId}>
                        {locale.t("generation.novelai.reference.fidelity")}
                      </label>
                      <InfoTip text={locale.t("generation.novelai.reference.fidelity_tip")} />
                    </span>
                    <EditableValue
                      value={reference.fidelity}
                      min={-1}
                      max={1}
                      step={0.05}
                      decimals={2}
                      onchange={(v) =>
                        generation.updateNovelAiDirectorReference(index, { fidelity: v })}
                    />
                  </div>
                  <input
                    id={fidelityInputId}
                    type="range"
                    min="0"
                    max="1"
                    step="0.05"
                    class="w-full accent-indigo-500"
                    value={Math.min(1, Math.max(0, reference.fidelity))}
                    oninput={(e) =>
                      generation.updateNovelAiDirectorReference(index, {
                        fidelity: Number(e.currentTarget.value),
                      })}
                  />
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}

    {#if generation.supportsNovelAiVibeTransfer}
      <div class="space-y-2 {hasActiveReference ? 'opacity-50' : ''}">
        <div class="flex items-center justify-between gap-2">
          <span class="text-xs text-neutral-400 flex items-center gap-1.5">
            {locale.t("generation.novelai.vibe.title")}
            <InfoTip text={withExclusive(locale.t("generation.novelai.vibe.tip"))} />
            {#if vibes.length > 0}
              <span class="text-[10px] tabular-nums text-neutral-500">
                {vibes.length}/{NOVELAI_MAX_VIBES}
              </span>
            {/if}
          </span>
        </div>

        {#if vibes.length < NOVELAI_MAX_VIBES}
          {@render uploader("vibe", vibeFull, () => vibeFileInput?.click())}
        {/if}
        <input
          bind:this={vibeFileInput}
          type="file"
          accept="image/*"
          multiple
          class="hidden"
          onchange={(e) => {
            void addFromFiles(e.currentTarget.files, "vibe");
            e.currentTarget.value = "";
          }}
        />

        <p class="text-[11px] text-neutral-500">
          {locale.t("generation.novelai.vibe.cost", { anlas: VIBE_ENCODE_COST })}
        </p>

        {#each vibes as vibe, index (index)}
          <div class="rounded-lg border border-neutral-800 bg-neutral-900/60 p-2 flex gap-3">
            {#if vibe.image}
              <img
                src={novelAiBase64ToSrc(vibe.image)}
                alt=""
                class="w-16 h-16 rounded object-cover border border-neutral-800 shrink-0"
              />
            {:else}
              <div
                class="w-16 h-16 rounded border border-neutral-800 shrink-0 flex items-center justify-center text-center text-[10px] text-neutral-600"
              >
                {locale.t("generation.novelai.vibe.encoded")}
              </div>
            {/if}
            <div class="flex-1 space-y-1.5 min-w-0">
              <div class="flex items-center justify-between gap-2">
                {#if isEncoded(vibe)}
                  <span
                    class="px-1.5 py-0.5 text-[10px] rounded bg-emerald-900/40 text-emerald-300 border border-emerald-800/60"
                  >
                    {locale.t("generation.novelai.vibe.encoded")}
                  </span>
                {:else}
                  <span></span>
                {/if}
                <button
                  class="px-2 py-0.5 text-[11px] rounded-md text-neutral-400 hover:text-red-400 hover:bg-neutral-800"
                  onclick={() => generation.removeNovelAiVibe(index)}
                >
                  {locale.t("generation.novelai.vibe.remove")}
                </button>
              </div>
              <label class="block text-[11px] text-neutral-500">
                {locale.t("generation.novelai.vibe.strength")}
                {vibe.strength.toFixed(2)}
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  class="w-full accent-indigo-500"
                  value={vibe.strength}
                  oninput={(e) =>
                    generation.updateNovelAiVibe(index, {
                      strength: Number(e.currentTarget.value),
                    })}
                />
              </label>
              <label class="block text-[11px] text-neutral-500">
                {locale.t("generation.novelai.vibe.information")}
                {vibe.information_extracted.toFixed(2)}
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  class="w-full accent-indigo-500"
                  value={vibe.information_extracted}
                  oninput={(e) =>
                    generation.updateNovelAiVibe(index, {
                      information_extracted: Number(e.currentTarget.value),
                    })}
                />
              </label>
            </div>
          </div>
        {/each}

        {#if vibes.length > 0}
          <label class="flex items-center gap-2 text-xs text-neutral-300">
            <input
              type="checkbox"
              class="accent-indigo-500"
              checked={generation.novelaiSettings.normalize_reference_strength}
              onchange={(e) =>
                generation.updateNovelAiSettings({
                  normalize_reference_strength: e.currentTarget.checked,
                })}
            />
            {locale.t("generation.novelai.vibe.normalize")}
            <InfoTip text={locale.t("generation.novelai.vibe.normalize_desc")} />
          </label>
        {/if}
      </div>
    {/if}
  {/if}
</div>

{#if pickerTarget !== null}
  <GalleryPickerModal
    open={true}
    multiple
    max={pickerMax}
    title={locale.t(
      pickerTarget === "reference"
        ? "generation.novelai.reference.gallery_pick_title"
        : "generation.novelai.vibe.gallery_pick_title",
    )}
    onselect={(images) => void addFromGallery(images)}
    onclose={() => { pickerTarget = null; }}
  />
{/if}

{#if zoomedSrc}
  <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
  <div
    use:portal
    class="fixed inset-0 z-210 bg-black/80 backdrop-blur-sm flex items-center justify-center p-4"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("generation.novelai.reference.zoom")}
    onclick={(e) => { if (e.target === e.currentTarget) zoomedSrc = null; }}
  >
    <div class="relative max-w-[90vw] max-h-[90vh]">
      <img
        src={zoomedSrc}
        alt=""
        class="max-w-[90vw] max-h-[85vh] rounded-lg border border-neutral-700 object-contain {CHECKERBOARD_CLASS}"
      />
      <button
        type="button"
        class="absolute top-2 right-2 w-7 h-7 flex items-center justify-center rounded-full bg-neutral-900/90 border border-neutral-700 text-neutral-300 hover:text-white"
        aria-label={locale.t("common.close")}
        onclick={() => { zoomedSrc = null; }}
      >✕</button>
    </div>
  </div>
{/if}
