<script lang="ts">
  /**
   * Bottom panel Characters tab: characters the prompt assistant pulled out of
   * a prompt, saved per model architecture so only the ones written for the
   * selected model show up.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery, isVideoImage } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import {
    architectureLabel,
    savedCharacters,
    type SavedCharacter,
  } from "../../stores/savedCharacters.svelte.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import type { CharacterMergeResult } from "../../utils/characterMerge.js";
  import CharacterMergeModal from "./CharacterMergeModal.svelte";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import { progress } from "../../stores/progress.svelte.js";
  import type { OutputImage } from "../../types/index.js";
  import {
    blobToCharacterThumbnail,
    currentImageToCharacterThumbnail,
    outputImageToCharacterThumbnail,
  } from "../../utils/characterThumbnail.js";

  let search = $state("");
  let extracting = $state(false);
  let editingId = $state<string | null>(null);
  let editName = $state("");
  let editPrompt = $state("");
  let editNegative = $state("");
  /** Character the prompt assistant is working into the prompt right now. */
  let mergingId = $state<string | null>(null);
  /** Card whose thumbnail options are showing. */
  let thumbMenuId = $state<string | null>(null);
  /** Card a file upload or gallery pick is for. */
  let thumbTargetId = $state<string | null>(null);
  /** Card whose thumbnail is being made right now. */
  let thumbBusyId = $state<string | null>(null);
  let thumbPickerOpen = $state(false);
  let thumbFileInput = $state<HTMLInputElement | null>(null);

  /** Character being refreshed from the prompt right now. */
  let updatingId = $state<string | null>(null);
  let review = $state<{
    character: SavedCharacter;
    before: { prompt: string; negative: string };
    result: CharacterMergeResult;
    boxMode: boolean;
    boxNegative: boolean;
  } | null>(null);

  const architecture = $derived(savedCharacters.currentArchitecture);
  const archLabel = $derived(architecture ? architectureLabel(architecture) : "");

  const filtered = $derived.by(() => {
    const q = search.toLowerCase().trim();
    const list = savedCharacters.currentCharacters;
    if (!q) return list;
    return list.filter(
      (c) => c.name.toLowerCase().includes(q) || c.prompt.toLowerCase().includes(q),
    );
  });

  /** The positive prompt plus, on NovelAI, every character box that will be sent. */
  function extractionSource(): string {
    const parts = [generation.positivePrompt.trim()];
    if (generation.isNovelAi) {
      generation.activeNovelAiCharacters.forEach((c, i) => {
        parts.push(`Character ${i + 1}: ${c.prompt.trim()}`);
      });
    }
    return parts.filter(Boolean).join("\n");
  }

  /** The negative prompt (UC) plus, on NovelAI, each sent character box's own UC. */
  function extractionNegative(): string {
    const parts = [generation.negativePrompt.trim()];
    if (generation.isNovelAi) {
      generation.activeNovelAiCharacters.forEach((c, i) => {
        const uc = c.negative_prompt.trim();
        if (uc) parts.push(`Character ${i + 1}: ${uc}`);
      });
    }
    return parts.filter(Boolean).join("\n");
  }

  async function extract() {
    const arch = architecture;
    if (!arch || extracting) return;
    if (!promptAssistant.isAvailable) {
      promptAssistant.setupModalOpen = true;
      return;
    }
    const source = extractionSource();
    if (!source) {
      gallery.showToast(locale.t("characters.toast.empty_prompt"), "error");
      return;
    }
    extracting = true;
    try {
      const found = await promptAssistant.extractCharacters(source, extractionNegative());
      if (found.length === 0) {
        gallery.showToast(locale.t("characters.toast.none_found"), "info");
        return;
      }
      const { added, updated } = savedCharacters.saveExtracted(found, arch);
      if (added === 0 && updated === 0) {
        gallery.showToast(locale.t("characters.toast.already_saved"), "info");
      } else {
        gallery.showToast(
          locale.t("characters.toast.saved", {
            added: String(added),
            updated: String(updated),
            arch: architectureLabel(arch),
          }),
          "success",
        );
      }
    } catch (e) {
      console.error("Character extraction failed:", e);
      const msg = String(e);
      gallery.showToast(
        msg.includes("invalid_character_extract")
          ? locale.t("characters.toast.invalid_reply")
          : mapLlmError(msg),
        "error",
      );
    } finally {
      extracting = false;
    }
  }

  /**
   * Have the prompt assistant work the character into the prompt, then review
   * it. Without an assistant, or with nothing in the prompt to merge into,
   * there is nothing to resolve, so it inserts the saved text as is.
   */
  async function use(character: SavedCharacter) {
    if (mergingId) return;
    if (savedCharacters.isInPrompt(character)) {
      gallery.showToast(locale.t("characters.toast.already_in_prompt", { name: character.name }), "info");
      return;
    }
    if (!promptAssistant.isAvailable || !generation.positivePrompt.trim()) {
      insertPlain(character);
      return;
    }
    const target = savedCharacters.boxTarget();
    const before = { prompt: generation.positivePrompt, negative: generation.negativePrompt };
    mergingId = character.id;
    try {
      const result = await promptAssistant.mergeCharacter({
        prompt: before.prompt,
        negative: before.negative,
        character,
        box: target
          ? {
              boxes: generation.activeNovelAiCharacters.map((c) => c.prompt),
              boxNegative: target.boxNegative,
            }
          : null,
      });
      review = {
        character,
        before,
        result,
        boxMode: target !== null,
        boxNegative: target?.boxNegative ?? false,
      };
    } catch (e) {
      console.error("Character merge failed:", e);
      const msg = String(e);
      gallery.showToast(
        msg.includes("invalid_character_merge")
          ? locale.t("characters.toast.merge_invalid")
          : mapLlmError(msg),
        "error",
      );
    } finally {
      mergingId = null;
    }
  }

  function applyReview(fields: { prompt?: string; negative?: string; character?: string; characterNegative?: string }) {
    const r = review;
    if (!r) return;
    review = null;
    // The merge was written against the prompt as it was; applying it over
    // edits made since would silently undo them.
    if (generation.positivePrompt !== r.before.prompt || generation.negativePrompt !== r.before.negative) {
      gallery.showToast(locale.t("characters.toast.prompt_changed", { name: r.character.name }), "error");
      return;
    }
    savedCharacters.applyMerge(fields);
    gallery.showToast(
      locale.t(
        r.boxMode && fields.character ? "characters.toast.added_character_box" : "characters.toast.added_prompt",
        { name: r.character.name },
      ),
      "success",
    );
  }

  function insertPlain(character: SavedCharacter) {
    review = null;
    const result = savedCharacters.insert(character);
    const key =
      result === "duplicate"
        ? "characters.toast.already_in_prompt"
        : result === "novelai_character"
          ? "characters.toast.added_character_box"
          : "characters.toast.added_prompt";
    gallery.showToast(
      locale.t(key, { name: character.name }),
      result === "duplicate" ? "info" : "success",
    );
  }

  /**
   * Refresh the card from what the current prompt says about this character,
   * leaving other characters' details out. Undo restores the old text.
   */
  async function refresh(character: SavedCharacter) {
    if (updatingId || mergingId) return;
    if (!promptAssistant.isAvailable) {
      promptAssistant.setupModalOpen = true;
      return;
    }
    const source = extractionSource();
    if (!source) {
      gallery.showToast(locale.t("characters.toast.empty_prompt"), "error");
      return;
    }
    updatingId = character.id;
    try {
      const update = await promptAssistant.updateCharacter(character, source, extractionNegative());
      if (update === "missing") {
        gallery.showToast(locale.t("characters.toast.update_missing", { name: character.name }), "info");
        return;
      }
      if (update.prompt === character.prompt && update.negative === character.negative) {
        gallery.showToast(locale.t("characters.toast.update_unchanged", { name: character.name }), "info");
        return;
      }
      const previous = { prompt: character.prompt, negative: character.negative };
      savedCharacters.update(character.id, update);
      gallery.showToast(locale.t("characters.toast.updated", { name: character.name }), "success", {
        actionLabel: locale.t("characters.undo"),
        onAction: () => savedCharacters.update(character.id, previous),
        durationMs: 8000,
      });
    } catch (e) {
      console.error("Character update failed:", e);
      const msg = String(e);
      gallery.showToast(
        msg.includes("invalid_character_update")
          ? locale.t("characters.toast.invalid_reply")
          : mapLlmError(msg),
        "error",
      );
    } finally {
      updatingId = null;
    }
  }

  /** Run one thumbnail source for a card and store what it produced. */
  async function setThumb(id: string, make: () => Promise<string | null>, emptyKey: string) {
    thumbBusyId = id;
    thumbMenuId = null;
    try {
      const thumb = await make();
      if (thumb) savedCharacters.setThumbnail(id, thumb);
      else gallery.showToast(locale.t(emptyKey), "error");
    } catch (e) {
      console.error("Character thumbnail failed:", e);
      gallery.showToast(locale.t("characters.thumbnail.failed"), "error");
    } finally {
      thumbBusyId = null;
    }
  }

  function thumbFromCurrent(character: SavedCharacter) {
    const latest = gallery.sessionImages.find((image) => !isVideoImage(image)) ?? null;
    if (!progress.lastOutputImage && !latest) {
      thumbMenuId = null;
      gallery.showToast(locale.t("characters.thumbnail.no_current"), "info");
      return;
    }
    void setThumb(
      character.id,
      () => currentImageToCharacterThumbnail(progress.lastOutputImage, latest),
      "characters.thumbnail.failed",
    );
  }

  function thumbFromFile(files: FileList | null) {
    const id = thumbTargetId;
    const file = files?.[0];
    if (thumbFileInput) thumbFileInput.value = "";
    if (!id || !file) return;
    void setThumb(id, () => blobToCharacterThumbnail(file), "characters.thumbnail.failed");
  }

  function thumbFromGallery(images: OutputImage[]) {
    thumbPickerOpen = false;
    const id = thumbTargetId;
    const image = images[0];
    if (!id || !image) return;
    void setThumb(id, () => outputImageToCharacterThumbnail(image), "characters.thumbnail.failed");
  }

  function startEdit(character: SavedCharacter) {
    editingId = character.id;
    editName = character.name;
    editPrompt = character.prompt;
    editNegative = character.negative;
  }

  function saveEdit() {
    if (!editingId) return;
    savedCharacters.update(editingId, { name: editName, prompt: editPrompt, negative: editNegative });
    editingId = null;
  }

  function remove(character: SavedCharacter) {
    if (!confirm(locale.t("characters.delete_confirm", { name: character.name }))) return;
    if (editingId === character.id) editingId = null;
    savedCharacters.remove(character.id);
  }
