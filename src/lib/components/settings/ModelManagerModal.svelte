<script lang="ts">
  import { onMount } from "svelte";
  import {
    createModelFolder,
    deleteModelFile,
    getModelInstallDirs,
    listModelFiles,
    listModelFolders,
    moveModelFile,
    type ManagedModelFile,
    type ManagedModelFolder,
    type ModelInstallDir,
  } from "../../utils/api.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { buildFolderTrees, countFilesRecursive, type FolderTreeNode } from "../../utils/modelFolderTree.js";
  import { modelFolderPath } from "../../utils/modelGallerySort.js";

  interface Props {
    onclose: () => void;
    mobileFriendly?: boolean;
  }

  interface ModelCategory {
    id: string;
    label: () => string;
  }

  let { onclose, mobileFriendly = false }: Props = $props();

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
  let folders = $state<ManagedModelFolder[]>([]);
  let installDirs = $state<ModelInstallDir[]>([]);
  // Install dirs for every category, so a file can be moved across categories
  // (e.g. a diffusion-only checkpoint stranded in `checkpoints/` relocated to
  // `diffusion_models/`). Loaded once on mount; the active category is kept
  // fresh by loadCategory().
  let allInstallDirs = $state<Record<string, ModelInstallDir[]>>({});
  let search = $state("");
  let viewMode = $state<"list" | "tree">("list");
  let collapsed = $state<Set<string>>(new Set());
  let newFolderNode = $state<string | null>(null);
  let newFolderName = $state("");
  let folderBusy = $state(false);
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

  const folderTrees = $derived.by(() => buildFolderTrees(installDirs, files, folders));

  onMount(() => {
    void loadCategory();
    void loadAllInstallDirs();
  });

  // Fetch install dirs for every category up front so cross-category move
  // destinations are available without switching tabs. Best-effort per
  // category: a failure leaves that category out of the move picker rather
  // than blocking the modal.
  async function loadAllInstallDirs() {
    const entries = await Promise.all(
      categories.map(async (category): Promise<[string, ModelInstallDir[]]> => {
        try {
          return [category.id, await getModelInstallDirs(category.id)];
        } catch {
          return [category.id, []];
        }
      }),
    );
    allInstallDirs = { ...allInstallDirs, ...Object.fromEntries(entries) };
  }

  function formatModified(ms: number): string {
    if (!ms) return locale.t("common.none");
    return new Date(ms).toLocaleDateString(locale.intlTag, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  }

  function categoryLabel(category: string): string {
    return categories.find((item) => item.id === category)?.label() ?? category;
  }

  interface MoveOption {
    category: string;
    directory: string;
    path: string;
    label: string;
  }

  interface MoveOptionGroup {
    dirLabel: string;
    options: MoveOption[];
  }

  function moveOptionValue(opt: MoveOption): string {
    return `${opt.category}|||${opt.directory}|||${opt.path}`;
  }

  function moveGroupsFor(file: ManagedModelFile): MoveOptionGroup[] {
    const currentDir = file.directory;
    const currentFolder = modelFolderPath(file.filename);
    // Same-category destinations: every install dir plus its subfolders,
    // excluding the file's current location.
    const groups: MoveOptionGroup[] = installDirs
      .map((dir) => {
        const options: MoveOption[] = [];
        if (!(dir.path === currentDir && currentFolder === "")) {
          options.push({ category: file.category, directory: dir.path, path: "", label: locale.t("common.none") });
        }
        for (const folder of folders) {
          if (folder.directory !== dir.path) continue;
          if (dir.path === currentDir && folder.path === currentFolder) continue;
          options.push({ category: file.category, directory: folder.directory, path: folder.path, label: folder.path });
        }
        return { dirLabel: dir.label, options };
      })
      .filter((group) => group.options.length > 0);

    // Cross-category destinations: the root of each other category's install
    // dir(s). Subfolders are omitted to keep the picker light; the user can
    // reorganize within the target category afterward.
    for (const category of categories) {
      if (category.id === file.category) continue;
      const dirs = allInstallDirs[category.id] ?? [];
      const options: MoveOption[] = dirs.map((dir) => ({
        category: category.id,
        directory: dir.path,
        path: "",
        label: dir.label,
      }));
      if (options.length > 0) groups.push({ dirLabel: category.label(), options });
    }

    return groups;
  }

  function nodeKey(node: FolderTreeNode): string {
    return `${node.directory}|||${node.path}`;
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
      const [nextDirs, nextFiles, nextFolders] = await Promise.all([
        getModelInstallDirs(category),
        listModelFiles(category),
        listModelFolders(category),
      ]);
      if (category !== activeCategory) return;
      installDirs = nextDirs;
      files = nextFiles;
      folders = nextFolders;
      allInstallDirs = { ...allInstallDirs, [category]: nextDirs };
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
      files = [];
      installDirs = [];
      folders = [];
    } finally {
      loading = false;
    }
  }

  function selectCategory(category: string) {
    if (category === activeCategory) return;
    activeCategory = category;
    search = "";
    collapsed = new Set();
    newFolderNode = null;
    void loadCategory();
  }

  function beginNewFolder(node: FolderTreeNode) {
    actionError = null;
    notice = null;
    newFolderNode = nodeKey(node);
    newFolderName = "";
  }

  function cancelNewFolder() {
    newFolderNode = null;
    newFolderName = "";
  }

  async function confirmNewFolder(node: FolderTreeNode) {
    const name = newFolderName.trim();
    if (!name) return;
    folderBusy = true;
    actionError = null;
    try {
      const folderPath = node.path ? `${node.path}/${name}` : name;
      await createModelFolder(activeCategory, node.directory, folderPath);
      newFolderNode = null;
      newFolderName = "";
      notice = locale.t("settings.models.folder_created", { name });
      await loadCategory();
    } catch (e) {
      actionError = e instanceof Error ? e.message : String(e);
    } finally {
      folderBusy = false;
    }
  }

  function toggleCollapse(node: FolderTreeNode) {
    const key = nodeKey(node);
    const next = new Set(collapsed);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsed = next;
  }

  function beginDelete(file: ManagedModelFile) {
    actionError = null;
    notice = null;
    moveTarget = null;
    deleteTarget = file;
  }

  function beginMove(file: ManagedModelFile) {
    const groups = moveGroupsFor(file);
    const firstOption = groups[0]?.options[0];
    if (!firstOption) return;
    actionError = null;
    notice = null;
    deleteTarget = null;
    moveTarget = file;
    moveDestination = moveOptionValue(firstOption);
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
    // Encoded as `category|||directory|||path`; category and directory never
    // contain the separator, so parse the first two segments off the front.
    const firstSep = moveDestination.indexOf("|||");
    if (firstSep < 0) return;
    const secondSep = moveDestination.indexOf("|||", firstSep + 3);
    if (secondSep < 0) return;
    const destCategory = moveDestination.slice(0, firstSep);
    const destDirectory = moveDestination.slice(firstSep + 3, secondSep);
    const destPath = moveDestination.slice(secondSep + 3);
    busy = true;
    actionError = null;
    const movedName = moveTarget.filename;
    try {
      const basename = moveTarget.filename.split("/").pop() ?? moveTarget.filename;
      const targetFilename = destPath ? `${destPath}/${basename}` : basename;
      await moveModelFile(
        moveTarget.category,
        moveTarget.filename,
        moveTarget.directory,
        destDirectory,
        targetFilename,
        destCategory,
      );
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
  class="fixed inset-0 z-80 flex bg-neutral-950 {mobileFriendly ? 'flex-col p-0' : 'items-center justify-center p-6'}"
  onclick={(e) => { if (e.target === e.currentTarget && !mobileFriendly) onclose(); }}
  onkeydown={handleKeydown}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div class="flex min-h-0 w-full {mobileFriendly ? 'h-full flex-col' : 'h-[min(760px,92vh)] w-[min(1140px,96vw)] items-stretch gap-3'}">
    <aside class="{mobileFriendly ? 'hidden' : 'hidden sm:flex'} w-44 shrink-0 flex-col rounded-2xl border border-neutral-700 bg-neutral-900 p-2 shadow-2xl">
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

    <div class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden {mobileFriendly ? 'rounded-none border-0' : 'rounded-2xl border border-neutral-700'} bg-neutral-800 shadow-2xl">
      <div class="shrink-0 border-b border-neutral-700 bg-neutral-800 {mobileFriendly ? 'px-4 py-3 pt-[max(env(safe-area-inset-top),0.75rem)]' : 'rounded-t-2xl px-5 py-4'}">
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <h2 class="text-base font-semibold text-neutral-100">{locale.t("settings.models.title")}</h2>
            <p class="text-xs text-neutral-500">{files.length} {locale.t("settings.models.files")} · {locale.formatBytes(totalSize)}</p>
          </div>
          <button
            type="button"
            onclick={onclose}
            class="shrink-0 rounded-lg p-2 text-neutral-400 transition-colors hover:bg-neutral-700 hover:text-neutral-100"
            title={locale.t("common.close")}
            aria-label={locale.t("common.close")}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
          </button>
        </div>
        {#if mobileFriendly}
          <div class="mt-3 -mx-1 flex gap-1.5 overflow-x-auto overscroll-x-contain pb-1">
            {#each categories as category}
              <button
                type="button"
                onclick={() => selectCategory(category.id)}
                class="shrink-0 rounded-full px-3 py-1.5 text-xs font-medium transition-colors {activeCategory === category.id
                  ? 'bg-indigo-600 text-white'
                  : 'bg-neutral-700 text-neutral-300'}"
              >
                {category.label()}
              </button>
            {/each}
          </div>
        {:else}
          <div class="mt-2 flex gap-1 overflow-x-auto sm:hidden">
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
        {/if}
      </div>

      <div class="flex shrink-0 flex-col gap-2 border-b border-neutral-700 bg-neutral-900 px-4 py-3 {mobileFriendly ? '' : 'sm:flex-row sm:flex-wrap sm:items-center'}">
        <div class="min-w-0">
          <p class="text-sm font-medium text-neutral-200">{categoryLabel(activeCategory)}</p>
          <p class="text-[10px] text-neutral-500">{installDirs.length} {locale.t("settings.models.directories")}</p>
        </div>
        <div class="flex min-w-0 flex-1 flex-col gap-2 sm:flex-row sm:items-center">
          <input
            type="search"
            bind:value={search}
            class="min-w-0 flex-1 rounded-lg border border-neutral-700 bg-neutral-900 px-3 py-2.5 text-sm text-neutral-100 placeholder-neutral-500 transition-colors focus:border-indigo-500 focus:outline-none"
            placeholder={locale.t("settings.models.search")}
          />
          <div class="flex shrink-0 overflow-hidden rounded-lg border border-neutral-700">
            <button
              type="button"
              onclick={() => (viewMode = "list")}
              class="px-3 py-2.5 text-sm transition-colors {viewMode === 'list' ? 'bg-indigo-600 text-white' : 'bg-neutral-800 text-neutral-300 hover:text-neutral-100'}"
            >
              {locale.t("settings.models.view_list")}
            </button>
            <button
              type="button"
              onclick={() => (viewMode = "tree")}
              class="px-3 py-2.5 text-sm transition-colors {viewMode === 'tree' ? 'bg-indigo-600 text-white' : 'bg-neutral-800 text-neutral-300 hover:text-neutral-100'}"
            >
              {locale.t("settings.models.view_tree")}
            </button>
          </div>
          <button
            type="button"
            onclick={() => loadCategory()}
            disabled={loading || busy}
            class="shrink-0 rounded-lg border border-neutral-700 bg-neutral-800 px-4 py-2.5 text-sm text-neutral-300 transition-colors hover:border-indigo-500 hover:text-indigo-300 disabled:opacity-50"
          >
            {locale.t("settings.models.refresh")}
          </button>
        </div>
      </div>

      {#if loadError}
        <div class="mx-4 mt-3 rounded-xl border border-red-800 bg-red-950 px-3 py-2 text-sm text-red-300">{loadError}</div>
      {/if}

      {#if notice}
        <div class="mx-4 mt-3 rounded-xl border border-green-800 bg-green-950 px-3 py-2 text-sm text-green-300">{notice}</div>
      {/if}

      {#snippet folderNode(node: FolderTreeNode, depth: number)}
        {@const key = nodeKey(node)}
        {@const isCollapsed = collapsed.has(key)}
        {@const fileCount = countFilesRecursive(node)}
        {@const hasContent = node.children.length > 0 || node.files.length > 0}
        <div class="select-none">
          <div
            class="flex items-center gap-1.5 rounded-lg px-2 py-1.5 hover:bg-neutral-800"
            style="padding-left: {depth * 16 + 8}px"
          >
            {#if hasContent}
              <button
                type="button"
                onclick={() => toggleCollapse(node)}
                class="shrink-0 rounded p-0.5 text-neutral-500 transition-transform hover:text-neutral-200 {isCollapsed ? '' : 'rotate-90'}"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6"/></svg>
              </button>
            {:else}
              <span class="w-4 shrink-0"></span>
            {/if}
            <span class="truncate text-sm text-neutral-200">{node.name}</span>
            <span class="shrink-0 text-[10px] tabular-nums text-neutral-500">({fileCount})</span>
            <button
              type="button"
              onclick={() => beginNewFolder(node)}
              class="ml-auto shrink-0 rounded p-1 text-neutral-500 transition-colors hover:text-indigo-300"
              title={locale.t("settings.models.new_folder")}
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>
            </button>
          </div>
          {#if newFolderNode === key}
            <div class="flex items-center gap-2 py-1" style="padding-left: {(depth + 1) * 16 + 8}px">
              <input
                bind:value={newFolderName}
                placeholder={locale.t("settings.models.folder_name_placeholder")}
                class="min-w-0 flex-1 rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 text-xs text-neutral-100 focus:border-indigo-500 focus:outline-none"
                onkeydown={(e) => {
                  if (e.key === "Enter") confirmNewFolder(node);
                  if (e.key === "Escape") cancelNewFolder();
                }}
              />
              <button
                type="button"
                onclick={() => confirmNewFolder(node)}
                disabled={folderBusy || !newFolderName.trim()}
                class="shrink-0 rounded-lg bg-indigo-600 px-2.5 py-1.5 text-xs text-white transition-colors hover:bg-indigo-500 disabled:opacity-50"
              >
                {locale.t("common.create")}
              </button>
              <button
                type="button"
                onclick={cancelNewFolder}
                disabled={folderBusy}
                class="shrink-0 rounded-lg border border-neutral-700 px-2.5 py-1.5 text-xs text-neutral-300 transition-colors hover:border-neutral-500 hover:text-neutral-100 disabled:opacity-50"
              >
                {locale.t("common.cancel")}
              </button>
            </div>
          {/if}
          {#if !isCollapsed}
            {#each node.children as child (nodeKey(child))}
              {@render folderNode(child, depth + 1)}
            {/each}
            {#each node.files as file (file.filename)}
              {@const moveGroups = moveGroupsFor(file)}
              <div
                class="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-neutral-800"
                style="padding-left: {(depth + 1) * 16 + 28}px"
              >
                <span class="min-w-0 flex-1 truncate text-sm text-neutral-300" title={file.filename}>{file.filename.split("/").pop()}</span>
                <span class="shrink-0 text-[10px] tabular-nums text-neutral-500">{locale.formatBytes(file.size_bytes)}</span>
                <button
                  type="button"
                  onclick={() => beginMove(file)}
                  disabled={moveGroups.length === 0 || busy}
                  class="shrink-0 rounded-lg border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 transition-colors hover:border-indigo-500 hover:text-indigo-300 disabled:opacity-40"
                  title={moveGroups.length === 0 ? locale.t("settings.models.no_move_target") : locale.t("settings.models.move")}
                >
                  {locale.t("settings.models.move")}
                </button>
                <button
                  type="button"
                  onclick={() => beginDelete(file)}
                  disabled={busy}
                  class="shrink-0 rounded-lg border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 transition-colors hover:border-red-500 hover:text-red-300 disabled:opacity-40"
                >
                  {locale.t("common.delete")}
                </button>
              </div>
            {/each}
          {/if}
        </div>
      {/snippet}

      <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-3 {mobileFriendly && !moveTarget && !deleteTarget ? 'pb-[max(env(safe-area-inset-bottom),0.75rem)]' : ''}">
        {#if loading}
          <div class="flex h-full items-center justify-center text-sm text-neutral-500">
            <div class="mr-2 h-4 w-4 rounded-full border-2 border-indigo-400 border-t-transparent animate-spin"></div>
            {locale.t("common.loading")}
          </div>
        {:else if viewMode === "tree" && !search.trim()}
          {#if folderTrees.every((root) => root.children.length === 0 && root.files.length === 0)}
            <div class="flex h-full items-center justify-center text-sm text-neutral-500">
              {locale.t("settings.models.empty")}
            </div>
          {:else}
            <div class="space-y-0.5">
              {#each folderTrees as root (nodeKey(root))}
                {@render folderNode(root, 0)}
              {/each}
            </div>
          {/if}
        {:else if filteredFiles.length === 0}
          <div class="flex h-full items-center justify-center text-sm text-neutral-500">
            {locale.t("settings.models.empty")}
          </div>
        {:else if mobileFriendly}
          <div class="space-y-2">
            {#each filteredFiles as file (`${file.directory}:${file.filename}`)}
              {@const moveGroups = moveGroupsFor(file)}
              <div class="rounded-xl border border-neutral-700 bg-neutral-900 p-3">
                <p class="break-all text-sm font-medium text-neutral-100">{file.filename}</p>
                <p class="mt-1 text-xs text-neutral-500">{file.directory_label}</p>
                <p class="mt-0.5 break-all text-[10px] text-neutral-600">{file.directory}</p>
                <div class="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-xs text-neutral-400">
                  <span class="tabular-nums">{locale.formatBytes(file.size_bytes)}</span>
                  <span class="text-neutral-600">·</span>
                  <span>{formatModified(file.modified_ms)}</span>
                </div>
                <div class="mt-3 flex gap-2">
                  <button
                    type="button"
                    onclick={() => beginMove(file)}
                    disabled={moveGroups.length === 0 || busy}
                    class="min-h-10 flex-1 rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-sm text-neutral-300 transition-colors active:bg-neutral-700 disabled:opacity-40"
                    title={moveGroups.length === 0 ? locale.t("settings.models.no_move_target") : locale.t("settings.models.move")}
                  >
                    {locale.t("settings.models.move")}
                  </button>
                  <button
                    type="button"
                    onclick={() => beginDelete(file)}
                    disabled={busy}
                    class="min-h-10 flex-1 rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-sm text-neutral-300 transition-colors active:bg-neutral-700 hover:border-red-500 hover:text-red-300 disabled:opacity-40"
                  >
                    {locale.t("common.delete")}
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {:else}
          <div class="overflow-x-auto rounded-xl border border-neutral-700 bg-neutral-900">
            <div class="min-w-190">
            {#each filteredFiles as file (`${file.directory}:${file.filename}`)}
              {@const moveGroups = moveGroupsFor(file)}
              <div class="grid grid-cols-[minmax(0,1fr)_150px_110px_120px_142px] items-center gap-3 border-b border-neutral-800 px-3 py-2.5 transition-colors last:border-b-0 hover:bg-neutral-800">
                <div class="min-w-0">
                  <p class="truncate text-sm text-neutral-100" title={file.filename}>{file.filename}</p>
                  <p class="truncate text-[10px] text-neutral-500" title={file.directory}>{file.directory_label} · {file.directory}</p>
                </div>
                <div class="truncate text-xs text-neutral-400" title={file.directory_label}>{file.directory_label}</div>
                <div class="text-xs tabular-nums text-neutral-400">{locale.formatBytes(file.size_bytes)}</div>
                <div class="text-xs text-neutral-500">{formatModified(file.modified_ms)}</div>
                <div class="flex justify-end gap-1.5">
                  <button
                    type="button"
                    onclick={() => beginMove(file)}
                    disabled={moveGroups.length === 0 || busy}
                    class="rounded-lg border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 transition-colors hover:border-indigo-500 hover:text-indigo-300 disabled:opacity-40"
                    title={moveGroups.length === 0 ? locale.t("settings.models.no_move_target") : locale.t("settings.models.move")}
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
        {@const moveGroups = moveGroupsFor(moveTarget)}
        <div class="shrink-0 border-t border-neutral-700 bg-neutral-900 px-4 py-3 {mobileFriendly ? 'pb-[max(env(safe-area-inset-bottom),0.75rem)]' : 'rounded-b-2xl'}">
          <div class="flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-center">
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm text-neutral-200">{locale.t("settings.models.move_title", { name: moveTarget.filename })}</p>
              {#if actionError}<p class="mt-1 text-xs text-red-300">{actionError}</p>{/if}
            </div>
            <select
              bind:value={moveDestination}
              class="w-full rounded-lg border border-neutral-700 bg-neutral-900 px-3 py-2.5 text-sm text-neutral-100 focus:border-indigo-500 focus:outline-none sm:w-72"
            >
              {#each moveGroups as group}
                <optgroup label={group.dirLabel}>
                  {#each group.options as opt}
                    <option value={moveOptionValue(opt)}>{opt.label}</option>
                  {/each}
                </optgroup>
              {/each}
            </select>
            <div class="flex gap-2 sm:contents">
            <button
              type="button"
              onclick={() => (moveTarget = null)}
              disabled={busy}
              class="min-h-10 flex-1 rounded-lg border border-neutral-700 px-3 py-2 text-sm text-neutral-300 transition-colors hover:border-neutral-500 hover:text-neutral-100 disabled:opacity-50 sm:flex-none"
            >
              {locale.t("common.cancel")}
            </button>
            <button
              type="button"
              onclick={confirmMove}
              disabled={busy || moveGroups.length === 0 || !moveDestination}
              class="min-h-10 flex-1 rounded-lg bg-indigo-600 px-3 py-2 text-sm text-white transition-colors hover:bg-indigo-500 disabled:opacity-50 sm:flex-none"
            >
              {busy ? locale.t("common.saving") : locale.t("settings.models.move")}
            </button>
            </div>
          </div>
        </div>
      {/if}

      {#if deleteTarget}
        <div class="shrink-0 border-t border-red-900 bg-red-950 px-4 py-3 {mobileFriendly ? 'pb-[max(env(safe-area-inset-bottom),0.75rem)]' : 'rounded-b-2xl'}">
          <div class="flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-center">
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm text-red-100">{locale.t("settings.models.delete_title", { name: deleteTarget.filename })}</p>
              <p class="text-xs text-red-300">{deleteTarget.directory_label}</p>
              {#if actionError}<p class="mt-1 text-xs text-red-300">{actionError}</p>{/if}
            </div>
            <div class="flex gap-2 sm:contents">
            <button
              type="button"
              onclick={() => (deleteTarget = null)}
              disabled={busy}
              class="min-h-10 flex-1 rounded-lg border border-neutral-700 px-3 py-2 text-sm text-neutral-300 transition-colors hover:border-neutral-500 hover:text-neutral-100 disabled:opacity-50 sm:flex-none"
            >
              {locale.t("common.cancel")}
            </button>
            <button
              type="button"
              onclick={confirmDelete}
              disabled={busy}
              class="min-h-10 flex-1 rounded-lg bg-red-600 px-3 py-2 text-sm text-white transition-colors hover:bg-red-500 disabled:opacity-50 sm:flex-none"
            >
              {busy ? locale.t("common.saving") : locale.t("common.delete")}
            </button>
            </div>
          </div>
        </div>
      {/if}

    </div>
  </div>
</div>