<script lang="ts">
  import {
    artistFavourites,
    CATEGORY_COLOR_PALETTE,
    type FavouriteCategory,
  } from "../favourites.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  let newName = $state("");
  let newColor = $state(CATEGORY_COLOR_PALETTE[0]);

  // Per-row edit state (keyed by category id)
  let editingId = $state<string | null>(null);
  let editName = $state("");
  let editColor = $state("");

  // Import/export UI state
  let importMode = $state<"merge" | "replace">("merge");
  let importStatus = $state<string | null>(null);
  let importError = $state<string | null>(null);
  let fileInput: HTMLInputElement | null = $state(null);

  function createCategory() {
    const trimmed = newName.trim();
    if (!trimmed) return;
    artistFavourites.createCategory(trimmed, newColor);
    newName = "";
    newColor = CATEGORY_COLOR_PALETTE[0];
  }

  function startEdit(cat: FavouriteCategory) {
    editingId = cat.id;
    editName = cat.name;
    editColor = cat.color;
  }

  function commitEdit() {
    if (!editingId) return;
    artistFavourites.updateCategory(editingId, { name: editName, color: editColor });
    editingId = null;
  }

  function cancelEdit() {
    editingId = null;
  }

  function deleteCategory(cat: FavouriteCategory) {
    const count = artistFavourites.countsByCategory[cat.id] ?? 0;
    const msg =
      count > 0
        ? locale.t('artist_gallery.fav_manager.delete_confirm_used', { name: cat.name, count })
        : locale.t('artist_gallery.fav_manager.delete_confirm', { name: cat.name });
    if (!confirm(msg)) return;
    artistFavourites.deleteCategory(cat.id);
    if (editingId === cat.id) editingId = null;
  }

  // ---------------------------------------------------------------------------
  // Export
  // ---------------------------------------------------------------------------

  async function exportFavourites() {
    const json = artistFavourites.exportJSON();
    const defaultName = `mooshieui-artist-favourites-${new Date().toISOString().slice(0, 10)}.json`;
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
      importError = null;
    } catch (e) {
      importError = e instanceof Error ? e.message : String(e);
    }
  }

  // ---------------------------------------------------------------------------
  // Import
  // ---------------------------------------------------------------------------

  async function importFavourites() {
    importStatus = null;
    importError = null;
    const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    try {
      let raw: string | null = null;
      if (isTauri) {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const { readTextFile } = await import("@tauri-apps/plugin-fs");
        const selected = await open({
          multiple: false,
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (!selected || typeof selected !== "string") return;
        raw = await readTextFile(selected);
      } else {
        // Browser: use hidden file input
        fileInput?.click();
        return;
      }
      applyImport(raw);
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
      const result = artistFavourites.importJSON(raw, importMode);
      importStatus = locale.t(
        result.categoriesAdded === 1
          ? 'artist_gallery.fav_manager.import_result_cat_one'
          : 'artist_gallery.fav_manager.import_result_cats',
        { added: result.added, updated: result.updated, categories: result.categoriesAdded },
      );
      importError = null;
    } catch (e) {
      importError = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div
  class="fixed inset-0 z-200 flex items-center justify-center bg-black/80 backdrop-blur-sm"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t('artist_gallery.fav_manager.aria')}
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label={locale.t('artist_gallery.fav_manager.close_aria')}
    onclick={onclose}
  ></button>
  <div class="relative z-10 w-full max-w-2xl rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
    <div class="mb-4 flex items-center justify-between">
      <h2 class="text-sm font-semibold text-neutral-100">{locale.t('artist_gallery.fav_manager.title')}</h2>
      <button
        type="button"
        class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
        onclick={onclose}
        aria-label={locale.t('artist_gallery.fav_manager.close_aria')}
      >✕</button>
    </div>

    <!-- Categories -->
    <section class="mb-5">
      <h3 class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400">{locale.t('artist_gallery.fav_manager.categories')}</h3>

      <!-- Create new -->
      <div class="mb-3 flex items-end gap-2 rounded-lg border border-neutral-800 bg-neutral-950/50 p-2">
        <div class="flex-1">
          <label for="cat-new-name" class="mb-1 block text-[10px] uppercase tracking-wide text-neutral-500">{locale.t('artist_gallery.fav_manager.name_label')}</label>
          <input
            id="cat-new-name"
            name="cat-new-name"
            type="text"
            bind:value={newName}
            onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); createCategory(); } }}
            placeholder={locale.t('artist_gallery.fav_manager.name_placeholder')}
            class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
          />
        </div>
        <div>
          <div class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">{locale.t('artist_gallery.fav_manager.colour_label')}</div>
          <div class="flex items-center gap-1">
            {#each CATEGORY_COLOR_PALETTE as swatch}
              <button
                type="button"
                aria-label={locale.t('artist_gallery.fav_manager.pick_colour', { colour: swatch })}
                class="h-5 w-5 rounded-full border transition-transform hover:scale-110 {newColor === swatch ? 'border-white ring-2 ring-white/60' : 'border-neutral-700'}"
                style="background-color: {swatch}"
                onclick={() => newColor = swatch}
              ></button>
            {/each}
            <input
              type="color"
              aria-label={locale.t('artist_gallery.fav_manager.custom_colour')}
              bind:value={newColor}
              class="ml-1 h-5 w-6 cursor-pointer rounded border border-neutral-700 bg-transparent p-0"
            />
          </div>
        </div>
        <button
          type="button"
          class="shrink-0 rounded-md border border-indigo-500 bg-indigo-600 px-3 py-1 text-sm font-medium text-white hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
          disabled={!newName.trim()}
          onclick={createCategory}
        >
          {locale.t('artist_gallery.fav_manager.add_btn')}
        </button>
      </div>

      <!-- List -->
      {#if artistFavourites.categories.length === 0}
        <p class="text-xs text-neutral-500">{locale.t('artist_gallery.fav_manager.empty')}</p>
      {:else}
        <ul class="space-y-1.5">
          {#each artistFavourites.categories as cat (cat.id)}
            {@const count = artistFavourites.countsByCategory[cat.id] ?? 0}
            <li class="flex items-center gap-2 rounded-md border border-neutral-800 bg-neutral-950/50 px-2 py-1.5">
              {#if editingId === cat.id}
                <input
                  type="color"
                  aria-label={locale.t('artist_gallery.fav_manager.cat_colour')}
                  bind:value={editColor}
                  class="h-6 w-7 cursor-pointer rounded border border-neutral-700 bg-transparent p-0"
                />
                <input
                  type="text"
                  bind:value={editName}
                  onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); commitEdit(); } else if (e.key === "Escape") { e.preventDefault(); cancelEdit(); } }}
                  class="flex-1 rounded border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-sm text-neutral-100 focus:border-indigo-500 focus:outline-none"
                />
                <button
                  type="button"
                  class="rounded border border-emerald-600 px-2 py-0.5 text-xs text-emerald-300 hover:bg-emerald-950"
                  onclick={commitEdit}
                >{locale.t('artist_gallery.fav_manager.save_btn')}</button>
                <button
                  type="button"
                  class="rounded border border-neutral-700 px-2 py-0.5 text-xs text-neutral-400 hover:text-neutral-200"
                  onclick={cancelEdit}
                >{locale.t('artist_gallery.fav_manager.cancel_btn')}</button>
              {:else}
                <span
                  class="h-4 w-4 shrink-0 rounded-full border border-neutral-700"
                  style="background-color: {cat.color}"
                  aria-hidden="true"
                ></span>
                <span class="flex-1 truncate text-sm text-neutral-100">{cat.name}</span>
                <span class="text-xs text-neutral-500">{count === 1 ? locale.t('artist_gallery.fav_manager.fav_one') : locale.t('artist_gallery.fav_manager.fav_count', { count })}</span>
                <button
                  type="button"
                  class="rounded border border-neutral-700 px-2 py-0.5 text-xs text-neutral-300 hover:border-indigo-500 hover:text-indigo-300"
                  onclick={() => startEdit(cat)}
                >{locale.t('artist_gallery.fav_manager.edit_btn')}</button>
                <button
                  type="button"
                  class="rounded border border-neutral-700 px-2 py-0.5 text-xs text-neutral-300 hover:border-red-500 hover:text-red-300"
                  onclick={() => deleteCategory(cat)}
                >{locale.t('artist_gallery.fav_manager.delete_btn')}</button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <!-- Import / Export -->
    <section class="border-t border-neutral-800 pt-4">
      <h3 class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400">{locale.t('artist_gallery.fav_manager.backup')}</h3>
      <p class="mb-3 text-xs text-neutral-500">
        {locale.t('artist_gallery.fav_manager.backup_desc')}
      </p>

      <div class="flex flex-wrap items-center gap-2">
        <button
          type="button"
          class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-sm text-neutral-200 hover:border-indigo-500"
          onclick={exportFavourites}
        >
          {locale.t('artist_gallery.fav_manager.export_btn')}
        </button>

        <div class="flex items-center gap-1 rounded-md border border-neutral-800 bg-neutral-900/50 p-1">
          <span class="px-1.5 text-xs text-neutral-500">{locale.t('artist_gallery.fav_manager.import_mode')}</span>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {importMode === 'merge' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => importMode = "merge"}
          >{locale.t('artist_gallery.fav_manager.merge')}</button>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {importMode === 'replace' ? 'bg-red-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => importMode = "replace"}
          >{locale.t('artist_gallery.fav_manager.replace')}</button>
        </div>

        <button
          type="button"
          class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-sm text-neutral-200 hover:border-indigo-500"
          onclick={importFavourites}
        >
          {locale.t('artist_gallery.fav_manager.import_btn')}
        </button>

        <!-- Hidden file input used only in browser mode -->
        <input
          bind:this={fileInput}
          type="file"
          accept="application/json,.json"
          class="hidden"
          onchange={onFilePicked}
        />
      </div>

      {#if importStatus}
        <p class="mt-2 text-xs text-emerald-400">{importStatus}</p>
      {/if}
      {#if importError}
        <p class="mt-2 text-xs text-red-400">{locale.t('artist_gallery.fav_manager.import_error', { error: importError })}</p>
      {/if}
    </section>
  </div>
</div>
