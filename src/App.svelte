<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import SetupWizard from "./lib/components/setup/SetupWizard.svelte";
  import GenerationPage from "./lib/components/generation/GenerationPage.svelte";
  import { connection } from "./lib/stores/connection.svelte.js";
  import { progress } from "./lib/stores/progress.svelte.js";
  import { gallery } from "./lib/stores/gallery.svelte.js";
  import { models } from "./lib/stores/models.svelte.js";
  import { getHistory, getOutputImage } from "./lib/utils/api.js";
  import { generation } from "./lib/stores/generation.svelte.js";
  import type { OutputImage } from "./lib/types/index.js";

  let setupComplete = $state<boolean | null>(null); // null = loading
  let currentPage = $state<"generate" | "gallery" | "queue" | "settings">(
    "generate"
  );
  let startupStatus = $state<string>("");

  async function fetchOutputImages(promptId: string) {
    try {
      const history = (await getHistory(promptId)) as Record<string, any>;
      const promptData = history[promptId];
      if (!promptData?.outputs) return;

      const newImages: OutputImage[] = [];
      for (const [nodeId, output] of Object.entries(
        promptData.outputs as Record<string, any>
      )) {
        if (output.images) {
          for (const img of output.images) {
            const bytes = await getOutputImage(
              img.filename,
              img.subfolder || ""
            );
            const blob = new Blob([new Uint8Array(bytes)], {
              type: "image/png",
            });
            const url = URL.createObjectURL(blob);
            newImages.push({
              filename: img.filename,
              subfolder: img.subfolder || "",
              type: img.type || "output",
              prompt_id: promptId,
              url,
            });
          }
        }
      }
      if (newImages.length > 0) {
        gallery.addImages(newImages);
        // Show the first output image in the preview area
        progress.lastOutputImage = newImages[0].url;
        // Persist to disk gallery
        gallery.persistImages(newImages);
      }
    } catch (e) {
      console.error("Failed to fetch output images:", e);
    }
  }

  onMount(async () => {
    // Check if first-run setup is needed
    try {
      setupComplete = await invoke<boolean>("check_setup");
    } catch {
      setupComplete = false;
    }

    if (!setupComplete) return;

    // Setup already done — initialize the main app
    await initApp();
  });

  async function onSetupDone() {
    setupComplete = true;
    await initApp();
  }

  async function initApp() {
    // Load persisted settings
    await generation.loadSettings();

    // Set up event listeners BEFORE starting so we don't miss events
    await Promise.all([
      listen("comfyui:connection", (event: any) => {
        console.log("Connection event:", event.payload);
        connection.connected = event.payload.connected;
        if (event.payload.connected) {
          startupStatus = "";
          models.refresh().then(() => {
            generation.applyDefaultsIfNeeded(models.checkpoints, models.vaes);
          });
        }
      }),
      listen("comfyui:server_ready", async () => {
        console.log("Server ready event received");
        startupStatus = "";
        // Load models now that server is up
        try {
          await models.refresh();
          console.log("Models loaded:", models.checkpoints);
          if (models.checkpoints.length > 0) {
            connection.connected = true;
            generation.applyDefaultsIfNeeded(models.checkpoints, models.vaes);
          }
        } catch (e) {
          console.error("Model refresh failed after server ready:", e);
        }
      }),
      listen("comfyui:server_error", (event: any) => {
        console.error("Server error:", event.payload);
        startupStatus = `Failed to start: ${event.payload?.error || "unknown error"}`;
      }),
      listen("comfyui:progress", (event: any) => {
        const data = event.payload;
        if (!progress.isGenerating) return;
        progress.currentStep = data.value;
        progress.totalSteps = data.max;
      }),
      listen("comfyui:preview", (event: any) => {
        const data = event.payload;
        if (!progress.isGenerating) return;
        progress.previewImage = `data:image/${data.format};base64,${data.image}`;
      }),
      listen("comfyui:executing", (event: any) => {
        const data = event.payload;
        console.log("Executing event:", data);
        // Ignore events not for our current generation
        if (data.prompt_id && progress.currentPromptId && data.prompt_id !== progress.currentPromptId) {
          return;
        }
        if (data.node === null) {
          // Only handle completion if we're actually generating
          if (!progress.isGenerating) return;
          const promptId = progress.currentPromptId;
          progress.reset();
          if (promptId) {
            fetchOutputImages(promptId);
          }
        } else {
          if (progress.isGenerating) {
            progress.currentNode = data.node;
          }
        }
      }),
      listen("comfyui:execution_error", (event: any) => {
        console.error("Execution error:", event.payload);
        // Only reset if this is our prompt
        const data = event.payload;
        if (data.prompt_id && progress.currentPromptId && data.prompt_id !== progress.currentPromptId) {
          return;
        }
        progress.reset();
      }),
      listen("comfyui:execution_success", (_event: any) => {
        // Success handled via executing node=null
      }),
    ]);

    // Start ComfyUI server — returns immediately, background task handles readiness
    // The backend will auto-connect WebSocket and emit comfyui:server_ready when done
    try {
      console.log("Starting ComfyUI...");
      const result = await invoke<string>("start_comfyui");
      console.log("start_comfyui returned:", result);
      if (result === "spawned") {
        startupStatus = "Starting ComfyUI...";
      } else if (result === "already_running") {
        startupStatus = "Connecting...";
      }
    } catch (e) {
      console.error("Failed to start ComfyUI:", e);
      startupStatus = `Failed to start: ${e}`;
    }

    // Load persisted gallery images from disk (independent of server status)
    gallery.loadFromDisk();
  }
