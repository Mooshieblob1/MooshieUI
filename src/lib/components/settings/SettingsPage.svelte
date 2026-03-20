<script lang="ts">
  import type { AppConfig } from "../../types/index.js";
  import { getConfig, updateConfig, stopComfyui, startComfyui } from "../../utils/api.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { autocomplete } from "../../stores/autocomplete.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { accessibility } from "../../stores/accessibility.svelte.js";
  import { check } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  declare const __APP_VERSION__: string;
  const appVersion = __APP_VERSION__ ?? "dev";

  let config = $state<AppConfig | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let saved = $state(false);
  let error = $state<string | null>(null);
  let restartNeeded = $state(false);
  let restarting = $state(false);
  let search = $state("");

  let tagUrlInput = $state("");
  let tagFileLoading = $state(false);

  // Model directory auto-detection
  interface DetectedModelDir {
    path: string;
    tool: string;
    has_checkpoints: boolean;
    has_loras: boolean;
    has_vae: boolean;
  }
  let detectedModelDirs = $state<DetectedModelDir[]>([]);
  let scanningModelDirs = $state(false);

  async function scanForModelDirs() {
    scanningModelDirs = true;
    try {
      const dirs = await invoke<DetectedModelDir[]>("detect_model_directories");
      // Filter out directories already in config
      const existing = new Set(
        (config?.extra_model_paths ?? "").split("\n").map((p: string) => p.trim()).filter(Boolean)
      );
      detectedModelDirs = dirs.filter((d) => !existing.has(d.path));
    } catch {
      detectedModelDirs = [];
    } finally {
      scanningModelDirs = false;
    }
  }

  // Move installation
  let currentInstallPath = $state("");
  let moveTargetPath = $state("");
  let moving = $state(false);
  let moveProgress = $state("");
  let moveError = $state<string | null>(null);
  let moveSuccess = $state(false);

  async function loadInstallPath() {
    try {
      currentInstallPath = await invoke<string>("get_install_path");
    } catch {
      currentInstallPath = "";
    }
  }

  async function browseMoveTarget() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choose New Install Location",
    });
    if (selected && typeof selected === "string") {
      moveTargetPath = selected;
    }
  }

  async function moveInstallation() {
    if (!moveTargetPath.trim()) return;
    moving = true;
    moveError = null;
    moveSuccess = false;
    moveProgress = "Starting move...";

    const unlisten = await listen("setup:progress", (event: any) => {
      const data = event.payload as { message: string };
      moveProgress = data.message;
    });

    try {
      await invoke("move_installation", { newPath: moveTargetPath.trim() });
      moveSuccess = true;
      moveProgress = "";
      currentInstallPath = moveTargetPath.trim();
      moveTargetPath = "";
      // Reload config since paths changed
      await loadConfig();
    } catch (e: any) {
      moveError = typeof e === "string" ? e : e.message || "Unknown error";
      moveProgress = "";
    } finally {
      moving = false;
      unlisten();
    }
  }

  function addDetectedModelDir(path: string) {
    if (!config) return;
    const current = config.extra_model_paths ?? "";
    const paths = current.split("\n").filter((p: string) => p.trim());
    if (!paths.includes(path)) {
      paths.push(path);
      config.extra_model_paths = paths.join("\n");
      checkRestartNeeded();
    }
    // Remove from detected list
    detectedModelDirs = detectedModelDirs.filter((d) => d.path !== path);
  }

  // Update check state
  type UpdateCheckState = "idle" | "checking" | "available" | "downloading" | "ready" | "up-to-date" | "error";
  let updateState = $state<UpdateCheckState>("idle");
  let updateVersion = $state("");
  let updateError = $state("");
  let updateDownloaded = $state(0);
  let updateTotal = $state(0);
  let updateObj: Awaited<ReturnType<typeof check>> | null = null;

  const updatePercent = $derived(updateTotal > 0 ? Math.round((updateDownloaded / updateTotal) * 100) : 0);

  async function checkForUpdates() {
    updateState = "checking";
    updateError = "";
    try {
      const update = await check();
      if (update) {
        updateObj = update;
        updateVersion = update.version;
        updateState = "available";
      } else {
        updateState = "up-to-date";
      }
    } catch (e) {
      updateState = "error";
      updateError = String(e);
    }
  }

  async function downloadAndInstallUpdate() {
    if (!updateObj) return;
    updateState = "downloading";
    try {
      await updateObj.downloadAndInstall((event) => {
        if (event.event === "Started") {
          updateTotal = event.data.contentLength ?? 0;
          updateDownloaded = 0;
        } else if (event.event === "Progress") {
          updateDownloaded += event.data.chunkLength;
        } else if (event.event === "Finished") {
          updateState = "ready";
        }
      });
      updateState = "ready";
    } catch (e) {
      updateState = "error";
      updateError = String(e);
    }
  }
  let dyslexicFont = $state(localStorage.getItem("mooshieui.dyslexicFont") === "true");

  $effect(() => {
    document.documentElement.classList.toggle("dyslexic-font", dyslexicFont);
    localStorage.setItem("mooshieui.dyslexicFont", String(dyslexicFont));
  });

  // Section collapse state (persisted across tab switches)
  const COLLAPSED_KEY = "mooshieui.settings.collapsed.v1";
  let collapsed: Record<string, boolean> = $state(loadCollapsedState());

  function loadCollapsedState(): Record<string, boolean> {
    const defaults: Record<string, boolean> = {
      connection: false,
      appearance: false,
      performance: false,
      paths: false,
      autocomplete: false,
      about: false,
    };
    try {
      const raw = localStorage.getItem(COLLAPSED_KEY);
      if (!raw) return defaults;
      const saved = JSON.parse(raw);
      return { ...defaults, ...saved };
    } catch {
      return defaults;
    }
  }

  let settingsCollapseSaveTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const val = JSON.stringify(collapsed);
    if (settingsCollapseSaveTimer) clearTimeout(settingsCollapseSaveTimer);
    settingsCollapseSaveTimer = setTimeout(() => {
      try { localStorage.setItem(COLLAPSED_KEY, val); } catch {}
    }, 300);
  });

  const sections = [
    { key: "connection", label: "Connection", keywords: "server mode url port remote autolaunch" },
    { key: "appearance", label: "Appearance", keywords: "theme dark light font scale size style presets fooocus" },
    { key: "performance", label: "Performance", keywords: "vram mode high low normal keep alive close" },
    { key: "paths", label: "Paths", keywords: "comfyui install venv python cli arguments extra args shared model directory models" },
    { key: "autocomplete", label: "Autocomplete", keywords: "tags taglist suggestions results url upload csv json danbooru" },
    { key: "about", label: "About", keywords: "version update check updates about" },
  ];

  function sectionVisible(key: string): boolean {
    if (!search.trim()) return true;
    const s = sections.find((sec) => sec.key === key);
    if (!s) return false;
    const q = search.toLowerCase();
    return s.label.toLowerCase().includes(q) || s.keywords.includes(q);
  }

  // Track original values for restart-needing settings
  let originalUrl = "";
  let originalPort = 0;
  let originalMode = "";
  let originalVramMode = "";
  let originalExtraArgs = "";
  let originalModelPaths = "";

  async function loadConfig() {
    config = await getConfig();
    snapshotRestartFields();
  }

  onMount(async () => {
    try {
      await loadConfig();
    } catch (e) {
      error = `Failed to load config: ${e}`;
    } finally {
      loading = false;
    }
    loadInstallPath();
  });

  function snapshotRestartFields() {
    if (!config) return;
    originalUrl = config.server_url;
    originalPort = config.server_port;
    originalMode = config.server_mode;
    originalVramMode = config.vram_mode;
    originalExtraArgs = config.extra_args.join(" ");
    originalModelPaths = config.extra_model_paths ?? "";
  }

  function checkRestartNeeded() {
    if (!config) return;
    restartNeeded =
      config.server_url !== originalUrl ||
      config.server_port !== originalPort ||
      config.server_mode !== originalMode ||
      config.vram_mode !== originalVramMode ||
      config.extra_args.join(" ") !== originalExtraArgs ||
      (config.extra_model_paths ?? "") !== originalModelPaths;
  }

  /** Auto-save for sliders, dropdowns, checkboxes — fires immediately on change. */
  async function autoSave() {
    if (!config) return;
    checkRestartNeeded();
    try {
      await updateConfig(config);
    } catch (e) {
      error = `Failed to save: ${e}`;
    }
  }

  /** Manual save for text inputs — triggered by Save button. */
  async function save() {
    if (!config) return;
    saving = true;
    error = null;
    try {
      await updateConfig(config);
      saved = true;
      snapshotRestartFields();
      checkRestartNeeded();
      setTimeout(() => (saved = false), 2000);
    } catch (e) {
      error = `Failed to save: ${e}`;
    } finally {
      saving = false;
    }
  }

  function applyTheme(theme: string) {
    document.documentElement.classList.toggle("light", theme === "light");
  }

  function applyFontScale(scale: number) {
    document.documentElement.style.setProperty("--font-scale", String(scale));
  }

  async function restartServer() {
    // Save first so restart picks up latest config
    if (config) {
      try { await updateConfig(config); } catch {}
    }
    restarting = true;
    error = null;
    try {
      connection.connected = false;
      await stopComfyui();
      await startComfyui();
      snapshotRestartFields();
      restartNeeded = false;
    } catch (e) {
      error = `Failed to restart: ${e}`;
    } finally {
      restarting = false;
    }
  }
