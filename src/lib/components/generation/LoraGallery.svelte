<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { getLoraCivitaiInfo, type LoraCivitaiInfo } from "../../utils/api.js";

  const CACHE_KEY = "mooshieui.lora.civitai.cache.v1";
  const CACHE_TTL = 24 * 60 * 60 * 1000; // 24 hours

  interface CacheEntry {
    data: LoraCivitaiInfo;
    fetchedAt: number;
  }

  // Load cache from localStorage on init
  function loadCache(): Record<string, CacheEntry> {
    if (typeof window === "undefined") return {};
    try {
      const raw = localStorage.getItem(CACHE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as Record<string, CacheEntry>;
        const now = Date.now();
        const result: Record<string, CacheEntry> = {};
        for (const [key, entry] of Object.entries(parsed)) {
          if (now - entry.fetchedAt < CACHE_TTL) {
            result[key] = entry;
          }
        }
        return result;
      }
    } catch {}
    return {};
  }

  let cache = $state<Record<string, CacheEntry>>(loadCache());
  let loading = $state<Record<string, boolean>>({});
  let errors = $state<Record<string, string>>({});
  let selectedLora = $state<string | null>(null);
  let imageIndex = $state<Record<string, number>>({});
  let searchQuery = $state("");

  function saveCache() {
    try {
      localStorage.setItem(CACHE_KEY, JSON.stringify(cache));
    } catch {}
  }

  // All available LoRAs, filtered by search
  const filteredLoras = $derived(() => {
    const q = searchQuery.toLowerCase().trim();
    let list = models.loras;
    if (q) {
      list = list.filter((name) => {
        const display = displayName(name).toLowerCase();
        const filename = name.toLowerCase();
        return display.includes(q) || filename.includes(q);
      });
    }
    // Sort: enabled first, then alphabetical
    const enabledSet = new Set(
      generation.loras.filter((l) => l.enabled && l.name).map((l) => l.name)
    );
    return [...list].sort((a, b) => {
      const aEnabled = enabledSet.has(a);
      const bEnabled = enabledSet.has(b);
      if (aEnabled !== bEnabled) return aEnabled ? -1 : 1;
      return displayName(a).localeCompare(displayName(b));
    });
  });

  function isLoraEnabled(filename: string): boolean {
    return generation.loras.some((l) => l.name === filename && l.enabled);
  }

  function toggleLoraByName(filename: string) {
    const idx = generation.loras.findIndex((l) => l.name === filename);
    if (idx >= 0) {
      generation.toggleLora(idx);
    } else {
      // Add as new enabled LoRA
      generation.loras = [
        ...generation.loras,
        { name: filename, strength_model: 1.0, strength_clip: 1.0, enabled: true },
      ];
    }
  }

  // Lazy fetch: only fetch info for visible LoRAs
  let fetchedSet = new Set<string>();
  let fetchQueue: string[] = [];
  let fetching = false;

  function enqueueFetch(filename: string) {
    if (cache[filename] || loading[filename] || fetchedSet.has(filename)) return;
    fetchedSet.add(filename);
    fetchQueue.push(filename);
    processFetchQueue();
  }

  async function processFetchQueue() {
    if (fetching) return;
    fetching = true;
    while (fetchQueue.length > 0) {
      const filename = fetchQueue.shift()!;
      await fetchLoraInfo(filename);
    }
    fetching = false;
  }

  async function fetchLoraInfo(filename: string) {
    loading = { ...loading, [filename]: true };
    errors = { ...errors, [filename]: "" };
    try {
      const info = await getLoraCivitaiInfo(filename);
      cache = { ...cache, [filename]: { data: info, fetchedAt: Date.now() } };
      saveCache();
    } catch (e) {
      errors = { ...errors, [filename]: String(e) };
    } finally {
      loading = { ...loading, [filename]: false };
    }
  }

  function refetchLora(filename: string) {
    fetchedSet.delete(filename);
    delete cache[filename];
    cache = { ...cache };
    saveCache();
    enqueueFetch(filename);
  }

  // IntersectionObserver action to trigger lazy loading
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
    return {
      destroy() { observer.disconnect(); }
    };
  }

  function getInfo(filename: string): LoraCivitaiInfo | null {
    return cache[filename]?.data ?? null;
  }

  function displayName(filename: string): string {
    const info = getInfo(filename);
    if (info?.civitai_name) return info.civitai_name;
    if (info?.modelspec_title) return info.modelspec_title;
    const base = filename.split(/[\\/]/).pop() ?? filename;
    return base.replace(/\.(safetensors|ckpt|pt|bin)$/i, "");
  }

  function currentImageUrl(filename: string): string | null {
    const info = getInfo(filename);
    if (!info?.civitai_images.length) return null;
    const idx = imageIndex[filename] ?? 0;
    return info.civitai_images[idx]?.url ?? null;
  }

  function nextImage(filename: string) {
    const info = getInfo(filename);
    if (!info?.civitai_images.length) return;
    const current = imageIndex[filename] ?? 0;
    imageIndex = { ...imageIndex, [filename]: (current + 1) % info.civitai_images.length };
  }

  function prevImage(filename: string) {
    const info = getInfo(filename);
    if (!info?.civitai_images.length) return;
    const current = imageIndex[filename] ?? 0;
    imageIndex = { ...imageIndex, [filename]: current === 0 ? info.civitai_images.length - 1 : current - 1 };
  }

  function addTriggerWord(word: string) {
    const current = generation.positivePrompt.trim();
    if (current.includes(word)) return;
    generation.positivePrompt = current ? `${current}, ${word}` : word;
  }

  function formatCount(n: number | undefined): string {
    if (n == null) return "";
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return String(n);
  }
