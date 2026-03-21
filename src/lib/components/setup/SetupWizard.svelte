<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import logo from "../../assets/logo.png";

  let {
    onSetupComplete,
  }: {
    onSetupComplete: () => void;
  } = $props();

  let phase = $state<"detecting" | "ready" | "installing" | "done" | "error">(
    "detecting"
  );
  let gpu = $state("cpu");
  let detectedGpu = $state("cpu");
  let gpuLabel = $derived(
    gpu === "nvidia"
      ? "NVIDIA GPU (CUDA)"
      : gpu === "amd"
        ? "AMD GPU (ROCm)"
        : gpu === "intel"
          ? "Intel Arc GPU (XPU)"
          : gpu === "mps"
            ? "Apple Silicon (Metal)"
            : "CPU only"
  );
  let progressMessage = $state("Preparing...");
  let progressPercent = $state(0);
  let errorMessage = $state("");

  // Install location
  let defaultInstallPath = $state("");
  let customInstallPath = $state("");
  let useCustomPath = $state(false);
  let installPath = $derived(useCustomPath && customInstallPath.trim() ? customInstallPath.trim() : "");

  // Detected model directories from other AI tools
  interface DetectedModelDir {
    path: string;
    tool: string;
    has_checkpoints: boolean;
    has_loras: boolean;
    has_vae: boolean;
  }
  let detectedModelDirs = $state<DetectedModelDir[]>([]);
  let selectedModelDirs = $state<Set<string>>(new Set());
  let scanningModels = $state(false);

  // Terminal log lines streamed from backend
  let logLines = $state<string[]>([]);
  let logContainer: HTMLDivElement | undefined = $state();

  // Per-step tracking
  const steps = [
    { id: "uv", label: "Download uv" },
    { id: "python", label: "Install Python 3.11" },
    { id: "comfyui", label: "Download ComfyUI" },
    { id: "venv", label: "Create virtual environment" },
    { id: "pytorch", label: "Install PyTorch" },
    { id: "deps", label: "Install dependencies" },
    { id: "nodes", label: "Install custom nodes" },
    { id: "config", label: "Configure system" },
  ];
  let currentStep = $state("");
  let completedSteps = $state<Set<string>>(new Set());

  // Download progress
  let downloadFilename = $state("");
  let downloadedBytes = $state(0);
  let downloadTotalBytes = $state(0);

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  const downloadPercent = $derived(
    downloadTotalBytes > 0
      ? Math.round((downloadedBytes / downloadTotalBytes) * 100)
      : 0
  );

  onMount(async () => {
    // Detect GPU and get default install path in parallel
    const [detectedGpuResult, installPathResult] = await Promise.allSettled([
      invoke<string>("detect_gpu"),
      invoke<string>("get_install_path"),
    ]);

    if (detectedGpuResult.status === "fulfilled") {
      gpu = detectedGpuResult.value;
      detectedGpu = detectedGpuResult.value;
    }
    if (installPathResult.status === "fulfilled") {
      defaultInstallPath = installPathResult.value;
    }

    // Scan for existing model directories in background
    scanningModels = true;
    invoke<DetectedModelDir[]>("detect_model_directories")
      .then((dirs) => {
        detectedModelDirs = dirs;
      })
      .catch(() => {
        detectedModelDirs = [];
      })
      .finally(() => {
        scanningModels = false;
      });

    phase = "ready";

    // Listen for progress events
    await listen("setup:progress", (event: any) => {
      const data = event.payload as {
        step: string;
        message: string;
        percent: number;
      };
      // Mark previous step as completed
      if (currentStep && currentStep !== data.step) {
        completedSteps = new Set([...completedSteps, currentStep]);
      }
      currentStep = data.step;
      progressMessage = data.message;
      progressPercent = data.percent;
      if (data.step === "done") {
        completedSteps = new Set([...completedSteps, "config"]);
        phase = "done";
        setTimeout(() => onSetupComplete(), 1500);
      }
    });

    // Listen for terminal log lines
    await listen("setup:log", (event: any) => {
      const line = event.payload as string;
      logLines = [...logLines, line];
      // Auto-scroll
      requestAnimationFrame(() => {
        if (logContainer) {
          logContainer.scrollTop = logContainer.scrollHeight;
        }
      });
    });

    // Listen for download progress
    await listen("download:progress", (event: any) => {
      const data = event.payload as {
        filename: string;
        downloaded: number;
        total: number;
        done: boolean;
      };
      if (data.done) {
        downloadFilename = "";
        downloadedBytes = 0;
        downloadTotalBytes = 0;
      } else {
        downloadFilename = data.filename;
        downloadedBytes = data.downloaded;
        downloadTotalBytes = data.total;
      }
    });
  });

  const gpuOptions = [
    { value: "nvidia", label: "NVIDIA GPU (CUDA)", icon: "🟢", color: "bg-green-900/50 text-green-400" },
    { value: "amd", label: "AMD GPU (ROCm)", icon: "🔴", color: "bg-red-900/50 text-red-400" },
    { value: "intel", label: "Intel Arc GPU (XPU)", icon: "🔵", color: "bg-blue-900/50 text-blue-400" },
    { value: "cpu", label: "CPU only", icon: "⚪", color: "bg-neutral-700 text-neutral-400" },
  ];

  async function browseInstallPath() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choose Install Location",
    });
    if (selected && typeof selected === "string") {
      customInstallPath = selected;
      useCustomPath = true;
    }
  }

  function toggleModelDir(path: string) {
    const next = new Set(selectedModelDirs);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
    }
    selectedModelDirs = next;
  }

  async function startInstall() {
    phase = "installing";
    progressPercent = 0;
    progressMessage = "Starting installation...";
    logLines = [];
    completedSteps = new Set();
    currentStep = "";
    try {
      await invoke("run_setup", {
        gpuType: gpu,
        installPath: installPath || null,
      });

      // If user selected model directories, save them to config
      if (selectedModelDirs.size > 0) {
        try {
          const modelPaths = [...selectedModelDirs].join("\n");
          const config = await invoke<any>("get_config");
          await invoke("update_config", {
            config: { ...config, extra_model_paths: modelPaths },
          });
        } catch (e) {
          // Non-fatal: models can be configured later in settings
          console.warn("Failed to save model directories:", e);
        }
      }
    } catch (e: any) {
      phase = "error";
      errorMessage = typeof e === "string" ? e : e.message || "Unknown error";
    }
  }

  function retry() {
    phase = "ready";
    errorMessage = "";
  }

  function stepStatus(stepId: string): "done" | "active" | "pending" {
    if (completedSteps.has(stepId)) return "done";
    if (currentStep === stepId) return "active";
    return "pending";
  }
