<script lang="ts">
  import { onMount, tick } from "svelte";
  import { createArtistGalleryStore, type ArtistSortField, type ArtistSortDir, type ArtistPageSize } from "../store.svelte.js";
  import { artistFavourites } from "../favourites.svelte.js";
  import type { ArtistSearchHit } from "../types.js";
  import ArtistLightbox from "./ArtistLightbox.svelte";
  import FavouritesManager from "./FavouritesManager.svelte";

  interface Props {
    manifestUrl: string;
    /** Optional integrator hook for "Insert tag into prompt" in the lightbox. */
    oninsertTag?: (tag: string) => void;
  }

  let { manifestUrl, oninsertTag }: Props = $props();

  const store = createArtistGalleryStore(manifestUrl);

  type SortField = ArtistSortField;
  type SortDir = ArtistSortDir;
  type PageSize = ArtistPageSize;

  // ---------------------------------------------------------------------------
  // Uniqueness scoring
  // ---------------------------------------------------------------------------
  // Log-normal distribution centred at exp(5.5) ≈ 245 posts, sigma=1.8 in log
  // space. Artists with 50–2 000 posts score highest; mega-popular (10k+) and
  // tiny (<10) score low — the "hidden gem" sweet spot.
  function logNormalScore(x: number, mu: number, sigma: number): number {
    if (x <= 0) return 0;
    const lx = Math.log(x);
    return Math.exp(-0.5 * ((lx - mu) / sigma) ** 2);
  }

  function baseUniqueness(postCount: number): number {
    return logNormalScore(postCount, 5.5, 1.8);
  }

  /** Per-entry jitter multipliers (0.7 – 1.3). Stored on the singleton store
   * so the current ranking survives navigating away and back. */
  function generateJitter(count: number): Float32Array {
    const arr = new Float32Array(count);
    for (let i = 0; i < count; i++) arr[i] = 0.7 + 0.6 * Math.random();
    return arr;
  }

  function rotateUniqueness() {
    store.uniquenessJitter = generateJitter(store.allEntries.length);
    store.sortField = "uniqueness";
    store.currentPage = 1;
    animKey++;
    requestAnimationFrame(() => {
      scrollContainer?.scrollTo({ top: 0, behavior: "instant" });
      scrollContainer?.dispatchEvent(new Event("scroll"));
    });
  }

  let allEntries = $derived(store.allEntries);
  let allLoading = $derived(store.allEntriesLoading);
  let allError = $derived(store.allEntriesError);

  const PAGE_SIZES: PageSize[] = [25, 50, 100];

  let active = $derived(store.lightboxEntry);
  let activeIndex = $derived(store.lightboxIndex);
  let searchDebounce: number | null = null;

  onMount(() => {
    store.init().then(async () => {
      // Load entries only once; subsequent mounts reuse cached data.
      if (store.allEntries.length === 0 && !store.allEntriesLoading) {
        store.allEntriesLoading = true;
        store.allEntriesError = null;
        try {
          const entries = await store.client.loadSearchIndex();
          store.allEntries = entries;
          if (store.uniquenessJitter.length !== entries.length) {
            store.uniquenessJitter = generateJitter(entries.length);
          }
        } catch (err) {
          store.allEntriesError = err instanceof Error ? err.message : String(err);
        } finally {
          store.allEntriesLoading = false;
        }
      }
      // Sync the debounced search input into the store's live query so
      // `store.results` is populated for the grid when returning to the page.
      if (store.queryInput.trim()) {
        void store.setQuery(store.queryInput);
      }
      // Restore scroll position on the next tick once the grid has rendered.
      await tick();
      requestAnimationFrame(() => {
        scrollContainer?.scrollTo({ top: store.scrollTop, behavior: "instant" });
      });
    });

    const onScroll = () => {
      if (scrollContainer) store.scrollTop = scrollContainer.scrollTop;
    };
    // Attach after mount; `scrollContainer` is bound via `bind:this` below.
    requestAnimationFrame(() => {
      scrollContainer?.addEventListener("scroll", onScroll, { passive: true });
    });
    return () => {
      scrollContainer?.removeEventListener("scroll", onScroll);
    };
  });

  function onSearchInput(value: string) {
    store.queryInput = value;
    if (searchDebounce !== null) window.clearTimeout(searchDebounce);
    searchDebounce = window.setTimeout(() => {
      void store.setQuery(value);
      searchDebounce = null;
    }, 120);
  }

  async function openHit(hit: ArtistSearchHit, index = -1) {
    // Instant open: build entry immediately from cached search index data
    const imageUrl = store.manifest && hit.hasImage && hit.imageId
      ? `${store.manifest.imageBaseUrl}/${store.manifest.releasePrefix}/images/${hit.imageId}.webp`
      : "";
    store.lightboxEntry = { tag: hit.tag, slug: hit.slug, imageId: hit.imageId, imageUrl, objectKey: "", postCount: hit.postCount, aliases: [], hasImage: hit.hasImage };
    store.lightboxIndex = index;
    // Background: fetch full shard entry to patch in aliases once loaded
    store.client.getArtist(hit.slug).then((full) => {
      if (full && store.lightboxEntry?.slug === hit.slug) store.lightboxEntry = full;
    }).catch(() => {});
  }

  async function navigateTo(index: number) {
    const clamped = Math.max(0, Math.min(gridEntries.length - 1, index));
    await openHit(gridEntries[clamped], clamped);
  }

  function closeLightbox() {
    store.lightboxEntry = null;
    store.lightboxIndex = -1;
    store.closeArtist();
  }

  // Zoom state lifted to the singleton store so it persists across
  // lightbox close/reopen and across page unmount/remount.
  let lightboxZoomed = $derived(store.lightboxZoomed);

  // Page-jump controls
  let pageInputValue = $state("");

  function goToRandomPage() {
    const page = Math.floor(Math.random() * totalPages) + 1;
    goToPage(page);
  }

  function commitPageInput() {
    const n = parseInt(pageInputValue, 10);
    if (!isNaN(n) && n >= 1 && n <= totalPages) {
      goToPage(n);
    }
    pageInputValue = "";
  }

  function thumbUrl(hit: ArtistSearchHit): string {
    if (!store.manifest || !hit.hasImage || !hit.imageId) return "";
    return `${store.manifest.imageBaseUrl}/${store.manifest.releasePrefix}/images/${hit.imageId}.webp`;
  }

  function formatCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`;
    return String(n);
  }

  function displayTag(tag: string): string {
    return tag.replace(/^@/, "").replace(/\\([()\[\]])/g, "$1").replace(/_/g, " ");
  }

  let copiedSlug = $state<string | null>(null);

  // ---------------------------------------------------------------------------
  // Favourites + animation
  // ---------------------------------------------------------------------------
  // Favourites (incl. categories) are persisted in the artistFavourites store.
  // "All" = show every favourite, string = show only favourites in that category,
  // "__uncat" = show only uncategorised favourites.
  let showOnlyFavourites = $derived(store.showOnlyFavourites);
  let favouriteCategoryFilter = $derived(store.favouriteCategoryFilter);
  let showFavouritesManager = $state(false);
  let showGenParams = $state(false);
  let animKey = $state(0);
  let sortField = $derived(store.sortField);
  let sortDir = $derived(store.sortDir);
  let pageSize = $derived(store.pageSize);
  let currentPage = $derived(store.currentPage);
  let queryInput = $derived(store.queryInput);
  let uniquenessJitter = $derived(store.uniquenessJitter);

  // Per-card category picker popover (keyed by slug). null = closed.
  let catPickerSlug = $state<string | null>(null);

  function toggleFavourite(slug: string, e: MouseEvent) {
    e.stopPropagation();
    artistFavourites.toggle(slug);
  }

  function openCategoryPicker(slug: string, e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    // Auto-favourite if not already; picker always implies "favourite this".
    if (!artistFavourites.isFavourite(slug)) {
      artistFavourites.add(slug);
    }
    catPickerSlug = catPickerSlug === slug ? null : slug;
  }

  function assignCategory(slug: string, categoryId: string | null) {
    artistFavourites.setCategory(slug, categoryId);
    catPickerSlug = null;
  }

  function setShowOnlyFavourites(val: boolean) {
    store.showOnlyFavourites = val;
    store.currentPage = 1;
    animKey++;
    requestAnimationFrame(() => {
      scrollContainer?.scrollTo({ top: 0, behavior: "instant" });
      scrollContainer?.dispatchEvent(new Event("scroll"));
    });
  }

  function setFavouriteCategoryFilter(value: "all" | "__uncat" | string) {
    store.favouriteCategoryFilter = value;
    store.currentPage = 1;
    animKey++;
    requestAnimationFrame(() => {
      scrollContainer?.scrollTo({ top: 0, behavior: "instant" });
      scrollContainer?.dispatchEvent(new Event("scroll"));
    });
  }

  function getInitialSliderValue(): number {
    try {
      const stored = localStorage.getItem("artist-gallery-card-size");
      if (stored !== null) return Math.max(0, Math.min(100, parseInt(stored, 10)));
    } catch { /* ignore */ }
    return 60;
  }

  let sliderValue = $state(getInitialSliderValue());
  const cardMinWidth = $derived(Math.round(100 * Math.pow(4, sliderValue / 100)));

  $effect(() => {
    try { localStorage.setItem("artist-gallery-card-size", String(sliderValue)); } catch { /* ignore */ }
  });

  async function copyTag(tag: string, slug: string) {
    const formatted = "@" + tag.replace(/^@/, "");
    await navigator.clipboard.writeText(formatted);
    copiedSlug = slug;
    window.setTimeout(() => { copiedSlug = null; }, 1500);
  }

  let scrollContainer = $state<HTMLDivElement | null>(null);

  function goToPage(page: number) {
    store.currentPage = page;
    // scrollTo then dispatch synthetic scroll so WebView2 re-evaluates
    // viewport intersection for eager images (known WebView2 overflow quirk)
    requestAnimationFrame(() => {
      scrollContainer?.scrollTo({ top: 0, behavior: "instant" });
      scrollContainer?.dispatchEvent(new Event("scroll"));
    });
  }

  function setSort(field: SortField) {
    if (field === "uniqueness" && sortField !== "uniqueness") {
      rotateUniqueness();
      return;
    }
    store.sortField = field;
    store.currentPage = 1;
    animKey++;
    requestAnimationFrame(() => {
      scrollContainer?.scrollTo({ top: 0, behavior: "instant" });
      scrollContainer?.dispatchEvent(new Event("scroll"));
    });
  }

  function setDir(dir: SortDir) {
    store.sortDir = dir;
    store.currentPage = 1;
    animKey++;
    requestAnimationFrame(() => {
      scrollContainer?.scrollTo({ top: 0, behavior: "instant" });
      scrollContainer?.dispatchEvent(new Event("scroll"));
    });
  }

  function setPageSize(size: PageSize) {
    // Maintain position: find which new page contains the first item currently visible
    const firstIndex = (safePage - 1) * pageSize;
    store.pageSize = size;
    store.currentPage = Math.max(1, Math.floor(firstIndex / size) + 1);
    requestAnimationFrame(() => {
      scrollContainer?.dispatchEvent(new Event("scroll"));
    });
  }

  const sortedEntries = $derived.by(() => {
    if (sortField === "uniqueness") {
      const jitter = uniquenessJitter;
      return [...allEntries]
        .map((e, i) => ({ e, score: baseUniqueness(e.postCount) * (jitter[i] ?? 1) }))
        .sort((a, b) => b.score - a.score)
        .map((x) => x.e);
    }
    const dir = sortDir === "asc" ? 1 : -1;
    return [...allEntries].sort((a, b) =>
      sortField === "name"
        ? a.slug.localeCompare(b.slug) * dir
        : (a.postCount - b.postCount) * dir,
    );
  });

  const filteredEntries = $derived.by(() => {
    if (!showOnlyFavourites) return sortedEntries;
    const favMap = artistFavourites.favourites;
    return sortedEntries.filter((e) => {
      const fav = favMap[e.slug];
      if (!fav) return false;
      if (favouriteCategoryFilter === "all") return true;
      if (favouriteCategoryFilter === "__uncat") return fav.categoryId === null;
      return fav.categoryId === favouriteCategoryFilter;
    });
  });
  const totalPages = $derived(Math.max(1, Math.ceil(filteredEntries.length / pageSize)));
  const safePage = $derived(Math.min(currentPage, totalPages));
  const pageEntries = $derived(
    filteredEntries.slice((safePage - 1) * pageSize, safePage * pageSize),
  );

  // When the user is typing in the search box, show search results directly
  // in the grid instead of the normal paginated results.
  const isSearching = $derived(queryInput.trim().length > 0);
  const gridEntries = $derived<ArtistSearchHit[]>(isSearching ? store.results : pageEntries);

  // ---------------------------------------------------------------------------
  // Image preload cache
  // ---------------------------------------------------------------------------
  // Keep a Map of HTMLImageElement refs so the browser doesn't evict images
  // from its cache when they're not in the DOM. The window covers the current
  // page plus 4 pages back and 1 page ahead.
  const _preloadCache = new Map<string, HTMLImageElement>();

  const _preloadUrls = $derived.by(() => {
    if (!store.manifest) return [] as string[];
    const { imageBaseUrl, releasePrefix } = store.manifest;
    const start = Math.max(1, safePage - 4);
    const end = Math.min(totalPages, safePage + 1);
    const urls: string[] = [];
    for (let p = start; p <= end; p++) {
      for (const hit of sortedEntries.slice((p - 1) * pageSize, p * pageSize)) {
        if (hit.hasImage && hit.imageId) {
          urls.push(`${imageBaseUrl}/${releasePrefix}/images/${hit.imageId}.webp`);
        }
      }
    }
    return urls;
  });

  $effect(() => {
    const urlSet = new Set(_preloadUrls);
    // Eagerly load anything not yet cached
    for (const url of urlSet) {
      if (!_preloadCache.has(url)) {
        const img = new Image();
        img.src = url;
        _preloadCache.set(url, img);
      }
    }
    // Release entries that have scrolled beyond the window
    for (const url of _preloadCache.keys()) {
      if (!urlSet.has(url)) _preloadCache.delete(url);
    }
  });
</script>

<div class="flex h-full w-full flex-col overflow-hidden bg-neutral-950 text-neutral-100">
  <header class="flex-none border-b border-neutral-800 bg-neutral-900/60 px-4 py-3">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-lg font-semibold">Artist Gallery</h1>
        <p class="text-xs text-neutral-500">
          {#if store.manifest}
            {store.manifest.artistsWithImage.toLocaleString()} artists ·
            Anima preview · release {store.manifest.releasePrefix} ·
            <button type="button" class="underline hover:text-neutral-300" onclick={() => showGenParams = true}>ℹ gen params</button>
          {:else if store.manifestError}
            <span class="text-red-400">failed to load: {store.manifestError}</span>
          {:else}
            loading manifest…
          {/if}
        </p>
      </div>

      <!-- Search -->
      <div class="relative w-full max-w-sm">
        <input
          type="search"
          placeholder="Search artist tag…"
          value={queryInput}
          oninput={(e) => onSearchInput(e.currentTarget.value)}
          class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
        />
        {#if store.searchLoading}
          <span class="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-neutral-500">…</span>
        {/if}
      </div>
    </div>

    <!-- Sort + page size toolbar -->
    {#if store.manifest}
      <div class="mt-3 flex flex-wrap items-center gap-2">
        <div class="flex items-center gap-0.5 rounded-lg border border-neutral-800 bg-neutral-900/50 p-1">
          <span class="px-1.5 text-xs text-neutral-500">Sort:</span>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {sortField === 'postCount' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setSort('postCount')}
          >
            Post Count
          </button>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {sortField === 'name' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setSort('name')}
          >
            Name
          </button>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {sortField === 'uniqueness' ? 'bg-amber-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setSort('uniqueness')}
            title="Surfaces hidden gems: artists with a distinctive style not yet overexposed"
          >
            Unique
          </button>
          <div class="mx-1 h-3 w-px shrink-0 bg-neutral-700"></div>
          {#if sortField === 'uniqueness'}
            <button
              type="button"
              class="rounded px-2 py-0.5 text-xs text-amber-400 transition-colors hover:text-amber-200"
              onclick={rotateUniqueness}
              title="Reshuffle the uniqueness ranking to discover a fresh set of hidden gems"
            >
              ↻ Rotate
            </button>
          {:else}
            <button
              type="button"
              class="rounded px-2 py-0.5 text-xs transition-colors {sortDir === 'desc' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
              onclick={() => setDir('desc')}
              title="Descending"
            >
              ↓ Desc
            </button>
            <button
              type="button"
              class="rounded px-2 py-0.5 text-xs transition-colors {sortDir === 'asc' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
              onclick={() => setDir('asc')}
              title="Ascending"
            >
              ↑ Asc
            </button>
          {/if}
        </div>

        <div class="flex items-center gap-0.5 rounded-lg border border-neutral-800 bg-neutral-900/50 p-1">
          <span class="px-1.5 text-xs text-neutral-500">Per page:</span>
          {#each PAGE_SIZES as size}
            <button
              type="button"
              class="rounded px-2 py-0.5 text-xs transition-colors {pageSize === size ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
              onclick={() => setPageSize(size)}
            >
              {size}
            </button>
          {/each}
        </div>

        <div class="flex items-center gap-2 rounded-lg border border-neutral-800 bg-neutral-900/50 px-2 py-1">
          <span class="text-xs text-neutral-500">Size:</span>
          <input
            type="range"
            min="0"
            max="100"
            value={sliderValue}
            oninput={(e) => { sliderValue = parseInt(e.currentTarget.value, 10); }}
            class="w-20 accent-indigo-500"
          />
        </div>

        <button
          type="button"
          class="rounded-lg border px-2 py-1 text-xs transition-colors {showOnlyFavourites ? 'border-red-500 bg-red-950/50 text-red-300' : 'border-neutral-800 bg-neutral-900/50 text-neutral-400 hover:text-neutral-200'}"
          onclick={() => setShowOnlyFavourites(!showOnlyFavourites)}
          title="Toggle favourites filter"
        >
          {showOnlyFavourites ? '♥' : '♡'} Favourites{artistFavourites.count > 0 ? ` (${artistFavourites.count})` : ''}
        </button>

        <button
          type="button"
          class="rounded-lg border border-neutral-800 bg-neutral-900/50 px-2 py-1 text-xs text-neutral-400 transition-colors hover:text-neutral-200"
          onclick={() => showFavouritesManager = true}
          title="Manage categories, import/export favourites"
        >
          ⚙ Manage
        </button>
      </div>

      {#if showOnlyFavourites}
        {@const counts = artistFavourites.countsByCategory}
        <div class="mt-2 flex flex-wrap items-center gap-1 rounded-lg border border-neutral-800 bg-neutral-900/50 p-1">
          <span class="px-1.5 text-xs text-neutral-500">Category:</span>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {favouriteCategoryFilter === 'all' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setFavouriteCategoryFilter('all')}
          >
            All ({artistFavourites.count})
          </button>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {favouriteCategoryFilter === '__uncat' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setFavouriteCategoryFilter('__uncat')}
          >
            Uncategorised ({counts[''] ?? 0})
          </button>
          {#each artistFavourites.categories as cat (cat.id)}
            <button
              type="button"
              class="flex items-center gap-1 rounded px-2 py-0.5 text-xs transition-colors {favouriteCategoryFilter === cat.id ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
              onclick={() => setFavouriteCategoryFilter(cat.id)}
              title={cat.name}
            >
              <span class="h-2.5 w-2.5 rounded-full border border-neutral-700" style="background-color: {cat.color}" aria-hidden="true"></span>
              <span class="max-w-36 truncate">{cat.name}</span>
              <span class="text-neutral-500">({counts[cat.id] ?? 0})</span>
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  </header>

  <div class="flex-1 overflow-y-auto" bind:this={scrollContainer}>
    {#if allError}
      <div class="p-8 text-center text-sm text-red-400">
        Failed to load artists: {allError}
      </div>
    {:else if allLoading}
      <div class="p-8 text-center text-sm text-neutral-500">loading artists…</div>
    {:else}
      <p class="px-4 pt-2 text-xs text-neutral-500">
        {#if isSearching}
          {store.searchLoading ? "Searching\u2026" : `${store.results.length.toLocaleString()} result${store.results.length === 1 ? '' : 's'} for "${queryInput}"`}
        {:else}
          Right-click a card or hover to copy its tag.
        {/if}
      </p>
      {#if totalPages > 1 && !isSearching}
        <div class="flex flex-wrap items-center justify-center gap-2 border-b border-neutral-800/60 px-4 py-2">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1 text-sm text-neutral-300 transition-colors hover:border-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
            disabled={safePage <= 1}
            onclick={() => goToPage(safePage - 1)}
          >
            ← Prev
          </button>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-300 transition-colors hover:border-amber-500"
            onclick={goToRandomPage}
            title="Jump to a random page"
          >
            ⚄ Random
          </button>
          <span class="text-sm text-neutral-400">
            Page {safePage} of {totalPages}
            <span class="text-neutral-600">·</span>
            {filteredEntries.length.toLocaleString()} artists
          </span>
          <div class="flex items-center gap-1">
            <input
              type="number"
              min="1"
              max={totalPages}
              placeholder="pg #"
              value={pageInputValue}
              oninput={(e) => { pageInputValue = e.currentTarget.value; }}
              onkeydown={(e) => { if (e.key === 'Enter') commitPageInput(); }}
              class="w-14 rounded border border-neutral-700 bg-neutral-800 px-1.5 py-0.5 text-xs text-neutral-200 focus:border-indigo-500 focus:outline-none"
            />
            <button
              type="button"
              class="rounded border border-neutral-700 bg-neutral-800 px-1.5 py-0.5 text-xs text-neutral-300 transition-colors hover:border-indigo-500"
              onclick={commitPageInput}
              title="Go to page"
            >↵</button>
          </div>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1 text-sm text-neutral-300 transition-colors hover:border-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
            disabled={safePage >= totalPages}
            onclick={() => goToPage(safePage + 1)}
          >
            Next →
          </button>
        </div>
      {/if}
      {#key animKey}
      <div class="grid gap-3 p-4" style="grid-template-columns: repeat(auto-fill, minmax({cardMinWidth}px, 1fr))">
        {#each gridEntries as hit, i (hit.slug)}
          {@const url = thumbUrl(hit)}
          {@const rank = (safePage - 1) * pageSize + i + 1}
          {@const isFav = artistFavourites.isFavourite(hit.slug)}
          {@const favCat = artistFavourites.categoryOf(hit.slug)}
          <div
            role="button"
            tabindex="0"
            class="card-slide-in group relative flex flex-col items-stretch rounded-lg border bg-neutral-900 cursor-pointer transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500 {copiedSlug === hit.slug ? 'border-emerald-500' : 'border-neutral-800 hover:border-indigo-500'} {catPickerSlug === hit.slug ? 'z-50' : ''}"
            style="--card-delay: {Math.min(i * 30, 450)}ms"
            onclick={() => { void openHit(hit, i); }}
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); void openHit(hit, i); } }}
            oncontextmenu={(e) => { e.preventDefault(); void copyTag(hit.tag, hit.slug); }}
            title="{hit.tag} · Right-click to copy tag"
          >
            <div class="relative aspect-3/4 w-full overflow-hidden rounded-t-lg bg-neutral-800">
              {#if url}
                <img
                  src={url}
                  alt={hit.tag}
                  loading="eager"
                  decoding="auto"
                  class="h-full w-full object-cover"
                />
              {:else}
                <div class="flex h-full w-full items-center justify-center text-xs text-neutral-500">
                  no preview
                </div>
              {/if}
              {#if sortField === 'uniqueness'}
                <div class="absolute left-1 top-1 rounded bg-amber-600/90 px-1 py-0.5 text-[10px] font-mono font-bold leading-none text-white">
                  #{rank}
                </div>
              {/if}
              <button
                type="button"
                class="absolute right-1 top-1 rounded border border-neutral-700 bg-neutral-900/90 px-1.5 py-0.5 text-[10px] text-neutral-200 opacity-0 transition-opacity group-hover:opacity-100 hover:border-indigo-500"
                onclick={(e) => { e.stopPropagation(); void copyTag(hit.tag, hit.slug); }}
                aria-label="Copy tag"
              >
                Copy
              </button>
              {#if copiedSlug === hit.slug}
                <div class="absolute inset-0 flex items-center justify-center bg-neutral-900/80">
                  <span class="rounded bg-emerald-600 px-2 py-1 text-xs font-semibold text-white">Copied!</span>
                </div>
              {/if}
            </div>
            <div class="flex items-center justify-between gap-1 px-2 py-1.5">
              <span class="min-w-0 truncate text-sm text-red-400">{displayTag(hit.tag)}</span>
              <div class="relative flex shrink-0 items-center gap-1">
                <span class="text-xs text-neutral-500">{formatCount(hit.postCount)}</span>
                {#if isFav}
                  <button
                    type="button"
                    class="flex h-4 w-4 items-center justify-center rounded-full border border-neutral-700 transition-transform hover:scale-110"
                    style={favCat ? `background-color: ${favCat.color}` : "background-color: transparent"}
                    onclick={(e) => openCategoryPicker(hit.slug, e)}
                    aria-label={favCat ? `Category: ${favCat.name}. Click to change.` : "Assign category"}
                    title={favCat ? `Category: ${favCat.name}` : "Assign category"}
                  ></button>
                {/if}
                <button
                  type="button"
                  class="text-sm leading-none transition-colors {isFav ? 'text-red-400' : 'text-neutral-600 hover:text-red-400'}"
                  onclick={(e) => toggleFavourite(hit.slug, e)}
                  oncontextmenu={(e) => openCategoryPicker(hit.slug, e)}
                  aria-label={isFav ? 'Remove from favourites' : 'Add to favourites'}
                  title={isFav ? 'Remove from favourites · right-click to categorise' : 'Add to favourites'}
                >
                  {isFav ? '♥' : '♡'}
                </button>
                {#if catPickerSlug === hit.slug}
                  <!-- Backdrop to dismiss -->
                  <button
                    type="button"
                    class="fixed inset-0 z-40 cursor-default"
                    aria-label="Close category picker"
                    onclick={(e) => { e.stopPropagation(); catPickerSlug = null; }}
                  ></button>
                  <div
                    class="absolute right-0 top-full z-50 mt-1 w-48 rounded-md border border-neutral-700 bg-neutral-900 p-1 shadow-xl"
                    role="menu"
                  >
                    <button
                      type="button"
                      role="menuitem"
                      class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs text-neutral-200 hover:bg-neutral-800"
                      onclick={(e) => { e.stopPropagation(); assignCategory(hit.slug, null); }}
                    >
                      <span class="h-2.5 w-2.5 rounded-full border border-neutral-700" aria-hidden="true"></span>
                      Uncategorised
                    </button>
                    {#each artistFavourites.categories as cat (cat.id)}
                      <button
                        type="button"
                        role="menuitem"
                        class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs text-neutral-200 hover:bg-neutral-800"
                        onclick={(e) => { e.stopPropagation(); assignCategory(hit.slug, cat.id); }}
                      >
                        <span class="h-2.5 w-2.5 rounded-full border border-neutral-700" style="background-color: {cat.color}" aria-hidden="true"></span>
                        <span class="truncate">{cat.name}</span>
                      </button>
                    {/each}
                    <div class="my-1 border-t border-neutral-800"></div>
                    <button
                      type="button"
                      role="menuitem"
                      class="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs text-indigo-300 hover:bg-neutral-800"
                      onclick={(e) => { e.stopPropagation(); catPickerSlug = null; showFavouritesManager = true; }}
                    >
                      ＋ New category…
                    </button>
                  </div>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
      {/key}

      <!-- Pagination -->
      {#if totalPages > 1 && !isSearching}
        <div class="flex flex-wrap items-center justify-center gap-2 px-4 py-4">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-sm text-neutral-300 transition-colors hover:border-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
            disabled={safePage <= 1}
            onclick={() => goToPage(safePage - 1)}
          >
            ← Prev
          </button>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-sm text-neutral-300 transition-colors hover:border-amber-500"
            onclick={goToRandomPage}
            title="Jump to a random page"
          >
            ⚄ Random
          </button>
          <span class="text-sm text-neutral-400">
            Page {safePage} of {totalPages}
            <span class="text-neutral-600">·</span>
            {filteredEntries.length.toLocaleString()} artists
          </span>
          <div class="flex items-center gap-1">
            <input
              type="number"
              min="1"
              max={totalPages}
              placeholder="pg #"
              value={pageInputValue}
              oninput={(e) => { pageInputValue = e.currentTarget.value; }}
              onkeydown={(e) => { if (e.key === 'Enter') commitPageInput(); }}
              class="w-14 rounded border border-neutral-700 bg-neutral-800 px-1.5 py-1 text-xs text-neutral-200 focus:border-indigo-500 focus:outline-none"
            />
            <button
              type="button"
              class="rounded border border-neutral-700 bg-neutral-800 px-1.5 py-1 text-xs text-neutral-300 transition-colors hover:border-indigo-500"
              onclick={commitPageInput}
              title="Go to page"
            >↵</button>
          </div>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-sm text-neutral-300 transition-colors hover:border-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
            disabled={safePage >= totalPages}
            onclick={() => goToPage(safePage + 1)}
          >
            Next →
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>

{#if active}
  <ArtistLightbox
    entry={active}
    onclose={closeLightbox}
    {oninsertTag}
    zoomed={lightboxZoomed}
    ontogglezoom={() => store.lightboxZoomed = !store.lightboxZoomed}
    onprev={activeIndex > 0 ? () => navigateTo(activeIndex - 1) : undefined}
    onnext={activeIndex >= 0 && activeIndex < gridEntries.length - 1 ? () => navigateTo(activeIndex + 1) : undefined}
  />
{/if}

{#if showFavouritesManager}
  <FavouritesManager onclose={() => showFavouritesManager = false} />
{/if}

{#if showGenParams}
  <div
    class="fixed inset-0 z-200 flex items-center justify-center bg-black/80 backdrop-blur-sm"
    role="dialog"
    aria-modal="true"
    aria-label="Generation parameters"
  >
    <button type="button" class="absolute inset-0 h-full w-full cursor-default" aria-label="Close" onclick={() => showGenParams = false}></button>
    <div class="relative z-10 w-full max-w-lg rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
      <div class="mb-4 flex items-center justify-between">
        <h2 class="text-sm font-semibold text-neutral-100">Preview Generation Parameters</h2>
        <button type="button" class="text-neutral-500 hover:text-neutral-200 text-lg leading-none" onclick={() => showGenParams = false} aria-label="Close">✕</button>
      </div>
      <div class="space-y-3 text-xs text-neutral-400">
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">Model Stack</h3>
          <table class="w-full">
            <tbody>
              <tr><td class="py-0.5 pr-4 text-neutral-500">UNet</td><td class="text-neutral-200">Anima SDXL Base</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">Text Encoder</td><td class="text-neutral-200">CLIP-L + CLIP-G (SDXL dual)</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">VAE</td><td class="text-neutral-200">sdxl_vae.safetensors</td></tr>
            </tbody>
          </table>
        </section>
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">Sampler</h3>
          <table class="w-full">
            <tbody>
              <tr><td class="py-0.5 pr-4 text-neutral-500">Sampler</td><td class="text-neutral-200">er_sde</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">Scheduler</td><td class="text-neutral-200">sgm_uniform</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">Steps</td><td class="text-neutral-200">30</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">CFG Scale</td><td class="text-neutral-200">4.0</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">Seed</td><td class="text-neutral-200">42</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">Resolution</td><td class="text-neutral-200">896 × 1152</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">Output</td><td class="text-neutral-200">WebP (lossy, q=85)</td></tr>
            </tbody>
          </table>
        </section>
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">Positive Prompt</h3>
          <p class="rounded bg-neutral-800 px-2 py-1.5 font-mono leading-relaxed text-neutral-200">score_9, score_8_up, score_7_up, masterpiece, best quality, <span class="text-red-400">@artist_tag</span></p>
        </section>
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">Negative Prompt</h3>
          <p class="rounded bg-neutral-800 px-2 py-1.5 font-mono leading-relaxed text-neutral-200">score_1, score_2, score_3, worst quality, low quality, blurry, watermark</p>
        </section>
      </div>
    </div>
  </div>
{/if}