</script>

<!-- Search + LoRA grid -->
<div class="flex flex-col h-full">
  <!-- Search bar -->
  <div class="px-2 pt-1.5 pb-1 shrink-0">
    <input
      type="text"
      bind:value={searchQuery}
      placeholder="Search LoRAs..."
      class="w-full bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
    />
  </div>

  {#if models.loras.length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
      <p>No LoRAs available — check your ComfyUI models folder</p>
    </div>
  {:else if filteredLoras().length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
      <p>No LoRAs matching "{searchQuery}"</p>
    </div>
  {:else}
    <div class="flex gap-2.5 flex-1 min-h-0 overflow-x-auto px-2 py-1.5">
      {#each filteredLoras() as loraName (loraName)}
        {@const info = getInfo(loraName)}
        {@const isLoading = loading[loraName]}
        {@const error = errors[loraName]}
        {@const imgUrl = currentImageUrl(loraName)}
        {@const imgCount = info?.civitai_images.length ?? 0}
        {@const imgIdx = imageIndex[loraName] ?? 0}
        {@const isSelected = selectedLora === loraName}
        {@const enabled = isLoraEnabled(loraName)}

        <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
        <div
          use:lazyFetch={loraName}
          class="shrink-0 w-48 flex flex-col rounded-lg border bg-neutral-900/60 overflow-hidden transition-colors {enabled
            ? 'border-indigo-500/60 ring-1 ring-indigo-500/20'
            : isSelected ? 'border-neutral-600' : 'border-neutral-800 hover:border-neutral-700'}"
        >
          <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
          <!-- Image area -->
          <div
            class="relative w-full aspect-[3/4] bg-neutral-950 overflow-hidden cursor-pointer"
            onclick={() => toggleLoraByName(loraName)}
          >
            {#if enabled}
              <div class="absolute top-1.5 left-1.5 z-10 px-1.5 py-0.5 rounded text-[9px] font-medium bg-indigo-600 text-white">
                ON
              </div>
            {/if}
            {#if isLoading}
              <div class="absolute inset-0 flex items-center justify-center">
                <div class="w-5 h-5 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
              </div>
            {:else if imgUrl}
              <img
                src={imgUrl}
                alt={displayName(loraName)}
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
                    onclick={() => prevImage(loraName)}
                    title="Previous image"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
                  </button>
                  <button
                    class="w-6 h-6 flex items-center justify-center rounded-full bg-black/60 text-neutral-300 hover:bg-black/80 text-xs"
                    onclick={() => nextImage(loraName)}
                    title="Next image"
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
                  </button>
                </div>
              {/if}
            {:else if error}
              <div class="absolute inset-0 flex flex-col items-center justify-center gap-2 p-2">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5 text-neutral-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
                <span class="text-[10px] text-neutral-600 text-center">Not on CivitAI</span>
                <button
                  class="text-[10px] text-indigo-400 hover:text-indigo-300"
                  onclick={(e) => { e.stopPropagation(); refetchLora(loraName); }}
                >
                  Retry
                </button>
              </div>
            {:else}
              <div class="absolute inset-0 flex items-center justify-center">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-6 h-6 text-neutral-700" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
              </div>
            {/if}
          </div>

          <!-- Info area -->
          <div class="flex-1 min-h-0 flex flex-col p-2 gap-1 overflow-y-auto">
            <div class="flex items-start justify-between gap-1">
              <button
                class="text-left"
                onclick={() => { selectedLora = isSelected ? null : loraName; }}
              >
                <h4 class="text-[11px] font-medium text-neutral-200 leading-tight line-clamp-2" title={loraName}>
                  {displayName(loraName)}
                </h4>
              </button>
              {#if info?.civitai_model_id}
                <a
                  href="https://civitai.com/models/{info.civitai_model_id}"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="shrink-0 text-neutral-500 hover:text-indigo-400 transition-colors"
                  title="View on CivitAI"
                  onclick={(e) => e.stopPropagation()}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                </a>
              {/if}
            </div>

            <!-- Stats row -->
            {#if info?.civitai_download_count || info?.civitai_thumbs_up_count || info?.civitai_base_model}
              <div class="flex items-center gap-2 text-[10px] text-neutral-500 flex-wrap">
                {#if info.civitai_base_model}
                  <span class="px-1.5 py-0.5 rounded bg-neutral-800 text-neutral-400">{info.civitai_base_model}</span>
                {/if}
                {#if info.civitai_download_count}
                  <span class="flex items-center gap-0.5" title="Downloads">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                    {formatCount(info.civitai_download_count)}
                  </span>
                {/if}
                {#if info.civitai_thumbs_up_count}
                  <span class="flex items-center gap-0.5" title="Likes">
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-2.5 h-2.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 9V5a3 3 0 0 0-3-3l-4 9v11h11.28a2 2 0 0 0 2-1.7l1.38-9a2 2 0 0 0-2-2.3zM7 22H4a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2h3"/></svg>
                    {formatCount(info.civitai_thumbs_up_count)}
                  </span>
                {/if}
              </div>
            {/if}

            <!-- Trigger words -->
            {#if info?.civitai_trigger_words.length || info?.modelspec_trigger_phrase}
              <div class="flex flex-wrap gap-1">
                {#if info.civitai_trigger_words.length}
                  {#each info.civitai_trigger_words as word}
                    <button
                      class="px-1.5 py-0.5 text-[10px] rounded bg-indigo-500/10 border border-indigo-500/30 text-indigo-300 hover:bg-indigo-500/20 hover:border-indigo-400/50 transition-colors"
                      onclick={(e) => { e.stopPropagation(); addTriggerWord(word); }}
                      title="Add to prompt: {word}"
                    >
                      {word}
                    </button>
                  {/each}
                {:else if info?.modelspec_trigger_phrase}
                  {#each info.modelspec_trigger_phrase.split(",").map((s) => s.trim()).filter(Boolean) as word}
                    <button
                      class="px-1.5 py-0.5 text-[10px] rounded bg-indigo-500/10 border border-indigo-500/30 text-indigo-300 hover:bg-indigo-500/20 hover:border-indigo-400/50 transition-colors"
                      onclick={(e) => { e.stopPropagation(); addTriggerWord(word); }}
                      title="Add to prompt: {word}"
                    >
                      {word}
                    </button>
                  {/each}
                {/if}
              </div>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