</script>

<div class="h-full flex flex-col overflow-hidden">
  <!-- Persistent top bar -->
  {#if config}
    <div class="shrink-0 px-6 py-3 bg-neutral-900 border-b border-neutral-800 flex items-center gap-3">
      <h1 class="text-lg font-medium text-neutral-100 shrink-0">Settings</h1>

      <input
        type="text"
        bind:value={search}
        placeholder="Search settings..."
        class="flex-1 min-w-0 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
      />

      <div class="ml-auto flex items-center gap-3 shrink-0">
      {#if restartNeeded}
        <div class="flex items-center gap-1.5 text-amber-200 text-xs mr-2">
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          Restart needed
        </div>
      {/if}

      <button
        class="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
        onclick={save}
        disabled={saving}
      >
        {#if saving}
          Saving...
        {:else if saved}
          Saved!
        {:else}
          Save
        {/if}
      </button>

      <button
        class="px-3 py-1.5 rounded-lg text-sm transition-colors disabled:opacity-50 {restartNeeded
          ? 'bg-red-700 hover:bg-red-600 text-white animate-pulse'
          : 'bg-neutral-700 hover:bg-neutral-600 text-neutral-100'}"
        onclick={restartServer}
        disabled={restarting}
      >
        {#if restarting}
          Restarting...
        {:else}
          Restart ComfyUI
        {/if}
      </button>
      </div>
    </div>
  {/if}

  <!-- Scrollable content -->
  <div class="flex-1 overflow-y-auto p-6">
    <div class="max-w-2xl mx-auto space-y-6">
      {#if loading}
        <div class="flex items-center justify-center py-12 text-neutral-500">
          <div class="w-6 h-6 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin"></div>
        </div>
      {:else if config}
        <!-- Connection -->
        {#if sectionVisible("connection")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.connection = !collapsed.connection)}
          >
            Connection
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.connection ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.connection}
          <div class="px-5 pb-5 space-y-4">
          <div>
            <label class="block text-xs text-neutral-400 mb-1">Server Mode<span class="text-amber-400">*</span></label>
            <select
              bind:value={config.server_mode}
              onchange={() => { autoSave(); }}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
            >
              <option value="autolaunch">Auto Launch (start ComfyUI automatically)</option>
              <option value="remote">Remote (connect to existing server)</option>
            </select>
          </div>

          {#if config.server_mode === "autolaunch"}
          <div class="flex items-center justify-between">
            <div>
              <p class="text-sm text-neutral-200">Auto-start ComfyUI on launch</p>
              <p class="text-xs text-neutral-500">When disabled, you can start ComfyUI manually from the banner</p>
            </div>
            <button
              class="w-10 h-5 rounded-full transition-colors cursor-pointer {config.auto_start !== false ? 'bg-indigo-600' : 'bg-neutral-700'}"
              onclick={() => { config.auto_start = config.auto_start === false; autoSave(); }}
              role="switch"
              aria-checked={config.auto_start !== false}
            >
              <div class="w-4 h-4 rounded-full bg-white shadow transition-transform {config.auto_start !== false ? 'translate-x-5' : 'translate-x-0.5'}"></div>
            </button>
          </div>
          {/if}

          <div class="grid grid-cols-3 gap-3">
            <div class="col-span-2">
              <label class="block text-xs text-neutral-400 mb-1">Server URL<span class="text-amber-400">*</span></label>
              <input
                type="text"
                bind:value={config.server_url}
                oninput={checkRestartNeeded}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                placeholder="http://127.0.0.1:8188"
              />
            </div>
            <div>
              <label class="block text-xs text-neutral-400 mb-1">Port<span class="text-amber-400">*</span></label>
              <input
                type="number"
                bind:value={config.server_port}
                oninput={checkRestartNeeded}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
                min="1"
                max="65535"
              />
            </div>
          </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Appearance -->
        {#if sectionVisible("appearance")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.appearance = !collapsed.appearance)}
          >
            Appearance
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.appearance ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.appearance}
          <div class="px-5 pb-5 space-y-4">
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label class="block text-xs text-neutral-400 mb-1">Theme</label>
              <select
                bind:value={config.theme}
                onchange={() => { if (config) { applyTheme(config.theme); autoSave(); } }}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
              >
                <option value="dark">Dark</option>
                <option value="light">Light</option>
              </select>
            </div>

            <div>
              <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
                Font Scale
                <span class="text-neutral-300">{Math.round(config.font_scale * 100)}%</span>
              </label>
              <input
                type="range"
                bind:value={config.font_scale}
                onchange={() => { autoSave(); }}
                oninput={() => { if (config) applyFontScale(config.font_scale); }}
                min="0.75"
                max="1.5"
                step="0.05"
                class="w-full accent-indigo-500"
              />
            </div>
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">Color Vision Simulator</label>
            <select
              bind:value={accessibility.visionSimulatorMode}
              onchange={() => accessibility.saveSettings()}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
            >
              <option value="none">None</option>
              <option value="protanopia">Protanopia</option>
              <option value="deuteranopia">Deuteranopia</option>
              <option value="tritanopia">Tritanopia</option>
            </select>
            <p class="text-[10px] text-neutral-500 mt-0.5">Applies a global filter to simulate color vision deficiencies.</p>
          </div>

          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="enable-style-presets"
              bind:checked={generation.stylePresetsEnabled}
              onchange={() => {
                generation.saveSettings();
              }}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="enable-style-presets" class="text-sm text-neutral-200">Enable Style Presets</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">Show Fooocus-style presets in the prompt panel. Off by default.</p>
            </div>
          </div>

          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="dyslexic-font"
              bind:checked={dyslexicFont}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="dyslexic-font" class="text-sm text-neutral-200">Dyslexic-Friendly Font</label>
              <p class="text-[10px] text-neutral-500 mt-0.5">Use OpenDyslexic font throughout the interface for improved readability.</p>
            </div>
          </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Performance -->
        {#if sectionVisible("performance")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.performance = !collapsed.performance)}
          >
            Performance
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.performance ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.performance}
          <div class="px-5 pb-5 space-y-4">
          <div>
            <label class="block text-xs text-neutral-400 mb-1">VRAM Mode<span class="text-amber-400">*</span></label>
            <select
              bind:value={config.vram_mode}
              onchange={() => { autoSave(); }}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
            >
              <option value="high">High VRAM (12GB+) — keep models in VRAM</option>
              <option value="normal">Normal — load for sampling, offload between</option>
              <option value="low">Low VRAM (&lt;6GB) — aggressive offloading</option>
              <option value="none">No VRAM — CPU offload everything</option>
            </select>
            <p class="text-[10px] text-neutral-500 mt-0.5">Auto-detected during setup. Change if generation is slow or you run out of VRAM.</p>
          </div>

          <div class="flex items-start gap-3">
            <input
              type="checkbox"
              id="keep-alive"
              bind:checked={config.keep_alive}
              onchange={() => { autoSave(); }}
              class="w-4 h-4 mt-0.5 accent-indigo-500 rounded"
            />
            <div>
              <label for="keep-alive" class="text-sm text-neutral-200">Keep ComfyUI running after app closes</label>
              <p class="text-[10px] text-amber-400/80 mt-0.5">Not recommended — ComfyUI will continue using VRAM even when the app is closed.</p>
            </div>
          </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Paths -->
        {#if sectionVisible("paths")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.paths = !collapsed.paths)}
          >
            Paths
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.paths ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.paths}
          <div class="px-5 pb-5 space-y-4">

          <!-- Move Installation -->
          <div class="rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 space-y-2">
            <div class="flex items-center justify-between">
              <p class="text-xs text-neutral-400">Data Location</p>
            </div>
            {#if currentInstallPath}
              <p class="text-xs text-neutral-500 font-mono truncate" title={currentInstallPath}>{currentInstallPath}</p>
            {/if}

            {#if moveSuccess}
              <div class="rounded border border-green-800/50 bg-green-900/20 px-2 py-1.5 text-[11px] text-green-300">
                Installation moved successfully. Restart the app for all changes to take effect.
              </div>
            {/if}

            {#if !moving}
              <div class="flex gap-1.5">
                <input
                  type="text"
                  bind:value={moveTargetPath}
                  class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-1.5 text-sm text-neutral-100 placeholder-neutral-500"
                  placeholder="New location..."
                />
                <button
                  onclick={browseMoveTarget}
                  class="px-2 py-1.5 rounded-lg border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors text-xs"
                >
                  Browse
                </button>
              </div>
              {#if moveTargetPath.trim()}
                <button
                  onclick={moveInstallation}
                  class="w-full px-3 py-2 text-xs rounded bg-amber-600 hover:bg-amber-500 text-white transition-colors"
                >
                  Move Installation to New Location
                </button>
                <p class="text-[10px] text-amber-400/70">This will stop ComfyUI, copy all data (~5-10 GB), and delete the old location. May take several minutes.</p>
              {/if}
            {:else}
              <div class="flex items-center gap-2 text-xs text-neutral-400">
                <div class="w-3.5 h-3.5 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin shrink-0"></div>
                <span>{moveProgress}</span>
              </div>
            {/if}

            {#if moveError}
              <div class="rounded border border-red-800/50 bg-red-900/20 px-2 py-1.5 text-[11px] text-red-300">{moveError}</div>
            {/if}
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">ComfyUI Installation</label>
            <input
              type="text"
              bind:value={config.comfyui_path}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              placeholder="/path/to/ComfyUI"
            />
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">Python Virtual Environment</label>
            <input
              type="text"
              bind:value={config.venv_path}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              placeholder="/path/to/venv"
            />
          </div>

          <div>
            <div class="flex items-center justify-between mb-1">
              <label class="block text-xs text-neutral-400">Shared Model Directories<span class="text-amber-400">*</span></label>
              <div class="flex gap-1.5">
                <button
                  class="px-2 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                  onclick={scanForModelDirs}
                  disabled={scanningModelDirs}
                >
                  {scanningModelDirs ? "Scanning..." : "Auto-Detect"}
                </button>
                <button
                  class="px-2 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                  onclick={() => {
                    if (config) {
                      const current = config.extra_model_paths ?? "";
                      config.extra_model_paths = current ? current + "\n" : "";
                      checkRestartNeeded();
                    }
                  }}
                  title="Add another model directory"
                >
                  + Add Directory
                </button>
              </div>
            </div>
            {#each (config.extra_model_paths ?? "").split("\n") as dirPath, i}
              <div class="flex gap-1.5 mb-1.5">
                <input
                  type="text"
                  value={dirPath}
                  oninput={(e) => {
                    if (config) {
                      const paths = (config.extra_model_paths ?? "").split("\n");
                      paths[i] = (e.target as HTMLInputElement).value;
                      config.extra_model_paths = paths.join("\n") || null;
                      checkRestartNeeded();
                    }
                  }}
                  class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                  placeholder="/path/to/shared/models (e.g. from another ComfyUI or Forge install)"
                />
                {#if (config.extra_model_paths ?? "").split("\n").length > 1}
                  <button
                    class="px-2 py-2 rounded-lg border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors text-xs"
                    onclick={() => {
                      if (config) {
                        const paths = (config.extra_model_paths ?? "").split("\n");
                        paths.splice(i, 1);
                        config.extra_model_paths = paths.join("\n") || null;
                        checkRestartNeeded();
                      }
                    }}
                    title="Remove this directory"
                  >
                    &times;
                  </button>
                {/if}
              </div>
            {/each}
            <p class="text-[10px] text-neutral-500 mt-0.5">Point to a root models folder (e.g. from ComfyUI, A1111, or Forge). Subdirectories like checkpoints/, loras/, vae/ are detected automatically. You can also point directly to a folder containing model files.</p>

            {#if detectedModelDirs.length > 0}
              <div class="mt-2 space-y-1">
                <p class="text-[10px] text-neutral-500">Found model directories from other tools:</p>
                {#each detectedModelDirs as dir}
                  <div class="flex items-center gap-1.5">
                    <button
                      class="flex-1 text-left px-2 py-1.5 rounded border border-neutral-700/50 bg-neutral-800/50 hover:border-indigo-500/50 transition-colors"
                      onclick={() => addDetectedModelDir(dir.path)}
                      title="Click to add"
                    >
                      <p class="text-[11px] text-neutral-300 truncate">{dir.path}</p>
                      <p class="text-[10px] text-neutral-500">
                        {dir.tool}
                        {#if dir.has_checkpoints} · checkpoints{/if}
                        {#if dir.has_loras} · LoRAs{/if}
                        {#if dir.has_vae} · VAEs{/if}
                      </p>
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>

          <div>
            <label class="block text-xs text-neutral-400 mb-1">Extra CLI Arguments<span class="text-amber-400">*</span></label>
            <input
              type="text"
              value={config.extra_args.join(" ")}
              oninput={(e) => {
                if (config) {
                  const val = (e.target as HTMLInputElement).value;
                  config.extra_args = val ? val.split(/\s+/) : [];
                  checkRestartNeeded();
                }
              }}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
              placeholder="--fp16 --force-channels-last"
            />
            <p class="text-[10px] text-neutral-500 mt-0.5">Additional arguments passed to ComfyUI on launch</p>
          </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- Autocomplete -->
        {#if sectionVisible("autocomplete")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.autocomplete = !collapsed.autocomplete)}
          >
            Autocomplete
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.autocomplete ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.autocomplete}
          <div class="px-5 pb-5 space-y-4">
            <!-- Current source -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">Tag Source</label>
              <div class="flex items-center gap-2 text-sm text-neutral-300">
                {#if autocomplete.sourceMode === "builtin"}
                  <span class="inline-block w-2 h-2 rounded-full bg-indigo-400"></span>
                  Built-in Danbooru ({autocomplete.tags.length.toLocaleString()} tags)
                {:else if autocomplete.sourceMode === "url"}
                  <span class="inline-block w-2 h-2 rounded-full bg-green-400"></span>
                  URL: <span class="text-neutral-400 truncate max-w-xs">{autocomplete.sourceUrl}</span>
                  ({autocomplete.tags.length.toLocaleString()} tags)
                {:else if autocomplete.sourceMode === "file"}
                  <span class="inline-block w-2 h-2 rounded-full bg-green-400"></span>
                  File: {autocomplete.sourceFileName}
                  ({autocomplete.tags.length.toLocaleString()} tags)
                {/if}
              </div>
            </div>

            <!-- Load from URL -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">Load from URL</label>
              <div class="flex gap-2">
                <input
                  type="text"
                  bind:value={tagUrlInput}
                  placeholder="https://example.com/tags.json or .csv"
                  class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
                />
                <button
                  class="px-3 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
                  disabled={!tagUrlInput.trim() || autocomplete.loading}
                  onclick={() => autocomplete.loadFromUrl(tagUrlInput.trim())}
                >
                  {autocomplete.loading ? "Loading..." : "Fetch"}
                </button>
              </div>
              <p class="text-[10px] text-neutral-500 mt-0.5">JSON array or CSV (name,category,count,aliases...)</p>
            </div>

            <!-- Upload file -->
            <div>
              <label class="block text-xs text-neutral-400 mb-1">Upload File</label>
              <label
                class="inline-flex items-center gap-2 px-3 py-2 bg-neutral-800 border border-neutral-700 rounded-lg text-sm text-neutral-300 hover:border-indigo-500 transition-colors cursor-pointer"
              >
                <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                {tagFileLoading ? "Reading..." : "Choose .json or .csv"}
                <input
                  type="file"
                  accept=".json,.csv,.txt"
                  class="hidden"
                  onchange={async (e) => {
                    const input = e.target as HTMLInputElement;
                    const file = input.files?.[0];
                    if (!file) return;
                    tagFileLoading = true;
                    try {
                      const text = await file.text();
                      await autocomplete.loadFromFile(text, file.name);
                    } finally {
                      tagFileLoading = false;
                      input.value = "";
                    }
                  }}
                />
              </label>
            </div>

            <!-- Reset to built-in -->
            {#if autocomplete.sourceMode !== "builtin"}
            <button
              class="text-xs text-neutral-400 hover:text-neutral-200 underline transition-colors"
              onclick={() => autocomplete.resetToBuiltin()}
            >
              Reset to built-in Danbooru tags
            </button>
            {/if}

            <!-- Error -->
            {#if autocomplete.error}
              <div class="px-3 py-2 bg-red-900/30 border border-red-800/50 rounded-lg text-red-200 text-xs">
                {autocomplete.error}
              </div>
            {/if}

            <!-- Max results -->
            <div>
              <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
                Max Suggestions
                <span class="text-neutral-300">{autocomplete.maxResults}</span>
              </label>
              <input
                type="range"
                value={autocomplete.maxResults}
                oninput={(e) => { autocomplete.setMaxResults(parseInt((e.target as HTMLInputElement).value)); }}
                min="3"
                max="30"
                step="1"
                class="w-full accent-indigo-500"
              />
              <p class="text-[10px] text-neutral-500 mt-0.5">Number of autocomplete results shown in the dropdown</p>
            </div>

            <!-- Undo/redo hint -->
            <div class="px-3 py-2 bg-neutral-800/50 border border-neutral-700/50 rounded-lg text-[10px] text-neutral-500">
              Tip: Use <kbd class="px-1 py-0.5 bg-neutral-700 rounded text-neutral-300">Ctrl+Z</kbd> / <kbd class="px-1 py-0.5 bg-neutral-700 rounded text-neutral-300">Ctrl+Y</kbd> in the prompt box to undo/redo autocompleted tags.
            </div>
          </div>
          {/if}
        </section>
        {/if}

        <!-- About & Updates -->
        {#if sectionVisible("about")}
        <section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden">
          <button
            class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200 hover:bg-neutral-800/50 transition-colors cursor-pointer"
            onclick={() => (collapsed.about = !collapsed.about)}
          >
            About
            <svg class="w-4 h-4 text-neutral-500 transition-transform {collapsed.about ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>

          {#if !collapsed.about}
          <div class="px-5 pb-5 space-y-4">
            <div class="flex items-center justify-between">
              <div>
                <p class="text-sm text-neutral-200">MooshieUI</p>
                <p class="text-xs text-neutral-500">Version {appVersion}</p>
              </div>
            </div>

            <!-- What's New -->
            <details class="rounded-lg border border-neutral-800 bg-neutral-950 overflow-hidden">
              <summary class="px-3 py-2 text-xs font-medium text-neutral-300 hover:text-neutral-100 cursor-pointer select-none transition-colors">
                What's New in v{appVersion}
              </summary>
              <div class="px-3 pb-3 pt-1 text-xs text-neutral-400 space-y-2">
                <p class="text-neutral-300 font-medium">v0.2.7</p>
                <ul class="list-disc list-inside space-y-0.5">
                  <li>Model and Sampler are now separate, independently collapsible and draggable sections</li>
                  <li>Section collapse states persist across tab switches and page reloads</li>
                  <li>Optional manual ComfyUI startup — toggle "Auto-start" off in Connection settings</li>
                  <li>Image panning with left click and middle mouse at any zoom level</li>
                  <li>Performance: fixed event listener leak, debounced storage writes, removed backdrop-blur scroll jank</li>
                </ul>
                <p class="text-neutral-300 font-medium mt-3">v0.2.6</p>
                <ul class="list-disc list-inside space-y-0.5">
                  <li>Choose custom install location during setup (no longer forced to AppData)</li>
                  <li>Move existing installation to a new drive/folder from Settings</li>
                  <li>Auto-detect model directories from ComfyUI, A1111/Forge, SwarmUI, StabilityMatrix</li>
                  <li>Fixed Model Hub search filters (file format, base model) returning errors</li>
                  <li>Fixed infinite scroll not loading past 30 results</li>
                  <li>Removed non-functional Status filter from Model Hub</li>
                  <li>Settings sections now remember collapsed/expanded state</li>
                  <li>App no longer re-triggers setup when AppData is moved</li>
                  <li>ControlNet settings UI</li>
                  <li>Mask editor for inpainting</li>
                  <li>Download manager banner</li>
                </ul>
              </div>
            </details>

            <div class="space-y-2">
              {#if updateState === "idle"}
                <button
                  onclick={checkForUpdates}
                  class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors cursor-pointer"
                >
                  Check for Updates
                </button>

              {:else if updateState === "checking"}
                <div class="flex items-center gap-2 text-sm text-neutral-400">
                  <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
                  Checking for updates...
                </div>

              {:else if updateState === "up-to-date"}
                <div class="flex items-center gap-2 text-sm text-emerald-400">
                  <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" /></svg>
                  You're on the latest version
                </div>
                <button
                  onclick={checkForUpdates}
                  class="text-xs text-neutral-500 hover:text-neutral-300 transition-colors cursor-pointer"
                >
                  Check again
                </button>

              {:else if updateState === "available"}
                <div class="px-3 py-2 bg-indigo-900/30 border border-indigo-800/50 rounded-lg">
                  <p class="text-sm text-indigo-200 mb-2">Version <strong>{updateVersion}</strong> is available</p>
                  <button
                    onclick={downloadAndInstallUpdate}
                    class="px-4 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white rounded-lg text-sm transition-colors cursor-pointer"
                  >
                    Download & Install
                  </button>
                </div>

              {:else if updateState === "downloading"}
                <div class="px-3 py-2 bg-indigo-900/30 border border-indigo-800/50 rounded-lg space-y-2">
                  <div class="flex items-center justify-between text-xs text-neutral-400">
                    <span>Downloading v{updateVersion}...</span>
                    {#if updateTotal > 0}
                      <span class="tabular-nums">{updatePercent}%</span>
                    {/if}
                  </div>
                  <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
                    <div
                      class="bg-indigo-500 h-full rounded-full transition-all duration-300"
                      style="width: {updateTotal > 0 ? updatePercent : 33}%"
                      class:animate-pulse={updateTotal === 0}
                    ></div>
                  </div>
                </div>

              {:else if updateState === "ready"}
                <div class="px-3 py-2 bg-emerald-900/30 border border-emerald-800/50 rounded-lg">
                  <p class="text-sm text-emerald-200 mb-2">Update downloaded. Restart to apply v{updateVersion}.</p>
                  <button
                    onclick={() => relaunch()}
                    class="px-4 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg text-sm transition-colors cursor-pointer"
                  >
                    Restart Now
                  </button>
                </div>

              {:else if updateState === "error"}
                <div class="px-3 py-2 bg-red-900/30 border border-red-800/50 rounded-lg">
                  <p class="text-xs text-red-200">{updateError}</p>
                </div>
                <button
                  onclick={checkForUpdates}
                  class="text-xs text-neutral-500 hover:text-neutral-300 transition-colors cursor-pointer"
                >
                  Try again
                </button>
              {/if}
            </div>

            <div class="rounded-lg border border-neutral-800 bg-neutral-950 px-3 py-2">
              <p class="text-[11px] text-neutral-500">To install on a different drive, set the <span class="font-mono text-neutral-400">MOOSHIEUI_DATA_DIR</span> environment variable to your preferred path before launching.</p>
            </div>
          </div>
          {/if}
        </section>
        {/if}

        <p class="text-[10px] text-neutral-500"><span class="text-amber-400">*</span> Requires a restart of ComfyUI to take effect.</p>

        {#if error}
          <div class="px-3 py-2 bg-red-900/30 border border-red-800/50 rounded-lg text-red-200 text-xs">
            {error}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>
