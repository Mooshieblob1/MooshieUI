<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { getCheckpointCivitaiInfo, type CheckpointCivitaiInfo } from "../../utils/api.js";

  const CACHE_KEY = "mooshieui.checkpoint.civitai.cache.v2";
  const CACHE_TTL = 24 * 60 * 60 * 1000; // 24 hours

  interface CacheEntry {
    data: CheckpointCivitaiInfo;
    fetchedAt: number;
  }

  /** Returns true when the entry actually contains CivitAI-sourced data. */
  function hasCivitaiData(entry: CacheEntry): boolean {
    const d = entry.data;
    return !!(d.civitai_model_id || d.civitai_version_id);
  }

  function loadCache(): Record<string, CacheEntry> {
    if (typeof window === "undefined") return {};
    try {
      const raw = localStorage.getItem(CACHE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as Record<string, CacheEntry>;
        const now = Date.now();
        const result: Record<string, CacheEntry> = {};
        for (const [key, entry] of Object.entries(parsed)) {
          // Drop expired entries and entries with no CivitAI data (stale auth-fail results)
          if (now - entry.fetchedAt < CACHE_TTL && hasCivitaiData(entry)) result[key] = entry;
        }
        return result;
      }
    } catch {}
    return {};
  }

  let civitaiCache = $state<Record<string, CacheEntry>>(loadCache());
  let loading = $state<Record<string, boolean>>({});
  let fetchedSet = new Set<string>();
  let fetchQueue: string[] = [];
  let fetching = false;

  let searchQuery = $state("");

  function saveCache() {
    try {
      localStorage.setItem(CACHE_KEY, JSON.stringify(civitaiCache));
    } catch {}
  }

  const filteredCheckpoints = $derived(() => {
    const q = searchQuery.toLowerCase().trim();
    let list = models.checkpoints;
    if (q) {
      list = list.filter((name) => {
        const display = displayName(name).toLowerCase();
        return display.includes(q) || name.toLowerCase().includes(q);
      });
    }
    return [...list].sort((a, b) => {
      const aActive = a === generation.checkpoint;
      const bActive = b === generation.checkpoint;
      if (aActive !== bActive) return aActive ? -1 : 1;
      return displayName(a).localeCompare(displayName(b));
    });
  });

  function displayName(filename: string): string {
    const info = civitaiCache[filename]?.data;
    if (info?.display_name) return info.display_name;
    const base = filename.split(/[\\/]/).pop() ?? filename;
    return base.replace(/\.(safetensors|ckpt|pt|bin)$/i, "");
  }

  function selectCheckpoint(filename: string) {
    // Clear any previously-selected split-model state so the displayed
    // checkpoint is actually what gets loaded. Without this, switching from
    // a split model (e.g. Anima Preview 3) to a regular checkpoint would
    // leave `useSplitModel = true` and the workflow would still load the
    // old diffusion_model / clip_model / vae instead of `filename`.
    generation.useSplitModel = false;
    generation.diffusionModel = null;
    generation.clipModel = null;
    generation.clipType = null;
    generation.checkpoint = filename;
    generation.applyModelSpecificPreset(filename);
    generation.saveSettings();
  }

  function enqueueFetch(filename: string) {
    if (civitaiCache[filename] || loading[filename] || fetchedSet.has(filename)) return;
    fetchedSet.add(filename);
    fetchQueue.push(filename);
    processFetchQueue();
  }

  async function processFetchQueue() {
    if (fetching) return;
    fetching = true;
    while (fetchQueue.length > 0) {
      const filename = fetchQueue.shift()!;
      await fetchInfo(filename);
    }
    fetching = false;
  }

  async function fetchInfo(filename: string) {
    loading = { ...loading, [filename]: true };
    try {
      const info = await getCheckpointCivitaiInfo(filename);
      civitaiCache = { ...civitaiCache, [filename]: { data: info, fetchedAt: Date.now() } };
      // Only persist to localStorage when CivitAI data was retrieved.
      // Empty results (failed auth, pre-fix) stay in-memory only so they
      // re-fetch fresh next session once an API key is configured.
      if (info.civitai_model_id || info.civitai_version_id) {
        saveCache();
      }
    } catch {
      // Silent fail — card falls back to placeholder SVG
    } finally {
      loading = { ...loading, [filename]: false };
    }
  }

  function lazyFetch(node: HTMLElement, filename: string) {
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          enqueueFetch(filename);
          observer.disconnect();
        }
      },
      { threshold: 0.1 }
    );
    observer.observe(node);
    return { destroy() { observer.disconnect(); } };
  }

  let imageIndex = $state<Record<string, number>>({});

  /**
   * Build the full ordered display image list for a checkpoint:
   * sidecar (data://) first if present, then all CivitAI images
   * (deduped so the first CivitAI image isn't repeated when it equals thumbnail_url).
   */
  function getDisplayImages(filename: string): string[] {
    const info = civitaiCache[filename]?.data;
    if (!info) return [];
    const urls: string[] = [];
    if (info.thumbnail_url) urls.push(info.thumbnail_url);
    if (info.civitai_images) {
      for (const img of info.civitai_images) {
        if (img.url !== info.thumbnail_url) urls.push(img.url);
      }
    }
    return urls;
  }

  function currentImageUrl(filename: string): string | null {
    const images = getDisplayImages(filename);
    if (!images.length) return null;
    return images[imageIndex[filename] ?? 0] ?? null;
  }

  function nextImage(filename: string) {
    const images = getDisplayImages(filename);
    if (images.length <= 1) return;
    const current = imageIndex[filename] ?? 0;
    imageIndex = { ...imageIndex, [filename]: (current + 1) % images.length };
  }

  function prevImage(filename: string) {
    const images = getDisplayImages(filename);
    if (images.length <= 1) return;
    const current = imageIndex[filename] ?? 0;
    imageIndex = { ...imageIndex, [filename]: current === 0 ? images.length - 1 : current - 1 };
  }

  function formatCount(n: number | undefined): string {
    if (n == null) return "";
    if (n >= 1_000_000) return `${locale.formatDecimalTrimmed(n / 1_000_000, 1)}M`;
    if (n >= 1_000) return `${locale.formatDecimalTrimmed(n / 1_000, 1)}K`;
    return locale.formatInteger(n);
  }
