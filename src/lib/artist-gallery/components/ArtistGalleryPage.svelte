<script lang="ts">
  import { onMount, tick } from "svelte";
  import { createArtistGalleryStore, type ArtistSortField, type ArtistSortDir, type ArtistPageSize } from "../store.svelte.js";
  import { artistFavourites } from "../favourites.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { formatPostCount, rankingPostCount } from "../counts.js";
  import type { ArtistSearchHit } from "../types.js";
  import ArtistLightbox from "./ArtistLightbox.svelte";
  import FavouritesManager from "./FavouritesManager.svelte";
  import { proxiedCdnUrl } from "../../utils/cdnFetch.js";
  import { cachedSrc } from "../imageCache.js";
  import { CharacterExplorer } from "../../animadex/index.js";
  import type { AnimadexCharacter } from "../../animadex/types.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { detectArtistsInPrompt } from "../detection.js";
  import { ARTIST_PREVIEW_RECIPE } from "../previewRecipe.js";
  import type { ArtistPreviewStatus, ArtistPreviewVariant } from "../previewRecipe.js";

  type ExplorerTab = "artists" | "characters";

  interface Props {
    manifestUrl: string;
    /** Initial explorer tab when mounting this page. */
    initialTab?: ExplorerTab;
    /** Optional integrator hook for "Insert tag into prompt" in the lightbox. */
    oninsertTag?: (tag: string) => void;
    /** Insert character tags into the positive prompt (opens chooser modal). */
    oninsertCharacter?: (character: AnimadexCharacter) => void;
    /** Kick off a local generation of the CDN recipe for this artist. */
    ongeneratePreview?: (slug: string, tag: string, variant: ArtistPreviewVariant) => void;
    /** Current local-preview state for a card. Omitted means no Generate button. */
    previewStatus?: (slug: string, variant: ArtistPreviewVariant) => ArtistPreviewStatus;
  }

  let { manifestUrl, initialTab = "artists", oninsertTag, oninsertCharacter, ongeneratePreview, previewStatus }: Props = $props();

  let explorerTab = $state<ExplorerTab>("artists");
  let lastInitialTab = $state<ExplorerTab>("artists");

  $effect(() => {
    if (initialTab !== lastInitialTab) {
      lastInitialTab = initialTab;
      explorerTab = initialTab;
    }
  });

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
    // Also load artist search index in gallery store so the active check has access to the index.
    void gallery.loadArtistIndex(manifestUrl);

    store.init().then(async () => {
      // Load entries only once; subsequent mounts reuse cached data.
      if (store.allEntries.length === 0 && !store.allEntriesLoading) {
        store.allEntriesLoading = true;
        store.allEntriesError = null;
        try {
          const [indexed, gap] = await Promise.all([
            store.client.loadSearchIndex(),
            store.client.loadNoPreviewHits(),
          ]);
          // Image-bearing entries first: `uniquenessJitter` is positional, and
          // the sort below relies on nothing beyond `hasImage`.
          const entries = [...indexed, ...gap];
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
      ? proxiedCdnUrl(`${store.manifest.imageBaseUrl}/${store.manifest.releasePrefix}/images/${hit.imageId}.${imgExt()}`)
      : "";
    store.lightboxEntry = { tag: hit.tag, slug: hit.slug, imageId: hit.imageId, imageUrl, objectKey: "", postCount: hit.postCount, belowThreshold: hit.belowThreshold, b: hit.b, aliases: [], hasImage: hit.hasImage };
    store.lightboxIndex = index;
    // Background: fetch full shard entry to patch in aliases (and v2 variants) once loaded
    store.client.getArtist(hit.slug).then((full) => {
      if (full && store.lightboxEntry?.slug === hit.slug) {
        store.lightboxEntry = {
          ...full,
          imageUrl: proxiedCdnUrl(full.imageUrl),
          images: full.images?.map((img) => ({ ...img, imageUrl: proxiedCdnUrl(img.imageUrl) })),
        };
      }
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

  // ---------------------------------------------------------------------------
  // Image variant toggle (index v2+; inert on single-variant v1 data)
  // ---------------------------------------------------------------------------
  let globalVariant = $derived(store.globalVariant);

  /** Variants the CDN ships for this artist. Cheap; safe to call over allEntries. */
  function cdnVariantCountOf(hit: ArtistSearchHit): number {
    return Math.max(1, hit.variantCount ?? hit.images?.length ?? 1);
  }

  /** Highest local variant that exists or is being generated (0 = none). */
  function localPreviewCount(hit: ArtistSearchHit): number {
    if (!previewStatus) return 0;
    let n = 0;
    for (const v of [1, 2] as const) {
      const st = previewStatus(hit.slug, v);
      if (st.state === "ready" || st.state === "running") n = v;
    }
    return n;
  }

  function variantCountOf(hit: ArtistSearchHit): number {
    if (hit.hasImage) return cdnVariantCountOf(hit);
    // Once a placeholder has a local variant 1, offer the flip so the second
    // recipe prompt is reachable.
    return localPreviewCount(hit) >= 1 ? 2 : 1;
  }

  /** Variant a placeholder's Generate button targets, clamped to what it offers. */
  function previewVariantOf(hit: ArtistSearchHit): ArtistPreviewVariant {
    return Math.min(store.resolveVariant(hit.slug), variantCountOf(hit)) >= 2 ? 2 : 1;
  }

  /** True when ANY entry in the dataset exposes >= 2 variants. */
  const hasVariants = $derived.by(() => allEntries.some((e) => cdnVariantCountOf(e) >= 2));
  /** Largest variant count across the dataset (drives the toolbar toggle). */
  const maxVariants = $derived.by(() =>
    allEntries.reduce((m, e) => Math.max(m, cdnVariantCountOf(e)), 1),
  );

  function setGlobalVariant(value: number) {
    store.setGlobalVariant(value);
  }

  function flipCard(hit: ArtistSearchHit, e: MouseEvent) {
    e.stopPropagation();
    const count = variantCountOf(hit);
    if (count < 2) return;
    const current = Math.min(store.resolveVariant(hit.slug), count);
    const next = (current % count) + 1;
    store.setVariant(hit.slug, next);
  }

  function imageIdForVariant(hit: ArtistSearchHit, variant: number): string {
    // Prefer an explicit per-variant entry when the shard data is present.
    const img = hit.images?.[variant - 1];
    if (img?.imageId) return img.imageId;
    // search.json hits carry no `images[]`, so derive the variant id by
    // swapping the `-p<n>` suffix (all v2 imageIds end in `-p1`/`-p2`).
    return hit.imageId.replace(/-p\d+$/, `-p${variant}`);
  }

  /** Image extension for the active dataset: AVIF in index v2+, WebP in v1. */
  function imgExt(): string {
    return (store.manifest?.version ?? 1) >= 2 ? "avif" : "webp";
  }

  function thumbUrl(hit: ArtistSearchHit): string {
    if (!hit.hasImage) {
      // No CDN image: a locally generated preview is the only candidate.
      return previewStatus?.(hit.slug, previewVariantOf(hit))?.src ?? "";
    }
    if (!store.manifest) return "";
    const count = variantCountOf(hit);
    const variant = Math.min(store.resolveVariant(hit.slug), count);
    const imageId = imageIdForVariant(hit, variant);
    if (!imageId) return "";
    return proxiedCdnUrl(`${store.manifest.imageBaseUrl}/${store.manifest.releasePrefix}/images/${imageId}.${imgExt()}`);
  }

  // Variant state for the open lightbox entry (kept in sync with its grid card).
  const activeVariantCount = $derived.by(() => {
    if (!active) return 1;
    const hit = gridEntries.find((h) => h.slug === active.slug);
    return Math.max(1, hit ? variantCountOf(hit) : active.images?.length ?? 1);
  });
  const activeVariant = $derived(
    active ? Math.min(store.resolveVariant(active.slug), activeVariantCount) : 1,
  );
  const activeCanFlip = $derived(activeVariantCount >= 2);

  function flipActiveVariant() {
    if (!active) return;
    const count = activeVariantCount;
    if (count < 2) return;
    const current = Math.min(store.resolveVariant(active.slug), count);
    const next = (current % count) + 1;
    store.setVariant(active.slug, next);
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

  function setIncludeNoPreview(val: boolean) {
    store.setIncludeNoPreview(val);
    store.currentPage = 1;
    animKey++;
    // Re-run any active search so the typeahead results respect the new filter.
    void store.setQuery(store.query);
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

  const detectedArtists = $derived.by(() => {
    if (gallery.artistIndexReady && gallery.artistTagIndex.size > 0) {
      return detectArtistsInPrompt(generation.positivePrompt, gallery.artistTagIndex);
    }
    return [];
  });

  function isArtistInPrompt(tag: string): boolean {
    if (gallery.artistIndexReady && gallery.artistTagIndex.size > 0) {
      const normalizedTag = tag.replace(/^@+/, "").toLowerCase().replace(/\s+/g, "_");
      const targetHit = gallery.artistTagIndex.get(normalizedTag);
      if (targetHit) {
        return detectedArtists.some((d) => d.slug === targetHit.slug);
      }
    }
    // Simple fallback
    const cleanTag = tag.replace(/^@+/, "").toLowerCase().trim();
    if (!cleanTag) return false;
    const promptLower = (generation.positivePrompt || "").toLowerCase();
    return promptLower.includes(cleanTag);
  }

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
      const scored: { e: ArtistSearchHit; score: number }[] = [];
      const placeholders: ArtistSearchHit[] = [];
      allEntries.forEach((e, i) => {
        if (e.hasImage) {
          scored.push({ e, score: baseUniqueness(rankingPostCount(e)) * (jitter[i] ?? 1) });
        } else {
          placeholders.push(e);
        }
      });
      scored.sort((a, b) => b.score - a.score);
      // A card with no image has nothing to look unique about, so placeholders
      // trail the scored set in plain post-count order instead of being
      // jittered into the middle of it.
      placeholders.sort(
        (a, b) => rankingPostCount(b) - rankingPostCount(a) || a.slug.localeCompare(b.slug),
      );
      return [...scored.map((x) => x.e), ...placeholders];
    }
    const dir = sortDir === "asc" ? 1 : -1;
    return [...allEntries].sort((a, b) =>
      sortField === "name"
        ? a.slug.localeCompare(b.slug) * dir
        : (rankingPostCount(a) - rankingPostCount(b)) * dir,
    );
  });

  /** True when a hit passes the active favourites + category filter. */
  function matchesFavouriteFilter(hit: ArtistSearchHit): boolean {
    const fav = artistFavourites.favourites[hit.slug];
    if (!fav) return false;
    if (favouriteCategoryFilter === "all") return true;
    if (favouriteCategoryFilter === "__uncat") return fav.categoryId === null;
    return fav.categoryId === favouriteCategoryFilter;
  }

  const filteredEntries = $derived.by(() => {
    const base = store.includeNoPreview ? sortedEntries : sortedEntries.filter((e) => e.hasImage);
    return showOnlyFavourites ? base.filter(matchesFavouriteFilter) : base;
  });
  const totalPages = $derived(Math.max(1, Math.ceil(filteredEntries.length / pageSize)));
  const safePage = $derived(Math.min(currentPage, totalPages));
  const pageEntries = $derived(
    filteredEntries.slice((safePage - 1) * pageSize, safePage * pageSize),
  );

  // When the user is typing in the search box, show search results directly
  // in the grid instead of the normal paginated results.
  const isSearching = $derived(queryInput.trim().length > 0);

  /** Same normalisation the client's search applies, so both paths agree. */
  function normalizeQuery(text: string): string {
    return text.toLowerCase().trim().replace(/\s+/g, "_").replace(/\\/g, "").replace(/^@+/, "");
  }

  /**
   * Favourites-scoped search. The client's remote search caps at 50 hits across
   * the WHOLE index, so post-filtering it to favourites would silently drop a
   * favourite ranked below that cut. Search the already-filtered favourites set
   * locally instead: complete, and prefix matches still rank first.
   */
  const favouriteSearchEntries = $derived.by(() => {
    const q = normalizeQuery(queryInput);
    if (!q) return filteredEntries;
    const prefix: ArtistSearchHit[] = [];
    const contains: ArtistSearchHit[] = [];
    for (const hit of filteredEntries) {
      const slugLower = hit.slug.toLowerCase();
      if (slugLower.startsWith(q)) prefix.push(hit);
      else if (slugLower.includes(q) || hit.tag.toLowerCase().includes(q)) contains.push(hit);
    }
    return [...prefix, ...contains];
  });

  const searchEntries = $derived<ArtistSearchHit[]>(
    showOnlyFavourites ? favouriteSearchEntries : store.results,
  );
  const gridEntries = $derived<ArtistSearchHit[]>(isSearching ? searchEntries : pageEntries);

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
          urls.push(proxiedCdnUrl(`${imageBaseUrl}/${releasePrefix}/images/${hit.imageId}.${imgExt()}`));
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
  <div class="flex-none border-b border-neutral-800 bg-neutral-900/80 px-4 pt-3">
    <div class="mb-3 flex gap-1 rounded-lg border border-neutral-800 bg-neutral-950/80 p-1 w-fit">
      <button
        type="button"
        class="rounded-md px-3 py-1 text-xs font-medium transition-colors {explorerTab === 'artists' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
        onclick={() => (explorerTab = 'artists')}
      >
        {locale.t('artist_gallery.tab_artists')}
      </button>
      <button
        type="button"
        class="rounded-md px-3 py-1 text-xs font-medium transition-colors {explorerTab === 'characters' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
        onclick={() => (explorerTab = 'characters')}
      >
        {locale.t('artist_gallery.tab_characters')}
      </button>
    </div>
  </div>

  {#if explorerTab === 'characters'}
    <CharacterExplorer oninsertCharacter={oninsertCharacter} />
  {:else}
  <header class="flex-none border-b border-neutral-800 bg-neutral-900/60 px-4 py-3">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-lg font-semibold">{locale.t('artist_gallery.title')}</h1>
        <p class="text-xs text-neutral-500">
          {#if store.manifest}
            {locale.t('artist_gallery.subtitle', { count: locale.formatInteger(store.manifest.artistsWithImage), release: store.manifest.releasePrefix })}
            <button type="button" class="underline hover:text-neutral-300" onclick={() => showGenParams = true}>{locale.t('artist_gallery.gen_params_btn')}</button>
          {:else if store.manifestError}
            <span class="text-red-400">{locale.t('artist_gallery.manifest_error', { error: store.manifestError })}</span>
          {:else}
            {locale.t('artist_gallery.loading_manifest')}
          {/if}
        </p>
      </div>
    </div>

    <!-- Search + sort + page size toolbar -->
    {#if store.manifest}
      <div class="mt-3 flex flex-wrap items-center gap-2">
        <!-- Search -->
        <div class="relative min-w-48 flex-1 sm:max-w-xs">
          <input
            type="search"
            placeholder={locale.t('artist_gallery.search_placeholder')}
            value={queryInput}
            oninput={(e) => onSearchInput(e.currentTarget.value)}
            class="w-full rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
          />
          {#if store.searchLoading}
            <span class="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-neutral-500">…</span>
          {/if}
        </div>

        <div class="flex items-center gap-0.5 rounded-lg border border-neutral-800 bg-neutral-900/50 p-1">
          <span class="px-1.5 text-xs text-neutral-500">{locale.t('artist_gallery.sort_label')}</span>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {sortField === 'postCount' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setSort('postCount')}
          >
            {locale.t('artist_gallery.sort_post_count')}
          </button>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {sortField === 'name' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setSort('name')}
          >
            {locale.t('artist_gallery.sort_name')}
          </button>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {sortField === 'uniqueness' ? 'bg-amber-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setSort('uniqueness')}
            title={locale.t('artist_gallery.sort_trending_title')}
          >
            {locale.t('artist_gallery.sort_trending')}
          </button>
          <div class="mx-1 h-3 w-px shrink-0 bg-neutral-700"></div>
          {#if sortField === 'uniqueness'}
            <button
              type="button"
              class="rounded px-2 py-0.5 text-xs text-amber-400 transition-colors hover:text-amber-200"
              onclick={rotateUniqueness}
              title={locale.t('artist_gallery.sort_rotate_title')}
            >
              {locale.t('artist_gallery.sort_rotate')}
            </button>
          {:else}
            <button
              type="button"
              class="rounded px-2 py-0.5 text-xs transition-colors {sortDir === 'desc' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
              onclick={() => setDir('desc')}
              title={locale.t('artist_gallery.sort_desc_title')}
            >
              {locale.t('artist_gallery.sort_desc')}
            </button>
            <button
              type="button"
              class="rounded px-2 py-0.5 text-xs transition-colors {sortDir === 'asc' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
              onclick={() => setDir('asc')}
              title={locale.t('artist_gallery.sort_asc_title')}
            >
              {locale.t('artist_gallery.sort_asc')}
            </button>
          {/if}
        </div>

        <div class="flex items-center gap-0.5 rounded-lg border border-neutral-800 bg-neutral-900/50 p-1">
          <span class="px-1.5 text-xs text-neutral-500">{locale.t('artist_gallery.per_page_label')}</span>
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
          <span class="text-xs text-neutral-500">{locale.t('artist_gallery.size_label')}</span>
          <input
            type="range"
            min="0"
            max="100"
            value={sliderValue}
            oninput={(e) => { sliderValue = parseInt(e.currentTarget.value, 10); }}
            class="w-20 accent-indigo-500"
          />
        </div>

        {#if hasVariants}
          <div class="flex items-center gap-0.5 rounded-lg border border-neutral-800 bg-neutral-900/50 p-1">
            <span class="px-1.5 text-xs text-neutral-500">{locale.t('artist_gallery.variant_label')}</span>
            {#each Array(maxVariants) as _, idx}
              {@const n = idx + 1}
              <button
                type="button"
                class="rounded px-2 py-0.5 text-xs transition-colors {globalVariant === n ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
                onclick={() => setGlobalVariant(n)}
              >
                {locale.t('artist_gallery.variant_n', { n })}
              </button>
            {/each}
          </div>
        {/if}

        <button
          type="button"
          class="rounded-lg border px-2 py-1 text-xs transition-colors {store.includeNoPreview ? 'border-indigo-500 bg-indigo-950/50 text-indigo-300' : 'border-neutral-800 bg-neutral-900/50 text-neutral-400 hover:text-neutral-200'}"
          onclick={() => setIncludeNoPreview(!store.includeNoPreview)}
          title={locale.t('artist_gallery.include_no_preview_title')}
        >
          {store.includeNoPreview ? '◉' : '○'} {locale.t('artist_gallery.include_no_preview_btn')}
        </button>

        <button
          type="button"
          class="rounded-lg border px-2 py-1 text-xs transition-colors {showOnlyFavourites ? 'border-red-500 bg-red-950/50 text-red-300' : 'border-neutral-800 bg-neutral-900/50 text-neutral-400 hover:text-neutral-200'}"
          onclick={() => setShowOnlyFavourites(!showOnlyFavourites)}
          title={locale.t('artist_gallery.favourites_title')}
        >
          {showOnlyFavourites ? '♥' : '♡'} {locale.t('artist_gallery.favourites_btn')}{artistFavourites.count > 0 ? ` (${artistFavourites.count})` : ''}
        </button>

        <button
          type="button"
          class="rounded-lg border border-neutral-800 bg-neutral-900/50 px-2 py-1 text-xs text-neutral-400 transition-colors hover:text-neutral-200"
          onclick={() => showFavouritesManager = true}
          title={locale.t('artist_gallery.manage_title')}
        >
          {locale.t('artist_gallery.manage_btn')}
        </button>
      </div>

      {#if showOnlyFavourites}
        {@const counts = artistFavourites.countsByCategory}
        <div class="mt-2 flex flex-wrap items-center gap-1 rounded-lg border border-neutral-800 bg-neutral-900/50 p-1">
          <span class="px-1.5 text-xs text-neutral-500">{locale.t('artist_gallery.category_label')}</span>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {favouriteCategoryFilter === 'all' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setFavouriteCategoryFilter('all')}
          >
            {locale.t('artist_gallery.category_all', { count: artistFavourites.count })}
          </button>
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs transition-colors {favouriteCategoryFilter === '__uncat' ? 'bg-indigo-600 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
            onclick={() => setFavouriteCategoryFilter('__uncat')}
          >
            {locale.t('artist_gallery.category_uncat', { count: counts[''] ?? 0 })}
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
          <button
            type="button"
            class="rounded px-2 py-0.5 text-xs text-indigo-300 transition-colors hover:bg-neutral-800 hover:text-indigo-200"
            onclick={() => showFavouritesManager = true}
            title={locale.t('artist_gallery.manage_title')}
          >
            {locale.t('artist_gallery.cat_new')}
          </button>
        </div>
        <p class="mt-1 px-1.5 text-[11px] text-neutral-500">{locale.t('artist_gallery.category_hint')}</p>
      {/if}
    {/if}
  </header>

  <div class="flex-1 overflow-y-auto" bind:this={scrollContainer}>
    {#if allError}
      <div class="p-8 text-center text-sm text-red-400">
        {locale.t('artist_gallery.load_error', { error: allError ?? '' })}
      </div>
    {:else if allLoading}
      <div class="p-8 text-center text-sm text-neutral-500">{locale.t('artist_gallery.loading_artists')}</div>
    {:else}
      <p class="px-4 pt-2 text-xs text-neutral-500">
        {#if isSearching}
          {store.searchLoading && !showOnlyFavourites ? locale.t('artist_gallery.searching') : (searchEntries.length === 1 ? locale.t('artist_gallery.search_result_one', { query: queryInput }) : locale.t('artist_gallery.search_results', { count: locale.formatInteger(searchEntries.length), query: queryInput }))}
        {:else}
          {locale.t('artist_gallery.hint_copy')}
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
            {locale.t('artist_gallery.prev')}
          </button>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-300 transition-colors hover:border-amber-500"
            onclick={goToRandomPage}
            title={locale.t('artist_gallery.random_title')}
          >
            {locale.t('artist_gallery.random')}
          </button>
          <span class="text-sm text-neutral-400">
            {locale.t('artist_gallery.page_of', { page: safePage, total: totalPages })}
            <span class="text-neutral-600">·</span>
            {locale.t('artist_gallery.artist_count', { count: locale.formatInteger(filteredEntries.length) })}
          </span>
          <div class="flex items-center gap-1">
            <input
              type="number"
              min="1"
              max={totalPages}
              placeholder={locale.t('artist_gallery.page_placeholder')}
              value={pageInputValue}
              oninput={(e) => { pageInputValue = e.currentTarget.value; }}
              onkeydown={(e) => { if (e.key === 'Enter') commitPageInput(); }}
              class="w-14 rounded border border-neutral-700 bg-neutral-800 px-1.5 py-0.5 text-xs text-neutral-200 focus:border-indigo-500 focus:outline-none"
            />
            <button
              type="button"
              class="rounded border border-neutral-700 bg-neutral-800 px-1.5 py-0.5 text-xs text-neutral-300 transition-colors hover:border-indigo-500"
              onclick={commitPageInput}
              title={locale.t('artist_gallery.go_to_page')}
            >↵</button>
          </div>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1 text-sm text-neutral-300 transition-colors hover:border-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
            disabled={safePage >= totalPages}
            onclick={() => goToPage(safePage + 1)}
          >
            {locale.t('artist_gallery.next')}
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
            class="card-slide-in group relative flex flex-col items-stretch rounded-lg border bg-neutral-900 cursor-pointer transition-colors focus:outline-none focus:ring-2 focus:ring-indigo-500 {copiedSlug === hit.slug ? 'border-emerald-500' : (isArtistInPrompt(hit.tag) ? 'border-amber-500/60 ring-1 ring-amber-500/20' : 'border-neutral-800 hover:border-indigo-500')} {catPickerSlug === hit.slug ? 'z-50' : ''}"
            style="--card-delay: {Math.min(i * 30, 450)}ms"
            onclick={() => { void openHit(hit, i); }}
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); void openHit(hit, i); } }}
            oncontextmenu={(e) => { e.preventDefault(); void copyTag(hit.tag, hit.slug); }}
            title={locale.t('artist_gallery.card_title', { tag: hit.tag })}
          >
            <div class="relative aspect-3/4 w-full overflow-hidden rounded-t-lg bg-neutral-800">
              {#if url}
                <img
                  use:cachedSrc={url}
                  alt={hit.tag}
                  loading="eager"
                  decoding="auto"
                  class="h-full w-full object-cover"
                />
              {:else if !hit.hasImage && previewStatus}
                {@const pv = previewVariantOf(hit)}
                {@const st = previewStatus(hit.slug, pv)}
                <div class="flex h-full w-full flex-col items-center justify-center gap-2 p-2 text-center">
                  <span class="text-xs text-neutral-500">{locale.t('artist_gallery.no_preview')}</span>
                  <button
                    type="button"
                    class="rounded border border-neutral-700 bg-neutral-900/90 px-2 py-1 text-[10px] text-neutral-200 transition-colors hover:border-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
                    disabled={st.state !== 'idle'}
                    onclick={(e) => { e.stopPropagation(); ongeneratePreview?.(hit.slug, hit.tag, pv); }}
                    title={st.state === 'unavailable'
                      ? locale.t('artist_gallery.generate_preview_missing', { models: (st.missing ?? []).join(', ') })
                      : locale.t('artist_gallery.generate_preview_title')}
                  >
                    {st.state === 'running'
                      ? locale.t('artist_gallery.generate_preview_running')
                      : locale.t('artist_gallery.generate_preview_btn')}
                  </button>
                </div>
              {:else}
                <div class="flex h-full w-full items-center justify-center text-xs text-neutral-500">
                  {locale.t('artist_gallery.no_preview')}
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
                aria-label={locale.t('artist_gallery.copy_tag_aria')}
              >
                {locale.t('artist_gallery.copy_btn')}
              </button>
              {#if variantCountOf(hit) >= 2}
                <button
                  type="button"
                  class="absolute bottom-1 left-1 rounded border border-neutral-700 bg-neutral-900/90 px-1.5 py-0.5 text-[10px] text-neutral-200 opacity-0 transition-opacity group-hover:opacity-100 hover:border-indigo-500"
                  onclick={(e) => flipCard(hit, e)}
                  aria-label={locale.t('artist_gallery.flip_variant_aria')}
                  title={locale.t('artist_gallery.flip_variant_aria')}
                >
                  ⇄ {Math.min(store.resolveVariant(hit.slug), variantCountOf(hit))}
                </button>
              {/if}
              {#if copiedSlug === hit.slug}
                <div class="absolute inset-0 flex items-center justify-center bg-neutral-900/80">
                  <span class="rounded bg-emerald-600 px-2 py-1 text-xs font-semibold text-white">{locale.t('artist_gallery.copied')}</span>
                </div>
              {/if}
            </div>
            <div class="flex items-center justify-between gap-1 px-2 py-1.5">
              <span class="min-w-0 truncate text-sm text-red-400">{displayTag(hit.tag)}</span>
              <div class="relative flex shrink-0 items-center gap-1">
                <span class="text-xs text-neutral-500">{formatPostCount(hit)}</span>
                {#if isFav}
                  <!-- Uncategorised reads as a dashed "+" chip so the picker is
                       findable; assigned shows the category colour. -->
                  <button
                    type="button"
                    class="flex h-4 w-4 items-center justify-center rounded-full border text-[10px] leading-none transition-transform hover:scale-110 {favCat ? 'border-neutral-700' : 'border-dashed border-neutral-500 text-neutral-400 hover:border-indigo-400 hover:text-indigo-300'}"
                    style={favCat ? `background-color: ${favCat.color}` : "background-color: transparent"}
                    onclick={(e) => openCategoryPicker(hit.slug, e)}
                    aria-label={favCat ? locale.t('artist_gallery.change_category_aria', { name: favCat.name }) : locale.t('artist_gallery.assign_category_aria')}
                    title={favCat ? favCat.name : locale.t('artist_gallery.assign_category_aria')}
                  >{favCat ? '' : '+'}</button>
                {/if}
                <button
                  type="button"
                  class="text-sm leading-none transition-colors {isFav ? 'text-red-400' : 'text-neutral-600 hover:text-red-400'}"
                  onclick={(e) => toggleFavourite(hit.slug, e)}
                  oncontextmenu={(e) => openCategoryPicker(hit.slug, e)}
                  aria-label={isFav ? locale.t('artist_gallery.remove_fav_aria') : locale.t('artist_gallery.add_fav_aria')}
                  title={isFav ? locale.t('artist_gallery.remove_fav_title') : locale.t('artist_gallery.add_fav_title')}
                >
                  {isFav ? '♥' : '♡'}
                </button>
                {#if catPickerSlug === hit.slug}
                  <!-- Backdrop to dismiss -->
                  <button
                    type="button"
                    class="fixed inset-0 z-40 cursor-default"
                    aria-label={locale.t('artist_gallery.close_cat_picker')}
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
                      {locale.t('artist_gallery.cat_uncategorised')}
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
                      {locale.t('artist_gallery.cat_new')}
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
            {locale.t('artist_gallery.prev')}
          </button>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-sm text-neutral-300 transition-colors hover:border-amber-500"
            onclick={goToRandomPage}
            title={locale.t('artist_gallery.random_title')}
          >
            {locale.t('artist_gallery.random')}
          </button>
          <span class="text-sm text-neutral-400">
            {locale.t('artist_gallery.page_of', { page: safePage, total: totalPages })}
            <span class="text-neutral-600">·</span>
            {locale.t('artist_gallery.artist_count', { count: locale.formatInteger(filteredEntries.length) })}
          </span>
          <div class="flex items-center gap-1">
            <input
              type="number"
              min="1"
              max={totalPages}
              placeholder={locale.t('artist_gallery.page_placeholder')}
              value={pageInputValue}
              oninput={(e) => { pageInputValue = e.currentTarget.value; }}
              onkeydown={(e) => { if (e.key === 'Enter') commitPageInput(); }}
              class="w-14 rounded border border-neutral-700 bg-neutral-800 px-1.5 py-1 text-xs text-neutral-200 focus:border-indigo-500 focus:outline-none"
            />
            <button
              type="button"
              class="rounded border border-neutral-700 bg-neutral-800 px-1.5 py-1 text-xs text-neutral-300 transition-colors hover:border-indigo-500"
              onclick={commitPageInput}
              title={locale.t('artist_gallery.go_to_page')}
            >↵</button>
          </div>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-sm text-neutral-300 transition-colors hover:border-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
            disabled={safePage >= totalPages}
            onclick={() => goToPage(safePage + 1)}
          >
            {locale.t('artist_gallery.next')}
          </button>
        </div>
      {/if}
    {/if}
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
    variant={activeVariant}
    canFlip={activeCanFlip}
    onflip={flipActiveVariant}
  />
{/if}

{#if showFavouritesManager}
  <FavouritesManager onclose={() => showFavouritesManager = false} client={store.client} />
{/if}

{#if showGenParams}
  <div
    class="fixed inset-0 z-200 flex items-center justify-center bg-black/80 backdrop-blur-sm"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t('artist_gallery.gen_params.title')}
  >
    <button type="button" class="absolute inset-0 h-full w-full cursor-default" aria-label={locale.t('artist_gallery.lightbox.close_aria')} onclick={() => showGenParams = false}></button>
    <div class="relative z-10 max-h-[85vh] w-full max-w-lg overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
      <div class="mb-4 flex items-center justify-between">
        <h2 class="text-sm font-semibold text-neutral-100">{locale.t('artist_gallery.gen_params.title')}</h2>
        <button type="button" class="text-neutral-500 hover:text-neutral-200 text-lg leading-none" onclick={() => showGenParams = false} aria-label={locale.t('artist_gallery.lightbox.close_aria')}>✕</button>
      </div>
      <div class="space-y-3 text-xs text-neutral-400">
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">{locale.t('artist_gallery.gen_params.model_stack')}</h3>
          <table class="w-full">
            <tbody>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.unet')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.unet}</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.text_encoder')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.textEncoder} ({ARTIST_PREVIEW_RECIPE.clipType})</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.vae')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.vae}</td></tr>
            </tbody>
          </table>
        </section>
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">{locale.t('artist_gallery.gen_params.sampler_section')}</h3>
          <table class="w-full">
            <tbody>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.sampler')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.sampler}</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.scheduler')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.scheduler}</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.steps')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.steps}</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.cfg_scale')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.cfg.toFixed(1)}</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.denoise')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.denoise.toFixed(1)}</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.seed')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.seed}</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.resolution')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.width} × {ARTIST_PREVIEW_RECIPE.height}</td></tr>
            </tbody>
          </table>
        </section>
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">{locale.t('artist_gallery.gen_params.output')}</h3>
          <table class="w-full">
            <tbody>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.output')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.delivery.format}</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.delivered_size')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.delivery.width} × {ARTIST_PREVIEW_RECIPE.delivery.height}</td></tr>
              <tr><td class="py-0.5 pr-4 text-neutral-500">{locale.t('artist_gallery.gen_params.quality')}</td><td class="text-neutral-200">{ARTIST_PREVIEW_RECIPE.delivery.quality}</td></tr>
            </tbody>
          </table>
        </section>
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">{locale.t('artist_gallery.gen_params.positive_1')}</h3>
          {#each [ARTIST_PREVIEW_RECIPE.positivePrompts[0].split('{artist_tag}')] as p1}
            <p class="rounded bg-neutral-800 px-2 py-1.5 font-mono leading-relaxed text-neutral-200">{p1[0]}<span class="text-red-400">&#123;artist_tag&#125;</span>{p1[1]}</p>
          {/each}
        </section>
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">{locale.t('artist_gallery.gen_params.positive_2')}</h3>
          {#each [ARTIST_PREVIEW_RECIPE.positivePrompts[1].split('{artist_tag}')] as p2}
            <p class="rounded bg-neutral-800 px-2 py-1.5 font-mono leading-relaxed text-neutral-200">{p2[0]}<span class="text-red-400">&#123;artist_tag&#125;</span>{p2[1]}</p>
          {/each}
        </section>
        <section>
          <h3 class="mb-1 font-medium text-neutral-300">{locale.t('artist_gallery.gen_params.negative')}</h3>
          <p class="rounded bg-neutral-800 px-2 py-1.5 font-mono leading-relaxed text-neutral-200">{ARTIST_PREVIEW_RECIPE.negativePrompt}</p>
        </section>
      </div>
    </div>
  </div>
{/if}
  {/if}
</div>
