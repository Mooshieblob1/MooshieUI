<script lang="ts">
  import { styles, type ArtistStyle } from "../../stores/styles.svelte.js";
  import { promptPresets, type PromptPreset, type PresetMode } from "../../stores/promptPresets.svelte.js";
  import StyleEditor from "./StyleEditor.svelte";
  import PresetEditor from "./PresetEditor.svelte";
  import PresetActivationModal from "./PresetActivationModal.svelte";

  interface Props {
    onclose?: () => void;
  }

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  let { onclose: _onclose }: Props = $props();

  let activeTab = $state<"styles" | "presets">("styles");

  // Styles tab state
  let editingId = $state<string | null>(null);
  let newName = $state("");

  // Presets tab state
  let editingPresetId = $state<string | null>(null);
  let activatingPresetId = $state<string | null>(null);
  let newPresetName = $state("");

  // Shared import/export
  let importStatus = $state<string | null>(null);
  let importError = $state<string | null>(null);
  let importMode = $state<"merge" | "replace">("merge");
  let fileInput: HTMLInputElement | null = $state(null);

  function createStyle() {
    const name = newName.trim() || `Style ${styles.styles.length + 1}`;
    const created = styles.create(name);
    newName = "";
    editingId = created.id;
  }

  function confirmDelete(style: ArtistStyle) {
    if (!confirm(`Delete "${style.name}"? This cannot be undone.`)) return;
    styles.remove(style.id);
  }

  function createPreset() {
    const name = newPresetName.trim() || `Preset ${promptPresets.presets.length + 1}`;
    const created = promptPresets.create(name);
    newPresetName = "";
    editingPresetId = created.id;
  }

  function confirmDeletePreset(preset: PromptPreset) {
    if (!confirm(`Delete "${preset.name}"? This cannot be undone.`)) return;
    promptPresets.remove(preset.id);
  }

  function onPresetActivateClick(preset: PromptPreset) {
    if (promptPresets.isActive(preset.id)) {
      promptPresets.deactivate(preset.id);
      return;
    }
    activatingPresetId = preset.id;
  }

  function presetModeLabel(mode: PresetMode | null): string {
    if (mode === "prepend") return "Prepend";
    if (mode === "append") return "Append";
    if (mode === "wildcard") return "Wildcard";
    return "";
  }

  function presetModeIcon(mode: PresetMode | null): string {
    if (mode === "prepend") return "↑";
    if (mode === "append") return "↓";
    if (mode === "wildcard") return "🎲";
    return "";
  }

  async function exportAll() {
    importStatus = null;
    importError = null;
    const isStylesTab = activeTab === "styles";
    const json = isStylesTab ? styles.exportJSON() : promptPresets.exportJSON();
    const kind = isStylesTab ? "styles" : "presets";
    const defaultName = `mooshieui-${kind}-${new Date().toISOString().slice(0, 10)}.json`;
    const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    try {
      if (isTauri) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const { writeTextFile } = await import("@tauri-apps/plugin-fs");
        const path = await save({
          defaultPath: defaultName,
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (!path) return;
        await writeTextFile(path, json);
        importStatus = `Exported to ${path}`;
      } else {
        const blob = new Blob([json], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = defaultName;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
        URL.revokeObjectURL(url);
        importStatus = `Downloaded ${defaultName}`;
      }
    } catch (e) {
      importError = e instanceof Error ? e.message : String(e);
    }
  }

  async function importStyles() {
    importStatus = null;
    importError = null;
    const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    try {
      if (isTauri) {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const { readTextFile } = await import("@tauri-apps/plugin-fs");
        const selected = await open({
          multiple: false,
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (!selected || typeof selected !== "string") return;
        const raw = await readTextFile(selected);
        applyImport(raw);
      } else {
        fileInput?.click();
      }
    } catch (e) {
      importError = e instanceof Error ? e.message : String(e);
    }
  }

  async function onFilePicked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    try {
      const raw = await file.text();
      applyImport(raw);
    } catch (err) {
      importError = err instanceof Error ? err.message : String(err);
    }
  }

  function applyImport(raw: string) {
    try {
      // Try both kinds — the envelope's `kind` field disambiguates.
      let result: { added: number; skipped: number } | null = null;
      let label = "";
      try {
        result = styles.importJSON(raw, importMode);
        label = "style";
      } catch {
        result = promptPresets.importJSON(raw, importMode);
        label = "preset";
      }
      importStatus = `Imported ${result.added} ${label}${result.added === 1 ? "" : "s"}${
        result.skipped > 0 ? ` (skipped ${result.skipped} duplicate${result.skipped === 1 ? "" : "s"})` : ""
      }.`;
      importError = null;
    } catch (e) {
      importError = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div class="flex flex-col h-full overflow-y-auto px-3 py-2">
    <div class="mb-3 flex items-start justify-between gap-3">
      <div>
        <h2 class="text-sm font-semibold text-neutral-100">
          {activeTab === "styles" ? "Artist Styles" : "Prompt Presets"}
        </h2>
        <p class="text-[11px] text-neutral-500">
          {#if activeTab === "styles"}
            Bundle artists with per-tag and overall weights. Active styles inject their tags into generations without touching your prompt textbox.
          {:else}
            Store reusable prompt fragments. On activation, choose whether to prepend, append, or use as a wildcard (one random entry per generation).
          {/if}
        </p>
      </div>
    </div>

    <!-- Tabs -->
    <div class="mb-4 flex gap-1 border-b border-neutral-800">
      <button
        type="button"
        class="px-3 py-1.5 text-xs transition-colors border-b-2 {activeTab === 'styles' ? 'border-indigo-500 text-neutral-100' : 'border-transparent text-neutral-500 hover:text-neutral-300'}"
        onclick={() => (activeTab = "styles")}
      >
        ✦ Styles <span class="text-[10px] text-neutral-500">({styles.styles.length})</span>
      </button>
      <button
        type="button"
        class="px-3 py-1.5 text-xs transition-colors border-b-2 {activeTab === 'presets' ? 'border-indigo-500 text-neutral-100' : 'border-transparent text-neutral-500 hover:text-neutral-300'}"
        onclick={() => (activeTab = "presets")}
      >
        ⚡ Presets <span class="text-[10px] text-neutral-500">({promptPresets.presets.length})</span>
      </button>
    </div>

{#if activeTab === "styles"}
    <!-- Create -->
    <section class="mb-5 flex items-end gap-2 rounded-lg border border-neutral-800 bg-neutral-950/50 p-2">
      <div class="flex-1">
        <label for="sty-mgr-new-name" class="mb-1 block text-[10px] uppercase tracking-wide text-neutral-500">New style</label>
        <input
          id="sty-mgr-new-name"
          type="text"
          bind:value={newName}
          onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); createStyle(); } }}
          placeholder="Name (optional)"
          class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
        />
      </div>
      <button
        type="button"
        class="rounded bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
        onclick={createStyle}
      >+ Create</button>
    </section>

    <!-- List -->
    <section class="mb-5">
      {#if styles.styles.length === 0}
        <p class="rounded border border-dashed border-neutral-800 bg-neutral-950/50 p-4 text-center text-[11px] text-neutral-500">
          You haven't created any styles yet.
        </p>
      {:else}
        <ul class="space-y-2">
          {#each styles.styles as style (style.id)}
            {@const active = styles.isActive(style.id)}
            <li class="flex items-center gap-3 rounded-lg border {active ? 'border-indigo-500/60 bg-indigo-500/5' : 'border-neutral-800 bg-neutral-950/60'} p-2">
              <div class="h-14 w-14 shrink-0 overflow-hidden rounded border border-neutral-800 bg-neutral-900">
                {#if style.thumbnail}
                  <img src={style.thumbnail} alt="" class="h-full w-full object-cover" />
                {:else}
                  <div class="flex h-full w-full items-center justify-center text-[9px] text-neutral-600">no thumb</div>
                {/if}
              </div>
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <p class="truncate text-sm text-neutral-100">{style.name}</p>
                  {#if active}
                    <span class="shrink-0 rounded-full border border-indigo-500/40 bg-indigo-500/10 px-1.5 py-0.5 text-[9px] uppercase tracking-wide text-indigo-300">active</span>
                  {/if}
                </div>
                <p class="truncate text-[11px] text-neutral-500">
                  {style.artists.length} artist{style.artists.length === 1 ? "" : "s"} · overall ×{style.overallWeight.toFixed(2)}
                </p>
              </div>
              <div class="flex shrink-0 items-center gap-1">
                <button
                  type="button"
                  class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] {active ? 'text-indigo-300 hover:text-neutral-200' : 'text-neutral-300 hover:text-indigo-200'}"
                  onclick={() => styles.toggleActive(style.id)}
                  title={active ? "Deactivate" : "Activate"}
                >{active ? "Deactivate" : "Activate"}</button>
                <button
                  type="button"
                  class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 hover:text-indigo-200"
                  onclick={() => (editingId = style.id)}
                >Edit</button>
                <button
                  type="button"
                  class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-400 hover:text-neutral-200"
                  onclick={() => styles.duplicate(style.id)}
                  title="Duplicate"
                >⧉</button>
                <button
                  type="button"
                  class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-400 hover:bg-red-500/10 hover:text-red-300"
                  onclick={() => confirmDelete(style)}
                  title="Delete"
                  aria-label={`Delete ${style.name}`}
                >✕</button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
{:else}
    <!-- Presets: create -->
    <section class="mb-5 flex items-end gap-2 rounded-lg border border-neutral-800 bg-neutral-950/50 p-2">
      <div class="flex-1">
        <label for="pst-mgr-new-name" class="mb-1 block text-[10px] uppercase tracking-wide text-neutral-500">New preset</label>
        <input
          id="pst-mgr-new-name"
          type="text"
          bind:value={newPresetName}
          onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); createPreset(); } }}
          placeholder="Name (optional)"
          class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
        />
      </div>
      <button
        type="button"
        class="rounded bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
        onclick={createPreset}
      >+ Create</button>
    </section>

    <!-- Presets: list -->
    <section class="mb-5">
      {#if promptPresets.presets.length === 0}
        <p class="rounded border border-dashed border-neutral-800 bg-neutral-950/50 p-4 text-center text-[11px] text-neutral-500">
          No presets yet. Create one to store a reusable prompt fragment or a wildcard list.
        </p>
      {:else}
        <ul class="space-y-2">
          {#each promptPresets.presets as preset (preset.id)}
            {@const active = promptPresets.isActive(preset.id)}
            {@const mode = promptPresets.activeMode(preset.id)}
            {@const choiceCount = promptPresets.countChoices(preset.id)}
            <li class="rounded-lg border {active ? 'border-indigo-500/60 bg-indigo-500/5' : 'border-neutral-800 bg-neutral-950/60'} p-2">
              <div class="flex items-center gap-3">
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <p class="truncate text-sm text-neutral-100">{preset.name}</p>
                    {#if active && mode}
                      <span class="shrink-0 rounded-full border border-indigo-500/40 bg-indigo-500/10 px-1.5 py-0.5 text-[9px] uppercase tracking-wide text-indigo-300">
                        {presetModeIcon(mode)} {presetModeLabel(mode)}
                      </span>
                    {/if}
                  </div>
                  <p class="mt-0.5 truncate font-mono text-[11px] text-neutral-500">
                    {preset.content || "(empty)"}
                  </p>
                  <p class="text-[10px] text-neutral-600">
                    {choiceCount} wildcard option{choiceCount === 1 ? "" : "s"}
                  </p>
                </div>
                <div class="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] {active ? 'text-indigo-300 hover:text-neutral-200' : 'text-neutral-300 hover:text-indigo-200'}"
                    onclick={() => onPresetActivateClick(preset)}
                    title={active ? "Deactivate" : "Activate…"}
                  >{active ? "Deactivate" : "Activate…"}</button>
                  {#if active && mode}
                    <button
                      type="button"
                      class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-400 hover:text-indigo-200"
                      onclick={() => (activatingPresetId = preset.id)}
                      title="Change mode"
                    >↻ mode</button>
                  {/if}
                  <button
                    type="button"
                    class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 hover:text-indigo-200"
                    onclick={() => (editingPresetId = preset.id)}
                  >Edit</button>
                  <button
                    type="button"
                    class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-400 hover:text-neutral-200"
                    onclick={() => promptPresets.duplicate(preset.id)}
                    title="Duplicate"
                  >⧉</button>
                  <button
                    type="button"
                    class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-400 hover:bg-red-500/10 hover:text-red-300"
                    onclick={() => confirmDeletePreset(preset)}
                    title="Delete"
                    aria-label={`Delete ${preset.name}`}
                  >✕</button>
                </div>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
{/if}

    <!-- Import / export -->
    <section class="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 space-y-2">
      <h3 class="text-[10px] uppercase tracking-wide text-neutral-500">
        Import / export ({activeTab === "styles" ? "styles" : "presets"})
      </h3>
      <div class="flex flex-wrap items-center gap-2">
        <button
          type="button"
          class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500"
          onclick={exportAll}
          disabled={activeTab === "styles" ? styles.styles.length === 0 : promptPresets.presets.length === 0}
        >Export all</button>
        <button
          type="button"
          class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500"
          onclick={importStyles}
        >Import…</button>
        <label class="ml-2 inline-flex items-center gap-1 text-[11px] text-neutral-400">
          <input type="radio" bind:group={importMode} value="merge" class="accent-indigo-500" />
          Merge
        </label>
        <label class="inline-flex items-center gap-1 text-[11px] text-neutral-400">
          <input type="radio" bind:group={importMode} value="replace" class="accent-indigo-500" />
          Replace all
        </label>
        <input
          bind:this={fileInput}
          type="file"
          accept="application/json,.json"
          class="hidden"
          onchange={onFilePicked}
        />
      </div>
      <p class="text-[10px] text-neutral-500">Import auto-detects whether the file contains styles or presets.</p>
      {#if importStatus}
        <p class="text-[11px] text-emerald-400">{importStatus}</p>
      {/if}
      {#if importError}
        <p class="text-[11px] text-red-400">{importError}</p>
      {/if}
    </section>
</div>

{#if editingId}
  <StyleEditor styleId={editingId} onclose={() => (editingId = null)} />
{/if}

{#if editingPresetId}
  <PresetEditor presetId={editingPresetId} onclose={() => (editingPresetId = null)} />
{/if}

{#if activatingPresetId}
  <PresetActivationModal
    presetId={activatingPresetId}
    onclose={() => (activatingPresetId = null)}
  />
{/if}
