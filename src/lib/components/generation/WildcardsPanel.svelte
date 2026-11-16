<script lang="ts">
  import { wildcards } from "../../stores/wildcards.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { openDirectory } from "../../utils/api.js";
  import { isBrowserMode } from "../../utils/ipc.js";
  import { parseWildcardFiles } from "../../utils/wildcards.js";

  let search = $state("");
  let selectedPath = $state<string | null>(null);
  let draft = $state("");
  let creating = $state(false);
  let newName = $state("");
  let busy = $state(false);
  let confirmDelete = $state(false);

  $effect(() => {
    void wildcards.load();
  });

  const files = $derived(
    wildcards.files.filter((f) => !search.trim() || f.path.toLowerCase().includes(search.trim().toLowerCase())),
  );
  const selected = $derived(wildcards.files.find((f) => f.path === selectedPath) ?? null);
  const dirty = $derived(selected !== null && draft !== selected.content);

  /** The names this file provides, as the user would type them. */
  const fileNames = $derived.by(() => {
    if (!selected) return [] as Array<{ name: string; count: number }>;
    const table = parseWildcardFiles([selected]);
    const base = selected.path.replace(/\.(txt|ya?ml|json)$/i, "").toLowerCase();
    let names = [...table.keys()];
    // A YAML or JSON map is addressed by its keys alone; drop the file-path copies.
    const bare = names.filter((n) => n !== base && !n.startsWith(`${base}/`));
    if (bare.length > 0) names = bare;
    return names.sort().map((name) => ({ name, count: table.get(name)?.length ?? 0 }));
  });

  function select(path: string) {
    if (dirty && path !== selectedPath) {
      gallery.showToast(locale.t("wildcards.unsaved_first"), "warning");
      return;
    }
    selectedPath = path;
    draft = wildcards.files.find((f) => f.path === path)?.content ?? "";
    confirmDelete = false;
  }

  async function save() {
    if (!selected) return;
    busy = true;
    try {
      await wildcards.save(selected.path, draft);
      gallery.showToast(locale.t("wildcards.saved"), "success");
    } catch (e) {
      gallery.showToast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function create() {
    let name = newName.trim().replace(/\\/g, "/");
    if (!name) return;
    if (!/\.(txt|ya?ml|json)$/i.test(name)) name += ".txt";
    busy = true;
    try {
      await wildcards.save(name, "");
      creating = false;
      newName = "";
      const created = wildcards.files.find((f) => f.path.toLowerCase() === name.toLowerCase());
      if (created) {
        selectedPath = created.path;
        draft = created.content;
      }
    } catch (e) {
      gallery.showToast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!selected) return;
    busy = true;
    try {
      await wildcards.remove(selected.path);
      selectedPath = null;
      draft = "";
      confirmDelete = false;
    } catch (e) {
      gallery.showToast(e instanceof Error ? e.message : String(e), "error");
    } finally {
      busy = false;
    }
  }

  function insert(name: string) {
    const token = `__${name}__`;
    const current = generation.positivePrompt.replace(/[\s,]+$/, "");
    generation.positivePrompt = current ? `${current}, ${token}` : token;
    generation.saveSettings();
    gallery.showToast(locale.t("wildcards.inserted", { token }), "info");
  }
</script>

<div class="flex h-full min-h-0 gap-2 p-2">
  <!-- File list -->
  <div class="flex w-56 shrink-0 flex-col gap-1.5 min-h-0">
    <div class="flex items-center gap-1">
      <input
        type="text"
        bind:value={search}
        placeholder={locale.t("wildcards.search")}
        aria-label={locale.t("wildcards.search")}
        class="min-w-0 flex-1 rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
      />
      <button
        type="button"
        class="shrink-0 rounded-md p-1 text-neutral-400 hover:bg-neutral-800 hover:text-neutral-100"
        title={locale.t("wildcards.new")}
        aria-label={locale.t("wildcards.new")}
        onclick={() => { creating = !creating; }}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><path d="M5 12h14"/><path d="M12 5v14"/></svg>
      </button>
      <button
        type="button"
        class="shrink-0 rounded-md p-1 text-neutral-400 hover:bg-neutral-800 hover:text-neutral-100 disabled:opacity-40"
        title={locale.t("wildcards.reload")}
        aria-label={locale.t("wildcards.reload")}
        disabled={wildcards.loading}
        onclick={() => void wildcards.load(true)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 {wildcards.loading ? 'animate-spin' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/></svg>
      </button>
      {#if !isBrowserMode && wildcards.dir}
        <button
          type="button"
          class="shrink-0 rounded-md p-1 text-neutral-400 hover:bg-neutral-800 hover:text-neutral-100"
          title={locale.t("wildcards.open_folder")}
          aria-label={locale.t("wildcards.open_folder")}
          onclick={() => void openDirectory(wildcards.dir)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
        </button>
      {/if}
    </div>

    {#if creating}
      <form class="flex items-center gap-1" onsubmit={(e) => { e.preventDefault(); void create(); }}>
        <input
          type="text"
          bind:value={newName}
          placeholder="hair/colors.txt"
          aria-label={locale.t("wildcards.new")}
          class="min-w-0 flex-1 rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1 font-mono text-[11px] text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
        />
        <button type="submit" disabled={busy || !newName.trim()} class="shrink-0 rounded-md bg-indigo-600 px-2 py-1 text-[11px] text-white hover:bg-indigo-500 disabled:opacity-40">{locale.t("wildcards.create")}</button>
      </form>
    {/if}

    <div class="min-h-0 flex-1 overflow-y-auto rounded-md border border-neutral-800">
      {#if wildcards.error}
        <p class="p-2 text-[11px] text-red-400">{wildcards.error}</p>
      {:else if wildcards.loaded && wildcards.files.length === 0}
        <p class="p-2 text-[11px] text-neutral-500">{locale.t("wildcards.empty")}</p>
      {/if}
      {#each files as file (file.path)}
        <button
          type="button"
          class="block w-full truncate px-2 py-1 text-left font-mono text-[11px] transition-colors {file.path === selectedPath ? 'bg-indigo-500/15 text-indigo-200' : 'text-neutral-300 hover:bg-neutral-800'}"
          title={file.path}
          onclick={() => select(file.path)}
        >{file.path}</button>
      {/each}
    </div>
    {#if wildcards.truncated}
      <p class="text-[10px] text-amber-400">{locale.t("wildcards.truncated")}</p>
    {/if}
  </div>

  <!-- Editor -->
  <div class="flex min-w-0 flex-1 flex-col gap-1.5 min-h-0">
    {#if selected}
      <div class="flex items-center gap-2">
        <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-neutral-300" title={selected.path}>{selected.path}</span>
        <button
          type="button"
          disabled={!dirty || busy}
          onclick={save}
          class="shrink-0 rounded-md bg-indigo-600 px-2.5 py-1 text-[11px] font-medium text-white hover:bg-indigo-500 disabled:opacity-40"
        >{locale.t("wildcards.save")}</button>
        {#if dirty}
          <button type="button" onclick={() => (draft = selected?.content ?? "")} class="shrink-0 rounded-md px-2 py-1 text-[11px] text-neutral-400 hover:text-neutral-100">{locale.t("wildcards.discard")}</button>
        {/if}
        {#if confirmDelete}
          <button type="button" disabled={busy} onclick={remove} class="shrink-0 rounded-md bg-red-700 px-2.5 py-1 text-[11px] font-medium text-white hover:bg-red-600">{locale.t("wildcards.confirm_delete")}</button>
          <button type="button" onclick={() => (confirmDelete = false)} class="shrink-0 rounded-md px-2 py-1 text-[11px] text-neutral-400 hover:text-neutral-100">{locale.t("wildcards.cancel")}</button>
        {:else}
          <button type="button" onclick={() => (confirmDelete = true)} class="shrink-0 rounded-md px-2 py-1 text-[11px] text-neutral-400 hover:text-red-400">{locale.t("wildcards.delete")}</button>
        {/if}
      </div>
      <div class="flex min-h-0 flex-1 gap-2">
        <textarea
          bind:value={draft}
          spellcheck="false"
          aria-label={selected.path}
          onkeydown={(e) => { if ((e.ctrlKey || e.metaKey) && e.key === "s") { e.preventDefault(); if (dirty) void save(); } }}
          class="min-h-0 min-w-0 flex-1 resize-none rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1.5 font-mono text-[11px] leading-relaxed text-neutral-100 focus:border-indigo-500 focus:outline-none"
        ></textarea>
        <div class="flex w-64 shrink-0 flex-col gap-1 overflow-y-auto">
          {#each fileNames as entry (entry.name)}
            <div class="rounded-md border border-neutral-800 px-2 py-1.5">
              <div class="flex items-center gap-1">
                <button
                  type="button"
                  class="min-w-0 flex-1 truncate text-left font-mono text-[11px] text-indigo-300 hover:text-indigo-200"
                  title={locale.t("wildcards.insert")}
                  onclick={() => insert(entry.name)}
                >__{entry.name}__</button>
                <span class="shrink-0 tabular-nums text-[10px] text-neutral-500">{entry.count}</span>
              </div>
              <div class="mt-1 flex items-center gap-2">
                <label class="flex cursor-pointer select-none items-center gap-1.5 text-[10px] text-neutral-400" title={locale.t("wildcards.in_order_tip")}>
                  <input
                    type="checkbox"
                    class="h-3 w-3 rounded accent-indigo-500"
                    checked={wildcards.isOrdered(entry.name)}
                    onchange={(e) => wildcards.setOrdered(entry.name, (e.target as HTMLInputElement).checked)}
                  />
                  {locale.t("wildcards.in_order")}
                </label>
                {#if wildcards.isOrdered(entry.name)}
                  <span class="tabular-nums text-[10px] text-neutral-500">{locale.t("wildcards.next", { n: String(wildcards.nextPosition(entry.name)), total: String(entry.count) })}</span>
                  <button type="button" class="text-[10px] text-neutral-500 underline-offset-2 hover:text-neutral-200 hover:underline" onclick={() => wildcards.resetCounter(entry.name)}>{locale.t("wildcards.restart")}</button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      </div>
    {:else}
      <div class="flex h-full items-center justify-center px-4 text-center text-[11px] text-neutral-500">
        <p>{locale.t("wildcards.pick_file")}</p>
      </div>
    {/if}
  </div>
</div>
