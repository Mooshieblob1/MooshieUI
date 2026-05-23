<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    loadCharacterFacets,
    searchCharacterFacet,
    searchCharacters,
  } from "../client.js";
  import type {
    AnimadexCharacter,
    CharacterFilterFacetName,
    CharacterSort,
    FacetValue,
  } from "../types.js";
  import { CHARACTER_FILTER_FACETS } from "../types.js";
  import CharacterLightbox from "./CharacterLightbox.svelte";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    oninsertCharacter?: (character: AnimadexCharacter) => void;
  }

  let { oninsertCharacter }: Props = $props();

  const FACET_LABEL_KEYS: Record<CharacterFilterFacetName, string> = {
    copyright: "animadex.facet.copyright",
    hair_color: "animadex.facet.hair_color",
    hair_length: "animadex.facet.hair_length",
    eye_color: "animadex.facet.eye_color",
    gender: "animadex.facet.gender",
  };

  const SEARCHABLE_FACETS = new Set<CharacterFilterFacetName>([
    "copyright",
    "hair_color",
    "eye_color",
  ]);

  function fmtCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1).replace(/\.0$/, "")}K`;
    return String(n);
  }

  let queryInput = $state("");
  let sort = $state<CharacterSort>("count");
  let randomSeed = $state<number | null>(null);
  let lorasOnly = $state(false);
  let page = $state(1);
  let filters = $state<Record<CharacterFilterFacetName, Set<string>>>(
    Object.fromEntries(CHARACTER_FILTER_FACETS.map((f) => [f, new Set<string>()])) as Record<
      CharacterFilterFacetName,
      Set<string>
    >,
  );
  let labels = $state<Record<string, string>>({});

  let catalogTotal = $state(0);
  let facetOptions = $state<Partial<Record<CharacterFilterFacetName, FacetValue[]>>>({});
  let facetTotals = $state<Partial<Record<CharacterFilterFacetName, number>>>({});
  let facetQuery = $state<Record<CharacterFilterFacetName, string>>(
    Object.fromEntries(CHARACTER_FILTER_FACETS.map((f) => [f, ""])) as Record<
      CharacterFilterFacetName,
      string
    >,
  );
  let collapsedFacets = $state<Set<CharacterFilterFacetName>>(
    new Set(CHARACTER_FILTER_FACETS.filter((f) => f !== "copyright")),
  );

  let results = $state<AnimadexCharacter[]>([]);
  let total = $state(0);
  let totalPages = $state(1);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let facetsLoading = $state(true);
  let facetsError = $state<string | null>(null);

  let lightboxIndex = $state(-1);
  let lightboxCharacter = $derived(
    lightboxIndex >= 0 && lightboxIndex < results.length ? results[lightboxIndex] : null,
  );

  let searchDebounce: number | null = null;
  let facetDebounce: Partial<Record<CharacterFilterFacetName, number>> = {};
  let searchSeq = 0;

  let scrollContainer = $state<HTMLDivElement | null>(null);
  let pageInputValue = $state("");
  let sliderValue = $state(60);
  const cardMinWidth = $derived(Math.round(100 * 4 ** (sliderValue / 100)));

  const activeFilterCount = $derived(
    CHARACTER_FILTER_FACETS.reduce((n, f) => n + filters[f].size, 0),
  );

  function filterSnapshot(): Record<CharacterFilterFacetName, string[]> {
    return Object.fromEntries(
      CHARACTER_FILTER_FACETS.map((f) => [f, [...filters[f]]]),
    ) as Record<CharacterFilterFacetName, string[]>;
  }

  async function runSearch(targetPage = 1) {
    const seq = ++searchSeq;
    loading = true;
    error = null;
    page = targetPage;
    try {
      const data = await searchCharacters({
        q: queryInput,
        sort,
        seed: sort === "random" ? randomSeed : null,
        page: targetPage,
        filters: filterSnapshot(),
        lorasOnly,
      });
      if (seq !== searchSeq) return;
      results = data.results;
      total = data.total;
      totalPages = Math.max(1, data.pages);
      lightboxIndex = -1;
    } catch (err) {
      if (seq !== searchSeq) return;
      error = err instanceof Error ? err.message : String(err);
      results = [];
    } finally {
      if (seq === searchSeq) loading = false;
    }
  }

  async function loadFacets() {
    facetsLoading = true;
    facetsError = null;
    try {
      const data = await loadCharacterFacets();
      catalogTotal = data.total;
      const opts: Partial<Record<CharacterFilterFacetName, FacetValue[]>> = {};
      const totals: Partial<Record<CharacterFilterFacetName, number>> = {};
      for (const name of CHARACTER_FILTER_FACETS) {
        const group = data.facets[name];
        if (group) {
          opts[name] = group.values;
          totals[name] = group.total;
        }
      }
      facetOptions = opts;
      facetTotals = totals;
    } catch (err) {
      facetsError = err instanceof Error ? err.message : String(err);
    } finally {
      facetsLoading = false;
    }
  }

  async function reloadFacetOptions(name: CharacterFilterFacetName) {
    try {
      const data = await searchCharacterFacet(name, facetQuery[name] ?? "");
      facetOptions = { ...facetOptions, [name]: data.values };
      facetTotals = { ...facetTotals, [name]: data.total };
    } catch {
      /* keep current list */
    }
  }

  function scheduleSearch(targetPage = 1) {
    if (searchDebounce !== null) window.clearTimeout(searchDebounce);
    searchDebounce = window.setTimeout(() => {
      void runSearch(targetPage);
      searchDebounce = null;
    }, 150);
  }

  function onQueryInput(value: string) {
    queryInput = value;
    scheduleSearch(1);
  }

  function toggleFilter(facet: CharacterFilterFacetName, value: string, label: string) {
    const next = { ...filters };
    const set = new Set(next[facet]);
    if (set.has(value)) {
      set.delete(value);
    } else {
      set.add(value);
      labels = { ...labels, [value]: label };
    }
    next[facet] = set;
    filters = next;
    scheduleSearch(1);
  }

  function removeFilter(facet: CharacterFilterFacetName, value: string) {
    const next = { ...filters };
    const set = new Set(next[facet]);
    set.delete(value);
    next[facet] = set;
    filters = next;
    scheduleSearch(1);
  }

  function clearAllFilters() {
    filters = Object.fromEntries(
      CHARACTER_FILTER_FACETS.map((f) => [f, new Set<string>()]),
    ) as Record<CharacterFilterFacetName, Set<string>>;
    queryInput = "";
    lorasOnly = false;
    scheduleSearch(1);
  }

  function setSort(next: CharacterSort) {
    sort = next;
    if (next === "random" && !randomSeed) {
      randomSeed = Math.floor(Math.random() * 1_000_000) + 1;
    }
    scheduleSearch(1);
  }

  function reshuffleRandom() {
    randomSeed = Math.floor(Math.random() * 1_000_000) + 1;
    if (sort === "random") scheduleSearch(page);
  }

  function toggleLorasOnly() {
    lorasOnly = !lorasOnly;
    scheduleSearch(1);
  }

  function onFacetSearchInput(name: CharacterFilterFacetName, value: string) {
    facetQuery = { ...facetQuery, [name]: value };
    if (!SEARCHABLE_FACETS.has(name)) return;
    if (facetDebounce[name] !== undefined) window.clearTimeout(facetDebounce[name]);
    facetDebounce[name] = window.setTimeout(() => {
      void reloadFacetOptions(name);
      facetDebounce[name] = undefined;
    }, 220);
  }

  function toggleFacetCollapsed(name: CharacterFilterFacetName) {
    const next = new Set(collapsedFacets);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    collapsedFacets = next;
  }

  function goToPage(p: number) {
    void runSearch(p);
    requestAnimationFrame(() => {
      scrollContainer?.scrollTo({ top: 0, behavior: "instant" });
    });
  }

  function goToRandomPage() {
    goToPage(Math.floor(Math.random() * totalPages) + 1);
  }

  function commitPageInput() {
    const n = parseInt(pageInputValue, 10);
    if (!Number.isNaN(n) && n >= 1 && n <= totalPages) goToPage(n);
    pageInputValue = "";
  }

  function openCharacter(c: AnimadexCharacter, index: number) {
    lightboxIndex = index;
  }

  function closeLightbox() {
    lightboxIndex = -1;
  }

  onMount(() => {
    void loadFacets().then(() => runSearch(1));
    return () => {
      if (searchDebounce !== null) window.clearTimeout(searchDebounce);
    };
  });
</script>

<div class="flex h-full min-h-0 w-full flex-col overflow-hidden bg-neutral-950 text-neutral-100">
  <header class="flex-none border-b border-neutral-800 bg-neutral-900/60 px-4 py-3">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <p class="text-xs text-neutral-500">
          {#if catalogTotal > 0}
            {locale.t("animadex.subtitle", { count: locale.formatInteger(catalogTotal) })}
          {:else if facetsLoading}
            {locale.t("animadex.loading_catalog")}
          {:else if facetsError}
            <span class="text-red-400">{facetsError}</span>
          {/if}
          ·
          <a
            href="https://animadex.net/?mode=characters"
            target="_blank"
            rel="noopener noreferrer"
            class="underline hover:text-neutral-300"
          >animadex.net</a>
        </p>
      </div>
      <div class="relative w-full max-w-sm">
        <input
          type="search"
          placeholder={locale.t("animadex.search_placeholder", {
            count: locale.formatInteger(catalogTotal || 34000),
          })}
          value={queryInput}
          oninput={(e) => onQueryInput(e.currentTarget.value)}
          class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
        />
        {#if loading}
          <span class="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-neutral-500">…</span>
        {/if}
      </div>
    </div>

    <div class="mt-3 flex flex-wrap items-center gap-2">
      <div class="flex items-center gap-0.5 rounded-lg border border-neutral-800 bg-neutral-900/50 p-1">
        <span class="px-1.5 text-xs text-neutral-500">{locale.t("animadex.sort_label")}</span>
        <button
          type="button"
          class="rounded px-2 py-0.5 text-xs transition-colors {sort === 'count' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
          onclick={() => setSort("count")}
        >{locale.t("animadex.sort_count")}</button>
        <button
          type="button"
          class="rounded px-2 py-0.5 text-xs transition-colors {sort === 'az' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
          onclick={() => setSort("az")}
        >{locale.t("animadex.sort_az")}</button>
        <button
          type="button"
          class="rounded px-2 py-0.5 text-xs transition-colors {sort === 'random' ? 'bg-amber-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
          onclick={() => setSort("random")}
        >{locale.t("animadex.sort_random")}</button>
        {#if sort === "random"}
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs text-amber-400 hover:text-amber-200"
            onclick={reshuffleRandom}
            title={locale.t("animadex.sort_random_reshuffle_title")}
          >{locale.t("animadex.sort_random_reshuffle")}</button>
        {/if}
      </div>

      <button
        type="button"
        class="rounded-lg border px-2 py-1 text-xs transition-colors {lorasOnly ? 'border-emerald-600 bg-emerald-950/40 text-emerald-300' : 'border-neutral-800 bg-neutral-900/50 text-neutral-400 hover:text-neutral-200'}"
        onclick={toggleLorasOnly}
        title={locale.t("animadex.loras_only_title")}
      >
        {locale.t("animadex.loras_only")}
      </button>

      <div class="flex items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-900/50 px-2 py-1">
        <span class="text-xs text-neutral-500">{locale.t("artist_gallery.size_label")}</span>
        <input
          type="range"
          min="0"
          max="100"
          value={sliderValue}
          oninput={(e) => {
            sliderValue = parseInt(e.currentTarget.value, 10);
          }}
          class="w-20 accent-indigo-500"
        />
      </div>

      {#if activeFilterCount > 0}
        <button
          type="button"
          class="rounded-lg border border-neutral-800 bg-neutral-900/50 px-2 py-1 text-xs text-neutral-400 hover:text-neutral-200"
          onclick={clearAllFilters}
        >
          {locale.t("animadex.clear_filters")}
        </button>
      {/if}
    </div>

    {#if activeFilterCount > 0}
      <div class="mt-2 flex flex-wrap gap-1">
        {#each CHARACTER_FILTER_FACETS as facet (facet)}
          {#each [...filters[facet]] as value (facet + value)}
            <button
              type="button"
              class="flex items-center gap-1 rounded-full border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-xs text-neutral-300 hover:border-red-500"
              onclick={() => removeFilter(facet, value)}
            >
              <span class="text-neutral-500">{locale.t(FACET_LABEL_KEYS[facet])}:</span>
              {labels[value] ?? value.replace(/_/g, " ")}
              <span class="text-neutral-500">✕</span>
            </button>
          {/each}
        {/each}
      </div>
    {/if}
  </header>

  <div class="flex min-h-0 flex-1 overflow-hidden">
    <aside class="hidden w-56 shrink-0 overflow-y-auto border-r border-neutral-800 bg-neutral-900/40 p-2 md:block">
      {#if facetsLoading}
        <p class="p-2 text-xs text-neutral-500">{locale.t("animadex.loading_filters")}</p>
      {:else}
        {#each CHARACTER_FILTER_FACETS as name (name)}
          {@const options = facetOptions[name] ?? []}
          {@const collapsed = collapsedFacets.has(name)}
          <div class="mb-2 rounded-lg border border-neutral-800">
            <button
              type="button"
              class="flex w-full items-center justify-between px-2 py-1.5 text-left text-xs font-medium text-neutral-300 hover:bg-neutral-800/80"
              onclick={() => toggleFacetCollapsed(name)}
            >
              <span>{locale.t(FACET_LABEL_KEYS[name])}</span>
              <span class="text-neutral-500">{collapsed ? "▸" : "▾"}</span>
            </button>
            {#if !collapsed}
              <div class="border-t border-neutral-800 px-2 py-1.5">
                {#if SEARCHABLE_FACETS.has(name)}
                  <input
                    type="search"
                    placeholder={locale.t("animadex.facet_search")}
                    value={facetQuery[name]}
                    oninput={(e) => onFacetSearchInput(name, e.currentTarget.value)}
                    class="mb-1.5 w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-200 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
                  />
                {/if}
                <div class="max-h-40 space-y-0.5 overflow-y-auto">
                  {#each options as opt (opt.value)}
                    {@const active = filters[name].has(opt.value)}
                    <button
                      type="button"
                      class="flex w-full items-center justify-between gap-1 rounded px-1 py-0.5 text-left text-[11px] transition-colors {active ? 'bg-indigo-600/30 text-indigo-200' : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}"
                      onclick={() => toggleFilter(name, opt.value, opt.label)}
                    >
                      <span class="min-w-0 truncate">{opt.label}</span>
                      <span class="shrink-0 text-neutral-600">{fmtCount(opt.count)}</span>
                    </button>
                  {/each}
                </div>
                {#if (facetTotals[name] ?? 0) > options.length}
                  <p class="mt-1 text-[10px] text-neutral-600">
                    {locale.t("animadex.facet_more", { total: locale.formatInteger(facetTotals[name] ?? 0) })}
                  </p>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </aside>

    <div class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
      <div class="flex-none border-b border-neutral-800/60 px-4 py-2 text-xs text-neutral-500">
        {#if loading}
          {locale.t("animadex.searching")}
        {:else if error}
          <span class="text-red-400">{locale.t("animadex.load_error", { error })}</span>
        {:else}
          {locale.t("animadex.result_count", { count: locale.formatInteger(total) })}
          {#if totalPages > 1}
            · {locale.t("artist_gallery.page_of", { page, total: totalPages })}
          {/if}
        {/if}
      </div>

      {#if totalPages > 1 && !loading && !error}
        <div class="flex flex-none flex-wrap items-center justify-center gap-2 border-b border-neutral-800/60 px-4 py-2">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1 text-sm text-neutral-300 hover:border-indigo-500 disabled:opacity-40"
            disabled={page <= 1}
            onclick={() => goToPage(page - 1)}
          >{locale.t("artist_gallery.prev")}</button>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-300 hover:border-amber-500"
            onclick={goToRandomPage}
          >{locale.t("artist_gallery.random")}</button>
          <div class="flex items-center gap-1">
            <input
              type="number"
              min="1"
              max={totalPages}
              placeholder={locale.t("artist_gallery.page_placeholder")}
              value={pageInputValue}
              oninput={(e) => {
                pageInputValue = e.currentTarget.value;
              }}
              onkeydown={(e) => {
                if (e.key === "Enter") commitPageInput();
              }}
              class="w-14 rounded border border-neutral-700 bg-neutral-800 px-1.5 py-0.5 text-xs text-neutral-200 focus:border-indigo-500 focus:outline-none"
            />
          </div>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1 text-sm text-neutral-300 hover:border-indigo-500 disabled:opacity-40"
            disabled={page >= totalPages}
            onclick={() => goToPage(page + 1)}
          >{locale.t("artist_gallery.next")}</button>
        </div>
      {/if}

      <div class="min-h-0 flex-1 overflow-y-auto" bind:this={scrollContainer}>
        {#if loading && results.length === 0}
          <div class="p-8 text-center text-sm text-neutral-500">{locale.t("animadex.loading_characters")}</div>
        {:else if !loading && results.length === 0}
          <div class="p-8 text-center text-sm text-neutral-500">{locale.t("animadex.no_results")}</div>
        {:else}
          <div
            class="grid gap-3 p-4"
            style="grid-template-columns: repeat(auto-fill, minmax({cardMinWidth}px, 1fr))"
          >
            {#each results as c, i (c.slug)}
              <div
                role="button"
                tabindex="0"
                class="group relative flex flex-col overflow-hidden rounded-lg border border-neutral-800 bg-neutral-900 transition-colors hover:border-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500"
                onclick={() => openCharacter(c, i)}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    openCharacter(c, i);
                  }
                }}
              >
                <div class="relative aspect-3/4 w-full overflow-hidden bg-neutral-800">
                  {#if c.has_image && c.thumb_url}
                    <img
                      src={c.thumb_url}
                      alt={c.name}
                      loading="lazy"
                      decoding="async"
                      class="h-full w-full object-cover"
                    />
                  {:else}
                    <div class="flex h-full w-full items-center justify-center text-xs text-neutral-500">
                      {locale.t("animadex.no_preview")}
                    </div>
                  {/if}
                  {#if c.loras.length > 0}
                    <span class="absolute left-1 top-1 rounded bg-emerald-700/90 px-1 py-0.5 text-[10px] font-semibold text-white">
                      + LoRA
                    </span>
                  {/if}
                </div>
                <div class="px-2 py-1.5">
                  <div class="truncate text-sm text-neutral-100">{c.name}</div>
                  <div class="truncate text-xs text-neutral-500">{c.copyright_name}</div>
                  <div class="text-[10px] text-neutral-600">{fmtCount(c.count)}</div>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

{#if lightboxCharacter}
  <CharacterLightbox
    character={lightboxCharacter}
    onclose={closeLightbox}
    {oninsertCharacter}
    onprev={lightboxIndex > 0 ? () => (lightboxIndex -= 1) : undefined}
    onnext={lightboxIndex < results.length - 1 ? () => (lightboxIndex += 1) : undefined}
  />
{/if}