</script>

<div class="flex flex-col h-full">
  <!-- Search bar -->
  <div class="px-2 pt-1.5 pb-1 shrink-0">
    <input
      type="text"
      bind:value={searchQuery}
      placeholder={locale.t('checkpoint.search_placeholder')}
      class="w-full bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
    />
  </div>

  {#if filteredCheckpoints().length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
      <p>{locale.t('checkpoint.no_results')}</p>
    </div>
  {:else}
    <div class="flex-1 min-h-0 overflow-y-auto px-2 py-1.5">
      <div class="grid gap-2.5" style="grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));">
      {#each filteredCheckpoints() as name (name)}
        {@const isActive = name === generation.checkpoint}
        {@const info = civitaiCache[name]?.data}
        {@const isLoading = loading[name]}
        {@const imgUrl = currentImageUrl(name)}
        {@const imgCount = getDisplayImages(name).length}
        {@const imgIdx = imageIndex[name] ?? 0}
        <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
        <div
          use:lazyFetch={name}
          class="aspect-[3/4] flex flex-col rounded-lg border bg-neutral-900/60 overflow-hidden transition-all text-left cursor-pointer {isActive
            ? 'border-indigo-500/60 ring-1 ring-indigo-500/20'
            : 'border-neutral-800 hover:border-neutral-600'}"
          title={name}
          onclick={() => selectCheckpoint(name)}
        >
          <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
          <!-- Preview image area -->
          <div
            class="relative flex-1 min-h-0 bg-neutral-950 overflow-hidden"
          >
            {#if isActive}
              <div class="absolute top-1.5 left-1.5 z-10 px-1.5 py-0.5 rounded text-[9px] font-medium bg-indigo-600 text-white">
                {locale.t('checkpoint.active')}
              </div>
            {/if}
            {#if imgUrl}
              <img
                src={imgUrl}
                alt={displayName(name)}
                class="w-full h-full object-cover"
                loading="lazy"
              />
              {#if imgCount > 1}
                <div class="absolute bottom-1 right-1 bg-black/70 text-[10px] text-neutral-300 px-1.5 py-0.5 rounded-full tabular-nums">
                  {imgIdx + 1}/{imgCount}
                </div>
                <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
                <div
                  class="absolute inset-0 flex items-center justify-between px-1 opacity-0 hover:opacity-100 transition-opacity"
                  onclick={(e) => e.stopPropagation()}
                >
                  <button
                    class="w-6 h-6 flex items-center justify-center rounded-full bg-black/60 text-neutral-300 hover:bg-black/80 text-xs"
                    onclick={() => prevImage(name)}
                    title={locale.t('lora.prev_image')}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
                  </button>
                  <button
                    class="w-6 h-6 flex items-center justify-center rounded-full bg-black/60 text-neutral-300 hover:bg-black/80 text-xs"
                    onclick={() => nextImage(name)}
                    title={locale.t('lora.next_image')}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
                  </button>
                </div>
              {/if}
            {:else if isLoading}
              <div class="absolute inset-0 flex items-center justify-center">
                <div class="w-4 h-4 border-2 border-neutral-600 border-t-indigo-500 rounded-full animate-spin"></div>
              </div>
            {:else}
              <div class="absolute inset-0 flex items-center justify-center">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  class="w-8 h-8 {isActive ? 'text-indigo-700' : 'text-neutral-700'}"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/>
                  <polyline points="3.27 6.96 12 12.01 20.73 6.96"/>
                  <line x1="12" y1="22.08" x2="12" y2="12"/>
                </svg>
              </div>
            {/if}
          </div>

          <!-- Info area -->
          <div class="shrink-0 flex flex-col p-1.5 gap-0.5 overflow-hidden max-h-[40%]">
            <div class="flex items-start justify-between gap-1">
              <button
                class="text-left flex-1 min-w-0"
              >
                <h4 class="text-[11px] font-medium text-neutral-200 leading-tight line-clamp-2" title={name}>
                  {displayName(name)}
                </h4>
              </button>
              {#if info?.civitai_model_id}
                <a
                  href="https://civitai.com/models/{info.civitai_model_id}"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="shrink-0 text-neutral-500 hover:text-indigo-400 transition-colors"
                  title={locale.t('lora.view_civitai')}
                  onclick={(e) => e.stopPropagation()}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                </a>
              {/if}
            </div>

            {#if info?.base_model || info?.civitai_download_count || info?.civitai_thumbs_up_count}
              <div class="flex items-center gap-1.5 text-[10px] text-neutral-500 flex-wrap">
                {#if info.base_model}
                  <span class="px-1.5 py-0.5 rounded bg-neutral-800 text-neutral-400">{info.base_model}</span>
                {/if}
                {#if info.civitai_download_count}
                  <span class="flex items-center gap-0.5" title={locale.t('modelhub.civitai.stat_downloads')}>
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                    {formatCount(info.civitai_download_count)}
                  </span>
                {/if}
                {#if info.civitai_thumbs_up_count}
                  <span class="flex items-center gap-0.5" title={locale.t('lora.likes')}>
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 9V5a3 3 0 0 0-3-3l-4 9v11h11.28a2 2 0 0 0 2-1.7l1.38-9a2 2 0 0 0-2-2.3zM7 22H4a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2h3"/></svg>
                    {formatCount(info.civitai_thumbs_up_count)}
                  </span>
                {/if}
              </div>
            {/if}
          </div>
        </div>
      {/each}
      </div>
    </div>
  {/if}
</div>
