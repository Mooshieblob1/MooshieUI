<script lang="ts">
  /**
   * Enhance as a chat with the prompt assistant, in a modal chat window.
   *
   * One window for every Enhance: NovelAI V5, every other image model, and
   * video (H3). Each keeps its own conversation (`enhanceChat.flow` follows the
   * model). The user asks for a prompt, gives feedback on one, or asks a
   * question; the assistant answers in the target model's format, with a reply
   * when it has something to say, and Apply writes the prompt for them. A V5
   * answer can also be opened in the per-field review (`NaiEnhanceReview`).
   *
   * Applying closes the window; Enhance opens it again on the same
   * conversation, so feedback on what was generated picks up where it left off.
   */
  import { tick } from "svelte";
  import { portal } from "../../utils/portal.js";
  import { enhanceChat, NAI_MAX_REFERENCES, sessionFlowOf, type ChatExchange } from "../../stores/enhanceChat.svelte.js";
  import { enhancerSessions } from "../../stores/enhancerSessions.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { savedCharacters } from "../../stores/savedCharacters.svelte.js";
  import { canSend, redoLast, sendMessage } from "../../utils/enhanceChatRunner.js";
  import { parseNaiResponse } from "../../utils/naiParse.js";
  import { NAI_LANGUAGES } from "../../utils/naiLanguage.js";
  import type { NaiLanguageChoice } from "../../utils/naiLanguage.js";
  import { NAI_SAVED_CHARACTER_LIMIT } from "../../utils/naiSavedCharacters.js";
  import type { NaiCanonPriority } from "../../utils/naiSavedCharacters.js";
  import { naiV5Variant } from "../../utils/novelaiModels.js";
  import { fileToNovelAiBase64, novelAiBase64ToSrc } from "../../utils/novelaiImage.js";
  import { readClipboardImageSafe } from "../../utils/api.js";
  import { writeClipboardText } from "../../utils/clipboardText.js";
  import { loadOutputImageForGenerationInput } from "../../utils/galleryActions.js";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import NaiEnhanceReview from "./NaiEnhanceReview.svelte";
  import type { OutputImage } from "../../types/index.js";

  const OPTIONS_KEY = "mooshieui.enhanceChat.options";

  function readStored(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }
  function writeStored(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* private mode: the default comes back next time */
    }
  }

  let optionsOpen = $state(readStored(OPTIONS_KEY) === "1");

  function toggleOptions() {
    optionsOpen = !optionsOpen;
    writeStored(OPTIONS_KEY, optionsOpen ? "1" : "0");
  }

  const flow = $derived(enhanceChat.flow);
  const isNai = $derived(flow === "nai");
  const isH3 = $derived(flow === "h3");
  const isGeneral = $derived(flow === "general");
  const exchanges = $derived(enhanceChat.exchanges(flow));
  const sending = $derived(enhanceChat.sending?.flow === flow ? enhanceChat.sending : null);
  const error = $derived(enhanceChat.error?.flow === flow ? enhanceChat.error : null);
  const lastAt = $derived(exchanges[exchanges.length - 1]?.at);
  /**
   * A session holds up to 50 exchanges, each with a full prompt. Opening the
   * window renders only the newest few; the rest are one click away.
   */
  const SHOWN_STEP = 12;
  let shown = $state(SHOWN_STEP);
  const hidden = $derived(Math.max(0, exchanges.length - shown));
  const visible = $derived(hidden > 0 ? exchanges.slice(hidden) : exchanges);
  /** How many of the oldest exchanges the newest request had to leave out. */
  const dropped = $derived(enhancerSessions.dropped[sessionFlowOf(flow)]);
  const variant = $derived(naiV5Variant(generation.checkpoint));
  const title = $derived(
    isH3
      ? locale.t("prompt_assistant.enhance_h3")
      : isNai
        ? locale.t("prompt_assistant.enhance_nai")
        : locale.t("prompt_assistant.enhance"),
  );
  // The saved characters a V5 answer is told about, named under the box so
  // the user knows "Julie" will resolve to one of them.
  const savedNames = $derived(
    savedCharacters.currentCharacters
      .slice(0, NAI_SAVED_CHARACTER_LIMIT)
      .map((c) => c.name.trim())
      .filter(Boolean),
  );
  /** At least two filled boxes on a V5 model, which is when isolating means anything. */
  const canIsolateCurrent = $derived(
    !!variant && generation.novelaiSettings.characters.filter((c) => c.prompt.trim()).length >= 2,
  );
  const sendable = $derived(!enhanceChat.busy && canSend(flow));

  function newChat() {
    const count = enhancerSessions.count(sessionFlowOf(flow));
    if (count > 0 && !confirm(locale.t("prompt_assistant.session_clear_confirm", { count }))) return;
    enhanceChat.newChat(flow);
  }

  function isolateCurrent() {
    if (!enhanceChat.openIsolateReview()) {
      gallery.showToast(locale.t("prompt_assistant.nai_isolate_nothing"), "info");
    }
  }

  // ---- Scrolling --------------------------------------------------------

  let scroller = $state<HTMLDivElement | null>(null);
  let textarea = $state<HTMLTextAreaElement | null>(null);

  async function scrollToEnd() {
    await tick();
    scroller?.scrollTo({ top: scroller.scrollHeight });
  }

  // Each opening starts from the newest few exchanges again.
  $effect(() => {
    if (enhanceChat.open) void flow;
    shown = SHOWN_STEP;
  });

  // New messages, an answer on its way, a failure, another flow, or the window
  // opening: the newest message is the one to see.
  $effect(() => {
    void exchanges.length;
    void sending;
    void error;
    void flow;
    if (enhanceChat.open && enhanceChat.view === "chat") void scrollToEnd();
  });

  $effect(() => {
    if (enhanceChat.open && enhanceChat.view === "chat") void tick().then(() => textarea?.focus());
  });

  // ---- Attaching images ---------------------------------------------------

  let fileInput = $state<HTMLInputElement | null>(null);
  let pickerOpen = $state(false);
  /** Set while a picked image is being decoded, so the buttons cannot stack. */
  let attaching = $state(false);

  /**
   * Downscale and attach whatever the sources hand over, through the same
   * in-browser pass the NovelAI reference fields use: a 4 MB phone photo
   * becomes a ~1024px PNG before it ever reaches IPC.
   */
  async function attach(blobs: Blob[]) {
    if (blobs.length === 0 || attaching) return;
    attaching = true;
    try {
      const encoded: string[] = [];
      for (const blob of blobs.slice(0, enhanceChat.referenceSlotsLeft)) {
        const base64 = await fileToNovelAiBase64(blob);
        if (base64) encoded.push(base64);
      }
      if (encoded.length === 0) {
        gallery.showToast(locale.t("prompt_assistant.nai_reference_failed"), "error");
        return;
      }
      enhanceChat.addReferences(encoded);
    } catch (e) {
      console.error("Reference image attach failed:", e);
      gallery.showToast(locale.t("prompt_assistant.nai_reference_failed"), "error");
    } finally {
      attaching = false;
    }
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

  /** Gallery entries go through the same PNG decode the generation inputs use. */
  async function attachFromGallery(images: OutputImage[]) {
    try {
      const blobs: Blob[] = [];
      for (const image of images.slice(0, enhanceChat.referenceSlotsLeft)) {
        const { bytes } = await loadOutputImageForGenerationInput(image);
        blobs.push(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
      }
      await attach(blobs);
    } catch (e) {
      console.error("Reference image load from gallery failed:", e);
      gallery.showToast(locale.t("prompt_assistant.nai_reference_failed"), "error");
    }
  }

  /** An image pasted into the message box is attached, text pastes as usual. */
  function onPaste(e: ClipboardEvent) {
    const files = Array.from(e.clipboardData?.files ?? []).filter((f) => f.type.startsWith("image/"));
    if (files.length === 0) return;
    e.preventDefault();
    if (enhanceChat.canAddReference) void attach(files);
  }

  // ---- Keyboard -----------------------------------------------------------

  function onComposerKeydown(e: KeyboardEvent) {
    if (e.key !== "Enter" || e.isComposing) return;
    // Enter sends and Shift+Enter starts a new line. Ctrl+Enter sends too, and
    // claiming it here keeps the window-wide Ctrl+Enter from generating.
    if (e.shiftKey && !(e.ctrlKey || e.metaKey)) return;
    e.preventDefault();
    void sendMessage();
  }

  function onPanelKeydown(e: KeyboardEvent) {
    // The gallery picker sits on top and closes itself on Escape.
    if (e.key === "Escape" && !pickerOpen) {
      e.stopPropagation();
      if (enhanceChat.view === "review") enhanceChat.backToChat();
      else enhanceChat.close();
    }
  }

  // ---- Answers --------------------------------------------------------------

  /** A V5 answer's fields, labelled for display. */
  function naiFields(prompt: string): { label: string; text: string; uc?: boolean }[] {
    const p = parseNaiResponse(prompt);
    const rows: { label: string; text: string; uc?: boolean }[] = [
      { label: locale.t("prompt_assistant.nai_field_base"), text: p.base },
    ];
    if (p.uc.trim()) rows.push({ label: locale.t("prompt_assistant.nai_field_uc"), text: p.uc, uc: true });
    p.characters.forEach((c, i) => {
      rows.push({ label: locale.t("prompt_assistant.nai_field_character", { index: String(i + 1) }), text: c });
      const uc = p.characterUcs[i];
      if (uc?.trim()) rows.push({ label: locale.t("prompt_assistant.nai_field_uc"), text: uc, uc: true });
    });
    return rows;
  }

  function applyNai(ex: ChatExchange) {
    if (!enhanceChat.applyAnswer(ex.at)) gallery.showToast(locale.t("prompt_assistant.chat_needs_v5"), "error");
  }

  function reviewNai(ex: ChatExchange) {
    if (!enhanceChat.reviewAnswer(ex.at)) gallery.showToast(locale.t("prompt_assistant.chat_needs_v5"), "error");
  }

  async function copy(text: string) {
    const ok = await writeClipboardText(text);
    gallery.showToast(locale.t(ok ? "common.copied" : "prompt_assistant.chat_copy_failed"), ok ? "success" : "error");
  }

  const btn =
    "rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40";
  const iconBtn =
    "flex h-7 w-7 items-center justify-center rounded-lg text-neutral-400 hover:bg-neutral-800 hover:text-neutral-100 disabled:opacity-40";
</script>

{#snippet thumbs(refs: { id: string; base64: string; label: string }[])}
  <div class="flex flex-wrap justify-end gap-1">
    {#each refs as ref, i (ref.id)}
      <div class="relative">
        <img
          src={novelAiBase64ToSrc(ref.base64)}
          alt={locale.t("prompt_assistant.nai_reference_alt", { n: i + 1 })}
          title={ref.label || undefined}
          class="h-14 w-14 rounded-md border border-neutral-700 object-cover"
        />
        <span class="absolute left-0.5 top-0.5 rounded bg-black/70 px-1 text-[9px] tabular-nums text-neutral-200">{i + 1}</span>
      </div>
    {/each}
  </div>
{/snippet}

{#snippet answer(ex: ChatExchange)}
  {@const m = ex.meta}
  {@const isLast = ex.at === lastAt}
  <div class="flex flex-col items-start gap-1.5">
    {#if ex.reply}
      <div class="max-w-full whitespace-pre-wrap break-words rounded-2xl rounded-tl-sm bg-neutral-800 px-3 py-2 text-xs leading-relaxed text-neutral-100">
        {ex.reply}
      </div>
    {/if}
    {#if ex.prompt}
      <div class="w-full rounded-xl border {m?.applied ? 'border-emerald-600/50' : 'border-neutral-700'} bg-neutral-950 p-2.5">
        {#if m?.notice}
          <div class="mb-2 rounded-lg border p-2 text-[11px] {m.notice.kind === 'warning' ? 'border-amber-500/40 bg-amber-500/10 text-amber-200' : 'border-indigo-500/40 bg-indigo-500/10 text-indigo-200'}">
            {m.notice.text}
          </div>
        {/if}
        {#if m?.note}
          <div class="mb-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-[11px] text-amber-200">{m.note}</div>
        {/if}
        {#if m?.problems && m.problems.length > 0}
          <div class="mb-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-[11px] text-amber-200">
            <div class="font-medium">{locale.t("prompt_assistant.nai_format_warning")}</div>
            <ul class="mt-1 list-disc space-y-0.5 pl-4">
              {#each m.problems as problem (problem)}
                <li>{problem}</li>
              {/each}
            </ul>
          </div>
        {/if}
        {#if m?.canon && m.canon.length > 0}
          <!-- A saved canon character whose card disagrees with canon. The
               answer went with one side; the other is one redo away. -->
          <div class="mb-2 rounded-lg border border-sky-500/40 bg-sky-500/10 p-2 text-[11px] text-sky-100">
            <div class="font-medium">{locale.t("prompt_assistant.nai_canon_title")}</div>
            <ul class="mt-1 list-disc space-y-0.5 pl-4">
              {#each m.canon as line, i (i)}
                <li>{line}</li>
              {/each}
            </ul>
            {#if isLast && m.request}
              <div class="mt-2 flex flex-wrap items-center gap-1.5">
                <span class="text-sky-200/80">{locale.t("prompt_assistant.nai_canon_question")}</span>
                {#each [["saved", "prompt_assistant.nai_canon_keep_saved"], ["canon", "prompt_assistant.nai_canon_use_canon"]] as [choice, key] (choice)}
                  <button
                    class="rounded-lg border px-2 py-0.5 text-[10px] disabled:cursor-default {enhanceChat.canonPriority === choice
                      ? 'border-sky-400 bg-sky-500/30 text-white'
                      : 'border-neutral-600 text-neutral-200 hover:bg-neutral-800'}"
                    aria-pressed={enhanceChat.canonPriority === choice}
                    disabled={enhanceChat.busy || enhanceChat.canonPriority === choice}
                    onclick={() => void redoLast("nai", ex.at, choice as NaiCanonPriority)}
                  >
                    {locale.t(key)}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        {#if isNai}
          <div class="max-h-80 space-y-1.5 overflow-y-auto">
            {#each naiFields(ex.prompt) as field, i (i)}
              <div>
                <div class="text-[9px] uppercase tracking-wide {field.uc ? 'text-red-400/80' : 'text-neutral-500'}">{field.label}</div>
                <div class="whitespace-pre-wrap break-words text-[11px] {field.uc ? 'text-neutral-400' : 'text-neutral-200'}">{field.text}</div>
              </div>
            {/each}
          </div>
        {:else}
          <div class="max-h-80 overflow-y-auto whitespace-pre-wrap break-words text-[11px] text-neutral-200">{ex.prompt}</div>
        {/if}

        <div class="mt-2 flex flex-wrap items-center gap-1.5">
          {#if isNai}
            <button
              class="rounded-lg bg-indigo-600 px-2.5 py-0.5 text-[11px] text-white hover:bg-indigo-500 disabled:opacity-40"
              disabled={!variant}
              onclick={() => applyNai(ex)}
            >
              {locale.t("prompt_assistant.chat_apply")}
            </button>
            <button class={btn} disabled={!variant} onclick={() => reviewNai(ex)}>
              {locale.t("prompt_assistant.chat_review")}
            </button>
          {:else}
            <button
              class="rounded-lg bg-indigo-600 px-2.5 py-0.5 text-[11px] text-white hover:bg-indigo-500"
              onclick={() => enhanceChat.applyText(ex.at, ex.prompt, "replace")}
            >
              {locale.t("prompt_assistant.replace")}
            </button>
            <!-- H3 is one structured document; gluing a second one onto it
                 breaks the format, so it only replaces. -->
            {#if isGeneral && generation.positivePrompt?.trim()}
              <button class={btn} onclick={() => enhanceChat.applyText(ex.at, ex.prompt, "append")}>
                {locale.t("prompt_assistant.append")}
              </button>
            {/if}
          {/if}
          <button class={btn} title={locale.t("common.copy")} aria-label={locale.t("common.copy")} onclick={() => void copy(ex.prompt)}>⧉</button>
          {#if m?.applied}
            <span class="text-[10px] text-emerald-400">✓ {locale.t("prompt_assistant.chat_applied")}</span>
          {/if}
        </div>
      </div>
    {/if}
    {#if isLast && m?.request}
      <button
        class="text-[10px] text-neutral-500 hover:text-neutral-200 disabled:opacity-40"
        disabled={enhanceChat.busy}
        onclick={() => void redoLast(flow, ex.at)}
      >
        ↻ {locale.t("prompt_assistant.chat_retry")}
      </button>
    {/if}
  </div>
{/snippet}

{#if enhanceChat.open}
  <div
    use:portal
    data-modal-open
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 sm:p-6"
    onclick={(e) => {
      // Closing keeps the conversation, so a click beside the chat may close it.
      // A review stays: picking fields is work a stray click should not undo.
      if (e.currentTarget === e.target && enhanceChat.view === "chat") enhanceChat.close();
    }}
    role="presentation"
  >
    <!-- The review is a two column diff of up to six fields, so it gets the
         width the old Enhance modal had; the chat reads better narrower. -->
    <div
      class="flex h-full w-full flex-col overflow-hidden border-neutral-700 bg-neutral-900 shadow-2xl sm:h-[85vh] sm:rounded-xl sm:border {enhanceChat.view === 'review'
        ? 'sm:max-w-[1400px]'
        : 'sm:max-w-3xl'}"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      tabindex="-1"
      onkeydown={onPanelKeydown}
    >
      <header class="flex items-center gap-2 border-b border-neutral-800 px-3 py-2">
        {#if enhanceChat.view === "review"}
          <button class={btn} onclick={() => enhanceChat.backToChat()}>← {locale.t("prompt_assistant.chat_back")}</button>
          <h2 class="truncate text-sm font-semibold text-neutral-100">{locale.t("prompt_assistant.nai_review_title")}</h2>
        {:else}
          <h2 class="truncate text-sm font-semibold text-neutral-100">✨ {title}</h2>
          {#if isNai && variant}
            <span class="shrink-0 rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-400">
              {locale.t(enhanceChat.curate || variant === "full" ? "prompt_assistant.nai_variant_full" : "prompt_assistant.nai_variant_curated")}
            </span>
          {/if}
        {/if}
        <div class="ml-auto flex items-center gap-1">
          {#if enhanceChat.view === "chat" && exchanges.length > 0}
            <button class={btn} onclick={newChat}>⟲ {locale.t("prompt_assistant.chat_new")}</button>
          {/if}
          <button class={iconBtn} title={locale.t("common.close")} aria-label={locale.t("common.close")} onclick={() => enhanceChat.close()}>✕</button>
        </div>
      </header>

      {#if enhanceChat.view === "review"}
        <div class="min-h-0 flex-1 overflow-y-auto p-3">
          <NaiEnhanceReview />
        </div>
      {:else}
        <div bind:this={scroller} class="min-h-0 flex-1 space-y-3 overflow-y-auto px-3 py-3">
          {#if hidden > 0}
            <button class="mx-auto block text-[10px] text-neutral-500 hover:text-neutral-200" onclick={() => (shown += SHOWN_STEP)}>
              {locale.t("prompt_assistant.chat_show_earlier", { count: hidden })}
            </button>
          {/if}
          {#each visible as ex, i (ex.at)}
            {#if dropped > 0 && i + hidden === dropped}
              <div class="flex items-center gap-2 text-[10px] text-neutral-500">
                <span class="h-px flex-1 bg-neutral-800"></span>
                {locale.t("prompt_assistant.session_trimmed")}
                <span class="h-px flex-1 bg-neutral-800"></span>
              </div>
            {/if}
            <div class="flex flex-col items-end gap-1">
              {#if ex.meta?.request?.references.length}
                {@render thumbs(ex.meta.request.references)}
              {/if}
              <div class="max-w-[85%] whitespace-pre-wrap break-words rounded-2xl rounded-tr-sm bg-indigo-600/80 px-3 py-2 text-xs text-white">{ex.user}</div>
            </div>
            {@render answer(ex)}
          {/each}

          {#if sending}
            <div class="flex flex-col items-end gap-1">
              {#if sending.references.length}
                {@render thumbs(sending.references)}
              {/if}
              {#if sending.text}
                <div class="max-w-[85%] whitespace-pre-wrap break-words rounded-2xl rounded-tr-sm bg-indigo-600/80 px-3 py-2 text-xs text-white">{sending.text}</div>
              {/if}
            </div>
            <div class="flex items-center gap-2 text-[11px] text-neutral-400">
              <span class="inline-block animate-spin">⟳</span>
              {promptAssistant.stage === "loading_model"
                ? locale.t("prompt_assistant.loading_model")
                : locale.t("prompt_assistant.chat_thinking")}
            </div>
          {/if}

          {#if error}
            <div class="flex items-start gap-2 rounded-lg border border-red-500/40 bg-red-500/10 p-2 text-[11px] text-red-200" role="alert">
              <span class="flex-1">{error.text}</span>
              <button class="text-red-300 hover:text-white" aria-label={locale.t("common.close")} onclick={() => enhanceChat.dismissError()}>✕</button>
            </div>
          {/if}
        </div>

        <div class="border-t border-neutral-800 p-2.5">
          {#if enhanceChat.references.length > 0}
            <div class="mb-2 flex flex-wrap gap-2">
              {#each enhanceChat.references as ref, i (ref.id)}
                <div class="w-20 shrink-0">
                  <div class="relative overflow-hidden rounded-lg border border-neutral-700 bg-neutral-900">
                    <img
                      src={novelAiBase64ToSrc(ref.base64)}
                      alt={locale.t("prompt_assistant.nai_reference_alt", { n: i + 1 })}
                      class="h-16 w-full object-cover"
                    />
                    <span class="absolute left-1 top-1 rounded bg-black/70 px-1 text-[10px] tabular-nums text-neutral-200">{i + 1}</span>
                    <button
                      class="absolute right-1 top-1 rounded bg-black/70 px-1 text-[10px] text-neutral-200 hover:bg-red-600/80"
                      title={locale.t("prompt_assistant.nai_reference_remove")}
                      aria-label={locale.t("prompt_assistant.nai_reference_remove")}
                      onclick={() => enhanceChat.removeReference(ref.id)}
                    >✕</button>
                  </div>
                  <input
                    type="text"
                    class="mt-1 w-full rounded border border-neutral-700 bg-neutral-950 px-1.5 py-0.5 text-[10px] text-neutral-200 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
                    placeholder={locale.t("prompt_assistant.nai_reference_label_placeholder")}
                    aria-label={locale.t("prompt_assistant.nai_reference_label_placeholder")}
                    value={ref.label}
                    oninput={(e) => enhanceChat.setReferenceLabel(ref.id, (e.currentTarget as HTMLInputElement).value)}
                  />
                </div>
              {/each}
            </div>
          {/if}

          <textarea
            bind:this={textarea}
            rows="3"
            class="max-h-48 min-h-[4.5rem] w-full resize-y rounded-lg border border-neutral-700 bg-neutral-950 p-2.5 text-xs text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
            placeholder={exchanges.length > 0
              ? locale.t("prompt_assistant.session_continue_placeholder")
              : isH3
                ? locale.t("prompt_assistant.h3_input_placeholder")
                : isGeneral
                  ? locale.t("prompt_assistant.general_input_placeholder")
                  : locale.t("prompt_assistant.nai_input_placeholder")}
            aria-label={title}
            bind:value={enhanceChat.input}
            onkeydown={onComposerKeydown}
            onpaste={onPaste}
          ></textarea>
          {#if isNai && savedNames.length > 0}
            <p class="mt-1 truncate text-[10px] text-neutral-500" title={savedNames.join(", ")}>
              {locale.t("prompt_assistant.nai_saved_characters", { names: savedNames.join(", ") })}
            </p>
          {/if}

          <div class="mt-1.5 flex items-center gap-1">
            <div class="flex items-center" title={locale.t("prompt_assistant.nai_references_hint", { max: NAI_MAX_REFERENCES })}>
              <button
                class={iconBtn}
                aria-label={locale.t("prompt_assistant.nai_reference_upload")}
                title={locale.t("prompt_assistant.nai_reference_upload")}
                disabled={attaching || !enhanceChat.canAddReference}
                onclick={() => fileInput?.click()}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48"/></svg>
              </button>
              <button
                class={iconBtn}
                aria-label={locale.t("prompt_assistant.nai_reference_paste")}
                title={locale.t("prompt_assistant.nai_reference_paste")}
                disabled={attaching || !enhanceChat.canAddReference}
                onclick={attachFromClipboard}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="8" y="2" width="8" height="4" rx="1"/><path d="M16 4h2a2 2 0 012 2v14a2 2 0 01-2 2H6a2 2 0 01-2-2V6a2 2 0 012-2h2"/></svg>
              </button>
              <button
                class={iconBtn}
                aria-label={locale.t("prompt_assistant.nai_reference_gallery")}
                title={locale.t("prompt_assistant.nai_reference_gallery")}
                disabled={attaching || !enhanceChat.canAddReference}
                onclick={() => (pickerOpen = true)}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>
              </button>
              {#if enhanceChat.references.length > 0}
                <span class="px-1 text-[10px] tabular-nums text-neutral-500">{enhanceChat.references.length}/{NAI_MAX_REFERENCES}</span>
              {/if}
            </div>
            <input
              bind:this={fileInput}
              type="file"
              accept="image/*"
              multiple
              class="hidden"
              onchange={(e) => {
                const el = e.currentTarget as HTMLInputElement;
                void attach(el.files ? Array.from(el.files) : []);
                // Cleared so picking the same file twice in a row still fires.
                el.value = "";
              }}
            />
            <!-- Off by default, and sticky once ticked: iterating on one image
                 wants it for the whole conversation. -->
            <label
              class="ml-1 flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-300"
              title={isNai
                ? locale.t("prompt_assistant.nai_include_existing_tooltip")
                : locale.t("prompt_assistant.general_include_existing_tooltip")}
            >
              <input
                type="checkbox"
                class="accent-indigo-500"
                checked={generation.naiEnhanceIncludeExisting}
                onchange={(e) => {
                  generation.naiEnhanceIncludeExisting = (e.currentTarget as HTMLInputElement).checked;
                  generation.saveSettings();
                }}
              />
              {locale.t("prompt_assistant.nai_include_existing")}
            </label>
            <button
              class="{iconBtn} ml-auto w-auto px-1.5 text-[10px] {optionsOpen ? 'text-neutral-100' : ''}"
              aria-expanded={optionsOpen}
              onclick={toggleOptions}
            >
              ⚙ {locale.t("prompt_assistant.chat_options")}
            </button>
            {#if sending}
              <button
                class="rounded-lg border border-neutral-600 px-3 py-1 text-xs text-neutral-200 hover:bg-neutral-800"
                onclick={() => enhanceChat.stop()}
              >
                ■ {locale.t("prompt_assistant.chat_stop")}
              </button>
            {:else}
              <button
                class="rounded-lg bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500 disabled:opacity-40"
                disabled={!sendable}
                onclick={() => void sendMessage()}
              >
                {locale.t("prompt_assistant.chat_send")}
              </button>
            {/if}
          </div>

          {#if optionsOpen}
            <div class="mt-2 flex flex-wrap items-center gap-2 border-t border-neutral-800 pt-2">
              <button class={btn} disabled={!generation.positivePrompt?.trim()} onclick={() => enhanceChat.copyExistingPrompt()}>
                {locale.t("prompt_assistant.nai_copy_existing")}
              </button>
              {#if isNai && canIsolateCurrent}
                <button class={btn} title={locale.t("prompt_assistant.nai_isolate_outfits_tip")} onclick={isolateCurrent}>
                  {locale.t("prompt_assistant.nai_isolate_outfits")}
                </button>
              {/if}
              {#if isGeneral}
                <div class="flex items-center gap-1 text-[10px] text-neutral-400">
                  <span>{locale.t("prompt_assistant.length")}:</span>
                  {#each ["short", "medium", "detailed"] as const as len (len)}
                    <button
                      class="rounded border px-1.5 py-0.5 {enhanceChat.length === len
                        ? 'border-[var(--theme-accent-500)] text-neutral-100'
                        : 'border-neutral-600 text-neutral-400 hover:bg-neutral-800'}"
                      onclick={() => (enhanceChat.length = len)}
                    >
                      {locale.t(`prompt_assistant.length_${len}`)}
                    </button>
                  {/each}
                </div>
                <label class="flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-300">
                  <input type="checkbox" class="accent-indigo-500" bind:checked={enhanceChat.includeArtists} />
                  {locale.t("prompt_assistant.include_artists")}
                </label>
              {:else if isNai}
                <!-- V5 writes the scene body in the prompt's own language. Detection
                     is unreliable on a short Latin prompt, so the guess is overridable. -->
                <select
                  class="rounded-lg border border-neutral-600 bg-neutral-900 px-1.5 py-0.5 text-[10px] text-neutral-300 focus:border-indigo-500 focus:outline-none"
                  title={locale.t("prompt_assistant.nai_language_tooltip")}
                  aria-label={locale.t("prompt_assistant.nai_language")}
                  value={generation.naiEnhanceLanguage}
                  onchange={(e) => {
                    generation.naiEnhanceLanguage = (e.currentTarget as HTMLSelectElement).value as NaiLanguageChoice;
                    generation.saveSettings();
                  }}
                >
                  <option value="auto">{locale.t("prompt_assistant.nai_language_auto")}</option>
                  {#each NAI_LANGUAGES as lang (lang.code)}
                    <option value={lang.code}>{lang.label}{lang.official ? "" : " *"}</option>
                  {/each}
                </select>
                <label class="flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-300" title={locale.t("prompt_assistant.nai_curate_tooltip")}>
                  <input type="checkbox" class="accent-indigo-500" bind:checked={enhanceChat.curate} />
                  {locale.t("prompt_assistant.nai_curate")}
                </label>
                <label class="flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-300" title={locale.t("prompt_assistant.nai_keep_artists_tooltip")}>
                  <input
                    type="checkbox"
                    class="accent-indigo-500"
                    checked={generation.naiEnhanceKeepArtists}
                    onchange={(e) => {
                      generation.naiEnhanceKeepArtists = (e.currentTarget as HTMLInputElement).checked;
                      generation.saveSettings();
                    }}
                  />
                  {locale.t("prompt_assistant.nai_keep_artists")}
                </label>
                <label class="flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-300" title={locale.t("prompt_assistant.nai_minor_rule_tooltip")}>
                  <input
                    type="checkbox"
                    class="accent-indigo-500"
                    checked={generation.naiEnhanceMinorRule}
                    onchange={(e) => {
                      generation.naiEnhanceMinorRule = (e.currentTarget as HTMLInputElement).checked;
                      generation.saveSettings();
                    }}
                  />
                  {locale.t("prompt_assistant.nai_minor_rule")}
                </label>
              {/if}
              <!-- General Enhance sends memory notes too, so it gets the same switch. -->
              {#if !isH3 && promptAssistant.canUseMemory}
                <label class="flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-300" title={locale.t("prompt_assistant.nai_use_memory_tooltip")}>
                  <input type="checkbox" class="accent-indigo-500" bind:checked={enhanceChat.useMemory} />
                  {locale.t("prompt_assistant.nai_use_memory")}
                </label>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<!-- Outside the window so its own overlay stacks on top of it, and mounted
     only while picking so the gallery is not read behind a closed chat. -->
{#if pickerOpen}
  <GalleryPickerModal
    open={pickerOpen}
    multiple
    max={enhanceChat.referenceSlotsLeft}
    title={locale.t("prompt_assistant.nai_reference_pick_title")}
    onselect={(images) => void attachFromGallery(images)}
    onclose={() => (pickerOpen = false)}
  />
{/if}
