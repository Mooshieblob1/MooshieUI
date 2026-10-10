<script lang="ts">
  /**
   * The Enhance modal: the NovelAI V5 rewrite, the general Enhance every other
   * image model uses, and the video (H3) rewrite (`naiEnhance.flow`). All share
   * the input stage. The general and H3 flows replace the old in-place Enhance
   * and the Compose modal: an empty box with "Edit current prompt" ticked
   * rewrites the prompt, text with it ticked edits the prompt, and text without
   * it writes a new one. Their review is a single before/after pair, since they
   * only touch the prompt box.
   *
   * What follows describes the V5 flow.
   *
   * Stage one takes what the user wants written. It is a blank box rather than
   * the prompt box's contents because V5 is asked for scenes as often as it is
   * asked to tidy up tags, and a prefilled box only offers the second.
   *
   * Stage two is a review gate. The other two enhance paths overwrite one
   * textarea and offer an undo, which is proportionate to what they change. A
   * V5 rewrite touches the base prompt, the undesired content and every
   * character box in one go, so nothing reaches the generation store until the
   * user confirms the rows.
   *
   * The `enhanceForNai` call lives here and not in the store: `naiEnhance` is a
   * feature store and may not import the prompt assistant.
   */
  import { naiEnhance, NAI_MAX_REFERENCES } from "../../stores/naiEnhance.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { enhancerSessions } from "../../stores/enhancerSessions.svelte.js";
  import { naiLanguageInfo, NAI_LANGUAGES, resolveNaiLanguage } from "../../utils/naiLanguage.js";
  import type { NaiLanguageChoice } from "../../utils/naiLanguage.js";
  import { NAI_VARIANT_BUDGET } from "../../utils/naiPrompt.js";
  import { NAI_UC_HEAVY, naiV5Variant } from "../../utils/novelaiModels.js";
  import { estimatePromptTokens } from "../../utils/promptTokens.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import { fileToNovelAiBase64, novelAiBase64ToSrc } from "../../utils/novelaiImage.js";
  import { readClipboardImageSafe } from "../../utils/api.js";
  import { buildH3Context } from "../../utils/h3Prompt.js";
  import { loadOutputImageForGenerationInput } from "../../utils/galleryActions.js";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import type { OutputImage } from "../../types/index.js";

  let fileInput = $state<HTMLInputElement | null>(null);
  let pickerOpen = $state(false);
  /** Set while a picked image is being decoded, so the buttons cannot stack. */
  let attaching = $state(false);

  /**
   * Downscale and attach whatever the three sources hand over.
   *
   * All of them funnel through `fileToNovelAiBase64`, the same in-browser
   * canvas pass the NovelAI reference fields use: a 4 MB phone photo becomes a
   * ~1024px PNG before it ever reaches IPC, and Rust downscales again to its own
   * pixel budget. Doing it here as well is not redundant, it is what keeps the
   * request body from carrying four full-size images across the wire.
   */
  async function attach(blobs: Blob[]) {
    if (blobs.length === 0 || attaching) return;
    attaching = true;
    try {
      const encoded: string[] = [];
      for (const blob of blobs.slice(0, naiEnhance.referenceSlotsLeft)) {
        const base64 = await fileToNovelAiBase64(blob);
        if (base64) encoded.push(base64);
      }
      if (encoded.length === 0) {
        gallery.showToast(locale.t("prompt_assistant.nai_reference_failed"), "error");
        return;
      }
      naiEnhance.addReferences(encoded);
    } catch (e) {
      console.error("Reference image attach failed:", e);
      gallery.showToast(locale.t("prompt_assistant.nai_reference_failed"), "error");
    } finally {
      attaching = false;
    }
  }

  async function attachFromFiles(files: FileList | null) {
    await attach(files ? Array.from(files) : []);
  }

  async function attachFromClipboard() {
    try {
      const bytes = await readClipboardImageSafe();
      if (!bytes || bytes.length === 0) {
        gallery.showToast(locale.t("common.no_clipboard_image"), "error");
        return;
      }
      await attach([new Blob([new Uint8Array(bytes)], { type: "image/png" })]);
    } catch (e) {
      console.error("Reference image paste failed:", e);
      gallery.showToast(locale.t("common.no_clipboard_image"), "error");
    }
  }

  /**
   * Gallery entries are JXL on disk, so they go through the same PNG decode the
   * generation inputs use rather than being read off the drive directly.
   */
  async function attachFromGallery(images: OutputImage[]) {
    try {
      const blobs: Blob[] = [];
      for (const image of images.slice(0, naiEnhance.referenceSlotsLeft)) {
        const { bytes } = await loadOutputImageForGenerationInput(image);
        blobs.push(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
      }
      await attach(blobs);
    } catch (e) {
      console.error("Reference image load from gallery failed:", e);
      gallery.showToast(locale.t("prompt_assistant.nai_reference_failed"), "error");
    }
  }

  const pending = $derived(naiEnhance.pending);
  /**
   * The variant the rewrite is written for. "Curate it" switches the model to
   * V5 Full on apply, so the rewrite targets Full's budget from the start.
   */
  const variant = $derived.by(() => {
    const v = naiV5Variant(generation.checkpoint);
    return v && naiEnhance.curate ? "full" : v;
  });
  const isGeneral = $derived(naiEnhance.flow === "general");
  const isH3 = $derived(naiEnhance.flow === "h3");
  /** The flows reviewed as one before/after pair rather than the V5 diff. */
  const isSingleRow = $derived(naiEnhance.flow !== "nai");

  const languageLabel = $derived(pending ? naiLanguageInfo(pending.language).label : "");

  const sessionFlow = $derived(isH3 ? "h3" : isGeneral ? "enhance" : "nai");
  const sessionCount = $derived(enhancerSessions.count(sessionFlow));

  function newSession() {
    if (!confirm(locale.t("prompt_assistant.session_clear_confirm", { count: sessionCount }))) return;
    enhancerSessions.clear(sessionFlow);
  }

  // General flow only. Not persisted, matching the Compose modal it replaces.
  let length = $state<"short" | "medium" | "detailed">("medium");
  let includeArtists = $state(false);

  /**
   * The general flow has a request to make when there is text, an image, or a
   * current prompt to polish. The V5 flow does not take the last one: it
   * always needs to be told what to write.
   */
  const canRun = $derived(
    naiEnhance.input.trim() !== "" ||
      naiEnhance.references.length > 0 ||
      (isSingleRow &&
        generation.naiEnhanceIncludeExisting &&
        !!generation.positivePrompt?.trim()),
  );

  /**
   * The whole selection, counted the way NovelAI counts it.
   *
   * Per-row bars answer "is this box heavy"; this answers the only question
   * that decides whether the image renders, because the budget is a property of
   * the request and not of any one field.
   */
  const selectedTokens = $derived(estimatePromptTokens(naiEnhance.selectedText));

  // The rewrite is written for one variant's budget and syntax, so a model
  // switch while a result is on screen invalidates it. The input stage survives:
  // what the user typed is worth just as much under the other variant.
  $effect(() => {
    if (
      naiEnhance.flow === "nai" &&
      naiEnhance.stage === "review" &&
      naiEnhance.pending?.variant !== variant
    ) {
      naiEnhance.dismiss();
    }
  });

  async function runGeneral() {
    const text = naiEnhance.input.trim();
    const refs = naiEnhance.references.map((r) => ({ ...r }));
    if (!canRun || naiEnhance.busy) return;
    naiEnhance.busy = true;
    try {
      const result = await promptAssistant.rewrite({
        text,
        existing: generation.naiEnhanceIncludeExisting ? (generation.positivePrompt ?? "") : null,
        family: generation.modelFamily,
        opts: {
          length,
          include_artists: includeArtists,
          reference_labels: refs.map((r) => r.label),
        },
        images: refs.map((r) => r.base64),
      });
      // Cancelled while waiting: drop the late answer, as the V5 flow does.
      if (naiEnhance.stage !== "input") return;
      if (!result.prompt.trim()) {
        gallery.showToast(locale.t("prompt_assistant.couldnt_enhance"), "error");
        return;
      }
      enhancerSessions.record("enhance", result.sessionUser, result.prompt);
      naiEnhance.showGeneralReview({
        before: generation.positivePrompt ?? "",
        after: result.prompt,
      });
    } catch (e) {
      console.error("Prompt enhance failed:", e);
      gallery.showToast(mapLlmError(String(e)), "error");
    } finally {
      naiEnhance.busy = false;
    }
  }

  async function runH3() {
    const text = naiEnhance.input.trim();
    const refs = naiEnhance.references.map((r) => ({ base64: r.base64, label: r.label }));
    if (!canRun || naiEnhance.busy) return;
    naiEnhance.busy = true;
    try {
      const result = await promptAssistant.enhanceForH3(
        text,
        buildH3Context({
          variant: generation.videoVariant,
          frames: generation.videoFrameLength,
          hasFirstFrame: !!generation.videoFirstFrame,
          hasLastFrame: !!generation.videoEffectiveLastFrame,
          referenceImageCount: generation.videoRefImageFilenames.length,
        }),
        generation.videoFirstFrame,
        {
          existing: generation.naiEnhanceIncludeExisting ? (generation.positivePrompt ?? "") : null,
          references: refs,
        },
      );
      if (naiEnhance.stage !== "input") return;
      if (!result.text) {
        gallery.showToast(locale.t("prompt_assistant.couldnt_enhance"), "error");
        return;
      }
      // A near-miss is offered but not remembered: the session keeps only
      // answers that passed the format check.
      if (result.ok) enhancerSessions.record("h3", result.sessionUser, result.text);
      naiEnhance.showGeneralReview({
        before: generation.positivePrompt ?? "",
        after: result.text,
        // A near-miss is still a better start than the prose it replaces, so it
        // is offered with the broken rule named. Idle mode is invisible until it
        // fires, so it is named too, or "the pose never changes" reads as a bug.
        notice: !result.ok
          ? {
              kind: "warning",
              text: locale.t("prompt_assistant.h3_format_warning", { rule: result.rule ?? "" }),
            }
          : result.idle
            ? { kind: "info", text: locale.t("prompt_assistant.h3_idle_applied") }
            : null,
      });
    } catch (e) {
      console.error("H3 prompt rewrite failed:", e);
      gallery.showToast(mapLlmError(String(e)), "error");
    } finally {
      naiEnhance.busy = false;
    }
  }

  async function run() {
    if (naiEnhance.flow === "general") return runGeneral();
    if (naiEnhance.flow === "h3") return runH3();
    const text = naiEnhance.input.trim();
    const v = variant;
    // Snapshotted before the await: the labels sent in the manifest line have to
    // be the ones that match the images sent alongside them, and the modal is
    // still editable while the request is in flight.
    const refs = naiEnhance.references.map((r) => ({ ...r }));
    // Images alone are a request. An attached picture with no typed instruction
    // means "do this", and the prompt builder supplies that verb, so the only
    // thing this guard still refuses is a turn with nothing in it at all.
    if ((!text && refs.length === 0) || !v || naiEnhance.busy) return;
    const language = resolveNaiLanguage(generation.naiEnhanceLanguage, text);
    const boxes = generation.novelaiSettings.characters;
    naiEnhance.busy = true;
    try {
      const result = await promptAssistant.enhanceForNai(text, {
        variant: v,
        // Curate it applies the Heavy preset, so the rewrite skips what it covers.
        ucPreset: naiEnhance.curate ? NAI_UC_HEAVY : generation.novelaiSettings.uc_preset,
        characterCount: boxes.length,
        // Ticked, what the user typed is applied as an edit to these fields
        // rather than treated as the whole of the idea.
        existing: generation.naiEnhanceIncludeExisting
          ? {
              base: generation.positivePrompt ?? "",
              uc: generation.negativePrompt ?? "",
              characters: boxes.map((c) => c.prompt ?? ""),
            }
          : null,
        language,
        references: refs.map((r) => r.label),
      }, refs.map((r) => r.base64));
      // Cancelling closes the modal but cannot recall the request, so a late
      // answer to a dismissed one is dropped rather than popped back up.
      if (naiEnhance.stage !== "input") return;
      if (!result.parsed.base.trim()) {
        gallery.showToast(locale.t("prompt_assistant.couldnt_enhance"), "error");
        return;
      }
      // Recorded once on screen, applied or not: the answer is part of the
      // conversation either way. Not when it failed validation, and not when
      // the modal was cancelled (the early return above).
      if (result.problems.length === 0) {
        enhancerSessions.record("nai", result.sessionUser, result.raw);
      }
      naiEnhance.showReview({
        variant: v,
        language,
        budget: NAI_VARIANT_BUDGET[v],
        note: result.parsed.note,
        problems: result.problems,
        // Every row starts ticked: the rewrite is one coherent answer, and
        // half-applying it leaves a base prompt describing characters that the
        // character boxes no longer match.
        base: { before: generation.positivePrompt, after: result.parsed.base, selected: true },
        uc: { before: generation.negativePrompt, after: result.parsed.uc, selected: true },
        characters: [
          ...result.parsed.characters.map((after, i) => ({
            before: boxes[i]?.prompt ?? "",
            after,
            selected: true,
            targetIndex: i < boxes.length ? i : null,
            removes: false,
          })),
          // Boxes the rewrite came back short of. The base prompt it wrote
          // describes the cast it returned, so a box left standing puts a
          // character in the image that nothing asked for -- the duplicate the
          // user sees after asking for a smaller cast. Offered as a ticked
          // removal row rather than cleared behind their back, and only for
          // boxes with something in them: an already blank slot sends nothing.
          ...boxes
            .map((box, i) => ({ box, i }))
            .slice(result.parsed.characters.length)
            .filter(({ box }) => (box.prompt ?? "").trim() !== "")
            .map(({ box, i }) => ({
              before: box.prompt ?? "",
              after: "",
              selected: true,
              targetIndex: i,
              removes: true,
            })),
        ],
      });
    } catch (e) {
      console.error("NovelAI V5 prompt rewrite failed:", e);
      gallery.showToast(mapLlmError(String(e)), "error");
    } finally {
      naiEnhance.busy = false;
    }
  }

  function close() {
    naiEnhance.dismiss();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      close();
      return;
    }
    // Ctrl+Enter confirms, wherever focus sits inside the modal -- so it works
    // after clicking a variant button, not only from the textarea.  Only at
    // the input stage: at the review stage there is nothing to submit, and
    // re-running the rewrite would spend another request on a press meant for
    // the panel underneath.
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && naiEnhance.stage === "input") {
      e.preventDefault();
      void run();
    }
  }
