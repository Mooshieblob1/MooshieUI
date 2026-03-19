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
    type CivitaiModelStatus,
  } from "../../utils/api.js";
  import { models } from "../../stores/models.svelte.js";

  const CIVITAI_API_KEY_KEY = "mooshieui.civitai.apiKey.v1";
  const CIVITAI_COLUMNS_KEY = "mooshieui.civitai.columns.v1";
  const CIVITAI_ARCH_CACHE_KEY = "mooshieui.civitai.architectures.v1";
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

  const statusOptions: Array<{ value: CivitaiModelStatus | ""; label: string }> = [
    { value: "", label: "All Statuses" },
    { value: "Published", label: "Published" },
    { value: "Draft", label: "Draft" },
    { value: "Training", label: "Training" },
    { value: "Scheduled", label: "Scheduled" },
    { value: "Unpublished", label: "Unpublished" },
    { value: "UnpublishedViolation", label: "Unpublished Violation" },
    { value: "GatherInterest", label: "Gather Interest" },
    { value: "Deleted", label: "Deleted" },
  ];

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
  let civitaiColumns = $state(3);

  let query = $state("");
  let selectedType = $state<CivitaiModelType | "">("");
  let selectedArchitecture = $state("");
  let selectedFileFormat = $state<CivitaiFileFormat | "">("");
  let selectedStatus = $state<CivitaiModelStatus | "">("");
  let sort = $state<CivitaiSort>("Most Downloaded");
  let period = $state<CivitaiPeriod>("AllTime");
  let includeNsfw = $state(false);
  let page = $state(1);
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

  let downloading = $state<Record<string, { downloaded: number; total: number }>>({});
  let failedPreviewUrls = $state<Record<string, true>>({});
  let expandedCards = $state<Record<number, boolean>>({});

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
      civitaiColumns = Math.max(1, Math.min(5, Math.round(parsed)));
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
    }

    page = nextPage;

    try {
      const response = await searchCivitaiModels({
        query: query.trim() || undefined,
        type: selectedType || undefined,
        baseModel: selectedArchitecture || undefined,
        fileFormat: selectedFileFormat || undefined,
        status: selectedStatus || undefined,
        sort,
        period,
        nsfw: includeNsfw,
        page: nextPage,
        limit: 100,
        apiKey: apiKey.trim() || undefined,
      });

      if (append) {
        const existing = new Set(items.map((item) => item.id));
        const incoming = response.items.filter((item) => !existing.has(item.id));
        items = [...items, ...incoming];
      } else {
        items = response.items;
      }

      totalPages = Math.max(1, response.metadata.totalPages || 1);
      totalItems = response.metadata.totalItems || response.items.length;
      hasMore = page < totalPages && response.items.length > 0;

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
    await fetchModels(page + 1, true);
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
      error = e instanceof Error ? e.message : String(e);
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
      return `https://${url.slice(7)}`;
    }
    return url;
  }

  function previewCandidates(model: CivitaiModel): string[] {
    const version = topVersion(model);
    if (!version?.images) return [];
    return version.images
      .map((img) => img.url)
      .filter((url): url is string => !!url)
      .map(normalizeImageUrl);
  }

  function previewImage(model: CivitaiModel): string | null {
    const candidates = previewCandidates(model);
    for (const url of candidates) {
      const key = `${model.id}::${url}`;
      if (!failedPreviewUrls[key]) return url;
    }
    return null;
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
    let observer: IntersectionObserver | null = null;

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

      if (scrollHost && loadMoreSentinel) {
        observer = new IntersectionObserver(
          (entries) => {
            const hit = entries.some((entry) => entry.isIntersecting);
            if (hit) {
              void loadNextPage();
            }
          },
          {
            root: scrollHost,
            rootMargin: "800px 0px",
            threshold: 0,
          },
        );
        observer.observe(loadMoreSentinel);
      }
    })();

    return () => {
      if (unlisten) unlisten();
      if (observer) observer.disconnect();
    };
  });
</script>