</script>

{#if setupComplete === null}
  <!-- Loading state -->
  <div class="flex items-center justify-center h-full bg-neutral-950">
    <div
      class="w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin"
    ></div>
  </div>
{:else if !setupComplete}
  <SetupWizard onSetupComplete={onSetupDone} />
{:else}
<div class="flex h-full bg-neutral-950 text-neutral-100">
  <!-- Sidebar -->
  <nav
    class="flex flex-col w-14 bg-neutral-900 border-r border-neutral-800 items-center py-4 gap-2"
  >
    <button
      class="w-10 h-10 rounded-lg flex items-center justify-center transition-colors {currentPage ===
      'generate'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}"
      onclick={() => (currentPage = "generate")}
      title="Generate"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-5 h-5"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        ><path d="M12 19l7-7 3 3-7 7-3-3z" /><path
          d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"
        /><path d="M2 2l7.586 7.586" /><circle cx="11" cy="11" r="2" /></svg
      >
    </button>
    <button
      class="w-10 h-10 rounded-lg flex items-center justify-center transition-colors {currentPage ===
      'gallery'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}"
      onclick={() => (currentPage = "gallery")}
      title="Gallery"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-5 h-5"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        ><rect x="3" y="3" width="7" height="7" /><rect
          x="14"
          y="3"
          width="7"
          height="7"
        /><rect x="3" y="14" width="7" height="7" /><rect
          x="14"
          y="14"
          width="7"
          height="7"
        /></svg
      >
    </button>
    <button
      class="w-10 h-10 rounded-lg flex items-center justify-center transition-colors {currentPage ===
      'queue'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}"
      onclick={() => (currentPage = "queue")}
      title="Queue"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-5 h-5"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        ><line x1="8" y1="6" x2="21" y2="6" /><line
          x1="8"
          y1="12"
          x2="21"
          y2="12"
        /><line x1="8" y1="18" x2="21" y2="18" /><line
          x1="3"
          y1="6"
          x2="3.01"
          y2="6"
        /><line x1="3" y1="12" x2="3.01" y2="12" /><line
          x1="3"
          y1="18"
          x2="3.01"
          y2="18"
        /></svg
      >
    </button>

    <div class="flex-1"></div>

    <button
      class="w-10 h-10 rounded-lg flex items-center justify-center transition-colors {currentPage ===
      'settings'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}"
      onclick={() => (currentPage = "settings")}
      title="Settings"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-5 h-5"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        ><circle cx="12" cy="12" r="3" /><path
          d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"
        /></svg
      >
    </button>

    <!-- Connection status dot -->
    <div
      class="w-3 h-3 rounded-full mb-2 transition-colors {connection.connected
        ? 'bg-green-500'
        : startupStatus
          ? 'bg-amber-500 animate-pulse'
          : 'bg-red-500'}"
      title={connection.connected ? "Connected" : startupStatus || "Disconnected"}
    ></div>
  </nav>

  <!-- Main content -->
  <main class="flex-1 overflow-hidden flex flex-col">
    {#if startupStatus && !connection.connected}
      <div class="flex items-center gap-2 px-4 py-2 bg-amber-900/30 border-b border-amber-800/50 text-amber-200 text-sm">
        <div class="w-4 h-4 border-2 border-amber-400 border-t-transparent rounded-full animate-spin"></div>
        {startupStatus}
      </div>
    {/if}
    <div class="flex-1 overflow-hidden">
    {#if currentPage === "generate"}
      <GenerationPage />
    {:else if currentPage === "gallery"}
      <div class="p-6 h-full overflow-y-auto">
        {#if gallery.loading}
          <div class="flex items-center justify-center h-full text-neutral-500">
            Loading gallery...
          </div>
        {:else if gallery.images.length === 0}
          <div
            class="flex items-center justify-center h-full text-neutral-500"
          >
            No images yet. Generate some!
          </div>
        {:else}
          <div class="grid grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
            {#each gallery.images as image}
              <div class="group relative aspect-square rounded-lg overflow-hidden border border-neutral-800 hover:border-indigo-500 transition-colors">
                <button
                  class="w-full h-full"
                  onclick={() => gallery.openLightbox(image)}
                >
                  {#if image.url}
                    <img
                      src={image.url}
                      alt={image.filename}
                      class="w-full h-full object-cover"
                    />
                  {/if}
                </button>
                <!-- Hover actions -->
                <div class="absolute top-1 right-1 flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                  <button
                    class="w-7 h-7 flex items-center justify-center rounded bg-neutral-900/80 hover:bg-neutral-700 text-neutral-300 text-xs backdrop-blur-sm"
                    title="Save As"
                    onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  </button>
                  <button
                    class="w-7 h-7 flex items-center justify-center rounded bg-neutral-900/80 hover:bg-neutral-700 text-neutral-300 text-xs backdrop-blur-sm"
                    title="Copy"
                    onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                  </button>
                  <button
                    class="w-7 h-7 flex items-center justify-center rounded bg-red-900/60 hover:bg-red-800 text-neutral-300 text-xs backdrop-blur-sm"
                    title="Delete"
                    onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {:else if currentPage === "queue"}
      <div class="flex items-center justify-center h-full text-neutral-500">
        Queue management (coming soon)
      </div>
    {:else if currentPage === "settings"}
      <div class="flex items-center justify-center h-full text-neutral-500">
        Settings (coming soon)
      </div>
    {/if}
    </div>
  </main>
</div>
{/if}

<!-- Lightbox overlay -->
{#if gallery.lightboxOpen && gallery.selectedImage}
  <div
    class="fixed inset-0 bg-black/90 z-50 flex items-center justify-center"
    role="dialog"
  >
    <!-- Close button -->
    <button
      class="absolute top-4 right-4 text-white text-2xl hover:text-neutral-300 z-10"
      onclick={() => gallery.closeLightbox()}
    >
      &times;
    </button>

    <!-- Action buttons -->
    <div class="absolute bottom-6 left-1/2 -translate-x-1/2 flex gap-3 z-10">
      <button
        class="flex items-center gap-2 px-4 py-2 bg-neutral-800/80 hover:bg-neutral-700 text-neutral-100 rounded-lg text-sm backdrop-blur-sm transition-colors"
        onclick={() => gallery.selectedImage && gallery.saveImageAs(gallery.selectedImage)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
        Save As
      </button>
      <button
        class="flex items-center gap-2 px-4 py-2 bg-neutral-800/80 hover:bg-neutral-700 text-neutral-100 rounded-lg text-sm backdrop-blur-sm transition-colors"
        onclick={() => gallery.selectedImage && gallery.copyToClipboard(gallery.selectedImage)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
        Copy
      </button>
      <button
        class="flex items-center gap-2 px-4 py-2 bg-red-900/60 hover:bg-red-800 text-neutral-100 rounded-lg text-sm backdrop-blur-sm transition-colors"
        onclick={() => gallery.selectedImage && gallery.deleteImage(gallery.selectedImage)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        Delete
      </button>
    </div>

    {#if gallery.selectedImage.url}
      <img
        src={gallery.selectedImage.url}
        alt={gallery.selectedImage.filename}
        class="max-w-[90vw] max-h-[85vh] object-contain"
      />
    {/if}
  </div>
{/if}

<!-- Toast notification -->
{#if gallery.toastMessage}
  <div class="fixed bottom-6 left-1/2 -translate-x-1/2 z-[60] px-4 py-2 bg-neutral-800 text-neutral-100 text-sm rounded-lg shadow-lg border border-neutral-700 animate-fade-in">
    {gallery.toastMessage}
  </div>
{/if}
