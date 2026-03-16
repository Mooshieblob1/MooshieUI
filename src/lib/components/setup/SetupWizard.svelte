<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

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
      progressMessage = data.message;
      progressPercent = data.percent;
      if (data.step === "done") {
        phase = "done";
        setTimeout(() => onSetupComplete(), 1500);
      }
    });
  });

  async function startInstall() {
    phase = "installing";
    progressPercent = 0;
    progressMessage = "Starting installation...";
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
</script>

<div class="flex items-center justify-center h-full bg-neutral-950 text-neutral-100">
  <div class="max-w-lg w-full mx-4">
    <!-- Logo / Title -->
    <div class="text-center mb-8">
      <h1 class="text-4xl font-bold bg-gradient-to-r from-indigo-400 to-purple-400 bg-clip-text text-transparent">
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
            ~5–10 GB disk space required. Installation may take 5–15 minutes
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
        <p class="text-neutral-400 text-sm mb-4">{progressMessage}</p>

        <!-- Progress bar -->
        <div class="w-full bg-neutral-800 rounded-full h-3 mb-2 overflow-hidden">
          <div
            class="bg-indigo-500 h-full rounded-full transition-all duration-500 ease-out"
            style="width: {progressPercent}%"
          ></div>
        </div>
        <p class="text-xs text-neutral-500 text-right">{progressPercent}%</p>

        <p class="text-xs text-neutral-600 mt-4">
          Please don't close the app during installation.
        </p>
      {:else if phase === "done"}
        <div class="text-center py-8">
          <div class="text-5xl mb-4">✅</div>
          <h2 class="text-xl font-semibold">All set!</h2>
          <p class="text-neutral-400 text-sm mt-2">
            Starting ComfyUI server...
          </p>
        </div>
      {:else if phase === "error"}
        <div class="text-center py-4">
          <div class="text-4xl mb-3">❌</div>
          <h2 class="text-xl font-semibold mb-2">Installation Failed</h2>
          <div
            class="bg-red-950/50 border border-red-800 rounded-lg p-3 mb-4 text-left"
          >
            <p class="text-red-300 text-sm font-mono break-all">
              {errorMessage}
            </p>
          </div>
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
      MooshieUI v0.1.0 — A friendly face for ComfyUI
    </p>
  </div>
</div>
