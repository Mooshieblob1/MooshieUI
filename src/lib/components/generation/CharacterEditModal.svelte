<script lang="ts">
  /**
   * Full-app editor for a saved character: its name, prompt and UC, and its
   * memory files, the notes on who the character is that the prompt assistant
   * reads. Everything is a draft until Save. Remember, and an Update that
   * found something lasting, come back as suggested memory changes the user
   * ticks, edits and keeps.
   */
  import { untrack } from "svelte";
  import { portal } from "../../utils/portal.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import {
    architectureLabel,
    savedCharacters,
    type SavedCharacter,
  } from "../../stores/savedCharacters.svelte.js";
  import {
    MEMORY_CONTEXT_LIMIT,
    MEMORY_FILE_LIMIT,
    MEMORY_MAX_FILES,
    MEMORY_STARTER_FILES,
    applyMemoryChanges,
    cleanMemoryFileName,
    memorySize,
    newMemoryFile,
    sameMemory,
    type CharacterMemoryFile,
    type MemoryChange,
  } from "../../utils/characterMemory.js";
  import { saveTextFile } from "../../utils/api.js";
  import { mapLlmError } from "../../utils/llmError.js";

  type Tab = "details" | "memory";

  interface Props {
    character: SavedCharacter;
    tab?: Tab;
    /** Memory changes to review on opening, from Update. */
    suggestions?: MemoryChange[];
    onclose: () => void;
  }

  let { character, tab: startTab = "details", suggestions = [], onclose }: Props = $props();

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
  }

  let name = $state(initial.name);
  let prompt = $state(initial.prompt);
  let negative = $state(initial.negative);
  let files = $state<CharacterMemoryFile[]>(initialFiles.map((f) => ({ ...f })));
  let selectedId = $state<string | null>(initialFiles[0]?.id ?? null);
  let tab = $state<Tab>(untrack(() => startTab));
  let fact = $state("");
  let remembering = $state(false);
  let review = $state<ReviewRow[] | null>(
    untrack(() => (suggestions.length > 0 ? suggestions.map((c, i) => ({ ...c, key: i, keep: true })) : null)),
  );
  /** File name being typed, committed (cleaned and made unique) on blur. */
  let nameDraft = $state("");
  let importInput = $state<HTMLInputElement | null>(null);

  const selected = $derived(files.find((f) => f.id === selectedId) ?? null);
  const detailsDirty = $derived(
    name !== initial.name || prompt !== initial.prompt || negative !== initial.negative,
  );
  const memoryDirty = $derived(!sameMemory(files, initialFiles));
  const dirty = $derived(detailsDirty || memoryDirty);
  const canSave = $derived(dirty && !!name.trim() && !!prompt.trim() && review === null && !remembering);
  const showMemory = $derived(promptAssistant.canUseMemory || initialFiles.length > 0 || review !== null);
  const used = $derived(memorySize(files));
  const canRemember = $derived(
    promptAssistant.canUseMemory && !remembering && !promptAssistant.isGenerating && !!fact.trim() && review === null,
  );

  const selectedName = $derived(selected?.name ?? "");
  $effect(() => {
    nameDraft = selectedName;
  });

  function close() {
    if (dirty && !confirm(locale.t("characters.editor.discard_confirm", { name: initial.name }))) return;
    onclose();
  }

  function save() {
    if (!canSave) return;
    if (detailsDirty) savedCharacters.update(character.id, { name, prompt, negative });
    if (memoryDirty) savedCharacters.setMemory(owner.id, files);
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

  async function remember() {
    const text = fact.trim();
    if (!canRemember || !text) return;
    remembering = true;
    try {
      const changes = await promptAssistant.rememberForCharacter(
        { name: name.trim() || initial.name, prompt, negative },
        files,
        text,
      );
      fact = "";
      if (changes.length === 0) {
        gallery.showToast(locale.t("characters.memory.nothing", { name: owner.name }), "info");
        return;
      }
      review = changes.map((c, i) => ({ ...c, key: i, keep: true }));
    } catch (err) {
      console.error("Character memory failed:", err);
      const msg = String(err);
      gallery.showToast(
        msg.includes("invalid_character_memory") ? locale.t("characters.memory.invalid") : mapLlmError(msg),
        "error",
      );
    } finally {
      remembering = false;
    }
  }

  function keepReview() {
    const rows = review;
    if (!rows) return;
    const kept = rows.filter((r) => r.keep && r.after.trim());
    files = applyMemoryChanges(files, kept);
    const first = kept[0];
    if (first) {
      selectedId =
        files.find((f) => f.id === first.fileId)?.id ??
        files.find((f) => f.name.toLowerCase() === first.name.toLowerCase())?.id ??
        selectedId;
    }
    review = null;
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
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
      {#if character.thumbnail}
        <img src={character.thumbnail} alt={character.name} class="w-10 h-10 rounded-md object-cover border border-neutral-700" />
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
        class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
        onclick={close}
        aria-label={locale.t("common.cancel")}
      >✕</button>
    </div>

    {#if showMemory}
      <div class="flex gap-1 px-5 pt-3 border-b border-neutral-800" role="tablist">
        {#each [["details", "characters.editor.details"], ["memory", "characters.editor.memory"]] as [id, key] (id)}
          <button
            type="button"
            role="tab"
            aria-selected={tab === id}
            class="-mb-px flex items-center gap-1.5 border-b-2 px-3 py-1.5 text-xs font-medium transition-colors {tab === id ? 'border-indigo-500 text-neutral-100' : 'border-transparent text-neutral-400 hover:text-neutral-200'}"
            onclick={() => { tab = id as Tab; }}
          >
            {locale.t(key)}
            {#if id === "memory" && files.length > 0}
              <span class="rounded-full bg-neutral-700 px-1.5 text-[10px] text-neutral-200">{files.length}</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}

    <div class="min-h-0 flex-1 overflow-hidden px-5 py-4 flex flex-col gap-3">
      {#if tab === "details" || !showMemory}
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
                  class="truncate rounded px-2 py-1 text-left font-mono text-[11px] transition-colors {file.id === selectedId ? 'bg-neutral-700 text-neutral-50' : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}"
                  title={file.name}
                  onclick={() => { selectedId = file.id; }}
                >{file.name}</button>
              {/each}
            </div>
            {#if files.length > 0}
              <span
                class="text-[10px] tabular-nums {used > MEMORY_CONTEXT_LIMIT ? 'text-amber-400' : 'text-neutral-500'}"
                title={locale.t("characters.memory.size_tip", { limit: String(MEMORY_CONTEXT_LIMIT) })}
              >{used} / {MEMORY_CONTEXT_LIMIT}</span>
            {/if}
          </div>

          <div class="min-h-0 min-w-0 flex-1 flex flex-col gap-1.5">
            {#if selected}
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