</script>

<div class="relative flex items-center justify-center h-full bg-neutral-950 text-neutral-100 overflow-hidden">
  <!-- Terminal background overlay (visible during installation) -->
  {#if phase === "installing" || phase === "done" || phase === "error"}
    <div
      bind:this={logContainer}
      class="absolute inset-0 overflow-y-auto p-4 pt-6 font-mono text-[11px] leading-relaxed text-green-500/25 pointer-events-none select-none"
      aria-hidden="true"
    >
      {#each logLines as line}
        <div class="whitespace-pre-wrap break-all">{line}</div>
      {/each}
    </div>
    <!-- Darkening overlay so the UI stays readable -->
    <div class="absolute inset-0 bg-neutral-950/70 pointer-events-none"></div>
  {/if}

  <!-- Main content (on top of terminal) -->
  <div class="relative z-10 max-w-lg w-full mx-4 max-h-[95vh] overflow-y-auto">
    <!-- Logo / Title -->
    <div class="text-center mb-8">
      <img
        src={logo}
        alt="MooshieUI logo"
        class="w-16 h-16 object-contain mx-auto mb-3 rounded-xl border border-neutral-700 bg-neutral-800/40 p-1"
      />
      <h1 class="text-4xl font-bold bg-linear-to-r from-indigo-400 to-purple-400 bg-clip-text text-transparent">
        MooshieUI
      </h1>
      <p class="text-neutral-400 mt-2 text-sm">
        Beginner-friendly AI image generation
      </p>
    </div>

    <div class="bg-neutral-900 rounded-xl border border-neutral-800 p-6">
      {#if phase === "detecting"}
        <div class="text-center py-8">
          <div
            class="w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin mx-auto"
          ></div>
          <p class="text-neutral-400 mt-4">Detecting your hardware...</p>
        </div>
      {:else if phase === "ready"}
        <h2 class="text-xl font-semibold mb-4">Welcome! Let's get set up.</h2>
        <p class="text-neutral-400 text-sm mb-6">
          MooshieUI will automatically install everything you need — ComfyUI,
          Python, and the right AI libraries for your hardware. No manual setup
          required.
        </p>

        <!-- GPU Selection -->
        <div class="mb-6">
          {#if gpu === "mps"}
            <div class="bg-neutral-800 rounded-lg p-4">
              <div class="flex items-center gap-3">
                <div class="w-8 h-8 rounded-lg flex items-center justify-center text-sm bg-blue-900/50 text-blue-400">🔵</div>
                <div>
                  <p class="text-sm font-medium text-neutral-200">Apple Silicon (Metal)</p>
                  <p class="text-xs text-neutral-500">Optimized acceleration will be configured</p>
                </div>
              </div>
            </div>
          {:else}
            <p class="text-xs text-neutral-400 mb-2">Select your GPU type:</p>
            <div class="space-y-1.5">
              {#each gpuOptions as opt}
                <button
                  type="button"
                  onclick={() => gpu = opt.value}
                  class="w-full flex items-center gap-3 rounded-lg p-3 text-left transition-colors cursor-pointer {gpu === opt.value
                    ? 'bg-indigo-600/15 border border-indigo-500/50'
                    : 'bg-neutral-800 border border-neutral-700/50 hover:border-neutral-600'}"
                >
                  <div class="w-8 h-8 rounded-lg flex items-center justify-center text-sm {opt.color}">
                    {opt.icon}
                  </div>
                  <div class="flex-1">
                    <p class="text-sm font-medium {gpu === opt.value ? 'text-indigo-300' : 'text-neutral-200'}">{opt.label}</p>
                  </div>
                  {#if opt.value === detectedGpu}
                    <span class="text-[10px] px-1.5 py-0.5 rounded bg-neutral-700/50 text-neutral-400">
                      detected
                    </span>
                  {/if}
                </button>
              {/each}
            </div>
            {#if gpu === "cpu"}
              <p class="text-xs text-amber-400/70 mt-2">Generation will be significantly slower without GPU acceleration.</p>
            {/if}
          {/if}
        </div>

        <!-- Install Location -->
        <div class="mb-6 rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 space-y-2">
          <div class="flex items-center justify-between">
            <p class="text-xs text-neutral-400">Install Location</p>
            <button
              type="button"
              class="text-[10px] px-1.5 py-0.5 rounded border transition-colors cursor-pointer {useCustomPath
                ? 'border-indigo-500/50 text-indigo-300'
                : 'border-neutral-700 text-neutral-500 hover:text-neutral-300 hover:border-neutral-500'}"
              onclick={() => {
                useCustomPath = !useCustomPath;
                if (!useCustomPath) customInstallPath = "";
              }}
            >
              {useCustomPath ? "Use default" : "Change"}
            </button>
          </div>

          {#if useCustomPath}
            <div class="flex gap-1.5">
              <input
                type="text"
                bind:value={customInstallPath}
                class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500"
                placeholder="Choose a folder..."
              />
              <button
                type="button"
                onclick={browseInstallPath}
                class="px-3 py-2 rounded-lg border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors text-xs cursor-pointer"
              >
                Browse
              </button>
            </div>
            <p class="text-[10px] text-neutral-600">Pick any drive or folder. All app data (~5-10 GB) will be stored here.</p>
          {:else}
            <p class="text-xs text-neutral-500 font-mono truncate" title={defaultInstallPath}>{defaultInstallPath || "Loading..."}</p>
          {/if}
        </div>

        <!-- Detected Model Directories -->
        {#if detectedModelDirs.length > 0}
          <div class="mb-6 rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 space-y-2">
            <p class="text-xs text-neutral-400">Existing model directories detected</p>
            <p class="text-[10px] text-neutral-600">Select directories to share models from other AI tools (no files will be copied).</p>
            <div class="space-y-1">
              {#each detectedModelDirs as dir}
                <button
                  type="button"
                  class="w-full flex items-start gap-2 rounded-lg p-2 text-left transition-colors cursor-pointer {selectedModelDirs.has(dir.path)
                    ? 'bg-indigo-600/15 border border-indigo-500/50'
                    : 'bg-neutral-800/50 border border-neutral-700/50 hover:border-neutral-600'}"
                  onclick={() => toggleModelDir(dir.path)}
                >
                  <div class="mt-0.5 w-3.5 h-3.5 rounded border shrink-0 flex items-center justify-center {selectedModelDirs.has(dir.path) ? 'border-indigo-500 bg-indigo-600' : 'border-neutral-600'}">
                    {#if selectedModelDirs.has(dir.path)}
                      <svg class="w-2.5 h-2.5 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                    {/if}
                  </div>
                  <div class="flex-1 min-w-0">
                    <p class="text-xs text-neutral-200 truncate" title={dir.path}>{dir.path}</p>
                    <p class="text-[10px] text-neutral-500">
                      {dir.tool}
                      {#if dir.has_checkpoints} · checkpoints{/if}
                      {#if dir.has_loras} · LoRAs{/if}
                      {#if dir.has_vae} · VAEs{/if}
                    </p>
                  </div>
                </button>
              {/each}
            </div>
          </div>
        {:else if scanningModels}
          <div class="mb-6 rounded-lg border border-neutral-800 bg-neutral-950/50 p-3">
            <p class="text-[10px] text-neutral-600">Scanning for existing model directories...</p>
          </div>
        {/if}

        <div class="text-xs text-neutral-500 mb-4 space-y-1">
          <p>This will install:</p>
          <ul class="list-disc list-inside ml-2 space-y-0.5">
            <li>uv package manager</li>
            <li>Python 3.11</li>
            <li>ComfyUI (latest)</li>
            <li>PyTorch ({gpuLabel})</li>
            <li>MooshieUI custom nodes</li>
          </ul>
          <p class="mt-2 text-neutral-600">
            ~5-10 GB disk space required. Installation may take 5-15 minutes
            depending on your connection.
          </p>
        </div>

        <button
          onclick={startInstall}
          class="w-full py-3 bg-indigo-600 hover:bg-indigo-500 rounded-lg font-semibold transition-colors cursor-pointer"
        >
          Install & Get Started
        </button>
      {:else if phase === "installing"}
        <h2 class="text-xl font-semibold mb-4">Installing...</h2>

        <!-- Step checklist -->
        <div class="space-y-1.5 mb-5">
          {#each steps as step}
            {@const status = stepStatus(step.id)}
            <div class="flex items-center gap-2.5 text-xs">
              {#if status === "done"}
                <div class="w-4 h-4 rounded-full bg-green-600 flex items-center justify-center shrink-0">
                  <svg class="w-2.5 h-2.5 text-white" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                </div>
                <span class="text-neutral-500 line-through">{step.label}</span>
              {:else if status === "active"}
                <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin shrink-0"></div>
                <span class="text-indigo-300 font-medium">{step.label}</span>
              {:else}
                <div class="w-4 h-4 rounded-full border border-neutral-700 shrink-0"></div>
                <span class="text-neutral-600">{step.label}</span>
              {/if}
            </div>
          {/each}
        </div>

        <!-- Overall progress bar -->
        <div class="mb-1">
          <div class="flex items-center justify-between text-xs text-neutral-500 mb-1">
            <span>{progressMessage}</span>
            <span>{progressPercent}%</span>
          </div>
          <div class="w-full bg-neutral-800 rounded-full h-2.5 overflow-hidden">
            <div
              class="bg-indigo-500 h-full rounded-full transition-[width] duration-500 ease-out"
              style="width: {progressPercent}%"
            ></div>
          </div>
        </div>

        <!-- Download progress (when actively downloading a file) -->
        {#if downloadFilename && downloadTotalBytes > 0}
          <div class="mt-3 bg-neutral-800/80 rounded-lg px-3 py-2">
            <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-1">
              <span class="truncate mr-2">{downloadFilename}</span>
              <span class="shrink-0 tabular-nums">{formatBytes(downloadedBytes)} / {formatBytes(downloadTotalBytes)} ({downloadPercent}%)</span>
            </div>
            <div class="w-full bg-neutral-700 rounded-full h-1.5 overflow-hidden">
              <div
                class="bg-indigo-400 h-full rounded-full transition-[width] duration-300 ease-out"
                style="width: {downloadPercent}%"
              ></div>
            </div>
          </div>
        {/if}

        <p class="text-xs text-neutral-600 mt-4">
          Please don't close the app during installation.
        </p>
      {:else if phase === "done"}
        <div class="text-center py-8">
          <div class="text-5xl mb-4">&#10003;</div>
          <h2 class="text-xl font-semibold">All set!</h2>
          <p class="text-neutral-400 text-sm mt-2">
            Starting ComfyUI server...
          </p>
        </div>
      {:else if phase === "error"}
        <div class="text-center py-4">
          <div class="text-4xl mb-3">&#10007;</div>
          <h2 class="text-xl font-semibold mb-2">Installation Failed</h2>
          <div
            class="bg-red-950/50 border border-red-800 rounded-lg p-3 mb-4 text-left"
          >
            <p class="text-red-300 text-sm font-mono break-all">
              {errorMessage}
            </p>
          </div>

          <!-- Show last few log lines for context -->
          {#if logLines.length > 0}
            <div class="bg-neutral-900 border border-neutral-800 rounded-lg p-3 mb-4 text-left max-h-32 overflow-y-auto">
              <p class="text-[10px] text-neutral-500 mb-1">Last output:</p>
              {#each logLines.slice(-10) as line}
                <p class="text-[11px] text-neutral-400 font-mono break-all">{line}</p>
              {/each}
            </div>
          {/if}

          <button
            onclick={retry}
            class="px-6 py-2 bg-neutral-800 hover:bg-neutral-700 rounded-lg text-sm transition-colors cursor-pointer"
          >
            Retry
          </button>
        </div>
      {/if}
    </div>

    <p class="text-center text-xs text-neutral-700 mt-4">
      MooshieUI — A friendly face for ComfyUI
    </p>
  </div>
</div>
