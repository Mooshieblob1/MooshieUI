<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import {
    loraPresets,
    type LoraPresetApplyMode,
  } from "../../stores/loraPresets.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import {
    civitaiLookupImage,
    fetchCachedImage,
    getLoraCivitaiInfo,
    saveModelSidecarThumbnail,
    type LoraCivitaiInfo,
  } from "../../utils/api.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import {
    buildFilenameTree,
    countTreeFiles,
    inferLoraFamily,
    sortModelFilenames,
    type FilenameTreeNode,
    type ModelGallerySort,
  } from "../../utils/modelGallerySort.js";
  import ModelPreviewActions from "./ModelPreviewActions.svelte";

  interface Props {
    cardSize?: number;
    onCardSizeChange?: (size: number) => void;
  }

  let { cardSize = 120, onCardSizeChange }: Props = $props();

  const CACHE_KEY = "mooshieui.lora.civitai.cache.v3";
  const CACHE_TTL = 24 * 60 * 60 * 1000; // 24 hours
  const SORT_KEY = "mooshieui.lora.gallery.sort.v1";

  interface CacheEntry {
    data: LoraCivitaiInfo;
    fetchedAt: number;
  }

  /** Returns true when the entry actually contains CivitAI-sourced data. */
  function hasCivitaiData(entry: CacheEntry): boolean {
    const d = entry.data;
    return !!(d.civitai_name || d.civitai_model_id);
  }

  function hasDisplayThumbnail(entry: CacheEntry): boolean {
    const d = entry.data;
    return !!(d.thumbnail_url || (d.civitai_images && d.civitai_images.length > 0));
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
          if (now - entry.fetchedAt >= CACHE_TTL) continue;
          if (hasCivitaiData(entry) || hasDisplayThumbnail(entry)) {
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
  let civitaiImageRef = $state("");
  let civitaiImportBusy = $state(false);
  let sortMode = $state<ModelGallerySort>(
    (typeof window !== "undefined" &&
      (localStorage.getItem(SORT_KEY) as ModelGallerySort | null)) ||
      "name",
  );
  let collapsedFolders = $state<Set<string>>(new Set());
  let selectedPresetId = $state<string>("");
  let newPresetName = $state("");
  let applyMode = $state<LoraPresetApplyMode>("replace");
  let presetStatus = $state<string | null>(null);
  let presetError = $state<string | null>(null);
  let loraInfoAccessBlocked = $state<string | null>(null);

  function isAccessDeniedError(message: string): boolean {
    const text = message.toLowerCase();
    return (
      text.includes(" 403") ||
      text.includes("status 403") ||
      text.includes("forbidden") ||
      text.includes("permission") ||
      text.includes("access to the model hub")
    );
  }

  function saveCache() {
    try {
      localStorage.setItem(CACHE_KEY, JSON.stringify(cache));
    } catch {}
  }

  $effect(() => {
    try {
      localStorage.setItem(SORT_KEY, sortMode);
    } catch {}
  });

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
    const enabledSet = new Set(
      generation.loras.filter((l) => l.enabled && l.name).map((l) => l.name),
    );
    const familyOf = (filename: string) =>
      inferLoraFamily(filename, cache[filename]?.data);
    const enabled = list.filter((n) => enabledSet.has(n));
    const disabled = list.filter((n) => !enabledSet.has(n));
    return [
      ...sortModelFilenames(enabled, sortMode, null, displayName, familyOf),
      ...sortModelFilenames(disabled, sortMode, null, displayName, familyOf),
    ];
  });

  // Folder tree for the "tree" sort mode, built from the already-filtered and
  // enabled-first list so grouping and ordering stay consistent with the grid.
  const loraTree = $derived(
    sortMode === "tree" ? buildFilenameTree(filteredLoras()) : null,
  );

  function toggleFolder(path: string) {
    const next = new Set(collapsedFolders);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    collapsedFolders = next;
  }

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
    generation.saveSettings();
  }

  function clearPresetMessages() {
    presetStatus = null;
    presetError = null;
  }

  function createPresetFromCurrent() {
    clearPresetMessages();
    const activeStack = generation.loras.filter((lora) => lora.name.trim().length > 0);
    if (activeStack.length === 0) {
      presetError = locale.t("lora_presets.error.no_loras");
      return;
    }
    const fallbackName = locale.t("lora_presets.default_name", { n: String(loraPresets.presets.length + 1) });
    const created = loraPresets.create(newPresetName.trim() || fallbackName, activeStack);
    newPresetName = "";
    selectedPresetId = created.id;
    presetStatus = locale.t("lora_presets.saved", { name: created.name });
  }

  function updatePresetFromCurrent() {
    clearPresetMessages();
    if (!selectedPresetId) {
      presetError = locale.t("lora_presets.error.select_first");
      return;
    }
    const selected = loraPresets.getById(selectedPresetId);
    if (!selected) {
      presetError = locale.t("lora_presets.error.not_found");
      return;
    }
    const activeStack = generation.loras.filter((lora) => lora.name.trim().length > 0);
    if (activeStack.length === 0) {
      presetError = locale.t("lora_presets.error.no_loras");
      return;
    }
    loraPresets.update(selected.id, { loras: activeStack });
    presetStatus = locale.t("lora_presets.updated", { name: selected.name });
  }

  function applySelectedPreset() {
    clearPresetMessages();
    if (!selectedPresetId) {
      presetError = locale.t("lora_presets.error.select_first");
      return;
    }
    const result = loraPresets.applyToLoras(
      generation.loras,
      selectedPresetId,
      applyMode,
      models.loras,
    );
    if (!result) {
      presetError = locale.t("lora_presets.error.not_found");
      return;
    }
    generation.loras = result.loras;
    generation.saveSettings();
    if (result.missingNames.length > 0) {
      presetError = locale.t("lora_presets.applied_missing", { names: result.missingNames.join(", ") });
    } else {
      presetStatus = locale.t("lora_presets.applied");
    }
  }

  function deleteSelectedPreset() {
    clearPresetMessages();
    if (!selectedPresetId) {
      presetError = locale.t("lora_presets.error.select_first");
      return;
    }
    const selected = loraPresets.getById(selectedPresetId);
    if (!selected) return;
    if (!confirm(locale.t("lora_presets.delete_confirm", { name: selected.name }))) return;
    loraPresets.remove(selected.id);
    selectedPresetId = "";
    presetStatus = locale.t("lora_presets.deleted");
  }

  // Lazy fetch: only fetch info for visible LoRAs
  let fetchQueue: string[] = [];
  let fetching = false;

  function enqueueFetch(filename: string) {
    if (loraInfoAccessBlocked) return;
    if (loading[filename] || fetchQueue.includes(filename)) return;
    const cached = cache[filename];
    if (cached && Date.now() - cached.fetchedAt < CACHE_TTL && hasDisplayThumbnail(cached)) {
      return;
    }
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
      // Only persist to localStorage when CivitAI data was retrieved.
      // Empty results (failed auth, pre-fix) stay in-memory only so they
      // re-fetch fresh next session once an API key is configured.
      if (info.civitai_name || info.civitai_model_id || info.thumbnail_url?.startsWith("data:")) {
        saveCache();
      }
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      errors = { ...errors, [filename]: message };
      if (isAccessDeniedError(message)) {
        loraInfoAccessBlocked = message;
        fetchQueue = [];
      }
    } finally {
      loading = { ...loading, [filename]: false };
    }
  }

  function refetchLora(filename: string) {
    loraInfoAccessBlocked = null;
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

  function getDisplayImages(filename: string): string[] {
    const info = getInfo(filename);
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
    const next = (current + 1) % images.length;
    imageIndex = { ...imageIndex, [filename]: next };
    const nextUrl = images[next];
    if (nextUrl && !nextUrl.startsWith("data:")) resolveImage(nextUrl);
  }

  function prevImage(filename: string) {
    const images = getDisplayImages(filename);
    if (images.length <= 1) return;
    const current = imageIndex[filename] ?? 0;
    const prev = current === 0 ? images.length - 1 : current - 1;
    imageIndex = { ...imageIndex, [filename]: prev };
    const prevUrl = images[prev];
    if (prevUrl && !prevUrl.startsWith("data:")) resolveImage(prevUrl);
  }

  async function invalidateLoraCache(filename: string) {
    delete cache[filename];
    cache = { ...cache };
    await fetchLoraInfo(filename);
  }

  async function setSidecarFromPreview(loraFilename: string) {
    const url = currentImageUrl(loraFilename);
    if (!url) {
      gallery.showToast(locale.t("checkpoint.no_preview_image"), "warning");
      return;
    }
    try {
      if (url.startsWith("data:")) {
        gallery.showToast(locale.t("checkpoint.sidecar_already_local"), "info");
        return;
      }
      await saveModelSidecarThumbnail({
        category: "loras",
        filename: loraFilename,
        imageUrl: url,
      });
      await invalidateLoraCache(loraFilename);
      gallery.showToast(locale.t("checkpoint.sidecar_saved"), "success");
    } catch (e) {
      gallery.showToast(
        locale.t("checkpoint.sidecar_failed", {
          message: e instanceof Error ? e.message : String(e),
        }),
        "error",
      );
    }
  }

  async function setSidecarFromGallery(loraFilename: string) {
    const last = gallery.lastSelectedImage;
    if (!last) {
      gallery.showToast(locale.t("model.thumb_no_gallery_image"), "warning");
      return;
    }
    // Wait for an in-flight persist so freshly generated images work too (#232).
    const gf = await gallery.resolveGalleryFilename(last);
    if (!gf) {
      gallery.showToast(locale.t("gallery.persisted_only_thumb"), "warning");
      return;
    }
    try {
      await saveModelSidecarThumbnail({
        category: "loras",
        filename: loraFilename,
        galleryFilename: gf,
      });
      await invalidateLoraCache(loraFilename);
      gallery.showToast(locale.t("checkpoint.sidecar_saved"), "success");
    } catch (e) {
      gallery.showToast(
        locale.t("checkpoint.sidecar_failed", {
          message: e instanceof Error ? e.message : String(e),
        }),
        "error",
      );
    }
  }

  async function importCivitaiImageMeta() {
    const ref = civitaiImageRef.trim();
    if (!ref) return;
    civitaiImportBusy = true;
    try {
      const data = await civitaiLookupImage(ref);
      const item = data.items?.[0];
      const meta = item?.meta as Record<string, unknown> | undefined;
      const prompt =
        (typeof meta?.prompt === "string" && meta.prompt) ||
        (typeof meta?.positivePrompt === "string" && meta.positivePrompt) ||
        "";
      if (!prompt) {
        gallery.showToast(locale.t("checkpoint.civitai_no_prompt"), "warning");
        return;
      }
      generation.positivePrompt = prompt;
      generation.saveSettings();
      gallery.showToast(locale.t("checkpoint.civitai_prompt_imported"), "success");
    } catch (e) {
      gallery.showToast(
        locale.t("checkpoint.civitai_import_failed", {
          message: e instanceof Error ? e.message : String(e),
        }),
        "error",
      );
    } finally {
      civitaiImportBusy = false;
    }
  }

  function addTriggerWord(loraName: string, word: string) {
    const current = generation.positivePrompt.trim();
    if (current.includes(word)) return;
    generation.positivePrompt = current ? `${current}, ${word}` : word;
    generation.recordInsertedLoraWord(loraName, word);
    generation.saveSettings();
  }

  function formatCount(n: number | undefined): string {
    if (n == null) return "";
    return locale.formatCompactCountUpperK(n);
  }

  // Resolved data-URL cache: civitai url → "data:image/...;base64,..."
  // Populated lazily when an image tile becomes visible.
  let resolvedImages = $state<Record<string, string>>({});
  let resolvingImages = new Set<string>();

  async function resolveImage(url: string): Promise<void> {
    if (resolvedImages[url] || resolvingImages.has(url)) return;
    resolvingImages.add(url);
    try {
      const dataUrl = await fetchCachedImage(url);
      resolvedImages = { ...resolvedImages, [url]: dataUrl };
    } catch (e) {
      // Leave unresolved — the fallback placeholder will show. Log so a broken
      // preview fetch is diagnosable instead of silently blank.
      console.warn(`LoraGallery: failed to load preview ${url}`, e);
    } finally {
      resolvingImages.delete(url);
    }
  }

  // Resolve the currently-visible image for a LoRA whenever it changes.
  $effect(() => {
    for (const loraName of Object.keys(cache)) {
      const url = currentImageUrl(loraName);
      if (url && !resolvedImages[url]) {
        resolveImage(url);
      }
    }
  });
</script>

<!-- Search + LoRA grid -->
<div class="flex flex-col h-full">
  <div class="mx-2 mt-1.5 rounded border border-neutral-800 bg-neutral-900/60 p-2 text-[11px] space-y-2">
    <p class="text-neutral-400">{locale.t("lora_presets.title")}</p>
    <div class="flex items-center gap-2">
      <select
        bind:value={selectedPresetId}
        class="flex-1 bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-[11px] text-neutral-100 focus:outline-none focus:border-indigo-500"
      >
        <option value="">{locale.t("lora_presets.select_placeholder")}</option>
        {#each loraPresets.presets as preset (preset.id)}
          <option value={preset.id}>{preset.name}</option>
        {/each}
      </select>
      <select
        bind:value={applyMode}
        class="bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-[11px] text-neutral-100 focus:outline-none focus:border-indigo-500"
      >
        <option value="replace">{locale.t("lora_presets.mode.replace")}</option>
        <option value="merge">{locale.t("lora_presets.mode.merge")}</option>
      </select>
      <button
        type="button"
        class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-200 hover:border-indigo-500"
        onclick={applySelectedPreset}
      >
        {locale.t("common.apply")}
      </button>
    </div>
    <div class="flex items-center gap-2">
      <input
        type="text"
        bind:value={newPresetName}
        placeholder={locale.t("lora_presets.name_placeholder")}
        class="flex-1 bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-[11px] text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
      />
      <button
        type="button"
        class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-200 hover:border-indigo-500"
        onclick={createPresetFromCurrent}
      >
        {locale.t("lora_presets.save_current")}
      </button>
      <button
        type="button"
        class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-300 hover:border-indigo-500"
        onclick={updatePresetFromCurrent}
      >
        {locale.t("common.update")}
      </button>
      <button
        type="button"
        class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-red-300 hover:border-red-500/50"
        onclick={deleteSelectedPreset}
      >
        {locale.t("common.delete")}
      </button>
    </div>
    {#if presetStatus}
      <p class="text-emerald-400">{presetStatus}</p>
    {/if}
    {#if presetError}
      <p class="text-amber-300">{presetError}</p>
    {/if}
  </div>

  <!-- Search bar + size slider -->
  <div class="px-2 pt-1.5 pb-1 shrink-0 space-y-1">
    <div class="flex gap-1 items-center">
      <select
        bind:value={sortMode}
        class="shrink-0 rounded border border-neutral-700 bg-neutral-800 px-1.5 py-1 text-[10px] text-neutral-200 focus:outline-none focus:border-indigo-500"
        title={locale.t('model_gallery.sort_tip')}
      >
        <option value="name">{locale.t('model_gallery.sort_name')}</option>
        <option value="folder">{locale.t('model_gallery.sort_folder')}</option>
        <option value="tree">{locale.t('model_gallery.sort_tree')}</option>
        <option value="family">{locale.t('model_gallery.sort_family')}</option>
      </select>
      <input
        type="text"
        bind:value={civitaiImageRef}
        placeholder={locale.t('checkpoint.civitai_image_ref_placeholder')}
        class="min-w-0 flex-1 bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-[10px] text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
      />
      <button
        type="button"
        disabled={civitaiImportBusy || !civitaiImageRef.trim()}
        onclick={() => void importCivitaiImageMeta()}
        class="shrink-0 rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[10px] text-neutral-200 hover:border-indigo-500 disabled:opacity-40"
        title={locale.t('checkpoint.civitai_image_ref_tip')}
      >
        {civitaiImportBusy ? "…" : locale.t('checkpoint.civitai_import_prompt')}
      </button>
    </div>
    <div class="flex items-center gap-2">
    <input
      type="text"
      bind:value={searchQuery}
      placeholder={locale.t('lora.search_placeholder')}
      class="flex-1 bg-neutral-800 border border-neutral-700 rounded px-2.5 py-1 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
    />
    <div use:scrollCapture>
      <input
        type="range"
        min="60"
        max="200"
        value={cardSize}
        oninput={(e) => onCardSizeChange?.(Number(e.currentTarget.value))}
        class="w-16 h-4 accent-indigo-500 cursor-pointer"
        title={locale.t('bottom_panel.card_size')}
      />
    </div>
    </div>
  </div>

  {#if loraInfoAccessBlocked}
    <div class="mx-2 mb-1.5 rounded border border-amber-700/50 bg-amber-950/40 px-2 py-1.5 text-[11px] text-amber-200">
      <div class="flex items-center justify-between gap-2">
        <p class="truncate" title={loraInfoAccessBlocked}>{loraInfoAccessBlocked}</p>
        <button
          type="button"
          class="shrink-0 text-amber-100 underline hover:text-white"
          onclick={() => {
            loraInfoAccessBlocked = null;
          }}
        >
          {locale.t("common.retry")}
        </button>
      </div>
    </div>
  {/if}

  {#if models.loras.length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
      <p>{locale.t('lora.no_loras')}</p>
    </div>
  {:else if filteredLoras().length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs">
      <p>{locale.t('lora.no_results', { query: searchQuery })}</p>
    </div>
  {:else}
    {#snippet loraCard(loraName: string)}
        {@const info = getInfo(loraName)}
        {@const isLoading = loading[loraName]}
        {@const error = errors[loraName]}
        {@const accessDenied = !!error && isAccessDeniedError(error)}
        {@const imgUrl = currentImageUrl(loraName)}
        {@const resolvedUrl = imgUrl
          ? (imgUrl.startsWith("data:") ? imgUrl : (resolvedImages[imgUrl] ?? null))
          : null}
        {@const imgCount = getDisplayImages(loraName).length}
        {@const imgIdx = imageIndex[loraName] ?? 0}
        {@const isSelected = selectedLora === loraName}
        {@const enabled = isLoraEnabled(loraName)}

        <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
        <div
          use:lazyFetch={loraName}
          class="aspect-[3/4] flex flex-col rounded-lg border bg-neutral-900/60 overflow-hidden transition-colors cursor-pointer group {enabled
            ? 'border-indigo-500/60 ring-1 ring-indigo-500/20'
            : isSelected ? 'border-neutral-600' : 'border-neutral-800 hover:border-neutral-700'}"
          onclick={() => { selectedLora = loraName; toggleLoraByName(loraName); }}
        >
          <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
          <!-- Image area -->
          <div
            class="relative w-full flex-1 min-h-0 bg-neutral-950 overflow-hidden"
          >
            {#if enabled}
              <div class="absolute top-1.5 left-1.5 z-10 px-1.5 py-0.5 rounded text-[9px] font-medium bg-indigo-600 text-white">
                {locale.t('lora.on')}
              </div>
            {/if}
            {#if isLoading || (imgUrl && !resolvedUrl)}
              <div class="absolute inset-0 flex items-center justify-center">
                <div class="w-5 h-5 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
              </div>
            {:else if resolvedUrl}
              <img
                src={resolvedUrl}
                alt={displayName(loraName)}
                class="w-full h-full object-cover"
              />
              {#if imgUrl}
                <ModelPreviewActions imageUrl={imgUrl} modelLabel={displayName(loraName)} />
              {/if}
            {:else if error}
              <div class="absolute inset-0 flex flex-col items-center justify-center gap-2 p-2">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5 text-neutral-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
                <span class="text-[10px] text-neutral-600 text-center" title={error}>
                  {error.includes('not found')
                    ? error
                    : accessDenied
                      ? error
                      : locale.t('lora.not_on_civitai')}
                </span>
                <button
                  class="text-[10px] text-indigo-400 hover:text-indigo-300"
                  onclick={(e) => { e.stopPropagation(); refetchLora(loraName); }}
                >
                  {locale.t('lora.retry')}
                </button>
              </div>
            {:else}
              <div class="absolute inset-0 flex flex-col items-center justify-center gap-1.5 px-2">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-6 h-6 text-neutral-700" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
                <p class="text-[9px] text-neutral-600 text-center leading-snug">{locale.t('model.thumb_empty_hint')}</p>
              </div>
            {/if}

            {#if !isLoading}
              <div
                class="absolute left-1 bottom-1 z-10 flex gap-0.5"
                onclick={(e) => e.stopPropagation()}
              >
                {#if imgUrl && !imgUrl.startsWith("data:")}
                  <button
                    type="button"
                    class="rounded bg-black/70 px-1.5 py-0.5 text-[9px] text-neutral-200 hover:bg-black/90"
                    title={locale.t('checkpoint.set_sidecar_tip')}
                    onclick={() => void setSidecarFromPreview(loraName)}
                  >
                    {locale.t('checkpoint.set_sidecar')}
                  </button>
                {/if}
                <button
                  type="button"
                  class="rounded bg-black/70 px-1.5 py-0.5 text-[9px] text-neutral-200 hover:bg-black/90"
                  title={locale.t('model.thumb_from_gallery_tip')}
                  onclick={() => void setSidecarFromGallery(loraName)}
                >
                  {locale.t('model.thumb_from_gallery')}
                </button>
              </div>
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
                    title={locale.t('lora.prev_image')}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
                  </button>
                  <button
                    class="w-6 h-6 flex items-center justify-center rounded-full bg-black/60 text-neutral-300 hover:bg-black/80 text-xs"
                    onclick={() => nextImage(loraName)}
                    title={locale.t('lora.next_image')}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
                  </button>
                </div>
              {/if}
            {/if}
          </div>

          <!-- Info area -->
          <div class="shrink-0 flex flex-col p-1.5 gap-0.5 overflow-hidden max-h-[40%]">
            <div class="flex items-start justify-between gap-1">
              <button
                class="text-left"
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
                  title={locale.t('lora.view_civitai')}
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

            <!-- Trigger words -->
            {#if info?.civitai_trigger_words?.length || info?.modelspec_trigger_phrase}
              <div class="flex flex-wrap gap-1">
                {#if info.civitai_trigger_words?.length}
                  {#each info.civitai_trigger_words as word}
                    <button
                      class="px-1.5 py-0.5 text-[10px] rounded bg-indigo-500/10 border border-indigo-500/30 text-indigo-300 hover:bg-indigo-500/20 hover:border-indigo-400/50 transition-colors"
                      onclick={(e) => { e.stopPropagation(); addTriggerWord(loraName, word); }}
                      title={locale.t('lora.add_to_prompt', { word })}
                    >
                      {word}
                    </button>
                  {/each}
                {:else if info?.modelspec_trigger_phrase}
                  {#each info.modelspec_trigger_phrase.split(",").map((s) => s.trim()).filter(Boolean) as word}
                    <button
                      class="px-1.5 py-0.5 text-[10px] rounded bg-indigo-500/10 border border-indigo-500/30 text-indigo-300 hover:bg-indigo-500/20 hover:border-indigo-400/50 transition-colors"
                      onclick={(e) => { e.stopPropagation(); addTriggerWord(loraName, word); }}
                      title={locale.t('lora.add_to_prompt', { word })}
                    >
                      {word}
                    </button>
                  {/each}
                {/if}
              </div>
            {/if}
          </div>
        </div>
    {/snippet}

    {#snippet folderGroup(node: FilenameTreeNode, depth: number)}
      {@const isCollapsed = collapsedFolders.has(node.path)}
      <div class="min-w-0">
        <button
          type="button"
          onclick={() => toggleFolder(node.path)}
          class="flex w-full items-center gap-1 rounded px-1 py-1 text-left text-[11px] text-neutral-300 hover:bg-neutral-800/60"
          title={node.path}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3 shrink-0 text-neutral-500 transition-transform {isCollapsed ? '' : 'rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6"/></svg>
          <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5 shrink-0 text-neutral-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z"/></svg>
          <span class="truncate font-medium">{node.name}</span>
          <span class="shrink-0 text-[10px] tabular-nums text-neutral-500">({countTreeFiles(node)})</span>
        </button>
        {#if !isCollapsed}
          <div class="ml-1.5 border-l border-neutral-800 pl-1.5">
            {#each node.folders as child (child.path)}
              {@render folderGroup(child, depth + 1)}
            {/each}
            {#if node.files.length > 0}
              <div class="grid gap-2.5 py-1" style="grid-template-columns: repeat(auto-fill, minmax({cardSize}px, 1fr));">
                {#each node.files as loraName (loraName)}
                  {@render loraCard(loraName)}
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/snippet}

    <div class="flex-1 min-h-0 overflow-y-auto px-2 py-1.5">
      {#if sortMode === "tree" && loraTree}
        <div class="space-y-0.5">
          {#each loraTree.folders as folder (folder.path)}
            {@render folderGroup(folder, 0)}
          {/each}
          {#if loraTree.files.length > 0}
            <div class="grid gap-2.5 py-1" style="grid-template-columns: repeat(auto-fill, minmax({cardSize}px, 1fr));">
              {#each loraTree.files as loraName (loraName)}
                {@render loraCard(loraName)}
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        <div class="grid gap-2.5" style="grid-template-columns: repeat(auto-fill, minmax({cardSize}px, 1fr));">
          {#each filteredLoras() as loraName (loraName)}
            {@render loraCard(loraName)}
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>
