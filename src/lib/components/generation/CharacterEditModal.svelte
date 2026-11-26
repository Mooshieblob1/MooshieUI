<script lang="ts">
  /**
   * Full-app editor for a saved character: its name, prompt and UC, and its
   * memory files, the notes on who the character is that the prompt assistant
   * reads. Everything is a draft until Save. Remember (a note the assistant
   * writes up properly), Write with AI (files drafted from canon, the prompt
   * and what the user asks for), and an Update that found something lasting
   * come back as suggested memory changes the user ticks, edits and keeps.
   */
  import { untrack } from "svelte";
  import { portal } from "../../utils/portal.js";
  import { enhanceChat } from "../../stores/enhanceChat.svelte.js";
  import { gallery, isVideoImage } from "../../stores/gallery.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { promptAssistant, type MemorySuggestions } from "../../stores/promptAssistant.svelte.js";
  import {
    NOVELAI_ARCHITECTURE,
    architectureLabel,
    savedCharacters,
    type SavedCharacter,
  } from "../../stores/savedCharacters.svelte.js";
  import {
    MEMORY_FILE_LIMIT,
    MEMORY_MAX_FILES,
    MEMORY_STARTER_FILES,
    addLearnedFact,
    applyMemoryChanges,
    cleanMemoryFileName,
    memoryContext,
    memoryForRequest,
    memorySize,
    newMemoryFile,
    sameMemory,
    type CharacterMemoryFile,
    type LearnedMemory,
    type LinkedCharacter,
    type MemoryChange,
  } from "../../utils/characterMemory.js";
  import { effectiveCharacterPreciseReferences } from "../../utils/characterPreciseRefLogic.js";
  import {
    blobsToCharacterImages,
    outputImagesToBlobs,
    readImageBlobsFromClipboard,
    tauriDropFiles,
    tauriPathsToFiles,
  } from "../../utils/characterReferenceInput.js";
  import { currentImageToCharacterThumbnail } from "../../utils/characterThumbnail.js";
  import { PRECISE_REFERENCE_COST } from "../../utils/novelaiCost.js";
  import {
    NAI_REF_OUTFIT,
    NOVELAI_MAX_DIRECTOR_REFERENCES,
    NOVELAI_OUTFIT_REFERENCE_OPTION,
    NOVELAI_REFERENCE_DESCRIPTIONS,
    novelAiReferenceBaseType,
    novelAiReferencePreferSameOutfit,
    novelAiReferenceSupportsSameOutfit,
    novelAiSetPreferSameOutfit,
  } from "../../utils/novelaiModels.js";
  import { CHECKERBOARD_CLASS } from "../../utils/transparentBackdrop.js";
  import { saveTextFile } from "../../utils/api.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import type { OutputImage } from "../../types/index.js";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  type Tab = "details" | "references" | "memory";

  interface Props {
    character: SavedCharacter;
    tab?: Tab;
    /** Memory changes to review on opening, from Update. */
    suggestions?: MemoryChange[];
    ongenerateref?: (character: SavedCharacter) => void;
    onclose: () => void;
  }

  let { character, tab: startTab = "details", suggestions = [], ongenerateref, onclose }: Props = $props();

  // Mounted once per edit, so the card is read once on purpose: the draft is
  // compared against what it was when the editor opened.
  const initial = untrack(() => ({
    name: character.name,
    prompt: character.prompt,
    negative: character.negative,
  }));
  const owner = untrack(() => savedCharacters.memoryOwner(character));
  const initialFiles = owner.memory.map((f) => ({ ...f }));
  const shared = untrack(
    () => character.parentId !== null || savedCharacters.variantsOf(character.id).length > 0,
  );

  interface ReviewRow extends MemoryChange {
    key: number;
    keep: boolean;
    /** Another character's memory owner this change is for (the matching side of a relationship), or null. */
    ownerId: string | null;
    ownerName: string;
  }

  /** Another saved character a request read, with the card whose memory it is. */
  type Linked = LinkedCharacter & { ownerId: string };

  let name = $state(initial.name);
  let prompt = $state(initial.prompt);
  let negative = $state(initial.negative);
  let files = $state<CharacterMemoryFile[]>(initialFiles.map((f) => ({ ...f })));
  let selectedId = $state<string | null>(initialFiles[0]?.id ?? null);
  let tab = $state<Tab>(untrack(() => startTab));
  let fact = $state("");
  let remembering = $state(false);
  /** The Write with AI panel is open in place of the file editor. */
  let composing = $state(false);
  let brief = $state("");
  let review = $state<ReviewRow[] | null>(
    untrack(() =>
      suggestions.length > 0
        ? suggestions.map((c, i) => ({ ...c, key: i, keep: true, ownerId: null, ownerName: owner.name }))
        : null,
    ),
  );
  /** Kept changes to other characters' memory, by memory owner, written on Save. */
  let otherDrafts = $state<Record<string, CharacterMemoryFile[]>>({});
  /** File name being typed, committed (cleaned and made unique) on blur. */
  let nameDraft = $state("");
  let importInput = $state<HTMLInputElement | null>(null);
  let refFileInput = $state<HTMLInputElement | null>(null);
  let refPickerOpen = $state(false);
  let refDragOver = $state(false);
  let refBusy = $state(false);
  let zoomedSrc = $state<string | null>(null);

  const liveCard = $derived(
    savedCharacters.characters.find((c) => c.id === character.id) ?? character,
  );
  const liveRefs = $derived(liveCard.preciseReferences ?? []);
  const effectiveRefs = $derived(effectiveCharacterPreciseReferences(liveCard));
  const showReferences = $derived(
    character.architecture === NOVELAI_ARCHITECTURE ||
      generation.isNovelAi ||
      liveRefs.length > 0,
  );
  const supportsOutfits = $derived(generation.novelAiModel?.outfitReferences ?? true);
  const strengthMax = $derived(
    character.architecture === NOVELAI_ARCHITECTURE
      ? Math.max(generation.novelAiModel?.preciseReferenceStrengthMax ?? 1, 5)
      : generation.novelAiModel?.preciseReferenceStrengthMax ?? 5,
  );
  const refBusyOrFull = $derived(
    refBusy || liveRefs.length >= NOVELAI_MAX_DIRECTOR_REFERENCES,
  );
  const portraitGenerating = $derived(savedCharacters.portraitPromptIds(liveCard.id).length > 0);

  const selected = $derived(files.find((f) => f.id === selectedId) ?? null);
  const detailsDirty = $derived(
    name !== initial.name || prompt !== initial.prompt || negative !== initial.negative,
  );
  const memoryDirty = $derived(!sameMemory(files, initialFiles));
  const dirty = $derived(detailsDirty || memoryDirty || Object.keys(otherDrafts).length > 0);
  const canSave = $derived(dirty && !!name.trim() && !!prompt.trim() && review === null && !remembering);
  const showMemory = $derived(promptAssistant.canUseMemory || initialFiles.length > 0 || review !== null);
  const contextLimit = $derived(promptAssistant.memoryBudget.perCharacter);
  const used = $derived(memorySize(files));
  const canRemember = $derived(
    promptAssistant.canUseMemory && !remembering && !promptAssistant.isGenerating && !!fact.trim() && review === null,
  );
  const canWrite = $derived(
    promptAssistant.canUseMemory && !remembering && !promptAssistant.isGenerating && review === null,
  );

  const selectedName = $derived(selected?.name ?? "");
  $effect(() => {
    nameDraft = selectedName;
  });

  function close() {
    if (dirty && !confirm(locale.t("characters.editor.discard_confirm", { name: initial.name }))) return;
    onclose();
  }

  function openChatEdit() {
    if (!promptAssistant.isAvailable) {
      promptAssistant.setupModalOpen = true;
      return;
    }
    if (dirty && !confirm(locale.t("characters.editor.discard_confirm", { name: initial.name }))) return;
    onclose();
    enhanceChat.openCharacterChat(character.id);
  }

  function save() {
    if (!canSave) return;
    if (detailsDirty) savedCharacters.update(character.id, { name, prompt, negative });
    if (memoryDirty) savedCharacters.setMemory(owner.id, files);
    for (const [id, memory] of Object.entries(otherDrafts)) savedCharacters.setMemory(id, memory);
    onclose();
  }

  function full(): boolean {
    if (files.length < MEMORY_MAX_FILES) return false;
    gallery.showToast(
      locale.t("characters.memory.full", { name: owner.name, max: String(MEMORY_MAX_FILES) }),
      "error",
    );
    return true;
  }

  function addFile(fileName = "notes.md", content = "") {
    if (full()) return;
    const file = newMemoryFile(fileName, content, files.map((f) => f.name));
    files = [...files, file];
    selectedId = file.id;
  }

  function setContent(id: string, content: string) {
    files = files.map((f) => (f.id === id ? { ...f, content, updatedAt: Date.now() } : f));
  }

  /** Commit the typed file name: cleaned, and kept as it was when empty or taken. */
  function commitName() {
    const file = selected;
    if (!file) return;
    const clean = cleanMemoryFileName(nameDraft);
    const taken = files.some((f) => f.id !== file.id && f.name.toLowerCase() === clean.toLowerCase());
    if (!clean || taken || clean === file.name) {
      nameDraft = file.name;
      return;
    }
    files = files.map((f) => (f.id === file.id ? { ...f, name: clean } : f));
  }

  function deleteFile(file: CharacterMemoryFile) {
    if (file.content.trim() && !confirm(locale.t("characters.memory.delete_confirm", { file: file.name }))) return;
    const index = files.findIndex((f) => f.id === file.id);
    files = files.filter((f) => f.id !== file.id);
    selectedId = files[Math.min(index, files.length - 1)]?.id ?? null;
  }

  async function importFiles(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const picked = [...(input.files ?? [])];
    input.value = "";
    for (const file of picked) {
      if (full()) break;
      addFile(file.name, (await file.text()).slice(0, MEMORY_FILE_LIMIT));
    }
  }

  async function exportFile(file: CharacterMemoryFile) {
    try {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        const { save: pick } = await import("@tauri-apps/plugin-dialog");
        const path = await pick({ defaultPath: file.name, filters: [{ name: "Markdown", extensions: ["md"] }] });
        if (!path) return;
        await saveTextFile(file.content, path);
      } else {
        const url = URL.createObjectURL(new Blob([file.content], { type: "text/markdown" }));
        const a = document.createElement("a");
        a.href = url;
        a.download = file.name;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
      }
    } catch (err) {
      console.error("Memory file export failed:", err);
      gallery.showToast(String(err), "error");
    }
  }

  /**
   * Other saved characters (same architecture, other memory) that `texts`
   * name, with their notes, so Remember and Write can connect to them.
   */
  function linkedCharacters(texts: string[]): Linked[] {
    const others = savedCharacters
      .withMemory(savedCharacters.characters.filter((c) => c.architecture === character.architecture))
      .filter((c) => c.ownerId !== owner.id);
    return memoryForRequest(others, texts, "", promptAssistant.memoryBudget).map((c) => ({
      name: c.name,
      prompt: c.prompt,
      notes: c.notes,
      ownerId: c.ownerId,
    }));
  }

  /** Another character's memory as this editor has it: with changes kept earlier, or as saved. */
  function filesOf(ownerId: string): CharacterMemoryFile[] {
    return otherDrafts[ownerId] ?? savedCharacters.characters.find((c) => c.id === ownerId)?.memory ?? [];
  }

  /** The linked character a reply means by `name`: exact, without a variant label, then contained. */
  function matchLinked(name: string, linked: readonly Linked[]): Linked | null {
    const want = name.trim().toLowerCase();
    const bare = (n: string) => n.replace(/\s*\([^)]*\)\s*$/, "").trim().toLowerCase();
    return (
      linked.find((c) => c.name.trim().toLowerCase() === want) ??
      linked.find((c) => bare(c.name) === bare(want)) ??
      linked.find((c) => want.includes(bare(c.name)) || bare(c.name).includes(want)) ??
      null
    );
  }

  /** The other characters' matching facts as review rows, one per file they change. */
  function otherRows(others: readonly LearnedMemory[], linked: readonly Linked[]): Omit<ReviewRow, "key" | "keep">[] {
    const pending = new Map<string, CharacterMemoryFile[]>();
    for (const other of others) {
      const target = matchLinked(other.name, linked);
      if (!target) continue;
      const next = addLearnedFact(pending.get(target.ownerId) ?? filesOf(target.ownerId), other.file, other.fact);
      if (next) pending.set(target.ownerId, next);
    }
    const rows: Omit<ReviewRow, "key" | "keep">[] = [];
    for (const [ownerId, next] of pending) {
      const before = filesOf(ownerId);
      const ownerName = savedCharacters.characters.find((c) => c.id === ownerId)?.name ?? "";
      for (const file of next) {
        const was = before.find((f) => f.id === file.id);
        if (was && was.content === file.content) continue;
        rows.push({
          fileId: was ? was.id : null,
          name: file.name,
          before: was?.content ?? "",
          after: file.content,
          ownerId,
          ownerName,
        });
      }
    }
    return rows;
  }

  /** Run a memory request and open its suggestions for review. */
  async function suggest(
    linked: Linked[],
    run: (linked: Linked[]) => Promise<MemorySuggestions>,
    nothingKey: string,
  ): Promise<boolean> {
    remembering = true;
    try {
      const { changes, others } = await run(linked);
      const rows = [
        ...changes.map((c) => ({ ...c, ownerId: null, ownerName: owner.name })),
        ...otherRows(others, linked),
      ];
      if (rows.length === 0) {
        gallery.showToast(locale.t(nothingKey, { name: owner.name }), "info");
        return true;
      }
      review = rows.map((r, i) => ({ ...r, key: i, keep: true }));
      return true;
    } catch (err) {
      console.error("Character memory failed:", err);
      const msg = String(err);
      gallery.showToast(
        msg.includes("invalid_character_memory") ? locale.t("characters.memory.invalid") : mapLlmError(msg),
        "error",
      );
      return false;
    } finally {
      remembering = false;
    }
  }

  async function remember() {
    const text = fact.trim();
    if (!canRemember || !text) return;
    const draft = { name: name.trim() || initial.name, prompt, negative };
    const done = await suggest(
      linkedCharacters([text]),
      (linked) => promptAssistant.rememberForCharacter(draft, files, text, linked),
      "characters.memory.nothing",
    );
    if (done) fact = "";
  }

  async function writeMemory() {
    if (!canWrite) return;
    const draft = { name: name.trim() || initial.name, prompt, negative };
    const ask = brief.trim();
    const done = await suggest(
      linkedCharacters([ask, memoryContext(files, 0)]),
      (linked) => promptAssistant.writeCharacterMemory(draft, files, ask, linked),
      "characters.memory.write_nothing",
    );
    if (done) {
      composing = false;
      brief = "";
    }
  }

  function keepReview() {
    const rows = review;
    if (!rows) return;
    const kept = rows.filter((r) => r.keep && r.after.trim());
    const mine = kept.filter((r) => r.ownerId === null);
    files = applyMemoryChanges(files, mine);
    const drafts = { ...otherDrafts };
    for (const id of new Set(kept.map((r) => r.ownerId).filter((id): id is string => id !== null))) {
      drafts[id] = applyMemoryChanges(filesOf(id), kept.filter((r) => r.ownerId === id));
    }
    otherDrafts = drafts;
    const first = mine[0];
    if (first) {
      selectedId =
        files.find((f) => f.id === first.fileId)?.id ??
        files.find((f) => f.name.toLowerCase() === first.name.toLowerCase())?.id ??
        selectedId;
    }
    review = null;
  }

  async function addRefBlobs(blobs: readonly Blob[], enablePr = false) {
    if (blobs.length === 0 || refBusy) return;
    refBusy = true;
    try {
      const { items, failed } = await blobsToCharacterImages(blobs);
      const res = savedCharacters.addPreciseReferences(liveCard.id, items, { enablePr });
      if (res.hitCap) {
        gallery.showToast(
          locale.t("characters.references.toast_full", {
            max: String(NOVELAI_MAX_DIRECTOR_REFERENCES),
          }),
          "info",
        );
      } else if (res.duplicates > 0 && res.added.length === 0) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_duplicate"), "info");
      } else if (failed > 0 && res.added.length === 0 && res.duplicates === 0) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
      }
    } finally {
      refBusy = false;
    }
  }

  async function addRefFromClipboard(clipboardItems?: DataTransferItemList) {
    const blobs = await readImageBlobsFromClipboard(clipboardItems);
    if (blobs.length === 0) {
      gallery.showToast(locale.t("common.no_clipboard_image"), "info");
      return;
    }
    await addRefBlobs(blobs);
  }

  async function addRefFromGallery(images: OutputImage[]) {
    refPickerOpen = false;
    if (images.length === 0 || refBusy) return;
    refBusy = true;
    try {
      const blobs = await outputImagesToBlobs(images);
      refBusy = false;
      if (blobs.length > 0) await addRefBlobs(blobs);
      else gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
    } finally {
      refBusy = false;
    }
  }

  async function addRefFromCurrent() {
    const latest = gallery.sessionImages.find((image) => !isVideoImage(image)) ?? null;
    if (!progress.lastOutputImage && !latest) {
      gallery.showToast(locale.t("characters.thumbnail.no_current"), "info");
      return;
    }
    refBusy = true;
    try {
      const imgs = await currentImageToCharacterThumbnail(progress.lastOutputImage, latest);
      if (!imgs) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
        return;
      }
      const res = savedCharacters.addPreciseReferences(liveCard.id, [imgs]);
      if (res.hitCap) {
        gallery.showToast(
          locale.t("characters.references.toast_full", { max: String(NOVELAI_MAX_DIRECTOR_REFERENCES) }),
          "info",
        );
      } else if (res.duplicates > 0 && res.added.length === 0) {
        gallery.showToast(locale.t("generation.novelai.reference.toast_duplicate"), "info");
      }
    } finally {
      refBusy = false;
    }
  }

  function addRefFromCardPicture() {
    const thumb = liveCard.thumbnail ?? liveCard.reference;
    const ref = liveCard.reference ?? liveCard.thumbnail;
    if (!thumb || !ref) return;
    const res = savedCharacters.addPreciseReferences(
      liveCard.id,
      [{ thumbnail: thumb, reference: ref }],
    );
    if (res.hitCap) {
      gallery.showToast(
        locale.t("characters.references.toast_full", { max: String(NOVELAI_MAX_DIRECTOR_REFERENCES) }),
        "info",
      );
    } else if (res.duplicates > 0 && res.added.length === 0) {
      gallery.showToast(locale.t("generation.novelai.reference.toast_duplicate"), "info");
    }
  }

  async function addRefFromTauriPaths(paths: string[]) {
    try {
      const files = await tauriPathsToFiles(paths);
      await addRefBlobs(files);
    } catch (err) {
      console.error("Character reference drop failed:", err);
      gallery.showToast(locale.t("generation.novelai.reference.toast_failed"), "error");
    }
  }

  function removeOneRef(refId: string) {
    const prev = [...liveRefs];
    savedCharacters.removePreciseReference(liveCard.id, refId);
    gallery.showToast(locale.t("characters.references.toast_removed"), "info", {
      actionLabel: locale.t("characters.undo"),
      onAction: () => savedCharacters.setPreciseReferences(liveCard.id, prev),
      durationMs: 6000,
    });
  }

  function clearAllRefs() {
    const prev = [...liveRefs];
    if (prev.length === 0) return;
    savedCharacters.clearPreciseReferences(liveCard.id);
    gallery.showToast(locale.t("characters.references.toast_cleared"), "info", {
      actionLabel: locale.t("characters.undo"),
      onAction: () => savedCharacters.setPreciseReferences(liveCard.id, prev),
      durationMs: 6000,
    });
  }

  $effect(() => {
    const handler = (event: ClipboardEvent) => {
      if (tab !== "references" || !showReferences || refPickerOpen || zoomedSrc || refBusyOrFull) return;
      const el = event.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      void addRefFromClipboard(event.clipboardData?.items);
    };
    window.addEventListener("paste", handler, { capture: true });
    return () => window.removeEventListener("paste", handler, { capture: true });
  });

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (zoomedSrc) {
        e.preventDefault();
        e.stopImmediatePropagation();
        zoomedSrc = null;
        return;
      }
      if (refPickerOpen) return;
      close();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  const fieldBase =
    "w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-neutral-100 focus:outline-none focus:border-indigo-500";
  const field = `${fieldBase} text-xs`;
  const iconButton =
    "shrink-0 w-6 h-6 flex items-center justify-center rounded text-neutral-500 hover:text-neutral-200 hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed";