<div class="h-full overflow-y-auto p-6" bind:this={scrollHost}>
  <div class="max-w-6xl mx-auto space-y-4">
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
          <div class="text-xs text-neutral-400 mb-1">CivitAI API Key (Optional)</div>
          <input
            id="civitai-api-key"
            name="civitaiApiKey"
            type="password"
            bind:value={apiKeyDraft}
            class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500"
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
        <div class="rounded-lg border border-amber-800/70 bg-amber-900/20 px-3 py-2 text-xs text-amber-200">
          CivitAI is requesting authentication or rate-limiting anonymous access. Add your personal API key to continue reliably.
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

        <div class="grid grid-cols-1 lg:grid-cols-7 gap-3 items-end">
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
          <div>
            <div class="text-xs text-neutral-400 mb-1">Model Status</div>
            <select id="civitai-status" name="civitaiStatus" bind:value={selectedStatus} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100">
              {#each statusOptions as option}
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
          max="5"
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
        <div
          class="gap-4"
          style="column-count: {civitaiColumns};"
        >
          {#each items as model}
            {@const version = topVersion(model)}
            {@const imageUrl = previewImage(model)}
            <article class="mb-4 break-inside-avoid rounded-xl border border-neutral-800 bg-neutral-900/50 overflow-hidden">
              <div class="relative w-full aspect-3/4 bg-neutral-900 border-b border-neutral-800 overflow-hidden">
                {#if imageUrl}
                  <img
                    src={imageUrl}
                    alt={model.name}
                    class="absolute inset-0 w-full h-full object-cover"
                    loading="lazy"
                    referrerpolicy="no-referrer"
                    onerror={() => markPreviewFailed(model.id, imageUrl)}
                  />
                {:else}
                  <div class="absolute inset-0 flex items-center justify-center text-xs text-neutral-500 px-4 text-center">
                    Preview unavailable for this listing
                  </div>
                {/if}
              </div>

              {#if !imageUrl}
                <div class="w-full bg-neutral-900/70 border-b border-neutral-800 flex items-center justify-center text-[10px] text-neutral-500 py-1">
                  No preview image
                </div>
              {/if}

              <div class="p-4 space-y-3">
                <div class="flex items-start justify-between gap-3">
                  <div>
                    <h3 class="text-sm font-semibold text-neutral-100">{model.name}</h3>
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
                    class="px-2 py-1 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                  >
                    Open
                  </a>
                </div>

                <div class="grid grid-cols-3 gap-2 text-[11px]">
                  <div class="rounded border border-neutral-800 bg-neutral-900 px-2 py-1">
                    <div class="text-neutral-500">Downloads</div>
                    <div class="text-neutral-200">{formatCount(model.stats?.downloadCount)}</div>
                  </div>
                  <div class="rounded border border-neutral-800 bg-neutral-900 px-2 py-1">
                    <div class="text-neutral-500">Rating</div>
                    <div class="text-neutral-200">{model.stats?.rating?.toFixed?.(2) ?? "-"}</div>
                  </div>
                  <div class="rounded border border-neutral-800 bg-neutral-900 px-2 py-1">
                    <div class="text-neutral-500">Votes</div>
                    <div class="text-neutral-200">{formatCount(model.stats?.ratingCount)}</div>
                  </div>
                </div>

                {#if version}
                  {@const expanded = isCardExpanded(model.id)}
                  {@const hasExtraRows = version.files.length > 1}
                  <div class="space-y-2">
                    <p class="text-xs text-neutral-400">Version: <span class="text-neutral-200">{version.name}</span></p>
                    {#if version.files.length === 0}
                      <p class="text-xs text-neutral-500">No downloadable files in this version.</p>
                    {:else}
                      <div class="relative">
                        <div class="space-y-2 overflow-hidden transition-all {expanded ? '' : 'max-h-30'}">
                          {#each version.files as file}
                            {@const dl = downloading[file.name]}
                            <div class="rounded border border-neutral-800 bg-neutral-900 px-2 py-2 space-y-2">
                              <div class="flex items-center justify-between gap-2">
                                <p class="text-[11px] text-neutral-200 truncate" title={file.name}>{file.name}</p>
                                <div class="text-[10px] text-neutral-500">{Math.round(file.sizeKB / 1024)} MB</div>
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

                        {#if hasExtraRows && !expanded}
                          <div class="pointer-events-none absolute inset-x-0 bottom-0 h-12 bg-linear-to-t from-neutral-900 via-neutral-900/80 to-transparent"></div>
                        {/if}
                      </div>

                      {#if hasExtraRows}
                        <button
                          class="mt-1 px-2 py-1 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                          onclick={() => toggleCardExpanded(model.id)}
                        >
                          {expanded ? "Show less" : "Show more"}
                        </button>
                      {/if}
                    {/if}
                  </div>
                {:else}
                  <p class="text-xs text-neutral-500">No versions available.</p>
                {/if}
              </div>
            </article>
          {/each}
        </div>
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