</script>

<svelte:window onkeydown={naiEnhance.isOpen ? onKeydown : undefined} />

{#snippet tokenBar(text: string, budget: number)}
  {@const tokens = estimatePromptTokens(text)}
  {@const ratio = Math.min(1, tokens / budget)}
  {@const over = tokens > budget}
  <div class="mt-1 flex items-center gap-2">
    <div class="h-1 flex-1 overflow-hidden rounded-full bg-neutral-800">
      <div
        class="h-full rounded-full transition-all {over ? 'bg-amber-400' : 'bg-indigo-500'}"
        style="width: {ratio * 100}%"
      ></div>
    </div>
    <span class="shrink-0 tabular-nums text-[10px] {over ? 'text-amber-400' : 'text-neutral-500'}">
      {tokens}/{budget}
    </span>
  </div>
{/snippet}

{#snippet row(
  label: string,
  before: string,
  after: string,
  selected: boolean,
  budget: number,
  toggle: () => void,
  hint: string,
)}
  <div
    class="rounded-lg border p-3 transition-colors {selected
      ? 'border-indigo-500/60 bg-neutral-800/40'
      : 'border-neutral-700 bg-neutral-900'}"
  >
    <label class="flex cursor-pointer items-center gap-2">
      <input type="checkbox" checked={selected} onchange={toggle} class="accent-indigo-500" />
      <span class="text-xs font-medium text-neutral-200">{label}</span>
      {#if hint}
        <span class="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-400">{hint}</span>
      {/if}
    </label>
    <div class="mt-2 grid gap-2 sm:grid-cols-2">
      <div>
        <div class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">
          {locale.t("prompt_assistant.nai_before")}
        </div>
        <div
          class="max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-neutral-950 p-2 text-[11px] text-neutral-500"
        >
          {before.trim() || locale.t("prompt_assistant.nai_empty")}
        </div>
      </div>
      <div>
        <div class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">
          {locale.t("prompt_assistant.nai_after")}
        </div>
        <div
          class="max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-neutral-950 p-2 text-[11px] text-neutral-200"
        >
          {after.trim() || locale.t("prompt_assistant.nai_empty")}
        </div>
        {@render tokenBar(after, budget)}
      </div>
    </div>
  </div>
{/snippet}

{#if naiEnhance.isOpen}
  <div
    data-modal-open
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 sm:p-8"
    onclick={(e) => {
      if (e.currentTarget === e.target) close();
    }}
    role="presentation"
  >
    <!-- App wide rather than panel wide: the review is a two column diff of up
         to six fields, and the input box is easier to judge at the width the
         prompt will actually be read at. -->
    <div
      class="flex max-h-[90vh] w-full max-w-[1400px] flex-col overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-label={isH3
        ? locale.t("prompt_assistant.enhance_h3")
        : isGeneral
          ? locale.t("prompt_assistant.general_input_title")
          : locale.t("prompt_assistant.nai_input_title")}
    >
      <div class="mb-3 flex items-start justify-between gap-3">
        <div>
          <h2 class="text-sm font-semibold text-neutral-100">
            {#if isSingleRow}
              {naiEnhance.stage === "review"
                ? locale.t("prompt_assistant.general_review_title")
                : isH3
                  ? locale.t("prompt_assistant.enhance_h3")
                  : locale.t("prompt_assistant.general_input_title")}
            {:else}
              {naiEnhance.stage === "review"
                ? locale.t("prompt_assistant.nai_review_title")
                : locale.t("prompt_assistant.nai_input_title")}
            {/if}
          </h2>
          <p class="mt-0.5 text-[11px] text-neutral-400">
            {#if isSingleRow}
              {naiEnhance.stage === "review"
                ? locale.t("prompt_assistant.general_review_subtitle")
                : isH3
                  ? locale.t("prompt_assistant.h3_input_subtitle")
                  : locale.t("prompt_assistant.general_input_subtitle")}
            {:else if naiEnhance.stage === "review" && pending}
              {locale.t("prompt_assistant.nai_review_subtitle", {
                variant:
                  pending.variant === "curated"
                    ? locale.t("prompt_assistant.nai_variant_curated")
                    : locale.t("prompt_assistant.nai_variant_full"),
                language: languageLabel,
              })}
            {:else}
              {locale.t("prompt_assistant.nai_input_subtitle")}
            {/if}
          </p>
          {#if sessionCount > 0}
            <p class="mt-0.5 text-[10px] text-neutral-500">
              {locale.t("prompt_assistant.session_count", { count: sessionCount })}
              {#if enhancerSessions.dropped[sessionFlow] > 0}
                · {locale.t("prompt_assistant.session_trimmed")}
              {/if}
            </p>
          {/if}
        </div>
        <div class="flex items-center gap-1.5">
          {#if sessionCount > 0}
            <button
              class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
              disabled={naiEnhance.busy}
              onclick={newSession}
            >
              ⟲ {locale.t("prompt_assistant.session_new")}
            </button>
          {/if}
          <button
            class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
            onclick={close}
          >
            ✕
          </button>
        </div>
      </div>

      {#if naiEnhance.stage === "input"}
        <textarea
          class="min-h-[9rem] w-full flex-1 resize-y rounded-lg border border-neutral-700 bg-neutral-950 p-3 text-xs text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
          placeholder={sessionCount > 0
            ? locale.t("prompt_assistant.session_continue_placeholder")
            : isH3
              ? locale.t("prompt_assistant.h3_input_placeholder")
              : isGeneral
                ? locale.t("prompt_assistant.general_input_placeholder")
                : locale.t("prompt_assistant.nai_input_placeholder")}
          aria-label={isH3
            ? locale.t("prompt_assistant.enhance_h3")
            : isGeneral
              ? locale.t("prompt_assistant.general_input_title")
              : locale.t("prompt_assistant.nai_input_title")}
          disabled={naiEnhance.busy}
          bind:value={naiEnhance.input}
        ></textarea>

        <!-- Below the box, not above it: what the user types is the request,
             and the images are what the request points at. -->
        <div class="mt-3 rounded-lg border border-neutral-800 bg-neutral-950/60 p-2.5">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-[11px] font-medium text-neutral-300">
              {locale.t("prompt_assistant.nai_references")}
            </span>
            <span class="text-[10px] tabular-nums text-neutral-500">
              {naiEnhance.references.length}/{NAI_MAX_REFERENCES}
            </span>
            <div class="ml-auto flex items-center gap-1.5">
              <button
                class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
                disabled={naiEnhance.busy || attaching || !naiEnhance.canAddReference}
                onclick={() => fileInput?.click()}
              >
                {locale.t("prompt_assistant.nai_reference_upload")}
              </button>
              <button
                class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
                disabled={naiEnhance.busy || attaching || !naiEnhance.canAddReference}
                onclick={attachFromClipboard}
              >
                {locale.t("prompt_assistant.nai_reference_paste")}
              </button>
              <button
                class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
                disabled={naiEnhance.busy || attaching || !naiEnhance.canAddReference}
                onclick={() => (pickerOpen = true)}
              >
                {locale.t("prompt_assistant.nai_reference_gallery")}
              </button>
            </div>
          </div>

          <input
            bind:this={fileInput}
            type="file"
            accept="image/*"
            multiple
            class="hidden"
            onchange={(e) => {
              const el = e.currentTarget as HTMLInputElement;
              void attachFromFiles(el.files);
              // Cleared so picking the same file twice in a row still fires.
              el.value = "";
            }}
          />

          {#if naiEnhance.references.length > 0}
            <div class="mt-2 flex flex-wrap gap-2">
              {#each naiEnhance.references as ref, i (ref.id)}
                <div class="w-28 shrink-0">
                  <div
                    class="relative overflow-hidden rounded-lg border border-neutral-700 bg-neutral-900"
                  >
                    <img
                      src={novelAiBase64ToSrc(ref.base64)}
                      alt={locale.t("prompt_assistant.nai_reference_alt", { n: i + 1 })}
                      class="h-24 w-full object-cover"
                    />
                    <span
                      class="absolute left-1 top-1 rounded bg-black/70 px-1 text-[10px] tabular-nums text-neutral-200"
                    >
                      {i + 1}
                    </span>
                    <button
                      class="absolute right-1 top-1 rounded bg-black/70 px-1 text-[10px] text-neutral-200 hover:bg-red-600/80"
                      title={locale.t("prompt_assistant.nai_reference_remove")}
                      aria-label={locale.t("prompt_assistant.nai_reference_remove")}
                      disabled={naiEnhance.busy}
                      onclick={() => naiEnhance.removeReference(ref.id)}
                    >
                      ✕
                    </button>
                  </div>
                  <input
                    type="text"
                    class="mt-1 w-full rounded border border-neutral-700 bg-neutral-950 px-1.5 py-0.5 text-[10px] text-neutral-200 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
                    placeholder={locale.t("prompt_assistant.nai_reference_label_placeholder")}
                    aria-label={locale.t("prompt_assistant.nai_reference_label_placeholder")}
                    disabled={naiEnhance.busy}
                    value={ref.label}
                    oninput={(e) =>
                      naiEnhance.setReferenceLabel(
                        ref.id,
                        (e.currentTarget as HTMLInputElement).value,
                      )}
                  />
                </div>
              {/each}
            </div>
          {/if}

          <p class="mt-2 text-[10px] leading-relaxed text-neutral-500">
            {locale.t("prompt_assistant.nai_references_hint", { max: NAI_MAX_REFERENCES })}
          </p>
        </div>

        <div class="mt-2 flex flex-wrap items-center gap-2">
          <!-- Off by default, and sticky once ticked: iterating on one image
               wants it for the whole session, while the first pass at a new
               idea is better off with the model unable to see the old prompt. -->
          <label
            class="flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-300"
            title={isSingleRow
              ? locale.t("prompt_assistant.general_include_existing_tooltip")
              : locale.t("prompt_assistant.nai_include_existing_tooltip")}
          >
            <input
              type="checkbox"
              class="accent-indigo-500"
              disabled={naiEnhance.busy}
              checked={generation.naiEnhanceIncludeExisting}
              onchange={(e) => {
                generation.naiEnhanceIncludeExisting = (
                  e.currentTarget as HTMLInputElement
                ).checked;
                generation.saveSettings();
              }}
            />
            {locale.t("prompt_assistant.nai_include_existing")}
          </label>
          <button
            class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
            disabled={naiEnhance.busy || !generation.positivePrompt?.trim()}
            onclick={() => naiEnhance.copyExistingPrompt()}
          >
            {locale.t("prompt_assistant.nai_copy_existing")}
          </button>
          {#if isGeneral}
            <div class="flex items-center gap-1 text-[10px] text-neutral-400">
              <span>{locale.t("prompt_assistant.length")}:</span>
              {#each ["short", "medium", "detailed"] as const as len (len)}
                <button
                  class="rounded border px-1.5 py-0.5 {length === len
                    ? 'border-[var(--theme-accent-500)] text-neutral-100'
                    : 'border-neutral-600 text-neutral-400 hover:bg-neutral-800'}"
                  disabled={naiEnhance.busy}
                  onclick={() => (length = len)}
                >
                  {locale.t(`prompt_assistant.length_${len}`)}
                </button>
              {/each}
            </div>
            <label class="flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-300">
              <input
                type="checkbox"
                class="accent-indigo-500"
                disabled={naiEnhance.busy}
                bind:checked={includeArtists}
              />
              {locale.t("prompt_assistant.include_artists")}
            </label>
          {:else if !isH3}
          <!-- V5 writes the scene body in the prompt's own language. Detection is
               unreliable on a short Latin prompt, so the guess is overridable. -->
          <select
            class="rounded-lg border border-neutral-600 bg-neutral-900 px-1.5 py-0.5 text-[10px] text-neutral-300 focus:border-indigo-500 focus:outline-none"
            title={locale.t("prompt_assistant.nai_language_tooltip")}
            aria-label={locale.t("prompt_assistant.nai_language")}
            value={generation.naiEnhanceLanguage}
            onchange={(e) => {
              generation.naiEnhanceLanguage = (e.currentTarget as HTMLSelectElement)
                .value as NaiLanguageChoice;
              generation.saveSettings();
            }}
          >
            <option value="auto">{locale.t("prompt_assistant.nai_language_auto")}</option>
            {#each NAI_LANGUAGES as lang (lang.code)}
              <option value={lang.code}>
                {lang.label}{lang.official ? "" : " *"}
              </option>
            {/each}
          </select>
          <label
            class="flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-300"
            title={locale.t("prompt_assistant.nai_curate_tooltip")}
          >
            <input
              type="checkbox"
              class="accent-indigo-500"
              disabled={naiEnhance.busy}
              bind:checked={naiEnhance.curate}
            />
            {locale.t("prompt_assistant.nai_curate")}
          </label>
          {/if}
        </div>

        <div class="mt-4 flex items-center justify-end gap-2">
          <button
            class="rounded-lg border border-neutral-600 px-3 py-1 text-xs text-neutral-300 hover:bg-neutral-800"
            onclick={close}
          >
            {locale.t("common.cancel")}
          </button>
          <button
            class="rounded-lg bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500 disabled:opacity-40"
            disabled={naiEnhance.busy || !canRun}
            onclick={run}
          >
            {#if naiEnhance.busy}
              <span class="inline-block animate-spin">⟳</span>
              {locale.t("prompt_assistant.nai_working")}
            {:else}
              {locale.t("prompt_assistant.nai_run")}
            {/if}
          </button>
        </div>
      {:else if isSingleRow && naiEnhance.generalPending}
        {@const general = naiEnhance.generalPending}
        {#if general.notice}
          <div
            class="mb-3 rounded-lg border p-2 text-[11px] {general.notice.kind === 'warning'
              ? 'border-amber-500/40 bg-amber-500/10 text-amber-200'
              : 'border-indigo-500/40 bg-indigo-500/10 text-indigo-200'}"
          >
            {general.notice.text}
          </div>
        {/if}
        <div class="grid gap-2 sm:grid-cols-2">
          <div>
            <div class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">
              {locale.t("prompt_assistant.nai_before")}
            </div>
            <div
              class="max-h-72 overflow-y-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-neutral-950 p-2 text-[11px] text-neutral-500"
            >
              {general.before.trim() || locale.t("prompt_assistant.nai_empty")}
            </div>
          </div>
          <div>
            <div class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">
              {locale.t("prompt_assistant.nai_after")}
            </div>
            <div
              class="max-h-72 overflow-y-auto whitespace-pre-wrap break-words rounded border border-indigo-500/60 bg-neutral-950 p-2 text-[11px] text-neutral-200"
            >
              {general.after}
            </div>
          </div>
        </div>

        <div class="mt-4 flex items-center justify-end gap-2">
          <button
            class="rounded-lg border border-neutral-600 px-3 py-1 text-xs text-neutral-300 hover:bg-neutral-800"
            onclick={close}
          >
            {locale.t("common.cancel")}
          </button>
          <!-- H3 is one structured document; gluing a second one onto it
               breaks the format, so it only replaces. -->
          {#if general.before.trim() && !isH3}
            <button
              class="rounded-lg border border-neutral-600 px-3 py-1 text-xs text-neutral-200 hover:bg-neutral-800"
              onclick={() => naiEnhance.applyGeneral("append")}
            >
              {locale.t("prompt_assistant.append")}
            </button>
          {/if}
          <button
            class="rounded-lg bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500"
            onclick={() => naiEnhance.applyGeneral("replace")}
          >
            {locale.t("prompt_assistant.replace")}
          </button>
        </div>
      {:else if pending}
        {#if naiEnhance.curate}
          <div
            class="mb-3 rounded-lg border border-indigo-500/40 bg-indigo-500/10 p-2 text-[11px] text-indigo-200"
          >
            {locale.t("prompt_assistant.nai_curate_notice")}
          </div>
        {/if}

        {#if pending.note}
          <div
            class="mb-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-[11px] text-amber-200"
          >
            {pending.note}
          </div>
        {/if}

        {#if pending.problems.length > 0}
          <!-- Same bargain as the H3 warning: the rewrite is still usually better
               than the prompt it replaces, so it is offered with the broken rule
               named rather than thrown away. -->
          <div
            class="mb-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-[11px] text-amber-200"
          >
            <div class="font-medium">{locale.t("prompt_assistant.nai_format_warning")}</div>
            <ul class="mt-1 list-disc space-y-0.5 pl-4">
              {#each pending.problems as problem (problem)}
                <li>{problem}</li>
              {/each}
            </ul>
          </div>
        {/if}

        <div class="mb-3 flex items-center gap-2">
          <button
            class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
            onclick={() => naiEnhance.setAll(true)}
          >
            {locale.t("prompt_assistant.nai_select_all")}
          </button>
          <button
            class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
            onclick={() => naiEnhance.setAll(false)}
          >
            {locale.t("prompt_assistant.nai_select_none")}
          </button>
        </div>

        <div class="space-y-2">
          {@render row(
            locale.t("prompt_assistant.nai_field_base"),
            pending.base.before,
            pending.base.after,
            pending.base.selected,
            pending.budget,
            () => naiEnhance.toggleBase(),
            "",
          )}
          {@render row(
            locale.t("prompt_assistant.nai_field_uc"),
            pending.uc.before,
            pending.uc.after,
            pending.uc.selected,
            pending.budget,
            () => naiEnhance.toggleUc(),
            "",
          )}
          {#each pending.characters as char, i (i)}
            {@render row(
              locale.t("prompt_assistant.nai_field_character", {
                // A removal row is named after the box it deletes, not its place
                // in this list, so the number matches the panel underneath.
                index: String((char.targetIndex ?? i) + 1),
              }),
              char.before,
              char.after,
              char.selected,
              pending.budget,
              () => naiEnhance.toggleCharacter(i),
              char.removes
                ? locale.t("prompt_assistant.nai_removed_character")
                : char.targetIndex === null
                  ? locale.t("prompt_assistant.nai_new_character")
                  : "",
            )}
          {/each}
        </div>

        <div class="mt-4 flex items-center justify-between gap-3">
          <span class="text-[11px] text-neutral-400">
            {locale.t("prompt_assistant.nai_selection_tokens", {
              tokens: String(selectedTokens),
              budget: String(pending.budget),
            })}
          </span>
          <div class="flex items-center gap-2">
            <button
              class="rounded-lg border border-neutral-600 px-3 py-1 text-xs text-neutral-300 hover:bg-neutral-800"
              onclick={close}
            >
              {locale.t("common.cancel")}
            </button>
            <button
              class="rounded-lg bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500 disabled:opacity-40"
              disabled={!naiEnhance.hasSelection}
              onclick={() => naiEnhance.apply()}
            >
              {locale.t("prompt_assistant.nai_apply_selected")}
            </button>
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}

<!-- Outside the modal markup above so its own overlay stacks on top rather than
     being clipped by the dialog's scroll container. -->
<GalleryPickerModal
  open={pickerOpen}
  multiple
  max={naiEnhance.referenceSlotsLeft}
  title={locale.t("prompt_assistant.nai_reference_pick_title")}
  onselect={(images) => void attachFromGallery(images)}
  onclose={() => (pickerOpen = false)}
/>
