<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import GenerationPage from "./lib/components/generation/GenerationPage.svelte";
  import { connection } from "./lib/stores/connection.svelte.js";
  import { progress } from "./lib/stores/progress.svelte.js";
  import { gallery } from "./lib/stores/gallery.svelte.js";
  import { models } from "./lib/stores/models.svelte.js";
  import { connectWs, getHistory, getOutputImage } from "./lib/utils/api.js";
  import { generation } from "./lib/stores/generation.svelte.js";
  import type { OutputImage } from "./lib/types/index.js";

  let currentPage = $state<"generate" | "gallery" | "queue" | "settings">(
    "generate"
  );

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
      }
    } catch (e) {
      console.error("Failed to fetch output images:", e);
    }
  }

  onMount(async () => {
    // Load persisted settings
    await generation.loadSettings();

    // Set up event listeners BEFORE connecting so we don't miss events
    const unlisteners = await Promise.all([
      listen("comfyui:connection", (event: any) => {
        console.log("Connection event:", event.payload);
        connection.connected = event.payload.connected;
        if (event.payload.connected) {
          models.refresh();
        }
      }),
      listen("comfyui:progress", (event: any) => {
        const data = event.payload;
        progress.currentStep = data.value;
        progress.totalSteps = data.max;
      }),
      listen("comfyui:preview", (event: any) => {
        const data = event.payload;
        progress.previewImage = `data:image/${data.format};base64,${data.image}`;
      }),
      listen("comfyui:executing", (event: any) => {
        const data = event.payload;
        console.log("Executing event:", data);
        if (data.node === null) {
          const promptId = progress.currentPromptId;
          progress.reset();
          if (promptId) {
            fetchOutputImages(promptId);
          }
        } else {
          progress.currentNode = data.node;
        }
      }),
      listen("comfyui:execution_error", (event: any) => {
        console.error("Execution error:", event.payload);
        progress.reset();
      }),
      listen("comfyui:execution_success", (_event: any) => {
        // Success handled via executing node=null
      }),
    ]);

    // Connect WebSocket (this emits comfyui:connection on success)
    try {
      console.log("Connecting WebSocket...");
      await connectWs();
      console.log("WebSocket connect_ws returned successfully");
    } catch (e) {
      console.error("Failed to connect WebSocket:", e);
    }

    // Also try loading models directly in case the WS event didn't fire
    try {
      console.log("Attempting initial model refresh...");
      await models.refresh();
      console.log("Models loaded:", models.checkpoints);
      if (models.checkpoints.length > 0) {
        connection.connected = true;
      }
    } catch (e) {
      console.error("Initial model refresh failed:", e);
    }

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  });
</script>

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
        : 'bg-red-500'}"
      title={connection.connected ? "Connected" : "Disconnected"}
    ></div>
  </nav>

  <!-- Main content -->
  <main class="flex-1 overflow-hidden">
    {#if currentPage === "generate"}
      <GenerationPage />
    {:else if currentPage === "gallery"}
      <div class="p-6">
        {#if gallery.images.length === 0}
          <div
            class="flex items-center justify-center h-full text-neutral-500"
          >
            No images yet. Generate some!
          </div>
        {:else}
          <div class="grid grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
            {#each gallery.images as image}
              <button
                class="aspect-square rounded-lg overflow-hidden border border-neutral-800 hover:border-indigo-500 transition-colors"
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
  </main>
</div>

<!-- Lightbox overlay -->
{#if gallery.lightboxOpen && gallery.selectedImage}
  <div
    class="fixed inset-0 bg-black/90 z-50 flex items-center justify-center"
    role="dialog"
  >
    <button
      class="absolute top-4 right-4 text-white text-2xl hover:text-neutral-300"
      onclick={() => gallery.closeLightbox()}
    >
      &times;
    </button>
    {#if gallery.selectedImage.url}
      <img
        src={gallery.selectedImage.url}
        alt={gallery.selectedImage.filename}
        class="max-w-[90vw] max-h-[90vh] object-contain"
      />
    {/if}
  </div>
{/if}
