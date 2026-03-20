<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import {
    downloadModel,
    listCivitaiArchitectures,
    searchCivitaiModels,
    type CivitaiModel,
    type CivitaiModelFile,
    type CivitaiModelType,
    type CivitaiPeriod,
    type CivitaiSort,
    type CivitaiFileFormat,
  } from "../../utils/api.js";
  import { models } from "../../stores/models.svelte.js";

  const CIVITAI_API_KEY_KEY = "mooshieui.civitai.apiKey.v1";
  const CIVITAI_COLUMNS_KEY = "mooshieui.civitai.columns.v1";
  const CIVITAI_ARCH_CACHE_KEY = "mooshieui.civitai.architectures.v2";
  const CIVITAI_ARCH_CACHE_MAX_AGE_MS = 1000 * 60 * 60 * 12;
  const ARCHITECTURE_LOAD_TIMEOUT_MS = 12000;

  const modelTypes: Array<{ value: CivitaiModelType | ""; label: string }> = [
    { value: "", label: "All Types" },
    { value: "Checkpoint", label: "Checkpoint" },
    { value: "LORA", label: "LoRA" },
    { value: "Controlnet", label: "ControlNet" },
    { value: "Upscaler", label: "Upscaler" },
    { value: "VAE", label: "VAE" },
    { value: "TextualInversion", label: "Textual Inversion" },
  ];

  const sortOptions: Array<{ value: CivitaiSort; label: string }> = [
    { value: "Highest Rated", label: "Highest Rated" },
    { value: "Most Downloaded", label: "Most Downloaded" },
    { value: "Newest", label: "Newest" },
  ];

  const periodOptions: Array<{ value: CivitaiPeriod; label: string }> = [
    { value: "AllTime", label: "All Time" },
    { value: "Month", label: "Month" },
    { value: "Week", label: "Week" },
    { value: "Day", label: "Day" },
  ];

  const architectureOptions = $state<Array<{ value: string; label: string }>>([
    { value: "", label: "All Base Models" },
  ]);

  const fileFormatOptions: Array<{ value: CivitaiFileFormat | ""; label: string }> = [
    { value: "", label: "All File Formats" },
    { value: "SafeTensor", label: "SafeTensor" },
    { value: "PickleTensor", label: "PickleTensor" },
    { value: "GGUF", label: "GGUF" },
    { value: "Diffusers", label: "Diffusers" },
    { value: "Core ML", label: "Core ML" },
    { value: "ONNX", label: "ONNX" },
    { value: "Other", label: "Other" },
  ];

  // Note: CivitAI public API does not support a "status" query parameter.
  // The filter was removed because it had no effect on search results.

  const categoryOptions = [
    { value: "checkpoints", label: "Checkpoint" },
    { value: "loras", label: "LoRA" },
    { value: "upscale_models", label: "Upscaler" },
    { value: "vae", label: "VAE" },
    { value: "controlnet", label: "ControlNet" },
    { value: "embeddings", label: "Textual Inversion" },
  ];

  const hfQuickLinks = [
    {
      label: "SDXL Base 1.0",
      url: "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors",
      filename: "sd_xl_base_1.0.safetensors",
      category: "checkpoints",
    },
    {
      label: "OmniSR X2",
      url: "https://huggingface.co/Acly/Omni-SR/resolve/main/OmniSR_X2_DIV2K.safetensors",
      filename: "OmniSR_X2_DIV2K.safetensors",
      category: "upscale_models",
    },
    {
      label: "OmniSR X4",
      url: "https://huggingface.co/Acly/Omni-SR/resolve/main/OmniSR_X4_DIV2K.safetensors",
      filename: "OmniSR_X4_DIV2K.safetensors",
      category: "upscale_models",
    },
  ] as const;

  let source = $state<"civitai" | "direct">("civitai");
  let civitaiColumns = $state(5);

  let query = $state("");
  let selectedType = $state<CivitaiModelType | "">("");
  let selectedArchitecture = $state("");
  let selectedFileFormat = $state<CivitaiFileFormat | "">("");
  let sort = $state<CivitaiSort>("Most Downloaded");
  let period = $state<CivitaiPeriod>("AllTime");
  let includeNsfw = $state(false);
  let page = $state(1);
  let nextCursor = $state<string | null>(null);
  let hasMore = $state(true);

  let apiKey = $state("");
  let apiKeyDraft = $state("");
  let keySaved = $state(false);
  let keyRecommended = $state(false);
  let loadingArchitectures = $state(false);
  let refreshingArchitectures = $state(false);
  let architectureHydratedFromApi = $state(false);
  let architectureError = $state<string | null>(null);

  let loading = $state(false);
  let loadingMore = $state(false);
  let error = $state<string | null>(null);
  let items = $state<CivitaiModel[]>([]);
  let totalPages = $state(1);
  let totalItems = $state(0);
  let civitaiFailures = $state(0);

  let scrollHost = $state<HTMLDivElement | null>(null);
  let loadMoreSentinel = $state<HTMLDivElement | null>(null);

  let directUrl = $state("");
  let directFilename = $state("");
  let directCategory = $state("checkpoints");
  let directStatus = $state<string | null>(null);
  let directInstalling = $state(false);

  let filterDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let queryDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  let initialSearchDone = $state(false);

  // Auto-search when dropdown/checkbox filters change (short debounce)
  $effect(() => {
    void selectedType;
    void selectedArchitecture;
    void selectedFileFormat;
    void sort;
    void period;
    void includeNsfw;

    if (!initialSearchDone) return;

    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    filterDebounceTimer = setTimeout(() => {
      void runSearch();
    }, 150);
  });

  // Auto-search when query text changes (longer debounce for typing)
  $effect(() => {
    void query;

    if (!initialSearchDone) return;

    if (queryDebounceTimer) clearTimeout(queryDebounceTimer);
    queryDebounceTimer = setTimeout(() => {
      void runSearch();
    }, 500);
  });

  let downloading = $state<Record<string, { downloaded: number; total: number }>>({});
  let failedPreviewUrls = $state<Record<string, true>>({});
  let expandedCards = $state<Record<number, boolean>>({});
  let selectedModel = $state<CivitaiModel | null>(null);

  // Virtual scrolling state
  let gridContainerRef = $state<HTMLDivElement | null>(null);
  let scrollY = $state(0);
  let viewportH = $state(800);
  let gridW = $state(0);
  let gridOffsetY = $state(0);
  const VGAP = 12; // gap-3 = 0.75rem = 12px
  const VBUFFER = 3; // extra rows above/below viewport

  const rowH = $derived.by(() => {
    if (!gridW || !civitaiColumns) return 280;
    const cardW = (gridW - VGAP * (civitaiColumns - 1)) / civitaiColumns;
    return cardW * (4 / 3) + VGAP;
  });
  const totalRows = $derived(Math.ceil(items.length / civitaiColumns));
  const firstRow = $derived.by(() => {
    const relScroll = scrollY - gridOffsetY;
    return Math.max(0, Math.floor(relScroll / rowH) - VBUFFER);
  });
  const lastRow = $derived.by(() => {
    const relScroll = scrollY - gridOffsetY;
    return Math.min(totalRows, Math.ceil((relScroll + viewportH) / rowH) + VBUFFER);
  });
  const visibleItems = $derived(items.slice(firstRow * civitaiColumns, lastRow * civitaiColumns));
  const topPad = $derived(firstRow * rowH);
  const botPad = $derived(Math.max(0, (totalRows - lastRow) * rowH));

  let rafPending = false;
  function handleScroll() {
    if (rafPending) return;
    rafPending = true;
    requestAnimationFrame(() => {
      if (scrollHost) {
        scrollY = scrollHost.scrollTop;
        viewportH = scrollHost.clientHeight;
      }
      rafPending = false;
    });
  }

  // Measure grid container width and offset relative to scrollHost
  $effect(() => {
    if (!gridContainerRef || !scrollHost) return;
    const ro = new ResizeObserver(() => {
      if (gridContainerRef) {
        gridW = gridContainerRef.clientWidth;
        // Compute offset relative to scroll container
        let offset = 0;
        let el: HTMLElement | null = gridContainerRef;
        while (el && el !== scrollHost) {
          offset += el.offsetTop;
          el = el.offsetParent as HTMLElement | null;
        }
        gridOffsetY = offset;
      }
    });
    ro.observe(gridContainerRef);
    ro.observe(scrollHost);
    return () => ro.disconnect();
  });

  // Reactively set up IntersectionObserver for infinite scroll whenever the sentinel element changes
  $effect(() => {
    const sentinel = loadMoreSentinel;
    const root = scrollHost;
    if (!sentinel || !root) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          void loadNextPage();
        }
      },
      {
        root,
        rootMargin: "400px 0px",
        threshold: 0,
      },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  });

  function formatCount(value: number | undefined): string {
    return new Intl.NumberFormat().format(value ?? 0);
  }

  function formatPercent(downloaded: number, total: number): number {
    if (total <= 0) return 0;
    return Math.max(0, Math.min(100, Math.round((downloaded / total) * 100)));
  }

  function mapCivitaiTypeToCategory(type: string): string | null {
    if (type === "Checkpoint") return "checkpoints";
    if (type === "LORA") return "loras";
    if (type === "Upscaler") return "upscale_models";
    if (type === "VAE") return "vae";
    if (type === "Controlnet") return "controlnet";
    return null;
  }

  function withToken(downloadUrl: string): string {
    const trimmed = apiKey.trim();
    if (!trimmed) return downloadUrl;
    try {
      const url = new URL(downloadUrl);
      if (!url.searchParams.get("token")) {
        url.searchParams.set("token", trimmed);
      }
      return url.toString();
    } catch {
      return downloadUrl;
    }
  }

  function loadApiKey() {
    try {
      const saved = localStorage.getItem(CIVITAI_API_KEY_KEY) ?? "";
      apiKey = saved;
      apiKeyDraft = saved;
    } catch {
      apiKey = "";
      apiKeyDraft = "";
    }
  }

  function loadCivitaiColumns() {
    try {
      const raw = localStorage.getItem(CIVITAI_COLUMNS_KEY);
      if (!raw) return;
      const parsed = Number(raw);
      if (!Number.isFinite(parsed)) return;
      civitaiColumns = Math.max(1, Math.min(8, Math.round(parsed)));
    } catch {
      civitaiColumns = 3;
    }
  }

  $effect(() => {
    try {
      localStorage.setItem(CIVITAI_COLUMNS_KEY, String(civitaiColumns));
    } catch {
      // Ignore persistence failures.
    }
  });

  function saveApiKey() {
    const normalized = apiKeyDraft.trim();
    apiKey = normalized;
    keySaved = true;
    keyRecommended = false;
    error = null;
    try {
      if (normalized) {
        localStorage.setItem(CIVITAI_API_KEY_KEY, normalized);
      } else {
        localStorage.removeItem(CIVITAI_API_KEY_KEY);
      }
    } catch {
      // Ignore storage failures and keep runtime key only.
    }
    setTimeout(() => {
      keySaved = false;
    }, 1500);

    // Refresh architecture filters in the background because auth can change available models.
    void fetchArchitectures();
  }

  function normalizeArchitectures(architectures: string[]): string[] {
    return [...new Set(architectures.map((a) => a.trim()).filter((a) => !!a))]
      .sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
  }

  function applyArchitectureOptions(architectures: string[]) {
    const normalized = normalizeArchitectures(architectures);

    architectureOptions.length = 0;
    architectureOptions.push({ value: "", label: "All Base Models" });
    for (const arch of normalized) {
      architectureOptions.push({ value: arch, label: arch });
    }

    if (selectedArchitecture && !normalized.includes(selectedArchitecture)) {
      selectedArchitecture = "";
    }
  }

  function mergeArchitectureOptions(architectures: string[]) {
    const existing = architectureOptions.slice(1).map((option) => option.value);
    applyArchitectureOptions([...existing, ...architectures]);
  }

  function collectArchitecturesFromModels(models: CivitaiModel[]): string[] {
    const values: string[] = [];
    for (const model of models) {
      for (const version of model.modelVersions ?? []) {
        const baseModel = version.baseModel?.trim();
        if (baseModel) values.push(baseModel);
      }
    }
    return values;
  }

  function loadArchitectureCache(): boolean {
    try {
      const raw = localStorage.getItem(CIVITAI_ARCH_CACHE_KEY);
      if (!raw) return false;

      const parsed = JSON.parse(raw) as { architectures?: string[]; updatedAt?: number };
      if (!Array.isArray(parsed.architectures) || !parsed.architectures.length) {
        return false;
      }

      applyArchitectureOptions(parsed.architectures);
      const age = Date.now() - (parsed.updatedAt ?? 0);
      return age >= 0 && age <= CIVITAI_ARCH_CACHE_MAX_AGE_MS;
    } catch {
      return false;
    }
  }

  function saveArchitectureCache(architectures: string[]) {
    try {
      localStorage.setItem(
        CIVITAI_ARCH_CACHE_KEY,
        JSON.stringify({
          architectures,
          updatedAt: Date.now(),
        }),
      );
    } catch {
      // Ignore persistence failures.
    }
  }

  function extractApiStatus(message: string): number | null {
    const m = message.match(/API error \((\d+)\):/);
    if (!m) return null;
    return Number(m[1]);
  }

  async function fetchModels(nextPage: number = 1, append: boolean = false) {
    if (append) {
      loadingMore = true;
    } else {
      loading = true;
      error = null;
      page = 1;
      nextCursor = null;
    }

    // If we are appending and have a cursor, continue with cursor-based pagination.
    const cursorParam = append ? nextCursor : null;

    if (!append && !cursorParam) {
      page = nextPage;
    }

    try {
      const response = await searchCivitaiModels({
        query: query.trim() || undefined,
        type: selectedType || undefined,
        baseModel: selectedArchitecture || undefined,
        fileFormat: selectedFileFormat || undefined,
        sort,
        period,
        nsfw: includeNsfw,
        page: cursorParam ? undefined : (append ? nextPage : page),
        cursor: cursorParam ?? undefined,
        limit: 30,
        apiKey: apiKey.trim() || undefined,
      });

      if (append) {
        const existing = new Set(items.map((item) => item.id));
        const incoming = response.items.filter((item) => !existing.has(item.id));
        items = [...items, ...incoming];
      } else {
        items = response.items;
      }

      if (response.metadata.currentPage) {
        page = response.metadata.currentPage;
      }
      nextCursor = response.metadata.nextCursor ?? null;
      totalPages = Math.max(1, response.metadata.totalPages || 1);
      totalItems = response.metadata.totalItems || response.items.length;
      if (nextCursor) {
        hasMore = response.items.length > 0;
      } else {
        hasMore = page < totalPages && response.items.length > 0;
      }

      // Fast path: populate architectures from loaded model data immediately.
      const inferredArchitectures = collectArchitecturesFromModels(response.items);
      if (inferredArchitectures.length > 0) {
        mergeArchitectureOptions(inferredArchitectures);
        saveArchitectureCache(architectureOptions.slice(1).map((option) => option.value));
      }

      civitaiFailures = 0;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      error = message;
      if (!append) {
        items = [];
        totalPages = 1;
        totalItems = 0;
        hasMore = false;
      }
      civitaiFailures += 1;

      const status = extractApiStatus(message);
      if (status === 401 || status === 403 || status === 429) {
        keyRecommended = true;
      }
      if (!apiKey.trim() && civitaiFailures >= 2) {
        keyRecommended = true;
      }
    } finally {
      loading = false;
      loadingMore = false;
    }
  }

  async function runSearch() {
    hasMore = true;
    page = 1;
    await fetchModels(1, false);
  }

  async function loadNextPage() {
    if (loading || loadingMore || !hasMore || source !== "civitai") return;
    const next = nextCursor ? page : page + 1;
    await fetchModels(next, true);
  }

  async function fetchArchitectures() {
    const hasExistingOptions = architectureOptions.length > 1;
    loadingArchitectures = !hasExistingOptions;
    refreshingArchitectures = hasExistingOptions;
    architectureError = null;
    try {
      let timeoutId: ReturnType<typeof setTimeout> | null = null;
      const timeoutPromise = new Promise<never>((_, reject) => {
        timeoutId = setTimeout(() => {
          reject(new Error("timeout"));
        }, ARCHITECTURE_LOAD_TIMEOUT_MS);
      });

      const architectures = await Promise.race([
        listCivitaiArchitectures(apiKey.trim() || undefined),
        timeoutPromise,
      ]);

      if (timeoutId) clearTimeout(timeoutId);
      const normalized = normalizeArchitectures(architectures);
      applyArchitectureOptions(normalized);
      saveArchitectureCache(normalized);
      architectureHydratedFromApi = true;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (message.includes("timeout")) {
        architectureError = "Timed out loading full architecture list. Showing discovered architectures.";
      } else {
        architectureError = "Could not load full architecture list right now.";
      }
      if (!hasExistingOptions) {
        architectureOptions.length = 0;
        architectureOptions.push({ value: "", label: "All Architectures" });
      }
    } finally {
      loadingArchitectures = false;
      refreshingArchitectures = false;
    }
  }

  async function installModel(model: CivitaiModel, file: CivitaiModelFile) {
    const category = mapCivitaiTypeToCategory(model.type);
    if (!category) {
      error = `Cannot auto-install type: ${model.type}. Use Open Link to download manually.`;
      return;
    }

    const key = file.name;
    downloading = {
      ...downloading,
      [key]: { downloaded: 0, total: 0 },
    };

    try {
      await downloadModel(withToken(file.downloadUrl), category, file.name);
      await models.refresh();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      const status = extractApiStatus(message);
      if (status === 401 || status === 403) {
        keyRecommended = true;
        error = "This model requires a CivitAI API key to download. Add your key above, then try again.";
      } else {
        error = message;
      }
    } finally {
      const next = { ...downloading };
      delete next[key];
      downloading = next;
    }
  }

  async function installFromDirectUrl() {
    directStatus = null;
    if (!directUrl.trim()) {
      directStatus = "Direct URL is required.";
      return;
    }
    if (!directFilename.trim()) {
      directStatus = "Filename is required.";
      return;
    }

    directInstalling = true;
    try {
      await downloadModel(directUrl.trim(), directCategory, directFilename.trim());
      await models.refresh();
      directStatus = "Model downloaded and installed.";
    } catch (e) {
      directStatus = e instanceof Error ? e.message : String(e);
    } finally {
      directInstalling = false;
    }
  }

  function useQuickLink(item: (typeof hfQuickLinks)[number]) {
    source = "direct";
    directUrl = item.url;
    directFilename = item.filename;
    directCategory = item.category;
    directStatus = null;
  }

  function topVersion(model: CivitaiModel) {
    return model.modelVersions[0];
  }

  function normalizeImageUrl(url: string): string {
    if (url.startsWith("http://")) {
      url = `https://${url.slice(7)}`;
    }
    return url;
  }

  /** Rewrite CivitAI image URL to request a smaller thumbnail.
   *  CivitAI CDN URLs: .../uuid/original=true/filename.jpeg → .../uuid/width=N/filename.jpeg */
  function thumbnailUrl(url: string, width: number = 450): string {
    url = normalizeImageUrl(url);
    // Replace original=true or existing width= with requested width
    if (/\/original=true\//.test(url)) {
      return url.replace(/\/original=true\//, `/width=${width}/`);
    }
    if (/\/width=\d+\//.test(url)) {
      return url.replace(/\/width=\d+\//, `/width=${width}/`);
    }
    return url;
  }

  function isNsfwImage(img: { nsfw?: string }): boolean {
    return !!img.nsfw && img.nsfw !== "None";
  }

  function previewCandidates(model: CivitaiModel): string[] {
    const version = topVersion(model);
    if (!version?.images) return [];
    // Prefer SFW images first, then fall back to NSFW ones
    const validImages = version.images.filter(
      (img) => !!img.url && !img.url.endsWith(".mp4"),
    );
    const sfw = validImages.filter((img) => !isNsfwImage(img));
    const ordered = sfw.length > 0 ? [...sfw, ...validImages.filter((img) => isNsfwImage(img))] : validImages;
    return ordered.map((img) => thumbnailUrl(img.url, 450));
  }

  function previewImage(model: CivitaiModel): string | null {
    const candidates = previewCandidates(model);
    for (const url of candidates) {
      const key = `${model.id}::${url}`;
      if (!failedPreviewUrls[key]) return url;
    }
    return null;
  }

  function isPreviewNsfw(model: CivitaiModel): boolean {
    const version = topVersion(model);
    if (!version?.images) return false;
    const validImages = version.images.filter(
      (img) => !!img.url && !img.url.endsWith(".mp4"),
    );
    if (validImages.length === 0) return false;
    // Check if the currently displayed image is NSFW
    const imageUrl = previewImage(model);
    if (!imageUrl) return validImages.some((img) => isNsfwImage(img));
    // Find which original image matches the displayed thumbnail
    for (const img of validImages) {
      if (thumbnailUrl(img.url, 450) === imageUrl) {
        return isNsfwImage(img);
      }
    }
    return false;
  }

  function markPreviewFailed(modelId: number, url: string) {
    const key = `${modelId}::${url}`;
    if (failedPreviewUrls[key]) return;
    failedPreviewUrls = {
      ...failedPreviewUrls,
      [key]: true,
    };
  }

  function modelUrl(model: CivitaiModel): string {
    return `https://civitai.com/models/${model.id}`;
  }

  function isCardExpanded(modelId: number): boolean {
    return !!expandedCards[modelId];
  }

  function toggleCardExpanded(modelId: number) {
    expandedCards = {
      ...expandedCards,
      [modelId]: !expandedCards[modelId],
    };
  }

  onMount(() => {
    let unlisten: (() => void) | null = null;

    loadApiKey();
    loadCivitaiColumns();
    loadArchitectureCache();
    void fetchArchitectures();

    void (async () => {
      unlisten = await listen("download:progress", (event: any) => {
        const payload = event.payload as {
          filename: string;
          downloaded: number;
          total: number;
          done: boolean;
        };

        if (!payload?.filename) return;

        if (payload.done) {
          const next = { ...downloading };
          delete next[payload.filename];
          downloading = next;
          return;
        }

        downloading = {
          ...downloading,
          [payload.filename]: {
            downloaded: payload.downloaded ?? 0,
            total: payload.total ?? 0,
          },
        };
      });

      await runSearch();
      initialSearchDone = true;
    })();

    return () => {
      if (unlisten) unlisten();
    };
  });
</script>

<div class="h-full overflow-y-auto p-6" style="will-change: scroll-position;" bind:this={scrollHost} onscroll={handleScroll}>
  <div class="mx-auto space-y-4">
    <div class="flex flex-col gap-1">
      <h2 class="text-lg font-semibold text-neutral-100">Model Hub</h2>
      <p class="text-xs text-neutral-400">
        CivitAI is optional. You can browse CivitAI, use your own API key when needed, or install from direct URLs.
      </p>
    </div>

    <div class="flex flex-wrap gap-2">
      <button
        class="px-3 py-1.5 text-xs rounded border transition-colors {source === 'civitai' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}"
        onclick={() => (source = "civitai")}
      >
        CivitAI Browse
      </button>
      <button
        class="px-3 py-1.5 text-xs rounded border transition-colors {source === 'direct' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}"
        onclick={() => (source = "direct")}
      >
        Direct URL / Hugging Face
      </button>
    </div>

    <section class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-4 space-y-3">
      <div class="grid grid-cols-1 lg:grid-cols-[1fr_auto] gap-3 items-end">
        <div>
          <div class="text-xs mb-1 {keyRecommended ? 'text-red-400' : 'text-neutral-400'}">CivitAI API Key {keyRecommended ? '(Required)' : '(Optional)'}</div>
          <input
            id="civitai-api-key"
            name="civitaiApiKey"
            type="password"
            bind:value={apiKeyDraft}
            class="w-full bg-neutral-800 border rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 {keyRecommended ? 'border-red-500 ring-1 ring-red-500/50' : 'border-neutral-700'}"
            placeholder="Paste CivitAI API key"
          />
        </div>
        <div class="flex items-center gap-2">
          <button
            class="px-3 py-2 text-xs rounded bg-indigo-600 hover:bg-indigo-500 text-white transition-colors"
            onclick={saveApiKey}
          >
            Save Key
          </button>
          {#if keySaved}
            <span class="text-[11px] text-green-300">Saved</span>
          {/if}
        </div>
      </div>

      {#if keyRecommended}
        <div class="rounded-lg border border-red-800/70 bg-red-900/20 px-3 py-2 text-xs text-red-200 space-y-1.5">
          <p>A CivitAI API key is needed to download this model.</p>
          <p class="text-red-300/80">To create one: go to
            <a href="https://civitai.com/user/account" target="_blank" rel="noreferrer" class="underline hover:text-white">civitai.com/user/account</a>
            → scroll to <span class="font-semibold">API Keys</span> → click <span class="font-semibold">Add API Key</span> → copy the key and paste it above.
          </p>
        </div>
      {/if}
    </section>

    {#if source === "civitai"}
      <section class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-4 space-y-3">
        <div class="grid grid-cols-1 lg:grid-cols-3 gap-3">
          <div class="lg:col-span-2">
            <div class="text-xs text-neutral-400 mb-1">Search</div>
            <input
              id="civitai-search"
              name="civitaiSearch"
              type="text"
              bind:value={query}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500"
              placeholder="Model name, creator, style..."
              onkeydown={(e) => {
                if (e.key === "Enter") runSearch();
              }}
            />
          </div>
          <div>
            <div class="text-xs text-neutral-400 mb-1">Type</div>
            <select id="civitai-type" name="civitaiType" bind:value={selectedType} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100">
              {#each modelTypes as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
          </div>
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-6 gap-3 items-end">
          <div>
            <div class="text-xs text-neutral-400 mb-1">Sort</div>
            <select id="civitai-sort" name="civitaiSort" bind:value={sort} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100">
              {#each sortOptions as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
          </div>
          <div>
            <div class="text-xs text-neutral-400 mb-1">Period</div>
            <select id="civitai-period" name="civitaiPeriod" bind:value={period} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100">
              {#each periodOptions as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
          </div>
          <div>
            <div class="text-xs text-neutral-400 mb-1">Base Model</div>
            <select id="civitai-architecture" name="civitaiArchitecture" bind:value={selectedArchitecture} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100">
              {#each architectureOptions as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
            {#if !architectureHydratedFromApi}
              <button
                class="mt-1 text-[10px] text-neutral-500 hover:text-neutral-300 transition-colors"
                onclick={() => fetchArchitectures()}
                disabled={loadingArchitectures || refreshingArchitectures}
              >
                {loadingArchitectures || refreshingArchitectures ? "Loading full base-model list..." : "Load full base-model list"}
              </button>
            {/if}
            {#if loadingArchitectures}
              <p class="text-[10px] text-neutral-500 mt-0.5">Loading base models from CivitAI...</p>
            {:else if refreshingArchitectures}
              <p class="text-[10px] text-neutral-500 mt-0.5">Refreshing base models...</p>
            {/if}
            {#if architectureError}
              <p class="text-[10px] text-amber-300 mt-0.5">{architectureError}</p>
            {/if}
          </div>
          <div>
            <div class="text-xs text-neutral-400 mb-1">File Format</div>
            <select id="civitai-file-format" name="civitaiFileFormat" bind:value={selectedFileFormat} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100">
              {#each fileFormatOptions as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
          </div>
          <label class="flex items-center gap-2 text-xs text-neutral-300 pb-2" for="civitai-nsfw">
            <input id="civitai-nsfw" name="civitaiNsfw" type="checkbox" bind:checked={includeNsfw} class="accent-indigo-500" />
            Include NSFW
          </label>
          <div class="flex items-center justify-end gap-2">
            <button
              class="px-3 py-2 text-xs rounded border border-neutral-700 text-neutral-300 hover:border-neutral-500 hover:text-neutral-100 transition-colors"
              onclick={runSearch}
              disabled={loading}
            >
              {loading ? "Loading..." : "Search"}
            </button>
          </div>
        </div>
      </section>

      {#if error}
        <div class="rounded-lg border border-red-900/70 bg-red-900/20 px-3 py-2 text-xs text-red-300">{error}</div>
      {/if}

      <div class="flex items-center justify-between text-xs text-neutral-500">
        <span>{formatCount(totalItems)} results</span>
        <span>{formatCount(items.length)} loaded</span>
      </div>

      <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 px-3 py-2">
        <div class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>Cards Per Row</span>
          <span class="text-neutral-200 tabular-nums">{civitaiColumns}</span>
        </div>
        <input
          id="civitai-columns"
          name="civitaiColumns"
          type="range"
          bind:value={civitaiColumns}
          min="1"
          max="8"
          step="1"
          class="w-full accent-indigo-500"
        />
      </div>

      {#if loading}
        <div class="rounded-xl border border-neutral-800 bg-neutral-900/50 p-8 text-sm text-neutral-400 text-center">
          Fetching models from CivitAI...
        </div>
      {:else if items.length === 0}
        <div class="rounded-xl border border-neutral-800 bg-neutral-900/50 p-8 text-sm text-neutral-400 text-center">
          No models found for this search.
        </div>
      {:else}
        <div bind:this={gridContainerRef}>
          {#if topPad > 0}<div style="height: {topPad}px;"></div>{/if}
          <div
            class="grid gap-3"
            style="grid-template-columns: repeat({civitaiColumns}, minmax(0, 1fr));"
          >
            {#each visibleItems as model (model.id)}
              {@const imageUrl = previewImage(model)}
              {@const nsfwPreview = isPreviewNsfw(model)}
              <button
                type="button"
                class="group relative rounded-lg border border-neutral-800 bg-neutral-900/50 overflow-hidden text-left cursor-pointer hover:border-indigo-500/50 transition-colors"
                style="contain: layout style paint; content-visibility: auto; contain-intrinsic-size: auto 280px;"
                onclick={() => (selectedModel = model)}
              >
                <div class="relative w-full aspect-3/4 bg-neutral-900 overflow-hidden">
                  {#if imageUrl}
                    <img
                      src={imageUrl}
                      alt={model.name}
                      class="absolute inset-0 w-full h-full object-cover {nsfwPreview ? 'blur-lg scale-110' : ''}"
                      loading="lazy"
                      decoding="async"
                      referrerpolicy="no-referrer"
                      onerror={() => markPreviewFailed(model.id, imageUrl)}
                    />
                    {#if nsfwPreview}
                      <div class="absolute inset-0 flex items-center justify-center">
                        <span class="px-2 py-1 rounded bg-red-600/80 text-[10px] font-bold text-white uppercase tracking-wider">NSFW</span>
                      </div>
                    {/if}
                  {:else}
                    <div class="absolute inset-0 flex items-center justify-center text-xs text-neutral-600">
                      No preview
                    </div>
                  {/if}
                  <div class="absolute inset-x-0 bottom-0 bg-linear-to-t from-black/80 via-black/40 to-transparent p-2 pt-6">
                    <p class="text-xs font-medium text-white truncate">{model.name}</p>
                    <p class="text-[10px] text-neutral-300 truncate">{model.type}{model.creator?.username ? ` • ${model.creator.username}` : ""}</p>
                  </div>
                </div>
              </button>
            {/each}
          </div>
          {#if botPad > 0}<div style="height: {botPad}px;"></div>{/if}
        </div>

        <!-- Detail modal -->
        {#if selectedModel}
          {@const model = selectedModel}
          {@const version = topVersion(model)}
          {@const imageUrl = previewImage(model)}
          {@const expanded = isCardExpanded(model.id)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="fixed inset-0 z-50 flex items-center justify-center bg-black/70"
            onmousedown={(e) => { if (e.target === e.currentTarget) selectedModel = null; }}
            onkeydown={(e) => { if (e.key === "Escape") selectedModel = null; }}
          >
            <div class="relative w-full max-w-lg max-h-[85vh] overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl m-4">
              <button
                class="absolute top-3 right-3 z-10 w-7 h-7 flex items-center justify-center rounded-full bg-black/50 text-neutral-300 hover:text-white hover:bg-black/70 transition-colors text-sm"
                onclick={() => (selectedModel = null)}
              >
                ✕
              </button>

              {#if imageUrl}
                <div class="relative w-full aspect-3/4 bg-neutral-950 overflow-hidden">
                  <img
                    src={thumbnailUrl(imageUrl, 1024)}
                    alt={model.name}
                    class="w-full h-full object-cover"
                    referrerpolicy="no-referrer"
                  />
                </div>
              {/if}

              <div class="p-5 space-y-4">
                <div class="flex items-start justify-between gap-3">
                  <div>
                    <h3 class="text-base font-semibold text-neutral-100">{model.name}</h3>
                    <p class="text-xs text-neutral-400">
                      {model.type}
                      {#if model.creator?.username}
                        • by {model.creator.username}
                      {/if}
                    </p>
                  </div>
                  <a
                    href={modelUrl(model)}
                    target="_blank"
                    rel="noreferrer"
                    class="shrink-0 px-2 py-1 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                  >
                    Open on CivitAI
                  </a>
                </div>

                <div class="grid grid-cols-3 gap-2 text-[11px]">
                  <div class="rounded border border-neutral-800 bg-neutral-950 px-2 py-1.5">
                    <div class="text-neutral-500">Downloads</div>
                    <div class="text-neutral-200">{formatCount(model.stats?.downloadCount)}</div>
                  </div>
                  <div class="rounded border border-neutral-800 bg-neutral-950 px-2 py-1.5">
                    <div class="text-neutral-500">Rating</div>
                    <div class="text-neutral-200">{model.stats?.rating?.toFixed?.(2) ?? "-"}</div>
                  </div>
                  <div class="rounded border border-neutral-800 bg-neutral-950 px-2 py-1.5">
                    <div class="text-neutral-500">Votes</div>
                    <div class="text-neutral-200">{formatCount(model.stats?.ratingCount)}</div>
                  </div>
                </div>

                {#if version}
                  {@const hasExtraRows = version.files.length > 1}
                  <div class="space-y-2">
                    <p class="text-xs text-neutral-400">Version: <span class="text-neutral-200">{version.name}</span></p>
                    {#if version.baseModel}
                      <p class="text-xs text-neutral-400">Base Model: <span class="text-neutral-200">{version.baseModel}</span></p>
                    {/if}
                    {#if version.files.length === 0}
                      <p class="text-xs text-neutral-500">No downloadable files in this version.</p>
                    {:else}
                      <div class="space-y-2">
                        {#each expanded || version.files.length <= 2 ? version.files : version.files.slice(0, 1) as file}
                          {@const dl = downloading[file.name]}
                          <div class="rounded border border-neutral-800 bg-neutral-950 px-3 py-2 space-y-2">
                            <div class="flex items-center justify-between gap-2">
                              <p class="text-xs text-neutral-200 truncate" title={file.name}>{file.name}</p>
                              <div class="text-[11px] text-neutral-500 shrink-0">{Math.round(file.sizeKB / 1024)} MB</div>
                            </div>
                            {#if dl}
                              {@const pct = formatPercent(dl.downloaded, dl.total)}
                              <div class="space-y-1">
                                <div class="w-full bg-neutral-800 rounded-full h-1.5 overflow-hidden">
                                  <div class="bg-indigo-400 h-full rounded-full" style="width: {pct}%"></div>
                                </div>
                                <p class="text-[10px] text-neutral-500">Downloading... {pct}%</p>
                              </div>
                            {/if}
                            <div class="flex items-center gap-2">
                              <a
                                href={withToken(file.downloadUrl)}
                                target="_blank"
                                rel="noreferrer"
                                class="px-2 py-1 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:border-neutral-500 hover:text-neutral-100 transition-colors"
                              >
                                Open Link
                              </a>
                              <button
                                class="px-2 py-1 text-[11px] rounded bg-indigo-600 hover:bg-indigo-500 text-white transition-colors disabled:opacity-50"
                                onclick={() => installModel(model, file)}
                                disabled={!!dl}
                              >
                                {dl ? "Installing..." : "Install to App"}
                              </button>
                            </div>
                          </div>
                        {/each}
                      </div>

                      {#if hasExtraRows}
                        <button
                          class="px-2 py-1 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                          onclick={() => toggleCardExpanded(model.id)}
                        >
                          {expanded ? "Show less" : `Show all ${version.files.length} files`}
                        </button>
                      {/if}
                    {/if}
                  </div>
                {:else}
                  <p class="text-xs text-neutral-500">No versions available.</p>
                {/if}
              </div>
            </div>
          </div>
        {/if}
      {/if}

      <div class="flex items-center justify-center gap-2 pt-2 text-xs text-neutral-500">
        {#if loadingMore}
          <span>Loading more models...</span>
        {:else if hasMore}
          <span>Scroll to load more</span>
        {:else if items.length > 0}
          <span>End of results</span>
        {/if}
      </div>
      <div class="h-8" bind:this={loadMoreSentinel}></div>
    {:else}
      <section class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-4 space-y-3">
        <div>
          <h3 class="text-sm font-semibold text-neutral-200">Install From Any Direct URL</h3>
          <p class="text-xs text-neutral-400 mt-1">Paste any legal, direct model URL (Hugging Face, mirror, private host) and install into your selected model category.</p>
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
          <div>
            <div class="text-xs text-neutral-400 mb-1">Direct URL</div>
            <input
              id="direct-url"
              name="directUrl"
              type="url"
              bind:value={directUrl}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500"
              placeholder="https://.../model.safetensors"
            />
          </div>
          <div>
            <div class="text-xs text-neutral-400 mb-1">Filename</div>
            <input
              id="direct-filename"
              name="directFilename"
              type="text"
              bind:value={directFilename}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500"
              placeholder="model.safetensors"
            />
          </div>
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-[1fr_auto] gap-3 items-end">
          <div>
            <div class="text-xs text-neutral-400 mb-1">Category</div>
            <select id="direct-category" name="directCategory" bind:value={directCategory} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100">
              {#each categoryOptions as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
          </div>
          <button
            class="px-3 py-2 text-xs rounded bg-indigo-600 hover:bg-indigo-500 text-white transition-colors disabled:opacity-50"
            onclick={installFromDirectUrl}
            disabled={directInstalling}
          >
            {directInstalling ? "Installing..." : "Install"}
          </button>
        </div>

        {#if directStatus}
          <div class="rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-2 text-xs text-neutral-300">{directStatus}</div>
        {/if}
      </section>

      <section class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-4 space-y-3">
        <h3 class="text-sm font-semibold text-neutral-200">Hugging Face Quick Links</h3>
        <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
          {#each hfQuickLinks as item}
            <div class="rounded-lg border border-neutral-800 bg-neutral-900 px-3 py-3 space-y-2">
              <p class="text-sm text-neutral-200">{item.label}</p>
              <p class="text-[11px] text-neutral-500 truncate" title={item.filename}>{item.filename}</p>
              <div class="flex items-center gap-2">
                <a href={item.url} target="_blank" rel="noreferrer" class="px-2 py-1 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:border-neutral-500 hover:text-neutral-100 transition-colors">Open</a>
                <button onclick={() => useQuickLink(item)} class="px-2 py-1 text-[11px] rounded border border-indigo-700 text-indigo-300 hover:border-indigo-500 hover:text-indigo-200 transition-colors">Use</button>
              </div>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  </div>
</div>
