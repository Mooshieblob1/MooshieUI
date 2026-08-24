<script lang="ts">
  /**
   * The NovelAI import dialog.
   *
   * Dropping or pasting a NovelAI image used to overwrite the whole panel on
   * the spot, which is destructive for anything half-written. This asks first,
   * the way novelai.net does: the three action buttons use the image as an
   * image, and the checkbox column decides which parts of its metadata get
   * written.
   *
   * The buttons act immediately and close, independently of the checkboxes,
   * because they are answers to "what do you want to do with this image?"
   * rather than extra things to import.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { novelaiImport } from "../../stores/novelaiImport.svelte.js";
  import {
    applyNovelAiSelection,
    ensureNovelAiReferenceModel,
    hasNovelAiCharacters,
    isNovelAiImg2ImgMetadata,
  } from "../../utils/metadataImport.js";
  import { fileToNovelAiBase64 } from "../../utils/novelaiImage.js";
  import InfoTip from "../ui/InfoTip.svelte";

  interface Props {
    /** Load the staged image into the img2img input. Owned by the page. */
    onImage2Image: (file: File) => Promise<void>;
  }

  let { onImage2Image }: Props = $props();

  const meta = $derived(novelaiImport.meta);
  const selection = $derived(novelaiImport.selection);
  const isImg2Img = $derived(!!meta && isNovelAiImg2ImgMetadata(meta));
  const hasCharacters = $derived(!!meta && hasNovelAiCharacters(meta));

  // Escape closes, wired only while the dialog is up.
  $effect(() => {
    if (!novelaiImport.isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !novelaiImport.busy) novelaiImport.close();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  function close() {
    if (!novelaiImport.busy) novelaiImport.close();
  }

  async function useAsImage2Image() {
    novelaiImport.busy = true;
    try {
      const file = await novelaiImport.imageFile();
      if (!file) {
        gallery.showToast(locale.t("metadata.toast.read_failed"), "error");
        return;
      }
      generation.mode = "img2img";
      await onImage2Image(file);
      novelaiImport.close();
    } catch (err) {
      console.error("NovelAI import: image2image failed", err);
      gallery.showToast(locale.t("metadata.toast.read_failed"), "error");
    } finally {
      novelaiImport.busy = false;
    }
  }

  async function useAsReference(kind: "vibe" | "precise") {
    novelaiImport.busy = true;
    try {
      const file = await novelaiImport.imageFile();
      if (!file) {
        gallery.showToast(locale.t("metadata.toast.read_failed"), "error");
        return;
      }
      // V5 has neither feature, so picking one moves the panel to V4.5 Full
      // before the image is attached, or the reference lists stay hidden.
      const switched = ensureNovelAiReferenceModel(kind);
      const base64 = await fileToNovelAiBase64(file);
      if (!base64) {
        gallery.showToast(locale.t("metadata.toast.read_failed"), "error");
        return;
      }
      if (kind === "vibe") generation.addNovelAiVibe(base64);
      else generation.addNovelAiDirectorReference(base64);
      if (switched) {
        gallery.showToast(locale.t("novelai.import.toast.switched_model"), "info");
      }
      novelaiImport.close();
    } catch (err) {
      console.error("NovelAI import: reference failed", err);
      gallery.showToast(locale.t("metadata.toast.read_failed"), "error");
    } finally {
      novelaiImport.busy = false;
    }
  }

  function importMetadata() {
    if (!meta) return;
    const applied = applyNovelAiSelection(meta, selection);
    if (applied.length > 0) {
      gallery.showToast(
        locale.t("metadata.toast.applied_all", { fields: applied.join(", ") }),
        "success",
      );
    } else {
      gallery.showToast(locale.t("metadata.toast.no_applicable"), "info");
    }
    novelaiImport.close();
  }

  const nothingSelected = $derived(
    !selection.prompt &&
      !selection.undesired &&
      !selection.characters &&
      !selection.clearCharacters &&
      !selection.settings &&
      !selection.seed,
  );
</script>

{#if novelaiImport.isOpen}
  <div
    class="fixed inset-0 z-200 flex items-center justify-center bg-black/80 p-4 backdrop-blur-sm"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("novelai.import.title")}
  >
    <button
      type="button"
      class="absolute inset-0 h-full w-full cursor-default"
      aria-label={locale.t("common.close")}
      onclick={close}
    ></button>

    <div
      class="relative z-10 flex max-h-[90vh] w-full max-w-sm flex-col overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-4 shadow-2xl"
    >
      <div class="mb-3 flex items-start justify-between gap-3">
        <h3 class="text-sm font-semibold text-neutral-100">
          {locale.t("novelai.import.title")}
        </h3>
        <button
          type="button"
          class="text-lg leading-none text-neutral-500 hover:text-neutral-200"
          onclick={close}
          aria-label={locale.t("common.close")}
        >✕</button>
      </div>

      {#if novelaiImport.previewUrl}
        <img
          src={novelaiImport.previewUrl}
          alt={novelaiImport.filename}
          class="mb-3 max-h-48 w-full rounded-lg border border-neutral-800 object-contain"
        />
      {/if}

      <div class="space-y-2">
        <button
          type="button"
          class="w-full rounded-lg bg-emerald-700 px-3 py-2 text-xs font-medium text-white hover:bg-emerald-600 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={novelaiImport.busy}
          onclick={useAsImage2Image}
        >
          {locale.t("novelai.import.action.img2img")}
        </button>
        <button
          type="button"
          class="w-full rounded-lg bg-emerald-700 px-3 py-2 text-xs font-medium text-white hover:bg-emerald-600 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={novelaiImport.busy}
          onclick={() => useAsReference("vibe")}
        >
          {locale.t("novelai.import.action.vibe")}
        </button>
        <button
          type="button"
          class="w-full rounded-lg bg-emerald-700 px-3 py-2 text-xs font-medium text-white hover:bg-emerald-600 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={novelaiImport.busy}
          onclick={() => useAsReference("precise")}
        >
          {locale.t("novelai.import.action.precise")}
        </button>
      </div>

      <p class="mt-4 text-[11px] text-neutral-400">
        {locale.t("novelai.import.has_metadata")}
      </p>

      {#if isImg2Img}
        <p class="mt-2 rounded border border-red-500/40 bg-red-500/10 px-2 py-1.5 text-[11px] text-red-300">
          {locale.t("novelai.import.img2img_warning")}
        </p>
      {/if}

      <div class="mt-3 space-y-1.5">
        <label class="flex items-center gap-2 text-xs text-neutral-300">
          <input
            type="checkbox"
            class="accent-emerald-500"
            checked={selection.prompt}
            onchange={(e) => novelaiImport.update({ prompt: e.currentTarget.checked })}
          />
          {locale.t("novelai.import.field.prompt")}
        </label>

        <label class="flex items-center gap-2 text-xs text-neutral-300">
          <input
            type="checkbox"
            class="accent-emerald-500"
            checked={selection.undesired}
            onchange={(e) => novelaiImport.update({ undesired: e.currentTarget.checked })}
          />
          {locale.t("novelai.import.field.undesired")}
        </label>

        <label
          class="flex items-center gap-2 text-xs {hasCharacters
            ? 'text-neutral-300'
            : 'text-neutral-600'}"
        >
          <input
            type="checkbox"
            class="accent-emerald-500"
            disabled={!hasCharacters}
            checked={selection.characters && hasCharacters}
            onchange={(e) => novelaiImport.update({ characters: e.currentTarget.checked })}
          />
          {locale.t("novelai.import.field.characters")}
        </label>

        <label
          class="ml-6 flex items-center gap-2 text-xs {hasCharacters && selection.characters
            ? 'text-neutral-300'
            : 'text-neutral-600'}"
        >
          <input
            type="checkbox"
            class="accent-emerald-500"
            disabled={!hasCharacters || !selection.characters}
            checked={selection.appendCharacters}
            onchange={(e) => novelaiImport.update({ appendCharacters: e.currentTarget.checked })}
          />
          {locale.t("novelai.import.field.append")}
          <InfoTip text={locale.t("novelai.import.field.append_tip")} />
        </label>

        <label class="ml-6 flex items-center gap-2 text-xs text-neutral-300">
          <input
            type="checkbox"
            class="accent-emerald-500"
            checked={selection.clearCharacters}
            onchange={(e) => novelaiImport.update({ clearCharacters: e.currentTarget.checked })}
          />
          {locale.t("novelai.import.field.clear")}
          <InfoTip text={locale.t("novelai.import.field.clear_tip")} />
        </label>

        <label class="flex items-center gap-2 text-xs text-neutral-300">
          <input
            type="checkbox"
            class="accent-emerald-500"
            checked={selection.settings}
            onchange={(e) => novelaiImport.update({ settings: e.currentTarget.checked })}
          />
          {locale.t("novelai.import.field.settings")}
        </label>

        <label class="flex items-center gap-2 text-xs text-neutral-300">
          <input
            type="checkbox"
            class="accent-emerald-500"
            checked={selection.seed}
            onchange={(e) => novelaiImport.update({ seed: e.currentTarget.checked })}
          />
          {locale.t("novelai.import.field.seed")}
        </label>

        <label class="flex items-center gap-2 text-xs text-neutral-300">
          <input
            type="checkbox"
            class="accent-emerald-500"
            checked={selection.clean}
            onchange={(e) => novelaiImport.update({ clean: e.currentTarget.checked })}
          />
          {locale.t("novelai.import.field.clean")}
          <InfoTip text={locale.t("novelai.import.field.clean_tip")} />
        </label>
      </div>

      <button
        type="button"
        class="mt-4 w-full rounded-lg bg-neutral-700 px-3 py-2 text-xs font-medium text-neutral-100 hover:bg-neutral-600 disabled:cursor-not-allowed disabled:opacity-50"
        disabled={novelaiImport.busy || nothingSelected}
        onclick={importMetadata}
      >
        {locale.t("novelai.import.submit")}
      </button>
    </div>
  </div>
{/if}
