<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
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
  let gpuLabel = $derived(
    gpu === "nvidia"
      ? "NVIDIA GPU (CUDA)"
      : gpu === "amd"
        ? "AMD GPU (ROCm)"
        : gpu === "mps"
          ? "Apple Silicon (Metal)"
          : "CPU only"
  );
  let progressMessage = $state("Preparing...");
  let progressPercent = $state(0);
  let errorMessage = $state("");

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
    // Detect GPU
    try {
      gpu = await invoke<string>("detect_gpu");
    } catch {
      gpu = "cpu";
    }
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

  async function startInstall() {
    phase = "installing";
    progressPercent = 0;
    progressMessage = "Starting installation...";
    logLines = [];
    completedSteps = new Set();
    currentStep = "";
    try {
      await invoke("run_setup");
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
  <div class="relative z-10 max-w-lg w-full mx-4">
    <!-- Logo / Title -->
    <div class="text-center mb-8">
      <img
        src={logo}
        alt="MooshieUI logo"
        class="w-16 h-16 object-contain mx-auto mb-3 rounded-xl border border-neutral-700 bg-neutral-800/40 p-1"
      />
      <h1 class="text-4xl font-bold bg-gradient-to-r from-indigo-400 to-purple-400 bg-clip-text text-transparent">
        MooshieUI
      </h1>
      <p class="text-neutral-400 mt-2 text-sm">
        Beginner-friendly AI image generation
      </p>
    </div>

    <div class="bg-neutral-900/95 backdrop-blur-sm rounded-xl border border-neutral-800 p-6">
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

        <div class="bg-neutral-800 rounded-lg p-4 mb-6">
          <div class="flex items-center gap-3">
            <div
              class="w-10 h-10 rounded-lg flex items-center justify-center text-lg
              {gpu === 'nvidia'
                ? 'bg-green-900/50 text-green-400'
                : gpu === 'amd'
                  ? 'bg-red-900/50 text-red-400'
                  : gpu === 'mps'
                    ? 'bg-blue-900/50 text-blue-400'
                    : 'bg-neutral-700 text-neutral-400'}"
            >
              {gpu === "nvidia"
                ? "🟢"
                : gpu === "amd"
                  ? "🔴"
                  : gpu === "mps"
                    ? "🔵"
                    : "⚪"}
            </div>
            <div>
              <p class="font-medium">{gpuLabel}</p>
              <p class="text-xs text-neutral-500">
                {gpu === "cpu"
                  ? "Generation will be slower without a GPU"
                  : "Optimized acceleration will be configured"}
              </p>
            </div>
          </div>
        </div>

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
              class="bg-indigo-500 h-full rounded-full transition-all duration-500 ease-out"
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
                class="bg-indigo-400 h-full rounded-full transition-all duration-300 ease-out"
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