</script>

<div
  use:portal
  class="fixed inset-0 z-210 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t("characters.editor.title", { name: initial.name })}
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label={locale.t("common.cancel")}
    onclick={close}
  ></button>
  <div class="relative z-10 flex h-[min(90vh,46rem)] w-full max-w-4xl flex-col rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl">
    <div class="flex items-center gap-3 px-5 pt-4">
      {#if liveCard.thumbnail}
        <img src={liveCard.thumbnail} alt={liveCard.name} class="w-10 h-10 rounded-md object-cover border border-neutral-700" />
      {/if}
      <h3 class="min-w-0 truncate text-sm font-semibold text-neutral-100">
        {locale.t("characters.editor.title", { name: initial.name })}
      </h3>
      <span class="shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-neutral-800 border border-neutral-700 text-neutral-400">
        {architectureLabel(character.architecture)}
      </span>
      <span class="flex-1"></span>
      <button
        type="button"
        class="rounded-lg border border-indigo-500/40 bg-indigo-500/15 px-2.5 py-1 text-[11px] font-medium text-indigo-200 hover:bg-indigo-500/25"
        title={locale.t("characters.chat.open_tip")}
        onclick={openChatEdit}
      >
        {locale.t("characters.chat.open")}
      </button>
      <button
        type="button"
        class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
        onclick={close}
        aria-label={locale.t("common.cancel")}
      >✕</button>
    </div>

    {#if showMemory || showReferences}
      <div class="flex gap-1 px-5 pt-3 border-b border-neutral-800" role="tablist">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "details"}
          class="-mb-px flex items-center gap-1.5 border-b-2 px-3 py-1.5 text-xs font-medium transition-colors {tab === 'details' ? 'border-indigo-500 text-neutral-100' : 'border-transparent text-neutral-400 hover:text-neutral-200'}"
          onclick={() => { tab = "details"; }}
        >
          {locale.t("characters.editor.details")}
        </button>
        {#if showReferences}
          <button
            type="button"
            role="tab"
            aria-selected={tab === "references"}
            class="-mb-px flex items-center gap-1.5 border-b-2 px-3 py-1.5 text-xs font-medium transition-colors {tab === 'references' ? 'border-indigo-500 text-neutral-100' : 'border-transparent text-neutral-400 hover:text-neutral-200'}"
            onclick={() => { tab = "references"; }}
          >
            {#if liveCard.usePreciseReference && effectiveRefs.length > 0}
              <span class="w-1.5 h-1.5 rounded-full bg-indigo-400"></span>
            {/if}
            {locale.t("characters.editor.references")}
            {#if liveRefs.length > 0}
              <span class="rounded-full bg-neutral-700 px-1.5 text-[10px] text-neutral-200">{liveRefs.length}</span>
            {/if}
          </button>
        {/if}
        {#if showMemory}
          <button
            type="button"
            role="tab"
            aria-selected={tab === "memory"}
            class="-mb-px flex items-center gap-1.5 border-b-2 px-3 py-1.5 text-xs font-medium transition-colors {tab === 'memory' ? 'border-indigo-500 text-neutral-100' : 'border-transparent text-neutral-400 hover:text-neutral-200'}"
            onclick={() => { tab = "memory"; }}
          >
            {locale.t("characters.editor.memory")}
            {#if files.length > 0}
              <span class="rounded-full bg-neutral-700 px-1.5 text-[10px] text-neutral-200">{files.length}</span>
            {/if}
          </button>
        {/if}
      </div>
    {/if}

    <div class="min-h-0 flex-1 overflow-hidden px-5 py-4 flex flex-col gap-3">
      {#if tab === "references" && showReferences}
        <div class="min-h-0 flex-1 overflow-y-auto flex flex-col gap-3">
          <div class="flex flex-wrap items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-950/60 px-3 py-2">
            <label class="flex items-center gap-2 text-xs font-medium text-neutral-200 cursor-pointer">
              <input
                type="checkbox"
                class="accent-indigo-500"
                checked={!!liveCard.usePreciseReference}
                onchange={(e) => savedCharacters.setUsePreciseReference(liveCard.id, e.currentTarget.checked)}
              />
              {locale.t("characters.references.use_toggle")}
              <InfoTip text={locale.t("characters.references.use_tip")} />
            </label>
            <div class="ml-auto flex items-center gap-2">
              {#if liveCard.usePreciseReference && effectiveRefs.length > 0}
                <span class="rounded bg-indigo-500/20 border border-indigo-500/40 px-2 py-0.5 text-[11px] tabular-nums text-indigo-200">
                  {locale.t("characters.references.anlas_badge", { anlas: String(effectiveRefs.length * PRECISE_REFERENCE_COST) })}
                </span>
              {/if}
              {#if liveRefs.length > 0}
                <button
                  type="button"
                  class="text-[11px] text-neutral-400 hover:text-red-300"
                  onclick={clearAllRefs}
                >
                  {locale.t("generation.novelai.reference.clear_all")}
                </button>
              {/if}
            </div>
          </div>

          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            use:tauriDropFiles={(paths) => void addRefFromTauriPaths(paths)}
            class="border border-dashed rounded-lg px-3 py-2.5 flex flex-wrap items-center justify-center gap-2 text-[11px] transition-colors {refDragOver
              ? 'border-indigo-500 bg-indigo-500/10'
              : 'border-neutral-700 hover:border-neutral-600 bg-neutral-950/40'} {refBusyOrFull
              ? 'opacity-45 pointer-events-none'
              : ''}"
            ondragenter={(e) => { e.preventDefault(); e.stopPropagation(); refDragOver = true; }}
            ondragover={(e) => { e.preventDefault(); e.stopPropagation(); refDragOver = true; }}
            ondragleave={() => { refDragOver = false; }}
            ondrop={(e) => {
              e.preventDefault();
              e.stopPropagation();
              refDragOver = false;
              const dropped = e.dataTransfer?.files;
              if (dropped && dropped.length > 0) void addRefBlobs(Array.from(dropped));
            }}
          >
            {#if ongenerateref}
              <button
                type="button"
                class="px-2.5 py-1 rounded bg-indigo-600 hover:bg-indigo-500 text-white font-medium flex items-center gap-1.5"
                disabled={portraitGenerating}
                onclick={() => ongenerateref(liveCard)}
              >
                {#if portraitGenerating}
                  <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
                {/if}
                {locale.t("characters.references.generate")}
              </button>
            {/if}
            <span class="text-neutral-700">|</span>
            <button
              type="button"
              class="text-emerald-400/90 hover:text-emerald-300"
              onclick={() => void addRefFromClipboard()}
            >{locale.t("generation.image.ctrl_v_paste")}</button>
            <span class="text-neutral-700">|</span>
            <button
              type="button"
              class="text-neutral-300 hover:text-white"
              onclick={() => refFileInput?.click()}
            >{locale.t("generation.image.select_image")}</button>
            <span class="text-neutral-700">|</span>
            <button
              type="button"
              class="text-indigo-400 hover:text-indigo-300"
              onclick={() => { refPickerOpen = true; }}
            >{locale.t("generation.novelai.reference.from_gallery")}</button>
            <span class="text-neutral-700">|</span>
            <button
              type="button"
              class="text-sky-400 hover:text-sky-300"
              onclick={() => void addRefFromCurrent()}
            >{locale.t("characters.thumbnail.current")}</button>
            {#if liveCard.thumbnail}
              <span class="text-neutral-700">|</span>
              <button
                type="button"
                class="text-neutral-300 hover:text-white"
                onclick={addRefFromCardPicture}
              >{locale.t("characters.references.add_card_picture")}</button>
            {/if}
            <span class="text-[10px] text-neutral-500 tabular-nums ml-auto">
              {liveRefs.length}/{NOVELAI_MAX_DIRECTOR_REFERENCES}
            </span>
          </div>
          <input
            bind:this={refFileInput}
            type="file"
            accept="image/*"
            multiple
            class="hidden"
            aria-hidden="true"
            tabindex="-1"
            onchange={(e) => {
              const picked = e.currentTarget.files;
              if (picked && picked.length > 0) void addRefBlobs(Array.from(picked));
              e.currentTarget.value = "";
            }}
          />

          {#if liveRefs.length > 0}
            <div class="grid gap-2 sm:grid-cols-2">
              {#each liveRefs as refItem (refItem.id)}
                {@const desc = refItem.description ?? "character"}
                {@const baseDesc = novelAiReferenceBaseType(desc)}
                <div class="rounded-lg border border-neutral-800 bg-neutral-900/70 p-2.5 flex gap-3">
                  <button
                    type="button"
                    class="shrink-0 w-16 h-16 rounded overflow-hidden border border-neutral-800 hover:border-indigo-500/70 cursor-zoom-in {CHECKERBOARD_CLASS}"
                    title={locale.t("generation.novelai.reference.zoom")}
                    onclick={() => { zoomedSrc = refItem.reference; }}
                  >
                    <img src={refItem.thumbnail} alt="" class="w-full h-full object-cover" />
                  </button>
                  <div class="flex-1 min-w-0 space-y-1.5">
                    <div class="flex items-center gap-1.5">
                      <select
                        class="flex-1 min-w-0 px-2 py-1 text-[11px] rounded-md bg-neutral-950 border border-neutral-800 text-neutral-200 focus:outline-none focus:border-indigo-600"
                        value={baseDesc}
                        onchange={(e) => {
                          const next = e.currentTarget.value;
                          if (next !== baseDesc) {
                            savedCharacters.updatePreciseReference(liveCard.id, refItem.id, { description: next });
                          }
                        }}
                      >
                        {#each NOVELAI_REFERENCE_DESCRIPTIONS as option (option.value)}
                          <option value={option.value}>{locale.t(option.labelKey)}</option>
                        {/each}
                        {#if supportsOutfits || desc === NAI_REF_OUTFIT}
                          <option value={NOVELAI_OUTFIT_REFERENCE_OPTION.value}>
                            {locale.t(NOVELAI_OUTFIT_REFERENCE_OPTION.labelKey)}
                          </option>
                        {/if}
                      </select>
                      <button
                        type="button"
                        class="px-2 py-0.5 text-[11px] rounded text-neutral-400 hover:text-red-400 hover:bg-neutral-800 shrink-0"
                        onclick={() => removeOneRef(refItem.id)}
                      >
                        {locale.t("generation.novelai.reference.remove")}
                      </button>
                    </div>
                    {#if supportsOutfits && novelAiReferenceSupportsSameOutfit(desc)}
                      <label class="flex items-center gap-1.5 text-[11px] text-neutral-300">
                        <input
                          type="checkbox"
                          class="accent-indigo-500"
                          checked={novelAiReferencePreferSameOutfit(desc)}
                          onchange={(e) =>
                            savedCharacters.updatePreciseReference(liveCard.id, refItem.id, {
                              description: novelAiSetPreferSameOutfit(desc, e.currentTarget.checked),
                            })}
                        />
                        {locale.t("generation.novelai.reference.prefer_same_outfit")}
                      </label>
                    {/if}
                    <div class="space-y-0.5">
                      <div class="flex items-center justify-between text-[11px] text-neutral-500">
                        <span>{locale.t("generation.novelai.reference.strength")}</span>
                        <EditableValue
                          value={refItem.strength ?? 1.0}
                          min={0}
                          max={10}
                          step={0.05}
                          decimals={2}
                          onchange={(v) =>
                            savedCharacters.updatePreciseReference(liveCard.id, refItem.id, { strength: v })}
                        />
                      </div>
                      <input
                        type="range"
                        min="0"
                        max={strengthMax}
                        step="0.05"
                        class="w-full accent-indigo-500"
                        value={Math.min(strengthMax, Math.max(0, refItem.strength ?? 1.0))}
                        oninput={(e) =>
                          savedCharacters.updatePreciseReference(liveCard.id, refItem.id, {
                            strength: Number(e.currentTarget.value),
                          })}
                      />
                    </div>
                  </div>
                </div>
              {/each}
            </div>
          {:else if liveCard.thumbnail}
            <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-2.5 flex items-center gap-3">
              <button
                type="button"
                class="shrink-0 w-14 h-14 rounded overflow-hidden border border-neutral-800 hover:border-indigo-500/70 cursor-zoom-in {CHECKERBOARD_CLASS}"
                title={locale.t("generation.novelai.reference.zoom")}
                onclick={() => { zoomedSrc = liveCard.reference ?? liveCard.thumbnail; }}
              >
                <img src={liveCard.thumbnail} alt="" class="w-full h-full object-cover" />
              </button>
              <div class="flex-1 min-w-0">
                <span class="inline-block rounded bg-neutral-800 border border-neutral-700 px-1.5 py-0.5 text-[10px] text-neutral-300">
                  {locale.t("characters.references.card_fallback_badge")}
                </span>
              </div>
            </div>
          {:else}
            <div class="flex-1 min-h-28 flex items-center justify-center rounded-lg border border-dashed border-neutral-800 text-xs text-neutral-500">
              {locale.t("characters.references.empty")}
            </div>
          {/if}
        </div>
      {:else if tab === "details" || !showMemory}
        <div class="min-h-0 flex-1 overflow-y-auto flex flex-col gap-3">
          <label class="flex flex-col gap-1">
            <span class="text-xs font-medium text-neutral-300">{locale.t("characters.name")}</span>
            <input type="text" bind:value={name} class={field} />
          </label>
          <label class="flex flex-col gap-1">
            <span class="text-xs font-medium text-neutral-300">{locale.t("characters.prompt")}</span>
            <textarea bind:value={prompt} rows="8" spellcheck="false" class="{field} resize-y leading-relaxed"></textarea>
          </label>
          <label class="flex flex-col gap-1">
            <span class="text-xs font-medium text-neutral-300">{locale.t("characters.negative")}</span>
            <textarea
              bind:value={negative}
              rows="4"
              spellcheck="false"
              class="{field} resize-y leading-relaxed focus:border-red-500/70"
            ></textarea>
          </label>
        </div>
      {:else if review}
        <div class="flex items-center gap-2">
          <h4 class="flex-1 text-xs font-semibold text-neutral-200">{locale.t("characters.memory.review_title")}</h4>
        </div>
        <div class="min-h-0 flex-1 overflow-y-auto flex flex-col gap-3">
          {#each review as row (row.key)}
            <div class="rounded-lg border p-2.5 flex flex-col gap-2 {row.keep ? 'border-neutral-700 bg-neutral-800/40' : 'border-neutral-800 opacity-60'}">
              <label class="flex items-center gap-2 text-xs text-neutral-200">
                <input type="checkbox" bind:checked={row.keep} class="accent-indigo-500" />
                {#if row.ownerId}
                  <span
                    class="rounded px-1.5 py-px text-[10px] bg-sky-500/15 text-sky-300"
                    title={locale.t("characters.memory.other_tip", { name: row.ownerName })}
                  >{row.ownerName}</span>
                {/if}
                <span class="font-mono">{row.name}</span>
                <span class="rounded px-1.5 py-px text-[10px] {row.fileId ? 'bg-amber-500/15 text-amber-300' : 'bg-emerald-500/15 text-emerald-300'}">
                  {locale.t(row.fileId ? "characters.memory.changed_badge" : "characters.memory.new_badge")}
                </span>
              </label>
              <div class="grid gap-2 {row.fileId ? 'sm:grid-cols-2' : ''}">
                {#if row.fileId}
                  <div class="flex flex-col gap-1 min-w-0">
                    <span class="text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.review.before")}</span>
                    <pre class="max-h-56 overflow-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-neutral-900 px-2 py-1.5 font-mono text-[11px] leading-relaxed text-neutral-400">{row.before}</pre>
                  </div>
                {/if}
                <label class="flex flex-col gap-1 min-w-0">
                  <span class="text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.review.after")}</span>
                  <textarea
                    bind:value={row.after}
                    rows="8"
                    maxlength={MEMORY_FILE_LIMIT}
                    spellcheck="false"
                    disabled={!row.keep}
                    class="{fieldBase} resize-y font-mono text-[11px] leading-relaxed"
                  ></textarea>
                </label>
              </div>
            </div>
          {/each}
        </div>
        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="px-3 py-1.5 text-xs rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
            onclick={() => { review = null; }}
          >{locale.t("characters.memory.discard")}</button>
          <button
            type="button"
            class="px-3 py-1.5 text-xs font-medium rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40"
            disabled={!review.some((r) => r.keep && r.after.trim())}
            onclick={keepReview}
          >{locale.t("characters.memory.keep")}</button>
        </div>
      {:else}
        {#if shared}
          <span class="text-[11px] text-neutral-500">{locale.t("characters.memory.shared", { name: owner.name })}</span>
        {/if}
        <div class="min-h-0 flex-1 flex flex-col sm:flex-row gap-3">
          <div class="sm:w-48 shrink-0 flex flex-col gap-1.5 min-h-0">
            <div class="flex items-center gap-1">
              <button
                type="button"
                class="flex-1 flex items-center justify-center gap-1 px-2 py-1 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
                disabled={files.length >= MEMORY_MAX_FILES}
                onclick={() => addFile()}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
                {locale.t("characters.memory.new_file")}
              </button>
              {#if promptAssistant.canUseMemory}
                <button
                  type="button"
                  class="{iconButton} text-[12px] {composing ? 'bg-neutral-800' : ''}"
                  title={locale.t("characters.memory.write_tip", { name: owner.name })}
                  aria-label={locale.t("characters.memory.write")}
                  aria-pressed={composing}
                  disabled={!canWrite && !composing}
                  onclick={() => { composing = !composing; }}
                >✨</button>
              {/if}
              <button
                type="button"
                class={iconButton}
                title={locale.t("characters.memory.import_tip")}
                aria-label={locale.t("characters.import")}
                disabled={files.length >= MEMORY_MAX_FILES}
                onclick={() => importInput?.click()}
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
              </button>
            </div>
            <div class="min-h-0 flex-1 overflow-y-auto flex sm:flex-col gap-1 max-sm:flex-wrap">
              {#each files as file (file.id)}
                <button
                  type="button"
                  class="truncate rounded px-2 py-1 text-left font-mono text-[11px] transition-colors {file.id === selectedId && !composing ? 'bg-neutral-700 text-neutral-50' : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}"
                  title={file.name}
                  onclick={() => { selectedId = file.id; composing = false; }}
                >{file.name}</button>
              {/each}
            </div>
            {#if files.length > 0}
              <span
                class="text-[10px] tabular-nums {used > contextLimit ? 'text-amber-400' : 'text-neutral-500'}"
                title={locale.t("characters.memory.size_tip", { limit: String(contextLimit) })}
              >{used} / {contextLimit}</span>
            {/if}
          </div>

          <div class="min-h-0 min-w-0 flex-1 flex flex-col gap-1.5">
            {#if composing}
              <form
                class="flex flex-1 flex-col gap-2 rounded-lg border border-neutral-800 bg-neutral-800/30 p-3"
                onsubmit={(e) => { e.preventDefault(); void writeMemory(); }}
              >
                <label for="character-memory-brief" class="text-xs font-medium text-neutral-200">
                  {locale.t("characters.memory.write_title", { name: owner.name })}
                </label>
                <textarea
                  id="character-memory-brief"
                  bind:value={brief}
                  rows="5"
                  disabled={remembering}
                  placeholder={locale.t("characters.memory.write_placeholder", { name: owner.name })}
                  class="{field} min-h-24 flex-1 resize-none leading-relaxed placeholder-neutral-500 disabled:opacity-50"
                  onkeydown={(e) => {
                    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                      e.preventDefault();
                      void writeMemory();
                    }
                  }}
                ></textarea>
                <div class="flex items-center justify-end gap-2">
                  <button
                    type="button"
                    class="px-3 py-1.5 text-xs rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 disabled:opacity-40"
                    disabled={remembering}
                    onclick={() => { composing = false; }}
                  >{locale.t("common.cancel")}</button>
                  <button
                    type="submit"
                    class="px-3 py-1.5 text-xs font-medium rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1.5"
                    disabled={!canWrite}
                  >
                    {#if remembering}
                      <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
                    {/if}
                    {locale.t("characters.memory.write_button")}
                  </button>
                </div>
              </form>
            {:else if selected}
              <div class="flex items-center gap-1">
                <input
                  type="text"
                  bind:value={nameDraft}
                  aria-label={locale.t("characters.memory.file_name")}
                  spellcheck="false"
                  class="{fieldBase} flex-1 py-1 font-mono text-[11px]"
                  onblur={commitName}
                  onkeydown={(e) => { if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur(); }}
                />
                <button
                  type="button"
                  class={iconButton}
                  title={locale.t("characters.memory.export_tip")}
                  aria-label={locale.t("characters.export")}
                  onclick={() => selected && exportFile(selected)}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                </button>
                <button
                  type="button"
                  class="{iconButton} hover:text-red-300 hover:bg-red-600/10"
                  title={locale.t("characters.memory.delete_file")}
                  aria-label={locale.t("characters.memory.delete_file")}
                  onclick={() => selected && deleteFile(selected)}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                </button>
              </div>
              <textarea
                value={selected.content}
                oninput={(e) => setContent(selectedId ?? "", (e.currentTarget as HTMLTextAreaElement).value)}
                aria-label={locale.t("characters.memory.content")}
                maxlength={MEMORY_FILE_LIMIT}
                spellcheck="false"
                readonly={remembering}
                class="{fieldBase} min-h-40 flex-1 resize-none font-mono text-[11px] leading-relaxed"
              ></textarea>
              <span class="self-end text-[10px] tabular-nums text-neutral-500">{selected.content.length} / {MEMORY_FILE_LIMIT}</span>
            {:else}
              <div class="flex flex-1 flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-neutral-800 p-6">
                <span class="text-xs text-neutral-500">{locale.t("characters.memory.empty")}</span>
                <div class="flex flex-wrap justify-center gap-1.5">
                  {#each MEMORY_STARTER_FILES as starter (starter)}
                    <button
                      type="button"
                      class="rounded-full border border-neutral-700 px-2.5 py-0.5 font-mono text-[11px] text-neutral-300 hover:border-neutral-500 hover:bg-neutral-800"
                      onclick={() => addFile(starter)}
                    >+ {starter}</button>
                  {/each}
                </div>
                {#if promptAssistant.canUseMemory}
                  <button
                    type="button"
                    class="mt-1 flex items-center gap-1.5 rounded px-3 py-1.5 text-xs font-medium bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40"
                    title={locale.t("characters.memory.write_tip", { name: owner.name })}
                    disabled={!canWrite}
                    onclick={() => { composing = true; }}
                  >✨ {locale.t("characters.memory.write")}</button>
                {/if}
              </div>
            {/if}
          </div>
        </div>

        <form
          class="flex items-center gap-2"
          onsubmit={(e) => { e.preventDefault(); void remember(); }}
          title={promptAssistant.canUseMemory ? undefined : locale.t("characters.memory.needs_external")}
        >
          <input
            type="text"
            bind:value={fact}
            disabled={!promptAssistant.canUseMemory || remembering}
            placeholder={locale.t("characters.memory.remember_placeholder", { name: owner.name })}
            aria-label={locale.t("characters.memory.remember")}
            class="{field} flex-1 placeholder-neutral-500 disabled:opacity-50"
          />
          <button
            type="submit"
            class="shrink-0 px-3 py-1.5 text-xs font-medium rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1.5"
            disabled={!canRemember}
            title={locale.t(promptAssistant.canUseMemory ? "characters.memory.remember_tip" : "characters.memory.needs_external")}
          >
            {#if remembering}
              <svg class="w-3 h-3 animate-spin" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 12a9 9 0 1 1-6.22-8.56" stroke-linecap="round"/></svg>
            {/if}
            {locale.t("characters.memory.remember")}
          </button>
        </form>
      {/if}
    </div>

    <div class="flex items-center justify-end gap-2 border-t border-neutral-800 px-5 py-3">
      <button
        type="button"
        class="px-3 py-1.5 text-xs rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
        onclick={close}
      >{locale.t("common.cancel")}</button>
      <button
        type="button"
        class="px-3 py-1.5 text-xs font-medium rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40 disabled:cursor-not-allowed"
        disabled={!canSave}
        onclick={save}
      >{locale.t("common.save")}</button>
    </div>
    <input
      bind:this={importInput}
      type="file"
      accept=".md,.markdown,.txt,text/markdown,text/plain"
      multiple
      class="hidden"
      aria-hidden="true"
      tabindex="-1"
      onchange={importFiles}
    />
  </div>
</div>

{#if refPickerOpen}
  <GalleryPickerModal
    open={true}
    multiple
    max={Math.max(1, NOVELAI_MAX_DIRECTOR_REFERENCES - liveRefs.length)}
    title={locale.t("generation.novelai.reference.gallery_pick_title")}
    onselect={(images) => void addRefFromGallery(images)}
    onclose={() => { refPickerOpen = false; }}
  />
{/if}

{#if zoomedSrc}
  <div
    use:portal
    class="fixed inset-0 z-[10000] bg-black/80 backdrop-blur-sm flex items-center justify-center p-4"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("generation.novelai.reference.zoom")}
    tabindex="-1"
  >
    <button
      type="button"
      class="absolute inset-0 h-full w-full cursor-default"
      aria-label={locale.t("common.close")}
      onclick={() => { zoomedSrc = null; }}
    ></button>
    <div class="relative z-10 max-w-[90vw] max-h-[90vh]">
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