</script>

<div class="flex flex-col h-full">
  <div class="px-2 pt-1.5 pb-1 shrink-0 flex items-center gap-2">
    <input
      type="text"
      name="saved-character-search"
      bind:value={search}
      placeholder={locale.t("characters.search_placeholder")}
      disabled={!architecture}
      class="flex-1 min-w-0 bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
    />
    {#if architecture}
      <span
        class="shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-neutral-800 border border-neutral-700 text-neutral-400"
        title={locale.t("characters.arch_tip", { arch: archLabel })}
      >
        {archLabel}
      </span>
    {/if}
    <button
      type="button"
      class="shrink-0 px-2.5 py-1 text-[11px] font-medium rounded bg-indigo-600 hover:bg-indigo-500 text-white transition-colors disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1.5"
      onclick={extract}
      disabled={!architecture || extracting || (promptAssistant.isGenerating && !extracting)}
      title={locale.t("characters.extract_tip")}
    >
      {#if extracting}
        <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
        {promptAssistant.stage === "loading_model"
          ? locale.t("prompt_assistant.loading_model")
          : locale.t("characters.extracting")}
      {:else}
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
        {locale.t("characters.extract")}
      {/if}
    </button>
  </div>

  {#if !architecture}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs px-4 text-center">
      <p>{locale.t("characters.no_architecture")}</p>
    </div>
  {:else if savedCharacters.currentCharacters.length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs px-4 text-center">
      <p>{locale.t("characters.empty", { arch: archLabel })}</p>
    </div>
  {:else if filtered.length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
      <p>{locale.t("characters.no_results")}</p>
    </div>
  {:else}
    <div class="flex-1 min-h-0 overflow-y-auto [scrollbar-gutter:stable] px-2 py-2">
      <div class="grid gap-2" style="grid-template-columns: repeat(auto-fill, minmax(min(220px, 100%), 1fr)); align-content: start;">
        {#each filtered as character (character.id)}
          <div class="rounded-lg border border-neutral-800 bg-neutral-900/60 p-2 flex flex-col gap-1.5 min-w-0">
            {#if editingId === character.id}
              <input
                type="text"
                bind:value={editName}
                aria-label={locale.t("characters.name")}
                class="w-full bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500"
              />
              <textarea
                bind:value={editPrompt}
                rows="3"
                spellcheck="false"
                aria-label={locale.t("characters.prompt")}
                class="w-full resize-y bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-[11px] leading-relaxed text-neutral-100 focus:outline-none focus:border-indigo-500"
              ></textarea>
              <textarea
                bind:value={editNegative}
                rows="2"
                spellcheck="false"
                placeholder={locale.t("characters.negative")}
                aria-label={locale.t("characters.negative")}
                class="w-full resize-y bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-[11px] leading-relaxed text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-red-500/70"
              ></textarea>
              <div class="flex justify-end gap-1.5">
                <button
                  type="button"
                  class="px-2 py-0.5 text-[11px] rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
                  onclick={() => { editingId = null; }}
                >
                  {locale.t("common.cancel")}
                </button>
                <button
                  type="button"
                  class="px-2 py-0.5 text-[11px] rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40"
                  disabled={!editName.trim() || !editPrompt.trim()}
                  onclick={saveEdit}
                >
                  {locale.t("common.save")}
                </button>
              </div>
            {:else}
              <div class="flex gap-2 min-w-0">
              <button
                type="button"
                class="shrink-0 w-14 h-14 rounded-md overflow-hidden border flex items-center justify-center transition-colors {thumbMenuId === character.id ? 'border-indigo-500' : 'border-neutral-700 hover:border-neutral-500'} bg-neutral-800 text-neutral-500"
                title={locale.t("characters.thumbnail.change")}
                aria-label={locale.t("characters.thumbnail.change")}
                aria-expanded={thumbMenuId === character.id}
                onclick={() => { thumbMenuId = thumbMenuId === character.id ? null : character.id; }}
              >
                {#if thumbBusyId === character.id}
                  <svg class="w-4 h-4 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
                {:else if character.thumbnail}
                  <img src={character.thumbnail} alt={character.name} class="w-full h-full object-cover" />
                {:else}
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
                {/if}
              </button>
              <div class="flex-1 min-w-0 flex flex-col gap-1.5">
              <div class="flex items-center gap-1 min-w-0">
                <span class="flex-1 min-w-0 truncate text-xs font-medium text-neutral-100" title={character.name}>{character.name}</span>
                <button
                  type="button"
                  class="shrink-0 px-2 py-0.5 text-[11px] rounded bg-indigo-600/80 hover:bg-indigo-500 text-white disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1"
                  title={locale.t(promptAssistant.isAvailable ? "characters.use_smart_tip" : "characters.use_tip")}
                  disabled={mergingId !== null || updatingId !== null}
                  onclick={() => use(character)}
                >
                  {#if mergingId === character.id}
                    <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
                    {locale.t("characters.merging")}
                  {:else}
                    {locale.t("characters.use")}
                  {/if}
                </button>
                <button
                  type="button"
                  class="shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
                  title={locale.t("characters.insert_plain_tip")}
                  aria-label={locale.t("characters.insert_plain")}
                  onclick={() => insertPlain(character)}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
                </button>
                <button
                  type="button"
                  class="shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed"
                  title={locale.t("characters.update_tip")}
                  aria-label={locale.t("characters.update")}
                  disabled={updatingId !== null || mergingId !== null}
                  onclick={() => refresh(character)}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 {updatingId === character.id ? 'animate-spin' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>
                </button>
                <button
                  type="button"
                  class="shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
                  title={locale.t("characters.edit")}
                  aria-label={locale.t("characters.edit")}
                  onclick={() => startEdit(character)}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"/></svg>
                </button>
                <button
                  type="button"
                  class="shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-red-300 hover:bg-red-600/10"
                  title={locale.t("characters.delete")}
                  aria-label={locale.t("characters.delete")}
                  onclick={() => remove(character)}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                </button>
              </div>
              <p class="text-[11px] leading-snug text-neutral-400 line-clamp-3 break-words" title={character.prompt}>{character.prompt}</p>
              {#if character.negative}
                <p class="text-[11px] leading-snug text-red-300/70 line-clamp-2 break-words" title={character.negative}>
                  <span class="text-red-400/80 font-medium">{locale.t("characters.negative_prefix")}</span> {character.negative}
                </p>
              {/if}
              </div>
              </div>
              {#if thumbMenuId === character.id}
                <div class="flex flex-wrap gap-1">
                  <button
                    type="button"
                    class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
                    onclick={() => thumbFromCurrent(character)}
                  >{locale.t("characters.thumbnail.current")}</button>
                  <button
                    type="button"
                    class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
                    onclick={() => { thumbTargetId = character.id; thumbFileInput?.click(); }}
                  >{locale.t("characters.thumbnail.upload")}</button>
                  <button
                    type="button"
                    class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
                    onclick={() => { thumbTargetId = character.id; thumbPickerOpen = true; }}
                  >{locale.t("characters.thumbnail.gallery")}</button>
                  {#if character.thumbnail}
                    <button
                      type="button"
                      class="px-2 py-0.5 text-[11px] rounded border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300"
                      onclick={() => { savedCharacters.setThumbnail(character.id, null); thumbMenuId = null; }}
                    >{locale.t("characters.thumbnail.remove")}</button>
                  {/if}
                </div>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

{#if review}
  <CharacterMergeModal
    name={review.character.name}
    before={review.before}
    result={review.result}
    boxMode={review.boxMode}
    boxNegative={review.boxNegative}
    onapply={applyReview}
    oninsertplain={() => { if (review) insertPlain(review.character); }}
    onclose={() => { review = null; }}
  />
{/if}

<input
  bind:this={thumbFileInput}
  type="file"
  accept="image/*"
  class="hidden"
  aria-hidden="true"
  tabindex="-1"
  onchange={(e) => thumbFromFile((e.currentTarget as HTMLInputElement).files)}
/>

<GalleryPickerModal
  open={thumbPickerOpen}
  title={locale.t("characters.thumbnail.pick_title")}
  onselect={thumbFromGallery}
  onclose={() => { thumbPickerOpen = false; }}
/>
