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
    architecturePromptStyle,
    characterArchitectures,
    savedCharacters,
    type SavedCharacter,
  } from "../../stores/savedCharacters.svelte.js";
  import { NOVELAI_MAX_DIRECTOR_REFERENCES, naiV5Variant } from "../../utils/novelaiModels.js";
  import { saveTextFile } from "../../utils/api.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import type { CharacterMergeResult } from "../../utils/characterMerge.js";
  import CharacterMergeModal from "./CharacterMergeModal.svelte";
  import CharacterDuplicatesModal, {
    type DuplicateDecision,
    type DuplicateMatch,
  } from "./CharacterDuplicatesModal.svelte";
  import type { ExtractedCharacter } from "../../utils/characterExtract.js";
  import { findDuplicate, sameContent } from "../../utils/characterDuplicates.js";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import { progress } from "../../stores/progress.svelte.js";
  import type { OutputImage } from "../../types/index.js";
  import {
    type CharacterImages,
    blobToCharacterThumbnail,
    characterReferenceBase64,
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

  /** Extract found characters that are already saved, awaiting the user's call. */
  let duplicateReview = $state<{
    arch: string;
    fresh: ExtractedCharacter[];
    matches: DuplicateMatch[];
    source: string;
    negative: string;
  } | null>(null);

  /** Top-level cards whose variants are showing. */
  let expanded = $state<Set<string>>(new Set());
  /** Card whose extra options (variants, copy) are showing. */
  let moreMenuId = $state<string | null>(null);
  /** Card being copied to another architecture right now. */
  let copyingId = $state<string | null>(null);
  let importInput = $state<HTMLInputElement | null>(null);

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

  /**
   * Top-level cards with their variants. While searching, a group shows when
   * its card or any variant matches, with the matching variants open.
   */
  const groups = $derived.by(() => {
    const q = search.toLowerCase().trim();
    const list = savedCharacters.currentCharacters;
    const ids = new Set(list.map((c) => c.id));
    const matches = (c: SavedCharacter) =>
      !q || c.name.toLowerCase().includes(q) || c.prompt.toLowerCase().includes(q);
    const out: { top: SavedCharacter; variants: SavedCharacter[]; open: boolean }[] = [];
    for (const top of list) {
      if (top.parentId && ids.has(top.parentId)) continue;
      const variants = list
        .filter((c) => c.parentId === top.id)
        .sort((a, b) => a.createdAt - b.createdAt);
      if (!q) {
        out.push({ top, variants, open: expanded.has(top.id) });
        continue;
      }
      const hits = variants.filter(matches);
      if (!matches(top) && hits.length === 0) continue;
      out.push({
        top,
        variants: hits.length > 0 ? hits : variants,
        open: hits.length > 0 || expanded.has(top.id),
      });
    }
    return out;
  });

  /** Top-level cards a card could be filed under as a variant. */
  function parentChoices(character: SavedCharacter): SavedCharacter[] {
    return savedCharacters.currentCharacters.filter((c) => !c.parentId && c.id !== character.id);
  }

  function toggleExpanded(id: string) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expanded = next;
  }

  /** Whether Use can attach a card's picture as a Precise Reference (not V5 yet). */
  const canReference = $derived(
    generation.isNovelAi &&
      generation.supportsNovelAiPreciseReference &&
      naiV5Variant(generation.checkpoint) === null,
  );

  /**
   * After Use, put the card's picture in as a Precise Reference on models
   * that take one. Vibe Transfer and Precise Reference cannot go together, so
   * it stays out rather than clearing vibes the user set up.
   */
  async function attachReference(character: SavedCharacter) {
    if (!canReference || !(character.reference ?? character.thumbnail)) return;
    const nai = generation.novelaiSettings;
    if (nai.vibes.length > 0) {
      gallery.showToast(locale.t("characters.toast.reference_vibes", { name: character.name }), "info");
      return;
    }
    if (nai.director_references.length >= NOVELAI_MAX_DIRECTOR_REFERENCES) {
      gallery.showToast(locale.t("characters.toast.reference_full", { name: character.name }), "info");
      return;
    }
    const base64 = await characterReferenceBase64(character);
    if (!base64 || generation.novelaiSettings.director_references.some((r) => r.image === base64)) return;
    generation.addNovelAiDirectorReference(base64);
    gallery.showToast(locale.t("characters.toast.reference_added", { name: character.name }), "success");
  }

  /** Start a variant as a copy of the card and open it for editing. */
  function addVariant(character: SavedCharacter) {
    moreMenuId = null;
    const id = savedCharacters.addVariant(character, locale.t("characters.dupes.variant_default"));
    if (!id) return;
    expanded = new Set([...expanded, character.parentId ?? character.id]);
    const created = savedCharacters.characters.find((c) => c.id === id);
    if (created) startEdit(created);
  }

  /**
   * Copy a card to another architecture. Between tag and natural-language
   * models the prompt assistant rewrites it; between two tag models (or with
   * no assistant set up) it goes over as is.
   */
  async function copyTo(character: SavedCharacter, target: string) {
    if (!target || copyingId) return;
    moreMenuId = null;
    const arch = architectureLabel(target);
    const style = architecturePromptStyle(target);
    const rewrite = style !== architecturePromptStyle(character.architecture);
    let text = { prompt: character.prompt, negative: character.negative };
    if (rewrite && promptAssistant.isAvailable) {
      copyingId = character.id;
      try {
        text = await promptAssistant.convertCharacter(
          character,
          architectureLabel(character.architecture),
          arch,
          style,
        );
      } catch (e) {
        console.error("Character copy failed:", e);
        const msg = String(e);
        gallery.showToast(
          msg.includes("invalid_character_convert")
            ? locale.t("characters.toast.copy_failed", { name: character.name, arch })
            : mapLlmError(msg),
          "error",
        );
        return;
      } finally {
        copyingId = null;
      }
    }
    savedCharacters.saveExtracted(
      [{ name: character.name, ...text, thumbnail: character.thumbnail, reference: character.reference }],
      target,
    );
    gallery.showToast(
      locale.t(
        rewrite && !promptAssistant.isAvailable ? "characters.toast.copied_as_is" : "characters.toast.copied",
        { name: character.name, arch },
      ),
      "success",
    );
  }

  /** Every saved card, for every architecture, as a JSON file. */
  async function exportCharacters() {
    const count = savedCharacters.characters.length;
    if (count === 0) return;
    const content = savedCharacters.exportJson();
    const filename = "mooshieui-characters.json";
    try {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({ defaultPath: filename, filters: [{ name: "JSON", extensions: ["json"] }] });
        if (!path) return;
        await saveTextFile(content, path);
      } else {
        const url = URL.createObjectURL(new Blob([content], { type: "application/json" }));
        const a = document.createElement("a");
        a.href = url;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
      }
      gallery.showToast(locale.t("characters.toast.exported", { count: String(count) }), "success");
    } catch (e) {
      console.error("Character export failed:", e);
      gallery.showToast(String(e), "error");
    }
  }

  async function importCharacters(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const result = savedCharacters.importJson(await file.text());
    if (!result) {
      gallery.showToast(locale.t("characters.toast.import_invalid"), "error");
      return;
    }
    gallery.showToast(
      locale.t("characters.toast.imported", { added: String(result.added), skipped: String(result.skipped) }),
      result.added > 0 ? "success" : "info",
    );
  }

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
      const negative = extractionNegative();
      const saved = savedCharacters.currentCharacters;
      const found = await promptAssistant.extractCharacters(source, negative, saved);
      if (found.length === 0) {
        gallery.showToast(locale.t("characters.toast.none_found"), "info");
        return;
      }
      // Sorted three ways: new, already saved word for word (nothing to do),
      // and saved but different, which the user decides on.
      const fresh: ExtractedCharacter[] = [];
      const matches: DuplicateMatch[] = [];
      for (const item of found) {
        const dup = findDuplicate(item, saved);
        if (!dup) fresh.push(item);
        else if (!sameContent(item, dup) && !matches.some((m) => m.saved.id === dup.id)) {
          matches.push({ item, saved: dup });
        }
      }
      if (matches.length > 0) {
        duplicateReview = { arch, fresh, matches, source, negative };
        return;
      }
      const added = savedCharacters.saveExtracted(fresh, arch);
      gallery.showToast(
        added === 0
          ? locale.t("characters.toast.already_saved")
          : locale.t("characters.toast.saved", {
              added: String(added),
              updated: "0",
              arch: architectureLabel(arch),
            }),
        added === 0 ? "info" : "success",
      );
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
   * Carry out the duplicate review: new characters and variants become cards,
   * and each Update merges the prompt's details into the saved card the same
   * way the card's own Update button does. One Undo covers every update.
   */
  async function applyDuplicates(decisions: DuplicateDecision[]) {
    const pending = duplicateReview;
    duplicateReview = null;
    if (!pending) return;
    const { arch, fresh, source, negative } = pending;
    const variants = decisions
      .filter((d) => d.action === "variant" && d.variantName.trim())
      .map((d) => ({
        name: d.variantName.trim(),
        prompt: d.item.prompt,
        negative: d.item.negative,
        parentId: d.saved.parentId ?? d.saved.id,
      }));
    const added = savedCharacters.saveExtracted(fresh, arch);
    const addedVariants = savedCharacters.saveExtracted(variants, arch);
    const previous: { id: string; prompt: string; negative: string }[] = [];
    extracting = true;
    try {
      for (const d of decisions.filter((x) => x.action === "update")) {
        let next: { prompt: string; negative: string };
        try {
          const merged = await promptAssistant.updateCharacter(d.saved, source, negative);
          // The duplicate check already placed this character in the prompt,
          // so a "not found" here means the model missed it: the extracted
          // text stands in.
          next = merged === "missing" ? { prompt: d.item.prompt, negative: d.item.negative } : merged;
        } catch (e) {
          console.error("Character update failed:", e);
          gallery.showToast(mapLlmError(String(e)), "error");
          continue;
        }
        if (next.prompt === d.saved.prompt && next.negative === d.saved.negative) continue;
        previous.push({ id: d.saved.id, prompt: d.saved.prompt, negative: d.saved.negative });
        savedCharacters.update(d.saved.id, next);
      }
    } finally {
      extracting = false;
    }
    if (added + addedVariants + previous.length === 0) return;
    gallery.showToast(
      locale.t("characters.toast.dupes_saved", {
        added: String(added),
        variants: String(addedVariants),
        updated: String(previous.length),
      }),
      "success",
      previous.length > 0
        ? {
            actionLabel: locale.t("characters.undo"),
            onAction: () => {
              for (const p of previous) savedCharacters.update(p.id, { prompt: p.prompt, negative: p.negative });
            },
            durationMs: 8000,
          }
        : false,
    );
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
    void attachReference(r.character);
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
    if (result !== "duplicate") void attachReference(character);
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
  async function setThumb(id: string, make: () => Promise<CharacterImages | null>, emptyKey: string) {
    thumbBusyId = id;
    thumbMenuId = null;
    try {
      const images = await make();
      if (images) savedCharacters.setThumbnail(id, images.thumbnail, images.reference);
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

{#snippet card(character: SavedCharacter)}
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
          class="shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800 {moreMenuId === character.id ? 'bg-neutral-800 text-neutral-200' : ''}"
          title={locale.t("characters.more")}
          aria-label={locale.t("characters.more")}
          aria-expanded={moreMenuId === character.id}
          onclick={() => { moreMenuId = moreMenuId === character.id ? null : character.id; }}
        >
          {#if copyingId === character.id}
            <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
          {:else}
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="19" cy="12" r="1.8"/></svg>
          {/if}
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
      {#if moreMenuId === character.id}
        <div class="flex flex-wrap items-center gap-1.5 text-[11px]">
          <button
            type="button"
            class="px-2 py-0.5 rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
            onclick={() => addVariant(character)}
          >{locale.t("characters.variants.add")}</button>
          <label class="flex items-center gap-1 text-neutral-400">
            {locale.t("characters.variants.of")}
            <select
              class="max-w-[9rem] bg-neutral-800 border border-neutral-700 rounded px-1 py-0.5 text-[11px] text-neutral-200 focus:outline-none focus:border-indigo-500"
              value={character.parentId ?? ""}
              onchange={(e) => {
                savedCharacters.setParent(character.id, (e.currentTarget as HTMLSelectElement).value || null);
                moreMenuId = null;
              }}
            >
              <option value="">{locale.t("characters.variants.none")}</option>
              {#each parentChoices(character) as parent (parent.id)}
                <option value={parent.id}>{parent.name}</option>
              {/each}
            </select>
          </label>
          <label class="flex items-center gap-1 text-neutral-400">
            {locale.t("characters.copy_to")}
            <select
              class="max-w-[9rem] bg-neutral-800 border border-neutral-700 rounded px-1 py-0.5 text-[11px] text-neutral-200 focus:outline-none focus:border-indigo-500 disabled:opacity-40"
              value=""
              disabled={copyingId !== null}
              onchange={(e) => copyTo(character, (e.currentTarget as HTMLSelectElement).value)}
            >
              <option value="">{locale.t("characters.copy_to_placeholder")}</option>
              {#each characterArchitectures().filter((a) => a !== character.architecture) as arch (arch)}
                <option value={arch}>{architectureLabel(arch)}</option>
              {/each}
            </select>
          </label>
        </div>
      {/if}
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
{/snippet}

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
      class="shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800"
      title={locale.t("characters.import_tip")}
      aria-label={locale.t("characters.import")}
      onclick={() => importInput?.click()}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
    </button>
    <button
      type="button"
      class="shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed"
      title={locale.t("characters.export_tip")}
      aria-label={locale.t("characters.export")}
      disabled={savedCharacters.characters.length === 0}
      onclick={exportCharacters}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
    </button>
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
  {:else if groups.length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
      <p>{locale.t("characters.no_results")}</p>
    </div>
  {:else}
    <div class="flex-1 min-h-0 overflow-y-auto [scrollbar-gutter:stable] px-2 py-2">
      <div class="grid gap-2" style="grid-template-columns: repeat(auto-fill, minmax(min(220px, 100%), 1fr)); align-content: start;">
        {#each groups as group (group.top.id)}
          <div class="flex flex-col gap-1.5 min-w-0">
            {@render card(group.top)}
            {#if group.variants.length > 0}
              <button
                type="button"
                class="self-start flex items-center gap-1 px-1.5 py-0.5 text-[10px] rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
                aria-expanded={group.open}
                onclick={() => toggleExpanded(group.top.id)}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5 transition-transform {group.open ? 'rotate-90' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
                {locale.t("characters.variants.count", { count: String(group.variants.length) })}
              </button>
              {#if group.open}
                <div class="ml-3 pl-2 border-l border-neutral-800 flex flex-col gap-1.5">
                  {#each group.variants as variant (variant.id)}
                    {@render card(variant)}
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

{#if duplicateReview}
  <CharacterDuplicatesModal
    matches={duplicateReview.matches}
    fresh={duplicateReview.fresh.map((c) => c.name)}
    onsave={applyDuplicates}
    onclose={() => { duplicateReview = null; }}
  />
{/if}

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

<input
  bind:this={importInput}
  type="file"
  accept="application/json,.json"
  class="hidden"
  onchange={importCharacters}
/>
