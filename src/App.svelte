<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import SetupWizard from "./lib/components/setup/SetupWizard.svelte";
  import GenerationPage from "./lib/components/generation/GenerationPage.svelte";
  import SettingsPage from "./lib/components/settings/SettingsPage.svelte";
  import { connection } from "./lib/stores/connection.svelte.js";
  import { progress } from "./lib/stores/progress.svelte.js";
  import { gallery } from "./lib/stores/gallery.svelte.js";
  import { models } from "./lib/stores/models.svelte.js";
  import { getHistory, getOutputImage, uploadImageBytes, loadGalleryImage, getConfig } from "./lib/utils/api.js";
  import { generation } from "./lib/stores/generation.svelte.js";
  import { autocomplete } from "./lib/stores/autocomplete.svelte.js";
  import { canvas } from "./lib/stores/canvas.svelte.js";
  import type { OutputImage } from "./lib/types/index.js";
  import UpdateNotification from "./lib/components/updater/UpdateNotification.svelte";

  declare const __APP_VERSION__: string;
  const appVersion = __APP_VERSION__ ?? "dev";

  const MAX_INPUT_PIXELS = 1024 * 1024;

  // Lightbox zoom state
  let lbScale = $state(1);
  let lbOriginX = $state(50);
  let lbOriginY = $state(50);

  function resetLightboxZoom() {
    lbScale = 1;
    lbOriginX = 50;
    lbOriginY = 50;
  }

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  // Reset zoom when lightbox opens
  $effect(() => {
    if (gallery.lightboxOpen) resetLightboxZoom();
  });

  function applyTheme(theme: string) {
    document.documentElement.classList.toggle("light", theme === "light");
  }

  function applyFontScale(scale: number) {
    document.documentElement.style.setProperty("--font-scale", String(scale));
  }

  async function normalizeImageBytes(
    imageBytes: number[],
    fallbackFilename: string,
  ): Promise<{ bytes: number[]; previewUrl: string; width: number; height: number; filename: string }> {
    const sourceBlob = new Blob([new Uint8Array(imageBytes)], { type: "image/png" });
    const sourceUrl = URL.createObjectURL(sourceBlob);

    const dims = await new Promise<{ width: number; height: number }>((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve({ width: img.naturalWidth, height: img.naturalHeight });
      img.onerror = () => reject(new Error("Failed to read image dimensions"));
      img.src = sourceUrl;
    });

    const sourcePixels = dims.width * dims.height;
    if (sourcePixels <= MAX_INPUT_PIXELS) {
      return {
        bytes: imageBytes,
        previewUrl: sourceUrl,
        width: dims.width,
        height: dims.height,
        filename: fallbackFilename,
      };
    }

    const scale = Math.sqrt(MAX_INPUT_PIXELS / sourcePixels);
    const targetWidth = Math.max(8, Math.round(dims.width * scale));
    const targetHeight = Math.max(8, Math.round(dims.height * scale));

    const resizedBlob = await new Promise<Blob>((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        const out = document.createElement("canvas");
        out.width = targetWidth;
        out.height = targetHeight;
        const ctx = out.getContext("2d");
        if (!ctx) {
          reject(new Error("Failed to create resize context"));
          return;
        }
        ctx.imageSmoothingEnabled = true;
        ctx.imageSmoothingQuality = "high";
        ctx.drawImage(img, 0, 0, targetWidth, targetHeight);
        out.toBlob((blob) => {
          if (!blob) {
            reject(new Error("Failed to encode resized image"));
            return;
          }
          resolve(blob);
        }, "image/png");
      };
      img.onerror = () => reject(new Error("Failed to decode source image"));
      img.src = sourceUrl;
    });

    URL.revokeObjectURL(sourceUrl);
    const resizedBuffer = await resizedBlob.arrayBuffer();
    const resizedBytes = Array.from(new Uint8Array(resizedBuffer));
    const resizedPreview = URL.createObjectURL(resizedBlob);

    return {
      bytes: resizedBytes,
      previewUrl: resizedPreview,
      width: targetWidth,
      height: targetHeight,
      filename: fallbackFilename,
    };
  }

  async function upscaleImage(image: OutputImage) {
    try {
      // Load image bytes from gallery or output
      let bytes: number[];
      if (image.gallery_filename) {
        bytes = await loadGalleryImage(image.gallery_filename);
      } else {
        bytes = await getOutputImage(image.filename, image.subfolder);
      }

      // Upload to ComfyUI input folder
      const response = await uploadImageBytes(bytes, image.filename);
      generation.inputImage = response.name;
      generation.mode = "img2img";
      generation.upscaleEnabled = true;
      currentPage = "generate";
      gallery.closeLightbox();
      gallery.showToast("Image loaded for upscaling");
    } catch (e) {
      console.error("Failed to set up upscale:", e);
      gallery.showToast("Failed to load image");
    }
  }

  async function loadImageForMode(
    image: OutputImage,
    mode: "img2img" | "inpainting",
  ) {
    try {
      let bytes: number[];
      if (image.gallery_filename) {
        bytes = await loadGalleryImage(image.gallery_filename);
      } else {
        bytes = await getOutputImage(image.filename, image.subfolder);
      }

      const normalized =
        mode === "inpainting"
          ? await normalizeImageBytes(bytes, image.filename || "inpaint_input.png")
          : null;

      const uploadBytes = normalized ? normalized.bytes : bytes;
      const uploadFilename = normalized ? normalized.filename : image.filename;

      const response = await uploadImageBytes(uploadBytes, uploadFilename);
      generation.inputImage = response.name;
      canvas.clearMask();
      generation.mode = mode;
      generation.upscaleEnabled = false;

      if (mode === "inpainting" && normalized) {
        generation.width = normalized.width;
        generation.height = normalized.height;

        canvas.setInpaintDrawMode("mask");
        canvas.isCanvasMode = true;
        canvas.stageImage(normalized.previewUrl);
        canvas.setReferenceImage(normalized.previewUrl);

        if (
          canvas.layers.length === 0 ||
          canvas.canvasWidth !== normalized.width ||
          canvas.canvasHeight !== normalized.height
        ) {
          canvas.initCanvas(normalized.width, normalized.height);
        }
      }

      currentPage = "generate";
      gallery.closeLightbox();

      gallery.showToast(
        mode === "inpainting"
          ? "Image loaded for inpainting"
          : "Image loaded for image-to-image",
      );
    } catch (e) {
      console.error(`Failed to set up ${mode}:`, e);
      gallery.showToast("Failed to load image");
    }
  }

  async function img2imgImage(image: OutputImage) {
    await loadImageForMode(image, "img2img");
  }

  async function inpaintImage(image: OutputImage) {
    await loadImageForMode(image, "inpainting");
  }

  async function rescanGalleryMetadata() {
    await gallery.rescanMetadata();
  }

  let setupComplete = $state<boolean | null>(null); // null = loading
  let currentPage = $state<"generate" | "gallery" | "queue" | "settings">(
    "generate"
  );
  let startupStatus = $state<string>("");

  let galleryImagesPerRow = $state(5);
  let gallerySortBy = $state<"date" | "name" | "size">("date");
  let gallerySortDir = $state<"asc" | "desc">("desc");
  let galleryGroupBy = $state<"none" | "date" | "month" | "mode" | "prompt">("none");
  let galleryView = $state<"huge" | "large" | "small" | "details">("large");
  let sortedGalleryImages = $state<OutputImage[]>([]);
  let groupedGalleryImages = $state<Array<{ label: string; images: OutputImage[] }>>([]);
  const GALLERY_PREFS_KEY = "mooshieui.gallery.prefs.v1";

  function getImageTimestamp(image: OutputImage): number {
    return image.generated_at_ms ?? 0;
  }

  function getImageSize(image: OutputImage): number {
    return image.file_size_bytes ?? 0;
  }

  function formatDate(ts: number | undefined): string {
    if (!ts) return "Unknown";
    return new Date(ts).toLocaleString();
  }

  function formatDateGroup(ts: number | undefined): string {
    if (!ts) return "Unknown Date";
    return new Date(ts).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  }

  function formatMonthGroup(ts: number | undefined): string {
    if (!ts) return "Unknown Month";
    return new Date(ts).toLocaleDateString(undefined, {
      year: "numeric",
      month: "long",
    });
  }

  function modeLabel(mode: OutputImage["generation_mode"]): string {
    if (mode === "txt2img") return "Text to Image";
    if (mode === "img2img") return "Image to Image";
    if (mode === "inpainting") return "Inpainting";
    return "Unknown Mode";
  }

  function loadGalleryPrefs() {
    try {
      const raw = localStorage.getItem(GALLERY_PREFS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as {
        imagesPerRow?: number;
        sortBy?: "date" | "name" | "size";
        sortDir?: "asc" | "desc";
        groupBy?: "none" | "date" | "month" | "mode" | "prompt";
        view?: "huge" | "large" | "small" | "details";
      };
      if (typeof parsed.imagesPerRow === "number") {
        galleryImagesPerRow = Math.max(2, Math.min(8, Math.round(parsed.imagesPerRow)));
      }
      if (parsed.sortBy) gallerySortBy = parsed.sortBy;
      if (parsed.sortDir) gallerySortDir = parsed.sortDir;
      if (parsed.groupBy) galleryGroupBy = parsed.groupBy;
      if (parsed.view) galleryView = parsed.view;
    } catch (e) {
      console.error("Failed to load gallery preferences:", e);
    }
  }

  function formatBytes(bytes: number | undefined): string {
    if (!bytes || bytes <= 0) return "-";
    const units = ["B", "KB", "MB", "GB"];
    let value = bytes;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
      value /= 1024;
      unitIndex += 1;
    }
    const rounded = unitIndex === 0 ? value.toFixed(0) : value.toFixed(1);
    return `${rounded} ${units[unitIndex]}`;
  }

  function viewColumns(view: "huge" | "large" | "small" | "details"): number {
    if (view === "huge") return Math.max(2, galleryImagesPerRow - 2);
    if (view === "small") return Math.min(10, galleryImagesPerRow + 2);
    return galleryImagesPerRow;
  }

  $effect(() => {
    void gallery.images;
    void gallerySortBy;
    void gallerySortDir;
    void galleryGroupBy;

    const sorted = [...gallery.images].sort((a, b) => {
      if (gallerySortBy === "name") {
        const cmp = a.filename.localeCompare(b.filename, undefined, { sensitivity: "base" });
        return gallerySortDir === "asc" ? cmp : -cmp;
      }
      if (gallerySortBy === "size") {
        const cmp = getImageSize(a) - getImageSize(b);
        return gallerySortDir === "asc" ? cmp : -cmp;
      }
      const cmp = getImageTimestamp(a) - getImageTimestamp(b);
      return gallerySortDir === "asc" ? cmp : -cmp;
    });

    sortedGalleryImages = sorted;

    if (galleryGroupBy !== "none") {
      const grouped = new Map<string, OutputImage[]>();
      for (const image of sorted) {
        const key =
          galleryGroupBy === "date"
            ? formatDateGroup(image.generated_at_ms)
            : galleryGroupBy === "month"
              ? formatMonthGroup(image.generated_at_ms)
              : galleryGroupBy === "mode"
                ? modeLabel(image.generation_mode)
                : (image.prompt_id || "No Prompt ID");
        const bucket = grouped.get(key) ?? [];
        bucket.push(image);
        grouped.set(key, bucket);
      }
      groupedGalleryImages = Array.from(grouped.entries()).map(([label, images]) => ({
        label,
        images,
      }));
    } else {
      groupedGalleryImages = [{ label: "All Images", images: sorted }];
    }
  });

  $effect(() => {
    void galleryImagesPerRow;
    void gallerySortBy;
    void gallerySortDir;
    void galleryGroupBy;
    void galleryView;

    try {
      localStorage.setItem(
        GALLERY_PREFS_KEY,
        JSON.stringify({
          imagesPerRow: galleryImagesPerRow,
          sortBy: gallerySortBy,
          sortDir: gallerySortDir,
          groupBy: galleryGroupBy,
          view: galleryView,
        }),
      );
    } catch (e) {
      console.error("Failed to save gallery preferences:", e);
    }
  });

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
              generation_mode: progress.currentMode,
              url,
              file_size_bytes: bytes.length,
              generated_at_ms: Date.now(),
            });
          }
        }
      }
      if (newImages.length > 0) {
        gallery.addImages(newImages);
        // Show the first output image in the preview area
        progress.setLastOutputForMode(
          progress.currentMode,
          newImages[0]?.url ?? null,
        );
        // Persist to disk gallery
        gallery.persistImages(newImages);
      }
    } catch (e) {
      console.error("Failed to fetch output images:", e);
    }
  }

  onMount(async () => {
    loadGalleryPrefs();

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
    // Apply UI preferences (theme, font scale) immediately
    try {
      const cfg = await getConfig();
      applyTheme(cfg.theme);
      applyFontScale(cfg.font_scale);
    } catch {
      // Config not ready yet, defaults are fine
    }

    // Load persisted settings
    await Promise.all([generation.loadSettings(), autocomplete.loadSettings()]);

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
        // Use node from progress event if available, fall back to currentNode from executing event
        const node = data.node ?? progress.currentNode;
        progress.updateProgress(data.value, data.max, node);
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
    class="flex flex-col w-20 bg-neutral-900 border-r border-neutral-800 items-stretch px-2 py-4 gap-2"
  >
    <button
      class="w-10 h-10 rounded-lg flex items-center justify-center transition-colors {currentPage ===
      'generate'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'} mx-auto"
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
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'} mx-auto"
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
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'} mx-auto"
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
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'} mx-auto"
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
      class="w-3 h-3 rounded-full mb-2 mx-auto transition-colors {connection.connected
        ? 'bg-green-500'
        : startupStatus
          ? 'bg-amber-500 animate-pulse'
          : 'bg-red-500'}"
      title={connection.connected ? "Connected" : startupStatus || "Disconnected"}
    ></div>

    <span class="text-[10px] text-neutral-500 text-center mb-2 select-none">v{appVersion}</span>
  </nav>

  <!-- Main content -->
  <main class="flex-1 overflow-hidden flex flex-col">
    <UpdateNotification />
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
          <div class="space-y-4">
            <div class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-3 space-y-3">
              <div class="grid grid-cols-1 lg:grid-cols-4 gap-3 items-end">
                <div class="lg:col-span-2">
                  <div class="text-xs text-neutral-400 mb-1">Images Per Row: {viewColumns(galleryView)}</div>
                  <input
                    type="range"
                    bind:value={galleryImagesPerRow}
                    min="2"
                    max="8"
                    step="1"
                    class="w-full accent-indigo-500"
                    disabled={galleryView === "details"}
                  />
                </div>
                <div>
                  <div class="text-xs text-neutral-400 mb-1">Sort By</div>
                  <select bind:value={gallerySortBy} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
                    <option value="date">Date</option>
                    <option value="name">Name</option>
                    <option value="size">Size</option>
                  </select>
                </div>
                <div>
                  <div class="text-xs text-neutral-400 mb-1">Group By</div>
                  <select bind:value={galleryGroupBy} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
                    <option value="none">None</option>
                    <option value="date">Date Generated</option>
                    <option value="month">Month Generated</option>
                    <option value="mode">Generation Mode</option>
                    <option value="prompt">Prompt ID</option>
                  </select>
                </div>
              </div>

              <div>
                <div class="text-xs text-neutral-400 mb-2">View</div>
                <div class="flex flex-wrap gap-2">
                  <button
                    onclick={() => (gallerySortDir = gallerySortDir === "asc" ? "desc" : "asc")}
                    class="px-3 py-1.5 text-xs rounded border transition-colors border-neutral-700 text-neutral-300 hover:border-neutral-500"
                    title="Toggle sort direction"
                  >
                    {gallerySortDir === "asc" ? "Ascending" : "Descending"}
                  </button>
                  <button onclick={() => (galleryView = "huge")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'huge' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">Huge Icons</button>
                  <button onclick={() => (galleryView = "large")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'large' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">Large Icons</button>
                  <button onclick={() => (galleryView = "small")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'small' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">Small Icons</button>
                  <button onclick={() => (galleryView = "details")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'details' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">Detailed View</button>
                  <button onclick={rescanGalleryMetadata} class="px-3 py-1.5 text-xs rounded border transition-colors border-amber-700/70 text-amber-300 hover:border-amber-500 hover:text-amber-200" title="Migrate legacy gallery entries and refresh metadata">
                    Re-scan Metadata
                  </button>
                </div>
              </div>
            </div>

            {#each groupedGalleryImages as group}
              <section class="space-y-2">
                {#if galleryGroupBy === "date"}
                  <h3 class="text-sm text-neutral-300 font-medium">{group.label}</h3>
                {:else if galleryGroupBy === "month" || galleryGroupBy === "mode" || galleryGroupBy === "prompt"}
                  <h3 class="text-sm text-neutral-300 font-medium">{group.label}</h3>
                {/if}

                {#if galleryView === "details"}
                  <div class="rounded-xl border border-neutral-800 overflow-hidden">
                    <div class="grid grid-cols-[72px_1fr_150px_120px_320px] gap-2 px-3 py-2 bg-neutral-900 text-[11px] uppercase tracking-wide text-neutral-500 border-b border-neutral-800">
                      <div>Preview</div>
                      <div>Name</div>
                      <div>Date</div>
                      <div>Size</div>
                      <div>Actions</div>
                    </div>
                    {#each group.images as image}
                      <div class="grid grid-cols-[72px_1fr_150px_120px_320px] gap-2 px-3 py-2 items-center border-b border-neutral-900/80 last:border-b-0">
                        <button class="w-14 h-14 rounded border border-neutral-800 overflow-hidden" onclick={() => gallery.openLightbox(image)}>
                          {#if image.url}
                            <img src={image.url} alt={image.filename} class="w-full h-full object-cover" />
                          {/if}
                        </button>
                        <div class="text-sm text-neutral-200 truncate" title={image.filename}>{image.filename}</div>
                        <div class="text-xs text-neutral-400">{formatDate(image.generated_at_ms)}</div>
                        <div class="text-xs text-neutral-400">{formatBytes(image.file_size_bytes)}</div>
                        <div class="flex flex-wrap gap-1">
                          <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => img2imgImage(image)}>I2I</button>
                          <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => inpaintImage(image)}>Inpaint</button>
                          <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => upscaleImage(image)}>Upscale</button>
                          <button class="px-2 py-1 text-[11px] rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100" onclick={() => gallery.saveImageAs(image)}>Save</button>
                          <button class="px-2 py-1 text-[11px] rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100" onclick={() => gallery.copyToClipboard(image)}>Copy</button>
                          <button class="px-2 py-1 text-[11px] rounded bg-red-900/80 hover:bg-red-800 text-neutral-100" onclick={() => gallery.deleteImage(image)}>Delete</button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <div
                    class="grid gap-3"
                    style="grid-template-columns: repeat({viewColumns(galleryView)}, minmax(0, 1fr));"
                  >
                    {#each group.images as image}
                      <div class="group relative rounded-lg overflow-hidden border border-neutral-800 hover:border-indigo-500 transition-colors {galleryView === 'huge' ? 'aspect-4/3' : galleryView === 'small' ? 'aspect-square' : 'aspect-square'}">
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
                        <div class="absolute inset-0 bg-black/55 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none"></div>
                        <div class="absolute inset-0 p-3 flex flex-wrap items-center justify-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none">
                          <button class="h-9 px-3 flex items-center justify-center rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black text-xs font-semibold backdrop-blur-sm shadow-lg pointer-events-auto" title="Image to Image" onclick={(e) => { e.stopPropagation(); img2imgImage(image); }}>I2I</button>
                          <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black text-xs font-semibold backdrop-blur-sm shadow-lg pointer-events-auto" title="Inpaint" onclick={(e) => { e.stopPropagation(); inpaintImage(image); }}>
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
                            Inpaint
                          </button>
                          <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black text-xs font-semibold backdrop-blur-sm shadow-lg pointer-events-auto" title="Upscale" onclick={(e) => { e.stopPropagation(); upscaleImage(image); }}>
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
                            Upscale
                          </button>
                          <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-neutral-900/90 hover:bg-neutral-700 text-neutral-100 text-xs font-semibold backdrop-blur-sm shadow-lg pointer-events-auto" title="Save As" onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}>
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                            Save
                          </button>
                          <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-neutral-900/90 hover:bg-neutral-700 text-neutral-100 text-xs font-semibold backdrop-blur-sm shadow-lg pointer-events-auto" title="Copy" onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}>
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                            Copy
                          </button>
                          <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-red-900/85 hover:bg-red-800 text-neutral-100 text-xs font-semibold backdrop-blur-sm shadow-lg pointer-events-auto" title="Delete" onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}>
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                            Delete
                          </button>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </section>
            {/each}
          </div>
        {/if}
      </div>
    {:else if currentPage === "queue"}
      <div class="flex items-center justify-center h-full text-neutral-500">
        Queue management (coming soon)
      </div>
    {:else if currentPage === "settings"}
      <SettingsPage />
    {/if}
    </div>
  </main>
</div>
{/if}

<!-- Lightbox overlay -->
{#if gallery.lightboxOpen && (gallery.selectedImage || gallery.lightboxUrl)}
  <div
    class="lightbox-backdrop fixed inset-0 bg-black/90 z-50 flex items-center justify-center"
    role="dialog"
    onclick={(e) => {
      if (e.target === e.currentTarget) gallery.closeLightbox();
    }}
    onkeydown={(e) => {
      if (e.key === "Escape") gallery.closeLightbox();
    }}
    tabindex="-1"
    use:focusOnMount
  >
    <!-- Close button -->
    <button
      class="absolute top-4 right-4 text-white text-2xl hover:text-neutral-300 z-10"
      onclick={() => gallery.closeLightbox()}
    >
      &times;
    </button>

    <!-- Action buttons (only for gallery images, not preview URLs) -->
    {#if gallery.selectedImage}
    <div class="absolute bottom-6 left-1/2 -translate-x-1/2 flex gap-3 z-10">
      <button
        class="flex items-center gap-2 px-4 py-2 bg-indigo-700/80 hover:bg-indigo-600 text-neutral-100 rounded-lg text-sm backdrop-blur-sm transition-colors"
        onclick={() => gallery.selectedImage && img2imgImage(gallery.selectedImage)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
        Image to Image
      </button>
      <button
        class="flex items-center gap-2 px-4 py-2 bg-indigo-700/80 hover:bg-indigo-600 text-neutral-100 rounded-lg text-sm backdrop-blur-sm transition-colors"
        onclick={() => gallery.selectedImage && inpaintImage(gallery.selectedImage)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
        Inpaint
      </button>
      <button
        class="flex items-center gap-2 px-4 py-2 bg-indigo-700/80 hover:bg-indigo-600 text-neutral-100 rounded-lg text-sm backdrop-blur-sm transition-colors"
        onclick={() => gallery.selectedImage && upscaleImage(gallery.selectedImage)}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
        Upscale
      </button>
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
    {/if}

    {#if gallery.selectedImage?.url || gallery.lightboxUrl}
      <img
        src={gallery.selectedImage?.url ?? gallery.lightboxUrl ?? ''}
        alt={gallery.selectedImage?.filename ?? 'Preview'}
        class="max-w-[90vw] max-h-[85vh] object-contain"
        style="transform: scale({lbScale}); transform-origin: {lbOriginX}% {lbOriginY}%; transition: {lbScale === 1 ? 'transform 0.2s ease' : 'none'};"
        onwheel={(e) => {
          e.preventDefault();
          const img = e.currentTarget as HTMLImageElement;
          const rect = img.getBoundingClientRect();
          const pctX = ((e.clientX - rect.left) / rect.width) * 100;
          const pctY = ((e.clientY - rect.top) / rect.height) * 100;
          lbOriginX = pctX;
          lbOriginY = pctY;
          const delta = e.deltaY > 0 ? -0.15 : 0.15;
          lbScale = Math.min(10, Math.max(0.5, lbScale + delta * lbScale));
        }}
        ondblclick={resetLightboxZoom}
      />
    {/if}
  </div>
{/if}

<!-- Toast notification -->
{#if gallery.toastMessage}
  <div class="fixed bottom-6 left-1/2 -translate-x-1/2 z-60 px-4 py-2 bg-neutral-800 text-neutral-100 text-sm rounded-lg shadow-lg border border-neutral-700 animate-fade-in">
    {gallery.toastMessage}
  </div>
{/if}
