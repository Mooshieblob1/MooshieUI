<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import SetupWizard from "./lib/components/setup/SetupWizard.svelte";
  import GenerationPage from "./lib/components/generation/GenerationPage.svelte";
  import SettingsPage from "./lib/components/settings/SettingsPage.svelte";
  import ModelHubPage from "./lib/components/modelhub/ModelHubPage.svelte";
  import { connection } from "./lib/stores/connection.svelte.js";
  import { progress } from "./lib/stores/progress.svelte.js";
  import { gallery } from "./lib/stores/gallery.svelte.js";
  import { models } from "./lib/stores/models.svelte.js";
  import { getOutputImage, uploadImageBytes, loadGalleryImage, getConfig, readImageMetadata } from "./lib/utils/api.js";
  import { generation } from "./lib/stores/generation.svelte.js";
  import { autocomplete } from "./lib/stores/autocomplete.svelte.js";
  import { canvas } from "./lib/stores/canvas.svelte.js";
  import { accessibility } from "./lib/stores/accessibility.svelte.js";
  import type { GenerationParams, OutputImage } from "./lib/types/index.js";
  import UpdateNotification from "./lib/components/updater/UpdateNotification.svelte";
  import DownloadBanner from "./lib/components/downloads/DownloadBanner.svelte";
  import { downloads } from "./lib/stores/downloads.svelte.js";
  import { lazyThumbnail } from "./lib/utils/lazyThumbnail.js";

  declare const __APP_VERSION__: string;
  const appVersion = __APP_VERSION__ ?? "dev";
  
  const visionSimClass = $derived(
    accessibility.visionSimulatorMode === "none"
      ? ""
      : `sim-${accessibility.visionSimulatorMode}`
  );

  const MAX_INPUT_PIXELS = 1024 * 1024;
  let lastProgressEventAt = 0;

  /** Images received via WebSocket during generation, keyed by prompt_id. */
  let pendingOutputImages = new Map<string, Array<{ blob: Blob; url: string }>>();

  // Lightbox zoom state — only scale needs reactivity (used in template conditionals)
  let lbScale = 1;
  let lbOffsetX = 0;
  let lbOffsetY = 0;
  let lbPanning = $state(false);
  // Pan tracking — plain variables, no reactivity needed
  let lbPanStartX = 0;
  let lbPanStartY = 0;
  let lbPanStartOffsetX = 0;
  let lbPanStartOffsetY = 0;
  let lbImgEl: HTMLImageElement | null = null;
  let lbRafId = 0;

  function applyLightboxTransform(smooth = false) {
    if (!lbImgEl) return;
    lbImgEl.style.transition = smooth ? 'transform 0.12s ease' : 'none';
    lbImgEl.style.transform = `translate(${lbOffsetX}px, ${lbOffsetY}px) scale(${lbScale})`;
  }

  function resetLightboxZoom() {
    lbScale = 1;
    lbOffsetX = 0;
    lbOffsetY = 0;
    lbPanning = false;
    applyLightboxTransform(true);
  }

  function startLightboxPan(e: MouseEvent) {
    if (e.button !== 0 && e.button !== 1) return;
    lbPanning = true;
    lbPanStartX = e.clientX;
    lbPanStartY = e.clientY;
    lbPanStartOffsetX = lbOffsetX;
    lbPanStartOffsetY = lbOffsetY;
    if (lbImgEl) lbImgEl.style.transition = 'none';
    e.preventDefault();
  }

  function updateLightboxPan(e: MouseEvent) {
    if (!lbPanning) return;
    e.preventDefault();
    lbOffsetX = lbPanStartOffsetX + (e.clientX - lbPanStartX);
    lbOffsetY = lbPanStartOffsetY + (e.clientY - lbPanStartY);
    if (!lbRafId) {
      lbRafId = requestAnimationFrame(() => {
        lbRafId = 0;
        applyLightboxTransform();
      });
    }
  }

  function stopLightboxPan() {
    lbPanning = false;
    if (lbRafId) {
      cancelAnimationFrame(lbRafId);
      lbRafId = 0;
    }
    applyLightboxTransform();
  }

  function zoomLightboxAtCursor(e: WheelEvent) {
    e.preventDefault();
    const img = e.currentTarget as HTMLImageElement;
    const rect = img.getBoundingClientRect();
    const px = e.clientX - (rect.left + rect.width / 2);
    const py = e.clientY - (rect.top + rect.height / 2);
    const nextScale = Math.min(10, Math.max(0.5, lbScale * (e.deltaY > 0 ? 0.9 : 1.1)));

    if (nextScale === lbScale) return;

    const worldX = (px - lbOffsetX) / lbScale;
    const worldY = (py - lbOffsetY) / lbScale;

    lbOffsetX = px - worldX * nextScale;
    lbOffsetY = py - worldY * nextScale;
    lbScale = nextScale;

    if (lbScale <= 1) {
      lbOffsetX = 0;
      lbOffsetY = 0;
    }
    applyLightboxTransform();
  }

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  // Reset zoom when lightbox opens (only on transition to open)
  let lbWasOpen = false;
  $effect(() => {
    const isOpen = gallery.lightboxOpen;
    if (isOpen && !lbWasOpen) resetLightboxZoom();
    lbWasOpen = isOpen;
  });

  function startMetadataResize(e: MouseEvent) {
    e.preventDefault();
    metadataResizing = true;
    const startX = e.clientX;
    const startWidth = metadataPanelWidth;
    function onMove(ev: MouseEvent) {
      const delta = ev.clientX - startX;
      metadataPanelWidth = Math.min(METADATA_MAX_WIDTH, Math.max(METADATA_MIN_WIDTH, startWidth + delta));
    }
    function onUp() {
      metadataResizing = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    }
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  /** Pretty-print a metadata key for display */
  function metadataLabel(key: string): string {
    const labels: Record<string, string> = {
      positive_prompt: "Prompt",
      negative_prompt: "Negative Prompt",
      model: "Model",
      vae: "VAE",
      seed: "Seed",
      steps: "Steps",
      cfg: "CFG Scale",
      sampler: "Sampler",
      scheduler: "Scheduler",
      denoise: "Denoise",
      mode: "Mode",
      size: "Size",
      loras: "LoRAs",
      upscale_model: "Upscale Model",
      upscale_scale: "Upscale Scale",
      upscale_denoise: "Upscale Denoise",
      date: "Date",
      generation_time: "Gen Time",
    };
    return labels[key] ?? key;
  }

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
      gallery.showToast("Image loaded for upscaling", "success");
    } catch (e) {
      console.error("Failed to set up upscale:", e);
      gallery.showToast("Failed to load image", "error");
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
        "success"
      );
    } catch (e) {
      console.error(`Failed to set up ${mode}:`, e);
      gallery.showToast("Failed to load image", "error");
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
  let currentPage = $state<"generate" | "gallery" | "modelhub" | "settings">(
    "generate"
  );
  let startupStatus = $state<string>("");

  let galleryImagesPerRow = $state(5);
  let gallerySortBy = $state<"date" | "name" | "size">("date");
  let gallerySortDir = $state<"asc" | "desc">("desc");
  let galleryGroupBy = $state<"none" | "date" | "month" | "mode" | "prompt" | "board">("none");
  let galleryBoardFilter = $state<string>("all");
  let newBoardName = $state("");
  let galleryView = $state<"huge" | "large" | "small" | "details">("large");
  let sortedGalleryImages = $state<OutputImage[]>([]);
  let groupedGalleryImages = $state<Array<{ label: string; images: OutputImage[] }>>([]);
  let lightboxMetadata = $state<Record<string, string> | null>(null);
  let loadingLightboxMetadata = $state(false);
  let metadataPanelWidth = $state(340);
  let metadataResizing = $state(false);
  let metadataPanelCollapsed = $state(false);
  const METADATA_MIN_WIDTH = 260;
  const METADATA_MAX_WIDTH = 600;
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

  function boardLabel(image: OutputImage): string {
    return gallery.getBoard(image);
  }

  function assignBoard(image: OutputImage, board: string) {
    gallery.setBoard(image, board);
  }

  function addBoard() {
    const name = newBoardName.trim();
    if (!name) return;
    gallery.addBoard(name);
    galleryBoardFilter = name;
    newBoardName = "";
  }

  function parseSize(size?: string): { width: number; height: number } | null {
    if (!size) return null;
    const match = size.match(/^(\d+)x(\d+)$/i);
    if (!match) return null;
    const width = Number(match[1]);
    const height = Number(match[2]);
    if (!Number.isFinite(width) || !Number.isFinite(height)) return null;
    return { width, height };
  }

  function buildPngMetadata(params: GenerationParams): Record<string, string> {
    const metadata: Record<string, string> = {
      positive_prompt: params.positive_prompt,
      negative_prompt: params.negative_prompt,
      steps: String(params.steps),
      sampler: params.sampler_name,
      scheduler: params.scheduler,
      cfg: String(params.cfg),
      seed: String(params.seed),
      size: `${params.width}x${params.height}`,
      model: params.use_split_model ? (params.diffusion_model ?? "") : params.checkpoint,
      vae: params.vae ?? "",
      mode: params.mode,
      date: new Date().toISOString().split("T")[0] ?? "",
    };

    // Only include denoise for img2img/inpainting (txt2img is always 1.0)
    if (params.mode !== "txt2img") {
      metadata.denoise = String(params.denoise);
    }

    if (params.loras.length > 0) {
      metadata.loras = params.loras
        .map((l) => `${l.name}:${l.strength_model.toFixed(2)}:${l.strength_clip.toFixed(2)}`)
        .join(", ");
    }

    if (params.output_bit_depth !== "8bit") {
      metadata.bit_depth = params.output_bit_depth;
    }

    if (params.upscale_enabled) {
      metadata.upscale_model = params.upscale_model ?? "";
      metadata.upscale_scale = String(params.upscale_scale);
      metadata.upscale_denoise = String(params.upscale_denoise);
    }

    return metadata;
  }

  type MetadataApplyMode = "settings" | "seed" | "remix";

  async function applyMetadataToGeneration(image: OutputImage, mode: MetadataApplyMode = "settings") {
    if (!image.gallery_filename) {
      gallery.showToast("Metadata is only available for saved gallery images", "info");
      return;
    }

    try {
      const metadata = await readImageMetadata(image.gallery_filename);
      if (!metadata) {
        gallery.showToast("No PNG metadata found", "info");
        return;
      }

      image.metadata = metadata;
      lightboxMetadata = metadata;

      if (mode === "seed") {
        if (metadata.seed !== undefined) {
          generation.seed = Number(metadata.seed) || generation.seed;
          gallery.showToast("Applied seed from PNG metadata", "success");
        } else {
          gallery.showToast("No seed found in PNG metadata", "info");
        }
        return;
      }

      if (metadata.positive_prompt !== undefined) generation.positivePrompt = metadata.positive_prompt;
      if (metadata.negative_prompt !== undefined) generation.negativePrompt = metadata.negative_prompt;
      if (metadata.steps !== undefined) generation.steps = Number(metadata.steps) || generation.steps;
      if (metadata.sampler !== undefined) generation.samplerName = metadata.sampler;
      if (metadata.scheduler !== undefined) generation.scheduler = metadata.scheduler;
      if (metadata.cfg !== undefined) generation.cfg = Number(metadata.cfg) || generation.cfg;
      if (metadata.denoise !== undefined) generation.denoise = Number(metadata.denoise) || generation.denoise;

      const size = parseSize(metadata.size);
      if (size) {
        generation.width = size.width;
        generation.height = size.height;
      }

      if (metadata.mode === "txt2img" || metadata.mode === "img2img" || metadata.mode === "inpainting") {
        generation.mode = metadata.mode;
      }

      if (metadata.model && models.checkpoints.includes(metadata.model)) {
        generation.checkpoint = metadata.model;
      }

      if (metadata.vae !== undefined) {
        generation.vae = metadata.vae;
      }

      if (mode === "remix") {
        generation.seed = -1;
        gallery.showToast("Loaded settings for remix (random seed)", "success");
        return;
      }

      if (metadata.seed !== undefined) generation.seed = Number(metadata.seed) || generation.seed;
      gallery.showToast("Applied generation settings from PNG metadata", "success");
    } catch (e) {
      console.error("Failed to apply metadata:", e);
      gallery.showToast("Failed to read metadata", "error");
    }
  }

  function loadGalleryPrefs() {
    try {
      const raw = localStorage.getItem(GALLERY_PREFS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as {
        imagesPerRow?: number;
        sortBy?: "date" | "name" | "size";
        sortDir?: "asc" | "desc";
        groupBy?: "none" | "date" | "month" | "mode" | "prompt" | "board";
        boardFilter?: string;
        view?: "huge" | "large" | "small" | "details";
      };
      if (typeof parsed.imagesPerRow === "number") {
        galleryImagesPerRow = Math.max(2, Math.min(8, Math.round(parsed.imagesPerRow)));
      }
      if (parsed.sortBy) gallerySortBy = parsed.sortBy;
      if (parsed.sortDir) gallerySortDir = parsed.sortDir;
      if (parsed.groupBy) galleryGroupBy = parsed.groupBy;
      if (parsed.boardFilter) galleryBoardFilter = parsed.boardFilter;
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

  const thumbSize = $derived(viewColumns(galleryView) <= 3 ? 480 : 384);

  $effect(() => {
    void gallery.images;
    void gallerySortBy;
    void gallerySortDir;
    void galleryGroupBy;
    void galleryBoardFilter;

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

    const filteredByBoard = galleryBoardFilter === "all"
      ? sorted
      : sorted.filter((image) => gallery.getBoard(image) === galleryBoardFilter);

    sortedGalleryImages = filteredByBoard;

    if (galleryGroupBy !== "none") {
      const grouped = new Map<string, OutputImage[]>();
      for (const image of filteredByBoard) {
        const key =
          galleryGroupBy === "date"
            ? formatDateGroup(image.generated_at_ms)
            : galleryGroupBy === "month"
              ? formatMonthGroup(image.generated_at_ms)
              : galleryGroupBy === "mode"
                ? modeLabel(image.generation_mode)
                : galleryGroupBy === "board"
                  ? gallery.getBoard(image)
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
      groupedGalleryImages = [{ label: "All Images", images: filteredByBoard }];
    }
  });

  $effect(() => {
    void galleryImagesPerRow;
    void gallerySortBy;
    void gallerySortDir;
    void galleryGroupBy;
    void galleryBoardFilter;
    void galleryView;

    try {
      localStorage.setItem(
        GALLERY_PREFS_KEY,
        JSON.stringify({
          imagesPerRow: galleryImagesPerRow,
          sortBy: gallerySortBy,
          sortDir: gallerySortDir,
          groupBy: galleryGroupBy,
          boardFilter: galleryBoardFilter,
          view: galleryView,
        }),
      );
    } catch (e) {
      console.error("Failed to save gallery preferences:", e);
    }
  });

  /**
   * Finalize images received via WebSocket during generation.
   * MooshieSaveImage sends PNG bytes directly over WS — no disk round-trip.
   */
  function finalizeOutputImages(
    promptId: string,
    mode: "txt2img" | "img2img" | "inpainting",
    wasUpscaled: boolean,
    params: GenerationParams | null,
    images: Array<{ blob: Blob; url: string }>,
  ) {
    if (images.length === 0) return;

    const newImages: OutputImage[] = images.map((img, i) => ({
      filename: `${promptId}_${i}.png`,
      subfolder: "",
      type: "output",
      prompt_id: promptId,
      generation_mode: mode,
      is_upscaled: wasUpscaled,
      url: img.url,
      file_size_bytes: img.blob.size,
      generated_at_ms: Date.now(),
    }));

    gallery.addImages(newImages);
    progress.setLastOutputForMode(mode, newImages[0]?.url ?? null);

    const metadata = params ? buildPngMetadata(params) : undefined;
    for (const image of newImages) {
      image.metadata = metadata ?? null;
    }
    // Pass blobs so persistImages can use the bytes-based API (no ComfyUI disk round-trip)
    const blobs = images.map((img) => img.blob);
    gallery.persistImages(newImages, metadata, blobs, generation.metadataMode);
  }

  onMount(async () => {
    // Apply dyslexic font if enabled
    if (localStorage.getItem("mooshieui.dyslexicFont") === "true") {
      document.documentElement.classList.add("dyslexic-font");
    }

    loadGalleryPrefs();
    downloads.init();

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

  let autoStartEnabled = true; // will be read from config

  async function initApp() {
    // Apply UI preferences (theme, font scale) immediately
    try {
      const cfg = await getConfig();
      applyTheme(cfg.theme);
      applyFontScale(cfg.font_scale);
      autoStartEnabled = cfg.auto_start !== false;
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
        lastProgressEventAt = Date.now();
        // Filter by prompt_id if available
        if (data.prompt_id && progress.activePromptId && data.prompt_id !== progress.activePromptId) return;
        if (data.prompt_id && !progress.activePromptId) {
          progress.setActivePrompt(data.prompt_id);
        }
        const node = data.node ?? progress.currentNode;
        progress.updateProgress(data.value, data.max, node);
      }),
      listen("comfyui:preview", (event: any) => {
        const data = event.payload;
        if (!progress.isGenerating) return;
        progress.previewImage = `data:image/${data.format};base64,${data.image}`;
      }),
      listen("comfyui:output_image", (event: any) => {
        // MooshieSaveImage sends final PNG bytes over WS — collect per prompt
        const data = event.payload;
        if (!progress.isGenerating) return;

        if (data.bit_depth === 16) {
          const now = Date.now();
          const sinceProgressMs = lastProgressEventAt > 0 ? now - lastProgressEventAt : null;
          const encodeMs = typeof data.encode_ms === "number" ? data.encode_ms : null;
          const imageBytes = typeof data.image_bytes === "number" ? data.image_bytes : null;

          if ((sinceProgressMs !== null && sinceProgressMs > 1500) || (encodeMs !== null && encodeMs > 250)) {
            console.warn("[16-bit diagnostics] output_image timing", {
              promptId: data.prompt_id ?? progress.activePromptId,
              sinceProgressMs,
              encodeMs,
              imageBytes,
              phaseLabel: progress.phaseLabel,
              currentStep: progress.currentStep,
              totalSteps: progress.totalSteps,
            });
          }
        }

        const raw = atob(data.image);
        const bytes = new Uint8Array(raw.length);
        for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
        const blob = new Blob([bytes], { type: "image/png" });
        const url = URL.createObjectURL(blob);
        const pid = data.prompt_id ?? progress.activePromptId;
        if (!pid) return;
        const arr = pendingOutputImages.get(pid) ?? [];
        arr.push({ blob, url });
        pendingOutputImages.set(pid, arr);
      }),
      listen("comfyui:executing", (event: any) => {
        const data = event.payload;
        console.log("Executing event:", data);
        // Ignore prompts not in our queue
        if (data.prompt_id && !progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) {
          return;
        }
        if (data.node === null) {
          if (!progress.isGenerating) return;
          const promptId = data.prompt_id;
          if (!promptId) return;
          const item = progress.completePrompt(promptId);
          if (item) {
            const images = pendingOutputImages.get(promptId) ?? [];
            pendingOutputImages.delete(promptId);
            finalizeOutputImages(promptId, item.mode, item.wasUpscaled, item.params, images);
          }
        } else {
          if (data.prompt_id) {
            progress.setActivePrompt(data.prompt_id);
          }
          progress.currentNode = data.node;
        }
      }),
      listen("comfyui:execution_error", (event: any) => {
        console.error("Execution error:", event.payload);
        const data = event.payload;
        if (data.prompt_id) {
          pendingOutputImages.delete(data.prompt_id);
          progress.removePrompt(data.prompt_id);
        } else {
          // No prompt_id — clear everything
          pendingOutputImages.clear();
          progress.cancelAll();
        }
      }),
      listen("comfyui:execution_success", (_event: any) => {
        // Success handled via executing node=null
      }),
    ]);

    // Start ComfyUI server — returns immediately, background task handles readiness
    // The backend will auto-connect WebSocket and emit comfyui:server_ready when done
    if (autoStartEnabled) {
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
    } else {
      startupStatus = "ComfyUI not started (auto-start disabled)";
    }

    // Load persisted gallery images from disk (independent of server status)
    gallery.loadFromDisk();
  }

  $effect(() => {
    void gallery.lightboxOpen;
    void gallery.selectedImage;

    if (!gallery.lightboxOpen || !gallery.selectedImage?.gallery_filename) {
      lightboxMetadata = null;
      loadingLightboxMetadata = false;
      return;
    }

    const target = gallery.selectedImage;
    const galleryFilename = target.gallery_filename;
    if (!galleryFilename) {
      loadingLightboxMetadata = false;
      lightboxMetadata = null;
      return;
    }
    loadingLightboxMetadata = true;
    readImageMetadata(galleryFilename)
      .then((metadata) => {
        if (gallery.selectedImage === target) {
          target.metadata = metadata;
          lightboxMetadata = metadata;
        }
      })
      .catch((e) => {
        console.error("Failed to load lightbox metadata:", e);
        if (gallery.selectedImage === target) {
          lightboxMetadata = null;
        }
      })
      .finally(() => {
        if (gallery.selectedImage === target) {
          loadingLightboxMetadata = false;
        }
      });
  });
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
<div class="flex h-full bg-neutral-950 text-neutral-100 {visionSimClass}">
  <!-- SVG filters for color vision simulation -->
  <svg style="display: none">
    <defs>
      <filter id="protanopia">
        <feColorMatrix in="SourceGraphic" type="matrix" values="0.567 0.433 0 0 0 0.558 0.442 0 0 0 0 0.242 0.758 0 0 0 0 0 1 0" />
      </filter>
      <filter id="deuteranopia">
        <feColorMatrix in="SourceGraphic" type="matrix" values="0.625 0.375 0 0 0 0.7 0.3 0 0 0 0 0.3 0.7 0 0 0 0 0 1 0" />
      </filter>
      <filter id="tritanopia">
        <feColorMatrix in="SourceGraphic" type="matrix" values="0.95 0.05 0 0 0 0 0.433 0.567 0 0 0 0.475 0.525 0 0 0 0 0 1 0" />
      </filter>
    </defs>
  </svg>

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
      'modelhub'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'} mx-auto"
      onclick={() => (currentPage = "modelhub")}
      title="Model Hub"
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
        ><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" /></svg
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
    <DownloadBanner />
    {#if startupStatus && !connection.connected}
      <div class="flex items-center gap-2 px-4 py-2 bg-amber-900/30 border-b border-amber-800/50 text-amber-200 text-sm">
        {#if !autoStartEnabled && !startupStatus.startsWith("Starting") && !startupStatus.startsWith("Connecting")}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
          {startupStatus}
          <button
            class="ml-2 px-3 py-1 bg-indigo-600 hover:bg-indigo-500 text-white rounded text-xs transition-colors cursor-pointer"
            onclick={async () => {
              try {
                startupStatus = "Starting ComfyUI...";
                const result = await invoke<string>("start_comfyui");
                if (result === "spawned") startupStatus = "Starting ComfyUI...";
                else if (result === "already_running") startupStatus = "Connecting...";
              } catch (e) {
                startupStatus = `Failed to start: ${e}`;
              }
            }}
          >
            Start ComfyUI
          </button>
        {:else}
          <div class="w-4 h-4 border-2 border-amber-400 border-t-transparent rounded-full animate-spin"></div>
          {startupStatus}
        {/if}
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
                    <option value="board">Board</option>
                  </select>
                </div>
              </div>

              <div class="grid grid-cols-1 lg:grid-cols-4 gap-3 items-end">
                <div>
                  <div class="text-xs text-neutral-400 mb-1">Board Filter</div>
                  <select bind:value={galleryBoardFilter} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
                    <option value="all">All Boards</option>
                    <option value="Unsorted">Unsorted</option>
                    {#each gallery.boards as board}
                      <option value={board}>{board}</option>
                    {/each}
                  </select>
                </div>
                <div class="lg:col-span-3">
                  <div class="text-xs text-neutral-400 mb-1">Create Board</div>
                  <div class="flex items-center gap-2">
                    <input
                      type="text"
                      bind:value={newBoardName}
                      class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-100 placeholder-neutral-500"
                      placeholder="e.g. Portraits"
                    />
                    <button
                      class="px-3 py-2 text-xs rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                      onclick={addBoard}
                      disabled={!newBoardName.trim()}
                    >
                      Add
                    </button>
                  </div>
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
                {:else if galleryGroupBy === "month" || galleryGroupBy === "mode" || galleryGroupBy === "prompt" || galleryGroupBy === "board"}
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
                          <img use:lazyThumbnail={{ image, size: thumbSize }} alt={image.filename} class="w-full h-full object-cover" />
                        </button>
                        <div class="text-sm text-neutral-200 truncate" title={image.filename}>{image.filename}</div>
                        <div class="text-xs text-neutral-400">{formatDate(image.generated_at_ms)}</div>
                        <div class="text-xs text-neutral-400">{formatBytes(image.file_size_bytes)}</div>
                        <div class="flex flex-wrap gap-1">
                          <select
                            class="px-2 py-1 text-[11px] rounded bg-neutral-800 border border-neutral-700 text-neutral-200"
                            value={boardLabel(image)}
                            onchange={(e) => assignBoard(image, (e.target as HTMLSelectElement).value)}
                            title="Assign board"
                          >
                            <option value="Unsorted">Unsorted</option>
                            {#each gallery.boards as board}
                              <option value={board}>{board}</option>
                            {/each}
                          </select>
                          <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => img2imgImage(image)}>I2I</button>
                          <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => inpaintImage(image)}>Inpaint</button>
                          {#if !image.is_upscaled}
                            <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => upscaleImage(image)}>Upscale</button>
                          {/if}
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
                          <img
                            use:lazyThumbnail={{ image, size: thumbSize }}
                            alt={image.filename}
                            class="w-full h-full object-cover"
                          />
                        </button>
                        <div class="absolute inset-0 bg-black/55 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none"></div>
                        <div class="absolute bottom-1 left-1 px-1.5 py-0.5 rounded bg-black/70 text-[10px] text-neutral-200 pointer-events-none">
                          {boardLabel(image)}
                        </div>
                        <div class="absolute inset-0 p-3 flex flex-wrap items-center justify-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none">
                          <button class="h-9 px-3 flex items-center justify-center rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black text-xs font-semibold shadow-lg pointer-events-auto" title="Image to Image" onclick={(e) => { e.stopPropagation(); img2imgImage(image); }}>I2I</button>
                          <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black text-xs font-semibold shadow-lg pointer-events-auto" title="Inpaint" onclick={(e) => { e.stopPropagation(); inpaintImage(image); }}>
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
                            Inpaint
                          </button>
                          {#if !image.is_upscaled}
                            <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black text-xs font-semibold shadow-lg pointer-events-auto" title="Upscale" onclick={(e) => { e.stopPropagation(); upscaleImage(image); }}>
                              <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
                              Upscale
                            </button>
                          {/if}
                          <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-neutral-900/90 hover:bg-neutral-700 text-neutral-100 text-xs font-semibold shadow-lg pointer-events-auto" title="Save As" onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}>
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                            Save
                          </button>
                          <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-neutral-900/90 hover:bg-neutral-700 text-neutral-100 text-xs font-semibold shadow-lg pointer-events-auto" title="Copy" onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}>
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                            Copy
                          </button>
                          <button class="h-9 px-3 flex items-center justify-center gap-1 rounded bg-red-900/85 hover:bg-red-800 text-neutral-100 text-xs font-semibold shadow-lg pointer-events-auto" title="Delete" onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}>
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
    {:else if currentPage === "modelhub"}
      <ModelHubPage />
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
    class="lightbox-backdrop fixed inset-0 bg-black/90 z-50 flex {visionSimClass}"
    role="dialog"
    onkeydown={(e) => {
      if (e.key === "Escape") gallery.closeLightbox();
    }}
    tabindex="-1"
    use:focusOnMount
  >
    <!-- Metadata side panel -->
    {#if gallery.selectedImage}
      <div class="h-full flex shrink-0" style="width: {metadataPanelCollapsed ? 36 : metadataPanelWidth}px;">
        {#if !metadataPanelCollapsed}
          <div class="flex-1 h-full overflow-y-auto bg-neutral-900/95 p-4 text-xs text-neutral-200 select-text" style="min-width: 0;">
            <div class="flex items-center justify-between gap-2 mb-3">
              <span class="font-semibold text-sm text-neutral-100">Image Info</span>
              <div class="flex items-center gap-1">
                {#if loadingLightboxMetadata}
                  <span class="text-[10px] text-neutral-400">Loading...</span>
                {/if}
                <button
                  class="p-1 rounded hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors"
                  onclick={() => (metadataPanelCollapsed = true)}
                  title="Collapse panel"
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
                </button>
              </div>
            </div>

            <!-- Board selector -->
            <div class="mb-3">
              <label class="block text-[10px] text-neutral-500 mb-1 uppercase tracking-wider">Board</label>
              <select
                class="w-full bg-neutral-800 border border-neutral-700 rounded px-2 py-1.5 text-xs text-neutral-200"
                value={boardLabel(gallery.selectedImage)}
                onchange={(e) => assignBoard(gallery.selectedImage!, (e.target as HTMLSelectElement).value)}
              >
                <option value="Unsorted">Unsorted</option>
                {#each gallery.boards as board}
                  <option value={board}>{board}</option>
                {/each}
              </select>
            </div>

            {#if lightboxMetadata}
              {@const promptKeys = ["positive_prompt", "negative_prompt"]}
              {@const settingKeys = Object.keys(lightboxMetadata).filter((k) => !promptKeys.includes(k))}

              <!-- Prompts -->
              {#each promptKeys as key}
                {#if lightboxMetadata[key]}
                  <div class="mb-3">
                    <label class="block text-[10px] text-neutral-500 mb-1 uppercase tracking-wider">{metadataLabel(key)}</label>
                    <p class="text-neutral-200 whitespace-pre-wrap wrap-break-word leading-relaxed">{lightboxMetadata[key]}</p>
                  </div>
                {/if}
              {/each}

              <!-- Settings grid -->
              {#if settingKeys.length > 0}
                <div class="border-t border-neutral-700/50 pt-2 mt-2 space-y-1.5">
                  {#each settingKeys as key}
                    <div class="flex justify-between gap-2">
                      <span class="text-neutral-500 shrink-0">{metadataLabel(key)}</span>
                      <span class="text-neutral-200 text-right break-all">{lightboxMetadata[key]}</span>
                    </div>
                  {/each}
                </div>
              {/if}
            {:else if !loadingLightboxMetadata}
              <span class="text-neutral-500">No embedded metadata found.</span>
            {/if}
          </div>
          <!-- Resize handle -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="w-1.5 cursor-col-resize hover:bg-indigo-500/40 active:bg-indigo-500/60 transition-colors shrink-0"
            onmousedown={startMetadataResize}
          ></div>
        {:else}
          <!-- Collapsed: just a narrow strip with expand button -->
          <div class="w-9 h-full bg-neutral-900/95 border-r border-neutral-700 flex flex-col items-center pt-4 shrink-0">
            <button
              class="p-1 rounded hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors"
              onclick={() => (metadataPanelCollapsed = false)}
              title="Show metadata panel"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
            </button>
          </div>
        {/if}
      </div>
    {/if}

    <!-- Image area -->
    <div
      class="flex-1 h-full flex items-center justify-center relative"
      onclick={(e) => { if (e.target === e.currentTarget) gallery.closeLightbox(); }}
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
          class="flex items-center gap-2 px-4 py-2 bg-indigo-700/80 hover:bg-indigo-600 text-neutral-100 rounded-lg text-sm transition-colors"
          onclick={() => gallery.selectedImage && img2imgImage(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
          Image to Image
        </button>
        <button
          class="flex items-center gap-2 px-4 py-2 bg-indigo-700/80 hover:bg-indigo-600 text-neutral-100 rounded-lg text-sm transition-colors"
          onclick={() => gallery.selectedImage && inpaintImage(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
          Inpaint
        </button>
        {#if gallery.selectedImage && !gallery.selectedImage.is_upscaled}
          <button
            class="flex items-center gap-2 px-4 py-2 bg-indigo-700/80 hover:bg-indigo-600 text-neutral-100 rounded-lg text-sm transition-colors"
            onclick={() => gallery.selectedImage && upscaleImage(gallery.selectedImage)}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
            Upscale
          </button>
        {/if}
        <button
          class="flex items-center gap-2 px-4 py-2 bg-neutral-800/80 hover:bg-neutral-700 text-neutral-100 rounded-lg text-sm transition-colors"
          onclick={() => gallery.selectedImage && gallery.saveImageAs(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          Save As
        </button>
        <button
          class="flex items-center gap-2 px-4 py-2 bg-neutral-800/80 hover:bg-neutral-700 text-neutral-100 rounded-lg text-sm transition-colors"
          onclick={() => gallery.selectedImage && gallery.copyToClipboard(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
          Copy
        </button>
        <button
          class="flex items-center gap-2 px-4 py-2 bg-emerald-700/80 hover:bg-emerald-600 text-neutral-100 rounded-lg text-sm transition-colors"
          onclick={() => gallery.selectedImage && applyMetadataToGeneration(gallery.selectedImage, "settings")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          Reuse Settings
        </button>
        <button
          class="flex items-center gap-2 px-4 py-2 bg-indigo-700/80 hover:bg-indigo-600 text-neutral-100 rounded-lg text-sm transition-colors"
          onclick={() => gallery.selectedImage && applyMetadataToGeneration(gallery.selectedImage, "remix")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0114.13-3.36L23 10M1 14l5.37 4.36A9 9 0 0020.49 15"/></svg>
          Remix
        </button>
        <button
          class="flex items-center gap-2 px-4 py-2 bg-neutral-800/80 hover:bg-neutral-700 text-neutral-100 rounded-lg text-sm transition-colors"
          onclick={() => gallery.selectedImage && applyMetadataToGeneration(gallery.selectedImage, "seed")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2v20"/><path d="M5 7h7"/><path d="M5 12h7"/><path d="M5 17h7"/></svg>
          Reuse Seed
        </button>
        <button
          class="flex items-center gap-2 px-4 py-2 bg-red-900/60 hover:bg-red-800 text-neutral-100 rounded-lg text-sm transition-colors"
          onclick={() => gallery.selectedImage && gallery.deleteImage(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
          Delete
        </button>
      </div>
      {/if}

      {#if gallery.lightboxUrl}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <img
          bind:this={lbImgEl}
          src={gallery.lightboxUrl}
          alt={gallery.selectedImage?.filename ?? 'Preview'}
          class="max-w-full max-h-[85vh] object-contain select-none {lbPanning ? 'cursor-grabbing' : 'cursor-grab'}"
          draggable="false"
          style="transform-origin: center center; will-change: transform;"
          onwheel={zoomLightboxAtCursor}
          onmousedown={(e) => { if (e.button === 1) e.preventDefault(); startLightboxPan(e); }}
          onmousemove={updateLightboxPan}
          onmouseup={stopLightboxPan}
          onmouseleave={stopLightboxPan}
          onauxclick={(e) => e.preventDefault()}
          ondblclick={resetLightboxZoom}
        />
      {:else if gallery.lightboxLoading}
        <div class="flex items-center justify-center">
          <div class="w-8 h-8 border-2 border-neutral-500 border-t-indigo-400 rounded-full animate-spin"></div>
        </div>
      {/if}
    </div>
  </div>
{/if}

<!-- Toast notification -->
{#if gallery.toast}
  {@const type = gallery.toast.type}
  <div
    class="fixed bottom-6 left-1/2 -translate-x-1/2 z-60 px-4 py-2 text-sm rounded-lg shadow-lg border animate-fade-in flex items-center gap-2
    {type === 'success' ? 'bg-green-800/90 border-green-700 text-green-100' : 
     type === 'error' ? 'bg-red-800/90 border-red-700 text-red-100' :
     'bg-neutral-800 text-neutral-100 border-neutral-700'}"
  >
    {#if type === 'success'}
      <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
    {:else if type === 'error'}
      <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="m15 9-6 6"/><path d="m9 9 6 6"/></svg>
    {:else}
      <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>
    {/if}
    {gallery.toast.message}
  </div>
{/if}
