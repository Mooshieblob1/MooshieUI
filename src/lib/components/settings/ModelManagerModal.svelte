<script lang="ts">
  import { onMount } from "svelte";
  import {
    deleteModelFile,
    getModelInstallDirs,
    listModelFiles,
    moveModelFile,
    type ManagedModelFile,
    type ModelInstallDir,
  } from "../../utils/api.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { models } from "../../stores/models.svelte.js";

  interface Props {
    onclose: () => void;
  }

  interface ModelCategory {
    id: string;
    label: () => string;
  }

  let { onclose }: Props = $props();

  const categories: ModelCategory[] = [
    { id: "checkpoints", label: () => locale.t("settings.paths.open_folder.checkpoints") },
    { id: "loras", label: () => locale.t("settings.paths.open_folder.loras") },
    { id: "vae", label: () => locale.t("settings.paths.open_folder.vae") },
    { id: "upscale_models", label: () => locale.t("settings.paths.open_folder.upscale") },
    { id: "controlnet", label: () => locale.t("settings.paths.open_folder.controlnet") },
    { id: "embeddings", label: () => locale.t("settings.paths.open_folder.embeddings") },
    { id: "ultralytics", label: () => locale.t("settings.paths.open_folder.facefix") },
    { id: "text_encoders", label: () => locale.t("settings.paths.open_folder.clip") },
    { id: "diffusion_models", label: () => locale.t("settings.paths.open_folder.diffusion") },
  ];

  let activeCategory = $state("checkpoints");
  let files = $state<ManagedModelFile[]>([]);
  let installDirs = $state<ModelInstallDir[]>([]);
  let search = $state("");
  let loading = $state(false);
  let loadError = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let busy = $state(false);
  let deleteTarget = $state<ManagedModelFile | null>(null);
  let moveTarget = $state<ManagedModelFile | null>(null);
  let moveDestination = $state("");

  const filteredFiles = $derived.by(() => {
    const query = search.trim().toLowerCase();
    if (!query) return files;
    return files.filter((file) =>
      `${file.filename} ${file.directory_label} ${file.directory}`.toLowerCase().includes(query),
    );
  });

  const totalSize = $derived(files.reduce((sum, file) => sum + file.size_bytes, 0));

  onMount(() => {
    void loadCategory();
  });

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function formatModified(ms: number): string {
    if (!ms) return locale.t("common.none");
    return new Date(ms).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  }

  function categoryLabel(category: string): string {
    return categories.find((item) => item.id === category)?.label() ?? category;
  }

  function availableMoveDirs(file: ManagedModelFile): ModelInstallDir[] {
    return installDirs.filter((dir) => dir.path !== file.directory);
  }

  async function loadCategory() {
    loading = true;
    loadError = null;
    actionError = null;
    notice = null;
    deleteTarget = null;
    moveTarget = null;
    try {
      const category = activeCategory;
      const [nextDirs, nextFiles] = await Promise.all([
        getModelInstallDirs(category),
        listModelFiles(category),
      ]);
      if (category !== activeCategory) return;
      installDirs = nextDirs;
      files = nextFiles;
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
      files = [];
      installDirs = [];
    } finally {
      loading = false;
    }
  }

  function selectCategory(category: string) {
    if (category === activeCategory) return;
    activeCategory = category;
    search = "";
    void loadCategory();
  }

  function beginDelete(file: ManagedModelFile) {
    actionError = null;
    notice = null;
    moveTarget = null;
    deleteTarget = file;
  }

  function beginMove(file: ManagedModelFile) {
    const destinations = availableMoveDirs(file);
    if (destinations.length === 0) return;
    actionError = null;
    notice = null;
    deleteTarget = null;
    moveTarget = file;
    moveDestination = destinations[0].path;
  }

  async function refreshModels() {
    await models.refresh().catch((e) => console.warn("Failed to refresh models after model management action:", e));
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    busy = true;
    actionError = null;
    const deletedName = deleteTarget.filename;
    try {
      await deleteModelFile(deleteTarget.category, deleteTarget.filename, deleteTarget.directory);
      deleteTarget = null;
      await Promise.all([loadCategory(), refreshModels()]);
      notice = locale.t("settings.models.deleted", { name: deletedName });
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function confirmMove() {
    if (!moveTarget || !moveDestination) return;
    busy = true;
    actionError = null;
    const movedName = moveTarget.filename;
    try {
      await moveModelFile(moveTarget.category, moveTarget.filename, moveTarget.directory, moveDestination);
      moveTarget = null;
      moveDestination = "";
      await Promise.all([loadCategory(), refreshModels()]);
      notice = locale.t("settings.models.moved", { name: movedName });
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    if (deleteTarget) {
      deleteTarget = null;
      return;
    }
    if (moveTarget) {
      moveTarget = null;
      return;
    }
    onclose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="fixed inset-0 z-80 flex items-center justify-center bg-neutral-950 p-6"
  onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}
  onkeydown={handleKeydown}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div class="flex h-[min(760px,92vh)] w-[min(1140px,96vw)] items-stretch gap-3">
    <aside class="hidden w-44 shrink-0 flex-col rounded-2xl border border-neutral-700 bg-neutral-900 p-2 shadow-2xl sm:flex">
      <nav class="flex-1 space-y-0.5 overflow-y-auto">
        {#each categories as category}
          <button
            type="button"
            onclick={() => selectCategory(category.id)}
            class="w-full rounded-xl px-3 py-2 text-left text-xs font-medium transition-colors {activeCategory === category.id
              ? 'bg-indigo-600 text-white shadow-sm'
              : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-100'}"
          >
            {category.label()}
          </button>
        {/each}
      </nav>
    </aside>

    <div class="flex min-w-0 flex-1 flex-col overflow-hidden rounded-2xl border border-neutral-700 bg-neutral-800 shadow-2xl">
      <div class="flex items-center justify-between rounded-t-2xl border-b border-neutral-700 bg-neutral-800 px-5 py-4">
        <div>
          <h2 class="text-base font-semibold text-neutral-100">{locale.t("settings.models.title")}</h2>
          <p class="text-xs text-neutral-500">{files.length} {locale.t("settings.models.files")} · {formatBytes(totalSize)}</p>
        </div>
        <div class="flex items-center gap-2">
          <div class="flex gap-1 overflow-x-auto sm:hidden">
            {#each categories as category}
              <button
                type="button"
                onclick={() => selectCategory(category.id)}
                class="shrink-0 rounded-lg px-2.5 py-1.5 text-[11px] font-medium transition-colors {activeCategory === category.id
                  ? 'bg-indigo-600 text-white'
                  : 'bg-neutral-700 text-neutral-300 hover:bg-neutral-600'}"
              >
                {category.label()}
              </button>
            {/each}
          </div>
          <button
            type="button"
            onclick={onclose}
            class="rounded-lg p-2 text-neutral-400 transition-colors hover:bg-neutral-700 hover:text-neutral-100"
            title={locale.t("common.close")}
            aria-label={locale.t("common.close")}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
          </button>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2 border-b border-neutral-700 bg-neutral-900 px-4 py-3">
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium text-neutral-200">{categoryLabel(activeCategory)}</p>
          <p class="text-[10px] text-neutral-500">{installDirs.length} {locale.t("settings.models.directories")}</p>
        </div>
        <input
          type="search"
          bind:value={search}
          class="w-full rounded-lg border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 transition-colors focus:border-indigo-500 focus:outline-none sm:w-72"
          placeholder={locale.t("settings.models.search")}
        />
        <button
          type="button"
          onclick={() => loadCategory()}
          disabled={loading || busy}
          class="rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-xs text-neutral-300 transition-colors hover:border-indigo-500 hover:text-indigo-300 disabled:opacity-50"
        >
          {locale.t("settings.models.refresh")}
        </button>
      </div>

      {#if loadError}
        <div class="mx-4 mt-3 rounded-xl border border-red-800 bg-red-950 px-3 py-2 text-sm text-red-300">{loadError}</div>
      {/if}

      {#if notice}
        <div class="mx-4 mt-3 rounded-xl border border-green-800 bg-green-950 px-3 py-2 text-sm text-green-300">{notice}</div>
      {/if}

      <div class="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {#if loading}
          <div class="flex h-full items-center justify-center text-sm text-neutral-500">
            <div class="mr-2 h-4 w-4 rounded-full border-2 border-indigo-400 border-t-transparent animate-spin"></div>
            {locale.t("common.loading")}
          </div>
        {:else if filteredFiles.length === 0}
          <div class="flex h-full items-center justify-center text-sm text-neutral-500">
            {locale.t("settings.models.empty")}
          </div>
        {:else}
          <div class="overflow-x-auto rounded-xl border border-neutral-700 bg-neutral-900">
            <div class="min-w-190">
            {#each filteredFiles as file (`${file.directory}:${file.filename}`)}
              {@const moveDirs = availableMoveDirs(file)}
              <div class="grid grid-cols-[minmax(0,1fr)_150px_110px_120px_142px] items-center gap-3 border-b border-neutral-800 px-3 py-2.5 transition-colors last:border-b-0 hover:bg-neutral-800">
                <div class="min-w-0">
                  <p class="truncate text-sm text-neutral-100" title={file.filename}>{file.filename}</p>
                  <p class="truncate text-[10px] text-neutral-500" title={file.directory}>{file.directory_label} · {file.directory}</p>
                </div>
                <div class="truncate text-xs text-neutral-400" title={file.directory_label}>{file.directory_label}</div>
                <div class="text-xs tabular-nums text-neutral-400">{formatBytes(file.size_bytes)}</div>
                <div class="text-xs text-neutral-500">{formatModified(file.modified_ms)}</div>
                <div class="flex justify-end gap-1.5">
                  <button
                    type="button"
                    onclick={() => beginMove(file)}
                    disabled={moveDirs.length === 0 || busy}
                    class="rounded-lg border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 transition-colors hover:border-indigo-500 hover:text-indigo-300 disabled:opacity-40"
                    title={moveDirs.length === 0 ? locale.t("settings.models.no_move_target") : locale.t("settings.models.move")}
                  >
                    {locale.t("settings.models.move")}
                  </button>
                  <button
                    type="button"
                    onclick={() => beginDelete(file)}
                    disabled={busy}
                    class="rounded-lg border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 transition-colors hover:border-red-500 hover:text-red-300 disabled:opacity-40"
                  >
                    {locale.t("common.delete")}
                  </button>
                </div>
              </div>
            {/each}
            </div>
          </div>
        {/if}
      </div>

      {#if moveTarget}
        {@const moveDirs = availableMoveDirs(moveTarget)}
        <div class="rounded-b-2xl border-t border-neutral-700 bg-neutral-900 px-4 py-3">
          <div class="flex flex-wrap items-center gap-3">
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm text-neutral-200">{locale.t("settings.models.move_title", { name: moveTarget.filename })}</p>
              {#if actionError}<p class="mt-1 text-xs text-red-300">{actionError}</p>{/if}
            </div>
            <select
              bind:value={moveDestination}
              class="w-full rounded-lg border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm text-neutral-100 focus:border-indigo-500 focus:outline-none sm:w-72"
            >
              {#each moveDirs as dir}
                <option value={dir.path}>{dir.label}</option>
              {/each}
            </select>
            <button
              type="button"
              onclick={() => (moveTarget = null)}
              disabled={busy}
              class="rounded-lg border border-neutral-700 px-3 py-2 text-xs text-neutral-300 transition-colors hover:border-neutral-500 hover:text-neutral-100 disabled:opacity-50"
            >
              {locale.t("common.cancel")}
            </button>
            <button
              type="button"
              onclick={confirmMove}
              disabled={busy || moveDirs.length === 0 || !moveDestination}
              class="rounded-lg bg-indigo-600 px-3 py-2 text-xs text-white transition-colors hover:bg-indigo-500 disabled:opacity-50"
            >
              {busy ? locale.t("common.saving") : locale.t("settings.models.move")}
            </button>
          </div>
        </div>
      {/if}

      {#if deleteTarget}
        <div class="rounded-b-2xl border-t border-red-900 bg-red-950 px-4 py-3">
          <div class="flex flex-wrap items-center gap-3">
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm text-red-100">{locale.t("settings.models.delete_title", { name: deleteTarget.filename })}</p>
              <p class="text-xs text-red-300">{deleteTarget.directory_label}</p>
              {#if actionError}<p class="mt-1 text-xs text-red-300">{actionError}</p>{/if}
            </div>
            <button
              type="button"
              onclick={() => (deleteTarget = null)}
              disabled={busy}
              class="rounded-lg border border-neutral-700 px-3 py-2 text-xs text-neutral-300 transition-colors hover:border-neutral-500 hover:text-neutral-100 disabled:opacity-50"
            >
              {locale.t("common.cancel")}
            </button>
            <button
              type="button"
              onclick={confirmDelete}
              disabled={busy}
              class="rounded-lg bg-red-600 px-3 py-2 text-xs text-white transition-colors hover:bg-red-500 disabled:opacity-50"
            >
              {busy ? locale.t("common.saving") : locale.t("common.delete")}
            </button>
          </div>
        </div>
      {/if}

    </div>
  </div>
</div>