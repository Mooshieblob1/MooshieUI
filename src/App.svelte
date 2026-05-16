<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { ipcInvoke, ipcListen, isTauri, isBrowserMode, startHeartbeat, getAuthToken, setAuthToken, setAuthUser, authHeaders, wasRememberMe } from "./lib/utils/ipc.js";
  import { useMobileLayout } from "./lib/utils/device.js";
  import SetupWizard from "./lib/components/setup/SetupWizard.svelte";
  import MobileApp from "./lib/components/mobile/MobileApp.svelte";
  import GenerationPage from "./lib/components/generation/GenerationPage.svelte";
  import SettingsPage from "./lib/components/settings/SettingsPage.svelte";
  import ModelHubPage from "./lib/components/modelhub/ModelHubPage.svelte";
  import { ArtistGalleryPage } from "./lib/artist-gallery/index.js";
  import { connection } from "./lib/stores/connection.svelte.js";
  import { progress } from "./lib/stores/progress.svelte.js";
  import { gallery } from "./lib/stores/gallery.svelte.js";
  import { models } from "./lib/stores/models.svelte.js";
  import { getOutputImage, uploadImageBytes, getConfig, readImageMetadata, getQueue, recoverPromptOutputs, readTempImage } from "./lib/utils/api.js";
  import { loadOutputImageForGenerationInput, uploadOutputImageForGenerationInput } from "./lib/utils/galleryActions.js";
  import { generation } from "./lib/stores/generation.svelte.js";
  import { autocomplete } from "./lib/stores/autocomplete.svelte.js";
  import { canvas } from "./lib/stores/canvas.svelte.js";
  import { accessibility } from "./lib/stores/accessibility.svelte.js";
  import { locale } from "./lib/stores/locale.svelte.js";
  import type { GenerationParams, OutputImage, InterrogationResult } from "./lib/types/index.js";
  import UpdateNotification from "./lib/components/updater/UpdateNotification.svelte";
  import DownloadBanner from "./lib/components/downloads/DownloadBanner.svelte";
  import { downloads } from "./lib/stores/downloads.svelte.js";
  import { compare } from "./lib/stores/compare.svelte.js";
  import { artistInsert } from "./lib/stores/artistInsert.svelte.js";
  import { styles as stylesStore } from "./lib/stores/styles.svelte.js";
  import logoUrl from "./lib/assets/logo.png";

  import { lazyThumbnail } from "./lib/utils/lazyThumbnail.js";
  import ContextMenu from "./lib/components/ui/ContextMenu.svelte";
  import type { ContextMenuItem } from "./lib/components/ui/ContextMenu.svelte";
  import InterrogateModal from "./lib/components/generation/InterrogateModal.svelte";
  import { interrogateGalleryImage, interrogateImage } from "./lib/utils/api.js";

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
  let pendingOutputImages = new Map<string, Array<{ blob: Blob; url: string; tempFilename?: string; displayTempFilename?: string }>>();
  /** In-flight output_image fetch promises per prompt_id (for SSE race-condition avoidance). */
  let pendingOutputFetches = new Map<string, Promise<void>[]>();
  /** Wait for pending fetches with a hard time limit to prevent hanging. */
  const FETCH_TIMEOUT_MS = 30_000;
  const GENERATION_DONE_TOAST_VISIBLE_MS = 6_000;
  const GENERATION_DONE_TOAST_EXIT_MS = 220;
  type PrimaryPage = "generate" | "gallery" | "modelhub" | "artists" | "settings";
  type GenerationDoneToast = {
    id: number;
    imageUrl: string;
    leaving: boolean;
  };
  async function awaitFetchesWithTimeout(fetches: Promise<void>[]): Promise<void> {
    const timeout = new Promise<void>((resolve) => setTimeout(resolve, FETCH_TIMEOUT_MS));
    await Promise.race([Promise.allSettled(fetches), timeout]);
  }
  let reconcileIntervalId: ReturnType<typeof setInterval> | null = null;
  let sseReconnectHandler: (() => void) | null = null;
  let generationDoneToastTimer: ReturnType<typeof setTimeout> | null = null;
  let generationDoneToastClearTimer: ReturnType<typeof setTimeout> | null = null;
  let generationDoneToastSeq = 0;
  /** Timestamp of the most recent SSE event per prompt — prevents false reconciliation. */
  let promptLastActivity = new Map<string, number>();

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
  let lbImgEl = $state<HTMLImageElement | null>(null);
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
    // Cursor position relative to the transformed image center
    const cx = rect.left + rect.width / 2;
    const cy = rect.top + rect.height / 2;
    // Cursor offset from center in screen pixels
    const dx = e.clientX - cx;
    const dy = e.clientY - cy;

    const nextScale = Math.min(10, Math.max(0.5, lbScale * (e.deltaY > 0 ? 0.9 : 1.1)));
    if (nextScale === lbScale) return;

    const ratio = 1 - nextScale / lbScale;
    lbOffsetX += dx * ratio;
    lbOffsetY += dy * ratio;
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

  function loadMoreGallery(node: HTMLElement) {
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          galleryRenderLimit += 48;
        }
      },
      { rootMargin: "200px" },
    );
    observer.observe(node);
    return { destroy() { observer.disconnect(); } };
  }

  // Reset zoom when lightbox opens (only on transition to open)
  let lbWasOpen = false;
  $effect(() => {
    const isOpen = gallery.lightboxOpen;
    if (isOpen && !lbWasOpen) resetLightboxZoom();
    lbWasOpen = isOpen;
  });

  // Document-level keyboard handler for lightbox (fallback for browser focus issues)
  $effect(() => {
    if (!gallery.lightboxOpen) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") gallery.closeLightbox();
      if (e.key === "ArrowLeft") navigateLightbox("prev");
      if (e.key === "ArrowRight") navigateLightbox("next");
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
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
    const keyMap: Record<string, string> = {
      positive_prompt: "gallery.meta.prompt",
      negative_prompt: "gallery.meta.negative_prompt",
      model: "gallery.meta.model",
      vae: "gallery.meta.vae",
      seed: "gallery.meta.seed",
      steps: "gallery.meta.steps",
      cfg: "gallery.meta.cfg",
      sampler: "gallery.meta.sampler",
      scheduler: "gallery.meta.scheduler",
      denoise: "gallery.meta.denoise",
      mode: "gallery.meta.mode",
      size: "gallery.meta.size",
      loras: "gallery.meta.loras",
      upscale_model: "gallery.meta.upscale_model",
      upscale_scale: "gallery.meta.upscale_scale",
      upscale_denoise: "gallery.meta.upscale_denoise",
      date: "gallery.meta.date",
      generation_time: "gallery.meta.generation_time",
    };
    const tKey = keyMap[key];
    return tKey ? locale.t(tKey) : key;
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
      generation.inputImage = await uploadOutputImageForGenerationInput(image, "refine_input.png");
      generation.mode = "img2img";
      generation.upscaleEnabled = true;
      currentPage = "generate";
      gallery.closeLightbox();
      gallery.showToast(locale.t("gallery.toast.loaded_upscale"), "success");
    } catch (e) {
      console.error("Failed to set up upscale:", e);
      gallery.showToast(locale.t("gallery.toast.failed_load"), "error");
    }
  }

  async function loadImageForMode(
    image: OutputImage,
    mode: "img2img" | "inpainting",
  ) {
    try {
      const source = await loadOutputImageForGenerationInput(
        image,
        mode === "inpainting" ? "inpaint_input.png" : "img2img_input.png",
      );

      const normalized =
        mode === "inpainting"
          ? await normalizeImageBytes(source.bytes, source.filename)
          : null;

      const uploadBytes = normalized ? normalized.bytes : source.bytes;
      const uploadFilename = normalized ? normalized.filename : source.filename;

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
          ? locale.t("gallery.toast.loaded_inpaint")
          : locale.t("gallery.toast.loaded_img2img"),
        "success"
      );
    } catch (e) {
      console.error(`Failed to set up ${mode}:`, e);
      gallery.showToast(locale.t("gallery.toast.failed_load"), "error");
    }
  }

  async function img2imgImage(image: OutputImage) {
    await loadImageForMode(image, "img2img");
  }

  async function inpaintImage(image: OutputImage) {
    await loadImageForMode(image, "inpainting");
  }

  async function inpaintLightboxPreview() {
    if (!gallery.lightboxUrl) return;
    await loadImageForMode(
      {
        filename: `preview_${Date.now()}.png`,
        subfolder: "",
        type: "output",
        prompt_id: "preview-lightbox",
        url: gallery.lightboxUrl,
      },
      "inpainting",
    );
  }

  function navigateLightbox(direction: "prev" | "next") {
    if (!gallery.selectedImage) return;
    // Try sorted gallery images first, fall back to session images for bottom panel
    let list = sortedGalleryImages;
    let idx = list.indexOf(gallery.selectedImage);
    if (idx === -1) {
      list = gallery.sessionImages;
      idx = list.indexOf(gallery.selectedImage);
    }
    if (idx === -1 || list.length < 2) return;
    const len = list.length;
    const next = direction === "prev" ? (idx - 1 + len) % len : (idx + 1) % len;
    const nextImage = list[next];
    if (nextImage) void gallery.openLightbox(nextImage);
  }

  async function rescanGalleryMetadata() {
    await gallery.rescanMetadata();
  }

  async function sortGalleryByArtist() {
    const result = await gallery.autoSortByArtist(connection.artistGalleryManifestUrl);
    if (result.sorted === 0 && result.scanned > 0) {
      gallery.showToast(locale.t("gallery.sort_by_artist_none"), "info");
    } else if (result.sorted > 0) {
      gallery.showToast(
        locale.t("gallery.sort_by_artist_done", {
          sorted: String(result.sorted),
          boards: String(result.boards.length),
        }),
        "success",
      );
    }
  }

  let setupComplete = $state<boolean | null>(null); // null = loading
  let currentPage = $state<PrimaryPage>("generate");
  let mobileCurrentTab = $state<PrimaryPage>("generate");
  let mobileGenerateNavigationVersion = $state(0);
  let generationDoneToast = $state<GenerationDoneToast | null>(null);

  // Auth gate state (browser mode LAN access)
  let authRequired = $state(false);
  let authChecked = $state(false);
  let userRole = $state<"admin" | "moderator" | "user" | "anonymous">("admin");
  let canUseModelhub = $state(true);
  let loginUser = $state("");
  let loginPass = $state("");
  let loginError = $state<string | null>(null);
  let loginBusy = $state(false);
  let rememberMe = $state(wasRememberMe());
  let mustChangePassword = $state(false);
  let newPass1 = $state("");
  let newPass2 = $state("");
  let changePassError = $state<string | null>(null);
  let changePassBusy = $state(false);

  async function checkAuth(): Promise<boolean> {
    if (!isBrowserMode) {
      authChecked = true;
      userRole = "admin";
      canUseModelhub = true;
      return true;
    }
    try {
      const resp = await fetch("/internal-api/_auth/status", {
        headers: getAuthToken() ? { Authorization: `Bearer ${getAuthToken()}` } : {},
      });
      const data = await resp.json();
      userRole = data.role ?? "anonymous";
      canUseModelhub = data.can_use_modelhub ?? false;
      if (data.role === "anonymous" && data.auth_required) {
        authRequired = true;
        authChecked = true;
        return false;
      }
      authRequired = false;
      authChecked = true;
      return true;
    } catch {
      // Can't reach server — proceed without auth gate
      authChecked = true;
      return true;
    }
  }

  async function handleLogin() {
    loginBusy = true;
    loginError = null;
    try {
      const resp = await fetch("/internal-api/_auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ username: loginUser, password: loginPass }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        loginError = data.error ?? "Login failed.";
        return;
      }
      setAuthToken(data.token, rememberMe);
      setAuthUser(loginUser.trim());
      // Re-check auth status to get the actual role (user vs moderator)
      const statusResp = await fetch("/internal-api/_auth/status", {
        headers: { Authorization: `Bearer ${data.token}` },
      });
      const statusData = await statusResp.json();
      userRole = statusData.role ?? "user";
      canUseModelhub = statusData.can_use_modelhub ?? false;

      // If the admin set a temporary password, force a change before proceeding
      if (data.must_change_password) {
        mustChangePassword = true;
        return;
      }

      authRequired = false;
      // Now continue the normal startup flow
      // LAN users skip setup check — setup is only for the host.
      // If the host hasn't finished setup yet, the server wouldn't be working anyway.
      setupComplete = true;
      await initApp();
    } catch (e) {
      loginError = String(e);
    } finally {
      loginBusy = false;
    }
  }

  async function handleSetNewPassword() {
    if (newPass1.length < 4) {
      changePassError = "Password must be at least 4 characters.";
      return;
    }
    if (newPass1 !== newPass2) {
      changePassError = "Passwords do not match.";
      return;
    }
    changePassBusy = true;
    changePassError = null;
    try {
      const resp = await fetch("/internal-api/_auth/change_password", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${getAuthToken()}`,
        },
        body: JSON.stringify({ current_password: loginPass, new_password: newPass1 }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        changePassError = data.error ?? "Failed to change password.";
        return;
      }
      // Password changed — proceed normally
      mustChangePassword = false;
      authRequired = false;
      loginPass = "";
      newPass1 = "";
      newPass2 = "";
      // LAN users skip setup check — setup is only for the host.
      setupComplete = true;
      await initApp();
    } catch (e) {
      changePassError = String(e);
    } finally {
      changePassBusy = false;
    }
  }
  let versionTapCount = $state(0);
  let startupStatus = $state<string>("");
  let startupStatusKind = $state<"idle" | "manual" | "starting" | "connecting" | "error">("idle");

  let galleryImagesPerRow = $state(5);
  let gallerySortBy = $state<"date" | "name" | "size">("date");
  let gallerySortDir = $state<"asc" | "desc">("desc");
  let galleryGroupBy = $state<"none" | "date" | "month" | "mode" | "prompt" | "board">("none");
  let galleryBoardFilter = $state<string>("all");
  let newBoardName = $state("");
  let galleryView = $state<"huge" | "large" | "small" | "details">("large");
  const sortedGalleryImages = $derived.by(() => {
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
    return galleryBoardFilter === "all"
      ? sorted
      : sorted.filter((image) => gallery.getBoard(image) === galleryBoardFilter);
  });
  const groupedGalleryImages = $derived.by(() => {
    if (galleryGroupBy !== "none") {
      const grouped = new Map<string, OutputImage[]>();
      for (const image of sortedGalleryImages) {
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
      return Array.from(grouped.entries()).map(([label, images]) => ({ label, images }));
    } else {
      return [{ label: locale.t("gallery.all_images"), images: sortedGalleryImages }];
    }
  });
  let galleryRenderLimit = $state(48);
  const galleryTotalCount = $derived(groupedGalleryImages.reduce((sum, g) => sum + g.images.length, 0));
  const galleryGroupsVisible = $derived.by(() => {
    let remaining = galleryRenderLimit;
    const result: Array<{ label: string; images: OutputImage[] }> = [];
    for (const group of groupedGalleryImages) {
      if (remaining <= 0) break;
      const images = group.images.slice(0, remaining);
      remaining -= images.length;
      if (images.length > 0) result.push({ label: group.label, images });
    }
    return result;
  });
  let lightboxMetadata = $state<Record<string, string> | null>(null);
  let loadingLightboxMetadata = $state(false);
  let metadataPanelWidth = $state(340);
  let metadataResizing = $state(false);
  let metadataPanelCollapsed = $state(false);
  const METADATA_MIN_WIDTH = 260;
  const METADATA_MAX_WIDTH = 600;
  const GALLERY_PREFS_KEY = "mooshieui.gallery.prefs.v1";

  /** Dir picker shown when manualSaveMode is on and 2+ dirs are configured. */
  let dirPickerImage = $state<OutputImage | null>(null);

  const WIN_STATE_KEY = "mooshieui.window.state.v1";

  function saveWindowMaximized(maximized: boolean) {
    try { localStorage.setItem(WIN_STATE_KEY, JSON.stringify({ maximized })); } catch {}
  }

  // Context menu state
  let contextMenuImage = $state<OutputImage | null>(null);
  let contextMenuX = $state(0);
  let contextMenuY = $state(0);
  let showContextMenu = $state(false);

  // Interrogation state (for lightbox + context menu)
  let showInterrogateModal = $state(false);

  // Artist tag insert: the actual replace/append logic lives in the shared
  // `artistInsert` store so the bottom-panel favourites tab can reuse the
  // same modal flow. `artistInsertPending` just mirrors the store for the
  // template below; `handleArtistTagInsert` bridges to the gallery page prop.
  const artistInsertPending = $derived(artistInsert.pending);

  function handleArtistTagInsert(tag: string) {
    artistInsert.request(tag);
    // Keep the existing UX where inserting from the gallery page snaps the
    // user back to the generate view so they can see the prompt update.
    if (!artistInsert.pending) {
      currentPage = "generate";
    }
  }

  function applyArtistTag(withAt: string, mode: "add" | "replace") {
    artistInsert.apply(withAt, mode);
    currentPage = "generate";
  }
  let interrogateResult = $state<InterrogationResult | null>(null);
  let interrogateLoading = $state(false);
  let interrogateStage = $state<string | null>(null);
  let interrogateDownloadProgress = $state<{ downloaded: number; total: number; filename: string } | null>(null);
  let interrogateImageUrl = $state<string | null>(null);
  let interrogateError = $state<string | null>(null);

  function openContextMenu(e: MouseEvent, image: OutputImage) {
    e.preventDefault();
    contextMenuImage = image;
    contextMenuX = e.clientX;
    contextMenuY = e.clientY;
    showContextMenu = true;
  }

  const contextMenuItems = $derived.by((): ContextMenuItem[] => {
    const image = contextMenuImage;
    if (!image) return [];
    return [
      { label: locale.t("gallery.get_tags"), action: () => interrogateFromGallery(image) },
      { label: "", action: () => {}, separator: true },
      { label: locale.t("gallery.img2img"), action: () => img2imgImage(image) },
      { label: locale.t("gallery.inpaint"), action: () => inpaintImage(image) },
      ...(!image.is_upscaled ? [{ label: locale.t("gallery.upscale"), action: () => upscaleImage(image) }] : []),
      { label: "", action: () => {}, separator: true },
      { label: locale.t("gallery.save_as"), action: () => gallery.saveImageAs(image) },
      { label: locale.t("gallery.copy"), action: () => gallery.copyToClipboard(image) },
      { label: "", action: () => {}, separator: true },
      { label: locale.t("gallery.delete"), action: () => gallery.deleteImage(image), destructive: true },
    ];
  });

  async function interrogateFromGallery(image: OutputImage) {
    showInterrogateModal = true;
    interrogateLoading = true;
    interrogateResult = null;
    interrogateStage = null;
    interrogateDownloadProgress = null;
    interrogateError = null;
    interrogateImageUrl = image.thumbnailUrl || image.url || null;

    const unlistenDownload = await listen<{ downloaded: number; total: number; filename: string; done: boolean }>(
      "interrogator:download_progress",
      (event) => {
        if (event.payload.done) {
          interrogateDownloadProgress = null;
        } else {
          interrogateDownloadProgress = event.payload;
        }
      }
    );

    const unlistenStage = await listen<string>("interrogator:stage", (event) => {
      interrogateStage = event.payload;
    });

    try {
      let result;
      if (image.gallery_filename) {
        result = await interrogateGalleryImage(image.gallery_filename);
      } else {
        const bytes = await getOutputImage(image.filename, image.subfolder);
        const uint8 = new Uint8Array(bytes);
        let binary = "";
        for (let i = 0; i < uint8.length; i++) {
          binary += String.fromCharCode(uint8[i]);
        }
        result = await interrogateImage(btoa(binary));
      }
      interrogateResult = result;
    } catch (e) {
      console.error("Interrogation failed:", e);
      interrogateError = e instanceof Error ? e.message : String(e);
    } finally {
      interrogateLoading = false;
      interrogateStage = null;
      unlistenDownload();
      unlistenStage();
    }
  }

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
    if (mode === "txt2img") return locale.t("gallery.mode.txt2img");
    if (mode === "img2img") return locale.t("gallery.mode.img2img");
    if (mode === "inpainting") return locale.t("gallery.mode.inpainting");
    return locale.t("gallery.mode.unknown");
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
      metadata.mooshie_upscale_steps = String(params.upscale_steps);
      if (params.upscale_tiling) {
        metadata.mooshie_upscale_tiling = "true";
        metadata.mooshie_upscale_tile_size = String(params.upscale_tile_size);
      }
      if (params.upscale_soft_guidance) {
        metadata.mooshie_soft_guidance = String(params.upscale_soft_guidance_multiplier);
      }
    }

    // MooshieUI-exclusive parameters
    metadata.mooshie_model_architecture = params.model_architecture;

    if (params.smart_guidance) {
      metadata.mooshie_smart_guidance = "true";
    }

    if (params.differential_diffusion) {
      metadata.mooshie_differential_diffusion = "true";
    }

    if (params.controlnet?.enabled) {
      if (params.controlnet.preset) {
        metadata.mooshie_controlnet_preset = params.controlnet.preset;
      }
      if (params.controlnet.controlnet_model) {
        metadata.mooshie_controlnet_model = params.controlnet.controlnet_model;
      }
      metadata.mooshie_controlnet_strength = String(params.controlnet.strength);
    }

    // Prompt scheduling — store as a separate metadata field, not inline in prompts
    const schedParts: string[] = [];
    for (const seg of params.positive_segments) {
      schedParts.push(`+${seg.text} [${Math.round(seg.start * 100)}%-${Math.round(seg.end * 100)}%]`);
    }
    for (const seg of params.negative_segments) {
      schedParts.push(`-${seg.text} [${Math.round(seg.start * 100)}%-${Math.round(seg.end * 100)}%]`);
    }
    if (schedParts.length > 0) {
      metadata.mooshie_prompt_schedule = schedParts.join(", ");
    }

    return metadata;
  }

  type MetadataApplyMode = "settings" | "seed" | "remix";

  async function applyMetadataToGeneration(image: OutputImage, mode: MetadataApplyMode = "settings") {
    if (!image.gallery_filename) {
      gallery.showToast(locale.t("gallery.toast.metadata_only_saved"), "info");
      return;
    }

    try {
      const metadata = await readImageMetadata(image.gallery_filename);
      if (!metadata) {
        gallery.showToast(locale.t("gallery.toast.no_metadata"), "info");
        return;
      }

      image.metadata = metadata;
      lightboxMetadata = metadata;

      if (mode === "seed") {
        if (metadata.seed !== undefined) {
          generation.seed = Number(metadata.seed) || generation.seed;
          gallery.showToast(locale.t("gallery.toast.applied_seed"), "success");
        } else {
          gallery.showToast(locale.t("gallery.toast.no_seed"), "info");
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

      // MooshieUI-exclusive params round-trip
      if (metadata.mooshie_smart_guidance !== undefined) {
        generation.smartGuidance = metadata.mooshie_smart_guidance === "true";
      }
      if (metadata.mooshie_differential_diffusion !== undefined) {
        generation.differentialDiffusion = metadata.mooshie_differential_diffusion === "true";
      }
      if (metadata.mooshie_controlnet_preset !== undefined) {
        generation.controlnetPreset = metadata.mooshie_controlnet_preset;
      }
      if (metadata.mooshie_controlnet_model !== undefined) {
        generation.controlnetModel = metadata.mooshie_controlnet_model;
      }
      if (metadata.mooshie_controlnet_strength !== undefined) {
        generation.controlnetStrength = Number(metadata.mooshie_controlnet_strength) || generation.controlnetStrength;
      }

      if (mode === "remix") {
        generation.seed = -1;
        gallery.showToast(locale.t("gallery.toast.loaded_remix"), "success");
        return;
      }

      if (metadata.seed !== undefined) generation.seed = Number(metadata.seed) || generation.seed;
      gallery.showToast(locale.t("gallery.toast.applied_settings"), "success");
    } catch (e) {
      console.error("Failed to apply metadata:", e);
      gallery.showToast(locale.t("gallery.toast.failed_metadata"), "error");
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

  // Reset pagination when the user changes sort/filter/group (but NOT on new image additions).
  $effect(() => {
    void gallerySortBy;
    void gallerySortDir;
    void galleryGroupBy;
    void galleryBoardFilter;
    galleryRenderLimit = 48;
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
    images: Array<{ blob: Blob; url: string; tempFilename?: string; displayTempFilename?: string }>,
  ) {
    if (images.length === 0) return;

    const newImages: OutputImage[] = images.map((img, i) => {
      const ext = img.blob.type === "image/jxl" ? "jxl" : "png";
      return {
        filename: `${promptId}_${i}.${ext}`,
        subfolder: "",
        type: "output",
        prompt_id: promptId,
        generation_mode: mode,
        is_upscaled: wasUpscaled,
        url: img.url,
        sessionBlob: img.blob,
        tempFilename: img.tempFilename,
        displayTempFilename: img.displayTempFilename,
        file_size_bytes: img.blob.size,
        generated_at_ms: Date.now(),
      };
    });

    gallery.addImages(newImages);
    progress.setLastOutputForMode(mode, newImages[0]?.url ?? null);

    const metadata = params ? buildPngMetadata(params) : undefined;
    for (const image of newImages) {
      image.metadata = metadata ?? null;
    }

    // In browser mode, embed metadata into the blob URLs immediately so that
    // right-click → Copy Image has stealth alpha from the start (no waiting
    // for persistImages to finish).  Uses the temp file path on the server to
    // avoid serializing multi-MB images as JSON number arrays.
    if (isBrowserMode && metadata) {
      for (let i = 0; i < images.length; i++) {
        const img = images[i]!;
        const outputImage = newImages[i]!;
        if (img.tempFilename) {
          embedTempMetadata(img.tempFilename, metadata, outputImage);
        }
      }
    }

    // Pass blobs and temp filenames so persistImages can use the most efficient path
    const blobs = images.map((img) => img.blob);
    const tempFilenames = images.map((img) => img.tempFilename);
    console.log("[finalizeOutputImages] images:", newImages.length, "blob[0].type:", blobs[0]?.type, "blob[0].size:", blobs[0]?.size, "filename[0]:", newImages[0]?.filename);
    gallery.persistImages(newImages, metadata, blobs, generation.metadataMode, tempFilenames);
    showGenerationDoneToast(newImages);

    // If a style was just applied to the prompt and it doesn't have a thumbnail yet,
    // automatically assign this generation's primary image to it.
    if (stylesStore.pendingStyleForThumbnail) {
      const styleId = stylesStore.pendingStyleForThumbnail;
      stylesStore.pendingStyleForThumbnail = null; // Clear immediately
      if (newImages.length === 0) return;

      const firstImage = newImages[0];
      const persistPromise = gallery.getPersistPromise(firstImage);
      if (persistPromise) {
        persistPromise.then(async (galleryFilename) => {
          if (!galleryFilename) return;
          // Resolve to a proper URL for this platform (thumbnail:// or https://thumbnail.localhost/ on Windows)
          let thumbnail = galleryFilename;
          try {
            if (isTauri) {
              const { convertFileSrc } = await import("@tauri-apps/api/core");
              thumbnail = convertFileSrc(galleryFilename, "thumbnail");
            } else if (isBrowserMode) {
              thumbnail = `/internal-api/_gallery_image/${galleryFilename}`;
            }
          } catch { /* keep raw filename as fallback */ }
          stylesStore.updateStyle(styleId, { thumbnail });
        });
      } else if (firstImage.gallery_filename) {
        // Already persisted synchronously (rare) — resolve URL the same way
        (async () => {
          let thumbnail = firstImage.gallery_filename!;
          try {
            if (isTauri) {
              const { convertFileSrc } = await import("@tauri-apps/api/core");
              thumbnail = convertFileSrc(thumbnail, "thumbnail");
            } else if (isBrowserMode) {
              thumbnail = `/internal-api/_gallery_image/${thumbnail}`;
            }
          } catch { /* keep raw filename */ }
          stylesStore.updateStyle(styleId, { thumbnail });
        })();
      }
    }
  }

  function clearGenerationDoneToastTimers() {
    if (generationDoneToastTimer) clearTimeout(generationDoneToastTimer);
    if (generationDoneToastClearTimer) clearTimeout(generationDoneToastClearTimer);
    generationDoneToastTimer = null;
    generationDoneToastClearTimer = null;
  }

  function viewingGeneratePage(): boolean {
    return useMobileLayout ? mobileCurrentTab === "generate" : currentPage === "generate";
  }

  function dismissGenerationDoneToast() {
    if (!generationDoneToast || generationDoneToast.leaving) return;
    if (generationDoneToastTimer) clearTimeout(generationDoneToastTimer);
    generationDoneToastTimer = null;
    generationDoneToast = { ...generationDoneToast, leaving: true };
    generationDoneToastClearTimer = setTimeout(() => {
      generationDoneToast = null;
      generationDoneToastClearTimer = null;
    }, GENERATION_DONE_TOAST_EXIT_MS);
  }

  function showGenerationDoneToast(images: OutputImage[]) {
    if (images.length === 0 || viewingGeneratePage()) return;
    const image = images.find((candidate) => candidate.url) ?? images[0];
    if (!image?.url) return;

    clearGenerationDoneToastTimers();
    generationDoneToast = {
      id: ++generationDoneToastSeq,
      imageUrl: image.url,
      leaving: false,
    };
    generationDoneToastTimer = setTimeout(
      dismissGenerationDoneToast,
      GENERATION_DONE_TOAST_VISIBLE_MS,
    );
  }

  function openGenerateFromDoneToast() {
    currentPage = "generate";
    if (useMobileLayout) {
      mobileCurrentTab = "generate";
      mobileGenerateNavigationVersion += 1;
    }
    dismissGenerationDoneToast();
  }

  $effect(() => {
    if (generationDoneToast && viewingGeneratePage()) {
      dismissGenerationDoneToast();
    }
  });

  /**
   * Embed metadata into a temp image on the server and upgrade the blob URL.
   * Runs async in the background — the image is already visible (blob URL),
   * this just replaces it with a metadata-embedded version.
   */
  async function embedTempMetadata(
    tempFilename: string,
    metadata: Record<string, string>,
    image: OutputImage,
  ) {
    try {
      const resp = await fetch("/internal-api/_embed_temp_metadata", {
        method: "POST",
        headers: { "content-type": "application/json", ...authHeaders() },
        body: JSON.stringify({
          tempFilename,
          metadata,
          metadataMode: generation.metadataMode,
        }),
      });
      if (!resp.ok) return;
      const result = await resp.json();
      const newTempFilename = result.tempFilename;
      if (!newTempFilename) return;

      // Fetch the metadata-embedded image as a new blob URL
      const imgResp = await fetch(
        `/internal-api/_temp_image/${encodeURIComponent(newTempFilename)}`,
        { headers: authHeaders() },
      );
      if (!imgResp.ok) return;
      const newBlob = await imgResp.blob();
      const newUrl = URL.createObjectURL(newBlob);

      // Revoke old blob URL and update the image
      const oldUrl = image.url;
      image.url = newUrl;
      image.sessionBlob = newBlob;
      image.tempFilename = newTempFilename;
      image.displayTempFilename = undefined;
      image.file_size_bytes = newBlob.size;

      // If the lightbox is showing this image's old blob URL, upgrade it
      if (gallery.lightboxOpen && gallery.lightboxUrl === oldUrl) {
        gallery.lightboxUrl = newUrl;
      }

      // Update progress store references so PreviewImage doesn't try to
      // load the revoked blob URL via displayImage / lastOutputImage.
      if (oldUrl) {
        progress.replaceOutputUrl(oldUrl, newUrl);
        window.setTimeout(() => {
          if (!gallery.sessionImages.some((img) => img.url === oldUrl)) {
            URL.revokeObjectURL(oldUrl);
          }
        }, 30_000);
      }

      // Trigger Svelte reactivity so lazyThumbnail actions pick up the new URL
      gallery.images = [...gallery.images];
      gallery.sessionImages = [...gallery.sessionImages];
    } catch (e) {
      // Non-critical — the image is still visible, just without embedded metadata
      console.warn("[embedTempMetadata] failed:", e);
    }
  }

  /**
   * Stitch completed grid cell images into a single XYZ-style grid image
   * with per-cell labels and a single MooshieUI watermark.
   */
  async function stitchGrid(
    cellImages: { blob: Blob; url: string }[],
    rows: number,
    cols: number,
    cellLabels: string[],
  ) {
    try {
      const loadImg = (src: string) => new Promise<HTMLImageElement>((resolve, reject) => {
        const img = new Image();
        img.onload = () => resolve(img);
        img.onerror = reject;
        img.src = src;
      });

      const [imgElements, logoImg] = await Promise.all([
        Promise.all(cellImages.map(({ url }) => loadImg(url))),
        loadImg(logoUrl),
      ]);

      const cellW = Math.max(...imgElements.map(img => img.naturalWidth));
      const cellH = Math.max(...imgElements.map(img => img.naturalHeight));
      const gap = 4;
      const fontSize = Math.max(14, Math.round(cellW * 0.028));
      const labelFont = `600 ${fontSize}px sans-serif`;
      const labelH = fontSize + 10;

      // Reserve footer space for the watermark below the grid
      const wmSize = Math.max(20, Math.round(cellW * 0.045));
      const wmFont = `600 ${Math.round(wmSize * 0.8)}px sans-serif`;
      const wmPad = Math.round(wmSize * 0.5);
      const footerH = wmSize + wmPad * 2 + wmPad;

      const totalW = cols * cellW + (cols - 1) * gap;
      const totalH = rows * (labelH + cellH) + (rows - 1) * gap + footerH;

      const cvs = document.createElement("canvas");
      cvs.width = totalW;
      cvs.height = totalH;
      const ctx = cvs.getContext("2d")!;
      ctx.fillStyle = "#000";
      ctx.fillRect(0, 0, totalW, totalH);

      // Draw each cell with its label above
      for (let i = 0; i < imgElements.length; i++) {
        const r = Math.floor(i / cols);
        const c = i % cols;
        const x = c * (cellW + gap);
        const y = r * (labelH + cellH + gap);

        // Per-cell label
        const label = cellLabels[i] ?? "";
        if (label) {
          ctx.font = labelFont;
          ctx.textAlign = "center";
          ctx.textBaseline = "top";
          ctx.fillStyle = "#e5e5e5";
          ctx.fillText(label, x + cellW / 2, y + 3, cellW - 8);
        }

        // Cell image
        const img = imgElements[i]!;
        const ox = (cellW - img.naturalWidth) / 2;
        const oy = (cellH - img.naturalHeight) / 2;
        ctx.drawImage(img, x + ox, y + labelH + oy);
      }

      // Single MooshieUI watermark in the footer area below the grid
      ctx.font = wmFont;
      const textW = ctx.measureText("MooshieUI").width;
      const pillW = wmSize + 6 + textW + wmPad * 2;
      const pillH = wmSize + wmPad;
      const gridBottom = rows * (labelH + cellH) + (rows - 1) * gap;
      const pillX = wmPad;
      const pillY = gridBottom + (footerH - pillH) / 2;

      ctx.fillStyle = "rgba(0, 0, 0, 0.6)";
      ctx.beginPath();
      ctx.roundRect(pillX, pillY, pillW, pillH, 6);
      ctx.fill();

      const lx = pillX + wmPad;
      const ly = pillY + (pillH - wmSize) / 2;
      ctx.drawImage(logoImg, lx, ly, wmSize, wmSize);

      ctx.font = wmFont;
      ctx.fillStyle = "rgba(255, 255, 255, 0.85)";
      ctx.textAlign = "left";
      ctx.textBaseline = "middle";
      ctx.fillText("MooshieUI", lx + wmSize + 6, pillY + pillH / 2);

      const gridBlob = await new Promise<Blob>((resolve, reject) => {
        cvs.toBlob(
          (b) => b ? resolve(b) : reject(new Error("toBlob failed")),
          "image/png",
        );
      });

      const gridUrl = URL.createObjectURL(gridBlob);
      const gridPromptId = `grid_${Date.now()}`;

      const gridImage: OutputImage = {
        filename: `${gridPromptId}.png`,
        subfolder: "",
        type: "output",
        prompt_id: gridPromptId,
        generation_mode: "txt2img",
        is_upscaled: false,
        url: gridUrl,
        file_size_bytes: gridBlob.size,
        generated_at_ms: Date.now(),
      };

      gallery.addImages([gridImage]);
      gallery.persistImages([gridImage], undefined, [gridBlob], generation.metadataMode);
    } catch (e) {
      console.error("Grid stitching failed:", e);
    }
  }

  /**
   * Save an image to a directory when manualSaveMode is on.
   * 0 dirs → native save-as dialog. 1 dir → save directly. 2+ dirs → show picker.
   */
  function saveToDir(image: OutputImage) {
    const dirs = generation.autoSaveDirs.filter(Boolean);
    if (dirs.length === 0) {
      gallery.saveImageAs(image);
    } else if (dirs.length === 1) {
      gallery.saveImageToDir(image, dirs[0]!);
    } else {
      dirPickerImage = image;
    }
  }

  onMount(async () => {
    // Start heartbeat in browser mode to keep backend alive
    startHeartbeat();

    // Restore window maximize state (Tauri only)
    if (isTauri) {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const raw = localStorage.getItem(WIN_STATE_KEY);
        if (raw) {
          const { maximized } = JSON.parse(raw) as { maximized?: boolean };
          if (maximized) await getCurrentWindow().maximize();
        }
        // Persist maximize/restore changes
        await getCurrentWindow().onResized(async () => {
          const maximized = await getCurrentWindow().isMaximized();
          saveWindowMaximized(maximized);
        });
      } catch {}
    }

    // Apply dyslexic font if enabled
    if (localStorage.getItem("mooshieui.dyslexicFont") === "true") {
      document.documentElement.classList.add("dyslexic-font");
    }

    loadGalleryPrefs();
    downloads.init();

    // Check auth for browser mode LAN access (before any ipcInvoke calls)
    const authOk = await checkAuth();
    if (!authOk) return;

    // Check if first-run setup is needed
    try {
      setupComplete = await ipcInvoke<boolean>("check_setup");
    } catch {
      setupComplete = false;
    }

    if (!setupComplete) return;

    // Setup already done — initialize the main app
    await initApp();
  });

  async function onSetupDone(selectedMode: "app" | "browser") {
    if (isTauri && selectedMode === "browser") {
      try {
        console.log("Setup selected browser mode, switching UI now...");
        startupStatus = locale.t("app.status.connecting");
        startupStatusKind = "connecting";
        await ipcInvoke("switch_to_browser_mode");
        return;
      } catch (e) {
        console.error("Failed to switch to browser mode after setup:", e);
        const message = locale.t("app.status.failed_to_start", { message: String(e) });
        setupComplete = true;
        await initApp();
        startupStatus = message;
        startupStatusKind = "error";
        gallery.showToast(message, "error", true);
        return;
      }
    }
    setupComplete = true;
    await initApp();
  }

  let autoStartEnabled = $state(true); // will be read from config

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
    await Promise.all([generation.loadSettings(), autocomplete.loadSettings(), locale.loadSettings()]);

    // Set up event listeners BEFORE starting so we don't miss events
    await Promise.all([
      ipcListen("comfyui:connection", (event: any) => {
        console.log("Connection event:", event.payload);
        connection.connected = event.payload.connected;
        if (event.payload.connected) {
          startupStatus = "";
          startupStatusKind = "idle";
          models.refresh().then(() => {
            generation.applyDefaultsIfNeeded(models.checkpoints, models.vaes);
          });
        }
      }),
      ipcListen("comfyui:server_ready", async () => {
        console.log("Server ready event received");
        startupStatus = "";
        startupStatusKind = "idle";
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
      ipcListen("comfyui:server_error", (event: any) => {
        console.error("Server error:", event.payload);
        startupStatus = locale.t("app.status.failed_to_start", {
          message: event.payload?.error || locale.t("app.status.unknown_error"),
        });
        startupStatusKind = "error";
      }),
      ipcListen("comfyui:progress", (event: any) => {
        const data = event.payload;
        if (!progress.isGenerating) return;
        lastProgressEventAt = Date.now();
        // Filter by prompt_id — reject events for other users' prompts
        if (data.prompt_id && !progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) return;
        if (data.prompt_id && progress.activePromptId && data.prompt_id !== progress.activePromptId) return;
        if (data.prompt_id) promptLastActivity.set(data.prompt_id, Date.now());
        if (data.prompt_id && !progress.activePromptId) {
          progress.setActivePrompt(data.prompt_id);
        }
        const node = data.node ?? progress.currentNode;
        progress.updateProgress(data.value, data.max, node);
      }),
      ipcListen("mooshie:queue_update", (event: any) => {
        const data = event.payload;
        if (data.prompt_id && data.position != null && data.total != null) {
          // Restore the prompt to pendingPrompts if this is an initial burst after
          // a page refresh (the in-memory queue was lost but the server still has it).
          if (!progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) {
            progress.restoreFromSnapshot([data.prompt_id]);
          }
          // Reset before each new batch (detected by total changing or position 0)
          if (data.position === 0 || data.total !== progress.queueTotal) {
            progress.resetQueuePosition();
          }
          progress.updateQueuePosition(data.prompt_id, data.position, data.total);
        }
      }),
      ipcListen("mooshie:server_progress", (event: any) => {
        const data = event.payload;
        if (data.active && data.max > 0) {
          progress.updateServerProgress(data.value, data.max);
        } else {
          progress.clearServerProgress();
        }
      }),
      ipcListen("mooshie:queue_cleared", (_event: any) => {
        // Admin/mod cleared the queue — cancel all pending state on this client
        promptLastActivity.clear();
        progress.cancelAll();
        compare.clearGridBatch();
      }),
      ipcListen("comfyui:preview", async (event: any) => {
        const data = event.payload;
        if (!progress.isGenerating) return;
        // Filter by prompt_id — reject events for other users' prompts
        if (data.prompt_id && !progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) return;
        if (data.prompt_id && progress.activePromptId && data.prompt_id !== progress.activePromptId) return;

        if (data.temp_filename) {
          // SSE/browser path: fetch image from temp endpoint
          try {
            const resp = await fetch(`/internal-api/_temp_image/${encodeURIComponent(data.temp_filename)}`, {
              headers: authHeaders(),
            });
            if (!resp.ok) return;
            const blob = await resp.blob();
            const url = URL.createObjectURL(blob);
            // Revoke the previous preview blob URL to avoid memory leaks
            if (progress.previewImage?.startsWith("blob:")) URL.revokeObjectURL(progress.previewImage);
            progress.previewImage = url;
          } catch (e) {
            console.warn("[preview] failed to fetch temp image:", e);
          }
        } else if (data.image) {
          if (progress.previewImage?.startsWith("blob:")) URL.revokeObjectURL(progress.previewImage);
          progress.previewImage = `data:image/${data.format};base64,${data.image}`;
        }
      }),
      ipcListen("comfyui:output_image", (event: any) => {
        // MooshieSaveImage sends final PNG bytes over WS — collect per prompt.
        // NOTE: The actual image fetch (for SSE/browser path) is async but we
        // must register the promise *synchronously* so that the executing
        // node=null handler can await it before consuming pendingOutputImages.
        const data = event.payload;
        console.log("[output_image] event received — format:", data.format, "temp_filename:", data.temp_filename, "display_temp:", data.display_temp_filename, "jxl_image?:", !!data.jxl_image, "image?:", !!data.image, "isGenerating:", progress.isGenerating);
        if (!progress.isGenerating) return;
        // Filter by prompt_id — reject events for other users' prompts
        if (data.prompt_id && !progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) return;

        const pid = data.prompt_id ?? progress.activePromptId;
        if (!pid) return;

        if (data.bit_depth === 16) {
          const now = Date.now();
          const sinceProgressMs = lastProgressEventAt > 0 ? now - lastProgressEventAt : null;
          const encodeMs = typeof data.encode_ms === "number" ? data.encode_ms : null;
          const imageBytes = typeof data.image_bytes === "number" ? data.image_bytes : null;

          if ((sinceProgressMs !== null && sinceProgressMs > 1500) || (encodeMs !== null && encodeMs > 250)) {
            console.warn("[16-bit diagnostics] output_image timing", {
              promptId: pid,
              sinceProgressMs,
              encodeMs,
              imageBytes,
              phaseLabel: progress.phaseLabel,
              currentStep: progress.currentStep,
              totalSteps: progress.totalSteps,
            });
          }
        }

        // Start the (possibly async) image fetch and register its promise
        // synchronously so the executing handler can await it.
        const fetchPromise = (async () => {
          let blob: Blob;
          let url: string;
          let tempFilename: string | undefined;
          let displayTempFilename: string | undefined;
          const isJxl = data.format === "jxl";

          if (data.temp_filename) {
            tempFilename = data.temp_filename;
            try {
              if (isTauri) {
                // Tauri desktop: no HTTP server — read temp files via invoke().
                // For JXL the event also carries display_temp_filename (WebP/PNG copy).
                if (isJxl) {
                  const displayFilename = data.display_temp_filename as string | undefined;
                  displayTempFilename = displayFilename;
                  console.log("[output_image] JXL temp path — jxl:", data.temp_filename, "display:", displayFilename, "display_format:", data.display_format);
                  const [jxlRaw, displayRaw] = await Promise.all([
                    readTempImage(data.temp_filename),
                    displayFilename ? readTempImage(displayFilename) : Promise.resolve(null as number[] | null),
                  ]);
                  console.log("[output_image] readTempImage done — jxlRaw:", jxlRaw?.length, "displayRaw:", displayRaw?.length ?? "null");
                  blob = new Blob([new Uint8Array(jxlRaw)], { type: "image/jxl" });
                  if (displayRaw && displayRaw.length > 0) {
                    const displayMime = data.display_format === "webp" ? "image/webp" : "image/png";
                    url = URL.createObjectURL(new Blob([new Uint8Array(displayRaw)], { type: displayMime }));
                    console.log("[output_image] display blob URL created, mime:", displayMime, "size:", displayRaw.length);
                  } else {
                    // No display copy — reuse last preview frame for display
                    url = progress.displayImage ?? "";
                    console.log("[output_image] no display copy, using displayImage:", url ? "present" : "EMPTY");
                  }
                } else {
                  const rawBytes = await readTempImage(data.temp_filename);
                  blob = new Blob([new Uint8Array(rawBytes)], { type: "image/png" });
                  url = URL.createObjectURL(blob);
                }
              } else {
                // SSE/browser path: fetch image from temp endpoint (avoids multi-MB SSE payloads)
                if (isJxl) {
                  // Fetch raw JXL for gallery save (?raw=true skips transcoding)
                  // and display copy (pre-built WebP/PNG from display_temp_filename,
                  // or server-side transcode as fallback).
                  const displayFilename = data.display_temp_filename as string | undefined;
                  displayTempFilename = displayFilename;
                  const displayUrl = displayFilename
                    ? `/internal-api/_temp_image/${encodeURIComponent(displayFilename)}`
                    : `/internal-api/_temp_image/${encodeURIComponent(data.temp_filename)}?format=webp`;
                  const [canonicalResp, displayResp] = await Promise.all([
                    fetch(`/internal-api/_temp_image/${encodeURIComponent(data.temp_filename)}?raw=true`, {
                      headers: authHeaders(),
                    }),
                    fetch(displayUrl, { headers: authHeaders() }),
                  ]);
                  if (!canonicalResp.ok) {
                    console.error(
                      "[output_image] JXL fetch failed:",
                      canonicalResp.status,
                      displayResp.status,
                    );
                    return;
                  }
                  blob = new Blob([await canonicalResp.arrayBuffer()], { type: "image/jxl" });
                  if (displayResp.ok) {
                    const displayBlob = await displayResp.blob();
                    url = URL.createObjectURL(displayBlob);
                  } else {
                    console.warn(
                      "[output_image] JXL display fetch failed; keeping canonical output:",
                      displayResp.status,
                    );
                    url = progress.displayImage ?? "";
                  }
                } else {
                  const resp = await fetch(
                    `/internal-api/_temp_image/${encodeURIComponent(data.temp_filename)}`,
                    { headers: authHeaders() },
                  );
                  if (!resp.ok) {
                    console.error("[output_image] failed to fetch temp image:", resp.status);
                    return;
                  }
                  blob = await resp.blob();
                  url = URL.createObjectURL(blob);
                }
              }
            } catch (e) {
              console.error("[output_image] failed to read temp image:", e);
              return;
            }
          } else if (data.image) {
            // Tauri path: decode inline base64.
            // For JXL output: `data.image` is the WebP display copy (WebView2
            // can't decode JXL), and `data.jxl_image` is the canonical lossless
            // JXL bytes for gallery saving. For PNG output: `data.image` is the PNG.
            const raw = atob(data.image);
            const bytes = new Uint8Array(raw.length);
            for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
            const displayMime = data.display_format === "webp" ? "image/webp" : "image/png";
            const displayBlob = new Blob([bytes], { type: displayMime });
            url = URL.createObjectURL(displayBlob);

            if (isJxl && data.jxl_image) {
              // Use the JXL bytes as the canonical save blob (lossless)
              const jxlRaw = atob(data.jxl_image);
              const jxlBytes = new Uint8Array(jxlRaw.length);
              for (let i = 0; i < jxlRaw.length; i++) jxlBytes[i] = jxlRaw.charCodeAt(i);
              blob = new Blob([jxlBytes], { type: "image/jxl" });
            } else {
              blob = displayBlob;
            }
          } else if (isJxl && data.jxl_image) {
            // JXL-only fallback: no display copy (WebP/PNG encode both failed in Rust).
            // Save the JXL to gallery anyway; preview stays on the last blurry frame.
            console.warn("[output_image] JXL has no display copy — saving to gallery only");
            const jxlRaw = atob(data.jxl_image);
            const jxlBytes = new Uint8Array(jxlRaw.length);
            for (let i = 0; i < jxlRaw.length; i++) jxlBytes[i] = jxlRaw.charCodeAt(i);
            blob = new Blob([jxlBytes], { type: "image/jxl" });
            url = progress.displayImage ?? "";
          } else {
            console.warn("[output_image] event has neither temp_filename nor image");
            return;
          }

          const arr = pendingOutputImages.get(pid) ?? [];
          arr.push({ blob, url, tempFilename, displayTempFilename });
          pendingOutputImages.set(pid, arr);
        })();

        const fetches = pendingOutputFetches.get(pid) ?? [];
        fetches.push(fetchPromise);
        pendingOutputFetches.set(pid, fetches);
      }),
      ipcListen("comfyui:executing", async (event: any) => {
        const data = event.payload;
        console.log("Executing event:", data);
        // Ignore prompts not in our queue
        if (data.prompt_id && !progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) {
          return;
        }
        // Record activity so the reconciler knows this prompt is alive
        if (data.prompt_id) promptLastActivity.set(data.prompt_id, Date.now());
        if (data.node === null) {
          if (!progress.isGenerating) return;
          const promptId = data.prompt_id;
          if (!promptId) return;

          // Wait for any in-flight output_image fetches to complete before
          // consuming pendingOutputImages.  The output_image handler is async
          // (fetches temp images over HTTP) and SSE events fire synchronously,
          // so without this await the images map would be empty.
          const fetches = pendingOutputFetches.get(promptId);
          if (fetches && fetches.length > 0) {
            await awaitFetchesWithTimeout(fetches);
            pendingOutputFetches.delete(promptId);
          }

          const item = progress.completePrompt(promptId);
          promptLastActivity.delete(promptId);
          if (item) {
            const images = pendingOutputImages.get(promptId) ?? [];
            pendingOutputImages.delete(promptId);
            finalizeOutputImages(promptId, item.mode, item.wasUpscaled, item.params, images);

            // Track grid batch completion — stitch when all cells are done
            if (images.length > 0 && compare.isGridPrompt(promptId)) {
              const gridResult = compare.addGridResult(promptId, images[0]!);
              if (gridResult) {
                stitchGrid(gridResult.images, gridResult.rows, gridResult.cols, gridResult.cellLabels);
              }
            }
          }
        } else {
          if (data.prompt_id) {
            progress.setActivePrompt(data.prompt_id);
          }
          progress.currentNode = data.node;
        }
      }),
      ipcListen("comfyui:execution_error", (event: any) => {
        console.error("Execution error:", event.payload);
        const data = event.payload;
        // Build a user-visible error message from the raw error string
        const rawErr = String(data.error ?? "");
        let toastMsg = "Generation failed";
        if (rawErr.includes("value_not_in_list") || rawErr.includes("Value not in list") || rawErr.includes("prompt_outputs_failed_validation")) {
          toastMsg = "Generation failed — a model or VAE may not be configured correctly. Check your model settings.";
        } else {
          try {
            const m = rawErr.match(/API error \(\d+\): ([\s\S]+)/);
            if (m) {
              const parsed = JSON.parse(m[1]);
              if (parsed.error?.message) toastMsg = `Generation failed: ${parsed.error.message}`;
            }
          } catch { /* ignore parse errors */ }
        }
        gallery.showToast(toastMsg, "error");
        if (data.prompt_id) {
          pendingOutputImages.delete(data.prompt_id);
          pendingOutputFetches.delete(data.prompt_id);
          promptLastActivity.delete(data.prompt_id);
          progress.removePrompt(data.prompt_id);
          compare.clearGridBatch();
        } else {
          // No prompt_id — clear everything
          pendingOutputImages.clear();
          pendingOutputFetches.clear();
          promptLastActivity.clear();
          progress.cancelAll();
          compare.clearGridBatch();
        }
      }),
      ipcListen("comfyui:execution_success", (_event: any) => {
        // Success handled via executing node=null
      }),
    ]);

    // Stuck-generation reconciliation: periodically check if our pending prompts
    // still exist in ComfyUI's queue. If not, they completed but events were lost
    // (e.g. SSE broadcast lag). Clear them so the UI doesn't hang.
    reconcileIntervalId = setInterval(async () => {
      if (!progress.isGenerating || !connection.connected) return;
      try {
        const q = await getQueue();
        const allPromptIds = new Set<string>();
        for (const item of [...q.queue_running, ...q.queue_pending]) {
          // ComfyUI queue entries: [number, prompt_id, ...] or {prompt_id: ...}
          const pid = Array.isArray(item)
            ? (item[1] as string)
            : (item as any)?.prompt_id;
          if (pid) allPromptIds.add(pid);
        }
        const now = Date.now();
        for (const p of progress.pendingPrompts) {
          // Skip prompts that received an SSE event within the last 30s —
          // they're clearly still alive even if the queue query missed them.
          // Fall back to enqueuedAt so brand-new prompts (not yet in ComfyUI's
          // queue because submission is async) are also guarded for 30s.
          // If both are missing (shouldn't happen, but defensive), treat as
          // just-enqueued so we don't immediately fire "generation lost".
          const lastEvent = promptLastActivity.get(p.promptId) ?? p.enqueuedAt ?? now;
          if (now - lastEvent < 30_000) continue;

          if (!allPromptIds.has(p.promptId)) {
            console.warn(`[reconcile] Prompt ${p.promptId} no longer in ComfyUI queue — completing`);
            // Wait for any in-flight output_image fetches
            const fetches = pendingOutputFetches.get(p.promptId);
            if (fetches && fetches.length > 0) {
              await awaitFetchesWithTimeout(fetches);
              pendingOutputFetches.delete(p.promptId);
            }
            const item = progress.completePrompt(p.promptId);
            promptLastActivity.delete(p.promptId);
            if (item) {
              let images = pendingOutputImages.get(p.promptId) ?? [];
              pendingOutputImages.delete(p.promptId);

              if (images.length === 0) {
                // SSE event was likely dropped during a reconnect — the image was
                // saved to a temp file on the server and cached by the cleanup
                // reactor.  Try to recover it before giving up.
                try {
                  const recovered = await recoverPromptOutputs(p.promptId);
                  for (const imgRef of recovered.images) {
                    try {
                      const resp = await fetch(
                        `/internal-api/_temp_image/${encodeURIComponent(imgRef.temp_filename)}`,
                        { headers: authHeaders() },
                      );
                      if (resp.ok) {
                        const blob = await resp.blob();
                        const url = URL.createObjectURL(blob);
                        images.push({ blob, url, tempFilename: imgRef.temp_filename });
                      }
                    } catch { /* individual image fetch failed */ }
                  }
                } catch { /* recovery command failed */ }
              }

              if (images.length > 0) {
                finalizeOutputImages(p.promptId, item.mode, item.wasUpscaled, item.params, images);
              } else {
                gallery.showToast("A generation was lost due to a connection issue — please try again.", "error");
              }
            }
          }
        }
      } catch {
        // Queue check failed — not critical
      }
    }, 5_000);

    // On SSE reconnect, immediately trigger a reconcile check so missed
    // completion events are caught within seconds rather than up to 15s later.
    const handleSseReconnect = () => {
      if (progress.isGenerating && connection.connected) {
        // Reset last-activity timestamps so the reconciler doesn't skip prompts
        for (const p of progress.pendingPrompts) {
          promptLastActivity.set(p.promptId, 0);
        }
      }
    };
    sseReconnectHandler = handleSseReconnect;
    window.addEventListener("mooshie:sse-reconnected", handleSseReconnect);

    // Start ComfyUI server — returns immediately, background task handles readiness
    // The backend will auto-connect WebSocket and emit comfyui:server_ready when done
    if (autoStartEnabled) {
      try {
        console.log("Starting ComfyUI...");
        const result = await ipcInvoke<string>("start_comfyui");
        console.log("start_comfyui returned:", result);
        if (result === "spawned") {
          startupStatus = locale.t("app.status.starting_comfyui");
          startupStatusKind = "starting";
        } else if (result === "already_running" || result === "skipped") {
          // SSE EventSource may not be connected yet, so the broadcast
          // comfyui:server_ready event could be lost. Handle it directly.
          startupStatus = locale.t("app.status.connecting");
          startupStatusKind = "connecting";
          try {
            await models.refresh();
            console.log("Models loaded (already running):", models.checkpoints);
            if (models.checkpoints.length > 0) {
              connection.connected = true;
              generation.applyDefaultsIfNeeded(models.checkpoints, models.vaes);
            }
            startupStatus = "";
            startupStatusKind = "idle";
          } catch (e) {
            console.error("Model refresh failed (already running):", e);
          }
        }
      } catch (e) {
        console.error("Failed to start ComfyUI:", e);
        startupStatus = locale.t("app.status.failed_to_start", { message: String(e) });
        startupStatusKind = "error";
      }
    } else {
      startupStatus = locale.t("app.status.auto_start_disabled");
      startupStatusKind = "manual";
    }

    // Load persisted gallery images from disk (independent of server status)
    gallery.loadFromDisk();

    // Warm the artist-tag detection index in the background so thumbnail
    // badges and "Sort by artist" work without an explicit fetch later.
    void gallery.loadArtistIndex(connection.artistGalleryManifestUrl);
  }

  $effect(() => {
    void gallery.lightboxOpen;
    void gallery.selectedImage;

    if (!gallery.lightboxOpen || !gallery.selectedImage) {
      lightboxMetadata = null;
      loadingLightboxMetadata = false;
      return;
    }

    const target = gallery.selectedImage;

    // Use in-memory metadata if already present (session images from current generation)
    if (target.metadata) {
      lightboxMetadata = target.metadata;
      loadingLightboxMetadata = false;
      return;
    }

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

  onDestroy(() => {
    if (reconcileIntervalId) clearInterval(reconcileIntervalId);
    if (sseReconnectHandler) window.removeEventListener("mooshie:sse-reconnected", sseReconnectHandler);
    clearGenerationDoneToastTimers();
  });
</script>

{#if authRequired}
  {#if mustChangePassword}
    <!-- Forced password change screen -->
    <div class="flex items-center justify-center h-full bg-neutral-950">
      <div class="w-80 space-y-4">
        <div class="flex items-center justify-center gap-3 mb-6">
          <img src={logoUrl} alt="MooshieUI" class="w-10 h-10 rounded-lg" />
          <h1 class="text-xl font-bold text-neutral-100">MooshieUI</h1>
        </div>
        <p class="text-sm text-neutral-400 text-center">Your password has been reset by an admin. Please choose a new password.</p>
        <input
          type="password"
          bind:value={newPass1}
          placeholder="New password (4+ characters)"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
          onkeydown={(e) => { if (e.key === "Enter") document.getElementById("confirm-pass")?.focus(); }}
        />
        <input
          id="confirm-pass"
          type="password"
          bind:value={newPass2}
          placeholder="Confirm new password"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
          onkeydown={(e) => { if (e.key === "Enter") handleSetNewPassword(); }}
        />
        {#if changePassError}
          <p class="text-xs text-red-400">{changePassError}</p>
        {/if}
        <button
          class="w-full py-2 rounded-lg text-sm font-medium transition-colors cursor-pointer {changePassBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
          disabled={changePassBusy}
          onclick={handleSetNewPassword}
        >
          {changePassBusy ? "Saving..." : "Set New Password"}
        </button>
      </div>
    </div>
  {:else}
  <!-- Login gate for LAN users -->
  <div class="flex items-center justify-center h-full bg-neutral-950">
    <div class="w-80 space-y-4">
      <div class="flex items-center justify-center gap-3 mb-6">
        <img src={logoUrl} alt="MooshieUI" class="w-10 h-10 rounded-lg" />
        <h1 class="text-xl font-bold text-neutral-100">MooshieUI</h1>
      </div>
      <p class="text-sm text-neutral-400 text-center">Sign in to continue</p>
      <input
        type="text"
        bind:value={loginUser}
        placeholder="Username"
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        onkeydown={(e) => { if (e.key === "Enter") handleLogin(); }}
      />
      <input
        type="password"
        bind:value={loginPass}
        placeholder="Password"
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        onkeydown={(e) => { if (e.key === "Enter") handleLogin(); }}
      />
      <label class="flex items-center gap-2 cursor-pointer select-none">
        <input
          type="checkbox"
          bind:checked={rememberMe}
          class="w-4 h-4 rounded border-neutral-600 bg-neutral-800 text-indigo-500 focus:ring-indigo-500 focus:ring-offset-0 cursor-pointer"
        />
        <span class="text-sm text-neutral-400">Remember me</span>
      </label>
      {#if loginError}
        <p class="text-xs text-red-400">{loginError}</p>
      {/if}
      <button
        class="w-full py-2 rounded-lg text-sm font-medium transition-colors cursor-pointer {loginBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
        disabled={loginBusy}
        onclick={handleLogin}
      >
        {loginBusy ? "Signing in..." : "Sign In"}
      </button>
    </div>
  </div>
  {/if}
{:else if setupComplete === null}
  <!-- Loading state -->
  <div class="flex items-center justify-center h-full bg-neutral-950">
    <div
      class="w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full animate-spin"
    ></div>
  </div>
{:else if !setupComplete}
  <SetupWizard onSetupComplete={onSetupDone} />
{:else if useMobileLayout}
  <MobileApp
    canUseModelhub={canUseModelhub}
    navigationTarget="generate"
    navigationVersion={mobileGenerateNavigationVersion}
    onTabChange={(tab) => (mobileCurrentTab = tab)}
  />
{:else}
<div class="flex h-full bg-neutral-950 text-neutral-100 md:gap-3 md:p-3 {visionSimClass}">
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
    class="flex w-14 shrink-0 flex-col items-stretch gap-1.5 border-r border-neutral-800 bg-neutral-900 px-1.5 py-3 md:rounded-2xl md:border md:shadow-2xl md:shadow-black/30"
  >
    <div class="relative mx-auto">
      <button
        class="w-8 h-8 rounded-lg flex items-center justify-center transition-colors {currentPage ===
        'generate'
          ? 'bg-indigo-600 text-white'
          : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}"
        onclick={() => (currentPage = "generate")}
        title={locale.t('nav.generate')}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          class="w-4.5 h-4.5"
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
      {#if progress.isGenerating}
        <div
          class="absolute -top-1 -right-1 min-w-4 h-4 rounded-full text-[9px] font-bold flex items-center justify-center px-0.5 pointer-events-none
            {progress.queuePosition !== null && progress.queuePosition > 0 ? 'bg-amber-500 text-black' : 'bg-indigo-400 text-white animate-pulse'}"
          title={progress.phaseLabel}
        >
          {#if progress.queuePosition !== null && progress.queuePosition > 0}
            #{progress.queuePosition + 1}
          {:else}
            ●
          {/if}
        </div>
      {/if}
      {#if progress.isGenerating && progress.totalSteps > 0 && currentPage !== "generate"}
        <div class="absolute bottom-0 left-0.5 right-0.5 h-0.5 bg-neutral-700 rounded-full overflow-hidden pointer-events-none">
          <div
            class="h-full rounded-full transition-[width] duration-200 {progress.wasUpscaled && progress.samplingPass >= 2 ? 'bg-emerald-400' : 'bg-indigo-400'}"
            style="width: {progress.percentage}%"
          ></div>
        </div>
      {/if}
    </div>
    <button
      class="w-8 h-8 rounded-lg flex items-center justify-center transition-colors {currentPage ===
      'gallery'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'} mx-auto"
      onclick={() => (currentPage = "gallery")}
      title={locale.t('nav.gallery')}
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-4.5 h-4.5"
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
    {#if canUseModelhub}
    <button
      class="w-8 h-8 rounded-lg flex items-center justify-center transition-colors {currentPage ===
      'modelhub'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'} mx-auto"
      onclick={() => (currentPage = "modelhub")}
      title={locale.t('nav.modelhub')}
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-4.5 h-4.5"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        ><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" /></svg
      >
    </button>
    {/if}
    <button
      class="w-8 h-8 rounded-lg flex items-center justify-center transition-colors {currentPage ===
      'artists'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'} mx-auto"
      onclick={() => (currentPage = "artists")}
      title="Artist Gallery"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-4.5 h-4.5"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        ><circle cx="12" cy="8" r="4" /><path d="M4 21c0-4 4-7 8-7s8 3 8 7" /></svg
      >
    </button>

    <div class="flex-1"></div>

    <button
      class="w-8 h-8 rounded-lg flex items-center justify-center transition-colors {currentPage ===
      'settings'
        ? 'bg-indigo-600 text-white'
        : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'} mx-auto"
      onclick={() => (currentPage = "settings")}
      title={locale.t('nav.settings')}
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="w-4.5 h-4.5"
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
      title={connection.connected ? locale.t('nav.connected') : startupStatus || locale.t('nav.disconnected')}
    ></div>

    <!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
    <span
      class="text-[10px] text-neutral-500 text-center mb-2 select-none cursor-default"
      onclick={() => {
        if (currentPage !== 'settings') return;
        versionTapCount++;
        if (versionTapCount >= 10) {
          versionTapCount = 0;
          if (generation.devModeUnlocked) {
            generation.devModeUnlocked = false;
            generation.devMode = false;
            gallery.showToast('🛠 Developer mode disabled', 'info');
          } else {
            generation.devModeUnlocked = true;
            gallery.showToast('🛠 Developer mode unlocked', 'success');
          }
        }
      }}
    >v{appVersion}</span>
  </nav>

  <!-- Main content -->
  <main class="flex min-w-0 flex-1 flex-col overflow-hidden md:rounded-2xl md:border md:border-neutral-800 md:bg-neutral-900 md:p-1 md:shadow-2xl md:shadow-black/30">
    <UpdateNotification {userRole} />
    <DownloadBanner />
    {#if startupStatus && !connection.connected}
      <div class="flex items-center gap-2 px-4 py-2 bg-amber-900/30 border-b border-amber-800/50 text-amber-200 text-sm">
        {#if startupStatusKind === "manual" || startupStatusKind === "error"}
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>
          {startupStatus}
          <button
            class="ml-2 px-3 py-1 bg-indigo-600 hover:bg-indigo-500 text-white rounded text-xs transition-colors cursor-pointer"
            onclick={async () => {
              try {
                startupStatus = locale.t("app.status.starting_comfyui");
                startupStatusKind = "starting";
                const result = await ipcInvoke<string>("start_comfyui");
                if (result === "spawned") {
                  startupStatus = locale.t("app.status.starting_comfyui");
                  startupStatusKind = "starting";
                } else if (result === "already_running" || result === "skipped") {
                  startupStatus = locale.t("app.status.connecting");
                  startupStatusKind = "connecting";
                  try {
                    await models.refresh();
                    if (models.checkpoints.length > 0) {
                      connection.connected = true;
                      generation.applyDefaultsIfNeeded(models.checkpoints, models.vaes);
                    }
                    startupStatus = "";
                    startupStatusKind = "idle";
                  } catch (refreshError) {
                    console.error("Model refresh failed (already running):", refreshError);
                  }
                }
              } catch (e) {
                startupStatus = locale.t("app.status.failed_to_start", { message: String(e) });
                startupStatusKind = "error";
              }
            }}
          >
            {locale.t("app.start_comfyui")}
          </button>
        {:else}
          <div class="w-4 h-4 border-2 border-amber-400 border-t-transparent rounded-full animate-spin"></div>
          {startupStatus}
        {/if}
      </div>
    {/if}
    <div class="flex-1 overflow-hidden md:min-h-0 md:rounded-xl md:bg-neutral-950">
    {#if currentPage === "generate"}
      <GenerationPage />
    {:else if currentPage === "gallery"}
      <div class="p-6 h-full overflow-y-auto will-change-scroll">
        {#if gallery.loading}
          <div class="flex items-center justify-center h-full text-neutral-500">
            {locale.t("gallery.loading")}
          </div>
        {:else if gallery.images.length === 0}
          <div
            class="flex items-center justify-center h-full text-neutral-500"
          >
            {locale.t("gallery.empty_generate")}
          </div>
        {:else}
          <div class="space-y-4">
            {#if gallery.hasExpiry}
              <div class="px-4 py-3 rounded-xl bg-amber-900/30 border border-amber-700/50 text-amber-300 text-sm flex items-start gap-3">
                <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5 shrink-0 mt-0.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
                <div>
                  <p class="font-medium text-amber-200">{locale.t('gallery.expiry_warning')}</p>
                  <p class="text-amber-400/80 text-xs mt-1">{locale.t('gallery.expiry_hint')}</p>
                  {#if gallery.expiringWithin24h > 0}
                    <p class="text-amber-200 text-xs mt-1 font-semibold">{locale.t('gallery.expiry_soon', { count: String(gallery.expiringWithin24h) })}</p>
                  {/if}
                  {#if gallery.storageInfo}
                    <p class="text-amber-400/60 text-xs mt-1">{locale.t('gallery.storage_usage')}: {gallery.storageLabel}</p>
                  {/if}
                </div>
              </div>
            {/if}
            <div class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-3 space-y-3">
              <div class="grid grid-cols-1 lg:grid-cols-4 gap-3 items-end">
                <div class="lg:col-span-2">
                  <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.images_per_row")} {viewColumns(galleryView)}</div>
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
                  <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.sort_by")}</div>
                  <select bind:value={gallerySortBy} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
                    <option value="date">{locale.t("gallery.sort_date")}</option>
                    <option value="name">{locale.t("gallery.sort_name")}</option>
                    <option value="size">{locale.t("gallery.sort_size")}</option>
                  </select>
                </div>
                <div>
                  <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.group_by")}</div>
                  <select bind:value={galleryGroupBy} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
                    <option value="none">{locale.t("gallery.group_none")}</option>
                    <option value="date">{locale.t("gallery.group_date")}</option>
                    <option value="month">{locale.t("gallery.group_month")}</option>
                    <option value="mode">{locale.t("gallery.group_mode")}</option>
                    <option value="prompt">{locale.t("gallery.group_prompt")}</option>
                    <option value="board">{locale.t("gallery.group_board")}</option>
                  </select>
                </div>
              </div>

              <div class="grid grid-cols-1 lg:grid-cols-4 gap-3 items-end">
                <div>
                  <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.board_filter")}</div>
                  <select bind:value={galleryBoardFilter} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
                    <option value="all">{locale.t("gallery.all_boards")}</option>
                    <option value="Unsorted">{locale.t("gallery.unsorted")}</option>
                    {#each gallery.boards as board}
                      <option value={board}>{board}</option>
                    {/each}
                  </select>
                </div>
                <div class="lg:col-span-3">
                  <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.create_board")}</div>
                  <div class="flex items-center gap-2">
                    <input
                      type="text"
                      bind:value={newBoardName}
                      class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-100 placeholder-neutral-500"
                      placeholder={locale.t("gallery.placeholder_board")}
                    />
                    <button
                      class="px-3 py-2 text-xs rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                      onclick={addBoard}
                      disabled={!newBoardName.trim()}
                    >
                      {locale.t("gallery.add")}
                    </button>
                  </div>
                </div>
              </div>

              <div>
                <div class="text-xs text-neutral-400 mb-2">{locale.t("gallery.view")}</div>
                <div class="flex flex-wrap gap-2">
                  <button
                    onclick={() => (gallerySortDir = gallerySortDir === "asc" ? "desc" : "asc")}
                    class="px-3 py-1.5 text-xs rounded border transition-colors border-neutral-700 text-neutral-300 hover:border-neutral-500"
                    title={locale.t("gallery.toggle_sort")}
                  >
                    {gallerySortDir === "asc" ? locale.t("gallery.ascending") : locale.t("gallery.descending")}
                  </button>
                  <button onclick={() => (galleryView = "huge")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'huge' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.huge_icons")}</button>
                  <button onclick={() => (galleryView = "large")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'large' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.large_icons")}</button>
                  <button onclick={() => (galleryView = "small")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'small' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.small_icons")}</button>
                  <button onclick={() => (galleryView = "details")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'details' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.detailed_view")}</button>
                  <button onclick={rescanGalleryMetadata} class="px-3 py-1.5 text-xs rounded border transition-colors border-amber-700/70 text-amber-300 hover:border-amber-500 hover:text-amber-200" title={locale.t("gallery.rescan_tooltip")}>
                    {locale.t("gallery.rescan_metadata")}
                  </button>
                  <button
                    onclick={sortGalleryByArtist}
                    disabled={gallery.autoSorting}
                    class="px-3 py-1.5 text-xs rounded border transition-colors border-indigo-700/70 text-indigo-300 hover:border-indigo-500 hover:text-indigo-200 disabled:opacity-50 disabled:cursor-not-allowed"
                    title={locale.t("gallery.sort_by_artist_tooltip")}
                  >
                    {gallery.autoSorting ? locale.t("gallery.sort_by_artist_running") : locale.t("gallery.sort_by_artist")}
                  </button>
                </div>
              </div>
            </div>

            {#each galleryGroupsVisible as group}
              <section class="space-y-2">
                {#if galleryGroupBy === "date"}
                  <h3 class="text-sm text-neutral-300 font-medium">{group.label}</h3>
                {:else if galleryGroupBy === "month" || galleryGroupBy === "mode" || galleryGroupBy === "prompt" || galleryGroupBy === "board"}
                  <h3 class="text-sm text-neutral-300 font-medium">{group.label}</h3>
                {/if}

                {#if galleryView === "details"}
                  <div class="rounded-xl border border-neutral-800 overflow-hidden">
                    <div class="grid grid-cols-[72px_1fr_150px_120px_320px] gap-2 px-3 py-2 bg-neutral-900 text-[11px] uppercase tracking-wide text-neutral-500 border-b border-neutral-800">
                      <div>{locale.t("gallery.col_preview")}</div>
                      <div>{locale.t("gallery.col_name")}</div>
                      <div>{locale.t("gallery.col_date")}</div>
                      <div>{locale.t("gallery.col_size")}</div>
                      <div>{locale.t("gallery.col_actions")}</div>
                    </div>
                    {#each group.images as image}
                      <div class="grid grid-cols-[72px_1fr_150px_120px_320px] gap-2 px-3 py-2 items-center border-b border-neutral-900/80 last:border-b-0" oncontextmenu={(e) => openContextMenu(e, image)}>
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
                            title={locale.t("gallery.assign_board")}
                          >
                            <option value="Unsorted">{locale.t("gallery.unsorted")}</option>
                            {#each gallery.boards as board}
                              <option value={board}>{board}</option>
                            {/each}
                          </select>
                          <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => img2imgImage(image)}>{locale.t("gallery.i2i")}</button>
                          <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => inpaintImage(image)}>{locale.t("gallery.inpaint")}</button>
                          {#if !image.is_upscaled}
                            <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => upscaleImage(image)}>{locale.t("gallery.upscale")}</button>
                          {/if}
                          <button class="px-2 py-1 text-[11px] rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100 disabled:opacity-50 disabled:cursor-not-allowed" disabled={gallery.saving} onclick={() => gallery.saveImageAs(image)}>{gallery.saving ? locale.t("gallery.saving") : locale.t("gallery.save")}</button>
                          {#if generation.manualSaveMode && !image.gallery_filename}
                            <button class="px-2 py-1 text-[11px] rounded bg-indigo-700 hover:bg-indigo-600 text-neutral-100" onclick={() => saveToDir(image)}>{locale.t("gallery.save_to_folder")}</button>
                          {/if}
                          <button class="px-2 py-1 text-[11px] rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100" onclick={() => gallery.copyToClipboard(image)}>{locale.t("gallery.copy")}</button>
                          <button class="px-2 py-1 text-[11px] rounded bg-red-900/80 hover:bg-red-800 text-neutral-100" onclick={() => gallery.deleteImage(image)}>{locale.t("gallery.delete")}</button>
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
                      <div class="group relative rounded-lg overflow-hidden border border-neutral-800 hover:border-indigo-500 transition-colors {galleryView === 'huge' ? 'aspect-4/3' : galleryView === 'small' ? 'aspect-square' : 'aspect-square'}" oncontextmenu={(e) => openContextMenu(e, image)}>
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
                        <div class="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none"></div>
                        <div class="absolute top-1 left-1 px-1.5 py-0.5 rounded bg-black/70 text-[10px] text-neutral-200 pointer-events-none">
                          {boardLabel(image)}
                        </div>
                        {#if galleryView !== 'small'}
                          {@const artist = gallery.primaryArtist(image)}
                          {#if artist}
                            <div
                              class="absolute top-1 right-1 px-1.5 py-0.5 rounded bg-red-900/80 text-[10px] text-red-200 pointer-events-none truncate max-w-[60%]"
                              title={locale.t("gallery.artist_detected", { tag: artist.tag })}
                            >
                              @{artist.slug}
                            </div>
                          {/if}
                        {/if}
                        {#if viewColumns(galleryView) <= 5}
                          <div class="absolute bottom-0 inset-x-0 flex justify-center items-center gap-1 px-1.5 pb-1.5 pt-6 opacity-0 group-hover:opacity-100 transition-opacity bg-linear-to-t from-black/80 to-transparent pointer-events-none">
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black text-[11px] font-bold shadow pointer-events-auto shrink-0" title={locale.t('gallery.img2img')} onclick={(e) => { e.stopPropagation(); img2imgImage(image); }}>I2I</button>
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black shadow pointer-events-auto shrink-0" title={locale.t('gallery.inpaint')} onclick={(e) => { e.stopPropagation(); inpaintImage(image); }}>
                              <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
                            </button>
                            {#if !image.is_upscaled}
                              <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black shadow pointer-events-auto shrink-0" title={locale.t('gallery.upscale')} onclick={(e) => { e.stopPropagation(); upscaleImage(image); }}>
                                <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
                              </button>
                            {/if}
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-neutral-800/90 hover:bg-neutral-700 text-neutral-200 shadow pointer-events-auto shrink-0 disabled:opacity-50 disabled:cursor-not-allowed" disabled={gallery.saving} title={locale.t('gallery.save_as')} onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}>
                              <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                            </button>
                            {#if generation.manualSaveMode && !image.gallery_filename}
                              <button class="w-7 h-7 flex items-center justify-center rounded bg-indigo-700/90 hover:bg-indigo-600 text-neutral-100 shadow pointer-events-auto shrink-0" title={locale.t('gallery.save_to_folder')} onclick={(e) => { e.stopPropagation(); saveToDir(image); }}>
                                <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
                              </button>
                            {/if}
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-neutral-800/90 hover:bg-neutral-700 text-neutral-200 shadow pointer-events-auto shrink-0" title={locale.t('gallery.copy')} onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}>
                              <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                            </button>
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-red-900/80 hover:bg-red-800 text-red-300 hover:text-red-200 shadow pointer-events-auto shrink-0" title={locale.t('gallery.delete')} onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}>
                              <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                            </button>
                          </div>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {/if}
              </section>
            {/each}
            {#if galleryRenderLimit < galleryTotalCount}
              <div use:loadMoreGallery class="h-4 w-full"></div>
            {/if}
          </div>
        {/if}
      </div>
    {:else if currentPage === "modelhub"}
      <ModelHubPage />
    {:else if currentPage === "artists"}
      <ArtistGalleryPage
        manifestUrl={connection.artistGalleryManifestUrl}
        oninsertTag={handleArtistTagInsert}
      />
    {:else if currentPage === "settings"}
      <SettingsPage {userRole} />
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
      if (e.key === "ArrowLeft") navigateLightbox("prev");
      if (e.key === "ArrowRight") navigateLightbox("next");
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
              <span class="font-semibold text-sm text-neutral-100">{locale.t("gallery.image_info")}</span>
              <div class="flex items-center gap-1">
                {#if loadingLightboxMetadata}
                  <span class="text-[10px] text-neutral-400">{locale.t("common.loading")}</span>
                {/if}
                <button
                  class="p-1 rounded hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors"
                  onclick={() => (metadataPanelCollapsed = true)}
                  title={locale.t("gallery.collapse_panel")}
                >
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
                </button>
              </div>
            </div>

            <!-- Board selector -->
            <div class="mb-3">
              <label class="block text-[10px] text-neutral-500 mb-1 uppercase tracking-wider">{locale.t("gallery.board")}</label>
              <select
                class="w-full bg-neutral-800 border border-neutral-700 rounded px-2 py-1.5 text-xs text-neutral-200"
                value={boardLabel(gallery.selectedImage)}
                onchange={(e) => assignBoard(gallery.selectedImage!, (e.target as HTMLSelectElement).value)}
              >
                <option value="Unsorted">{locale.t("gallery.unsorted")}</option>
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
              <span class="text-neutral-500">{locale.t("gallery.no_metadata")}</span>
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
              title={locale.t("gallery.show_panel")}
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

      <!-- Arrow navigation -->
      {#if gallery.selectedImage && (sortedGalleryImages.length > 1 || gallery.sessionImages.length > 1)}
        <button
          class="absolute left-3 top-1/2 -translate-y-1/2 z-10 w-10 h-10 flex items-center justify-center rounded-full bg-black/40 hover:bg-black/70 text-white transition-colors"
          onclick={() => navigateLightbox("prev")}
          title="Previous image (←)"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
        </button>
        <button
          class="absolute right-14 top-1/2 -translate-y-1/2 z-10 w-10 h-10 flex items-center justify-center rounded-full bg-black/40 hover:bg-black/70 text-white transition-colors"
          onclick={() => navigateLightbox("next")}
          title="Next image (→)"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
        </button>
      {/if}

      <!-- Action buttons -->
      {#if gallery.selectedImage}
      <div class="absolute bottom-6 left-1/2 -translate-x-1/2 z-10 flex items-center gap-1.5 bg-neutral-900/70 backdrop-blur-sm rounded-xl px-2 py-1.5 border border-neutral-700/50">
        <!-- Generation group -->
        <button
          title={locale.t("gallery.img2img")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && img2imgImage(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
        </button>
        <button
          title={locale.t("gallery.inpaint")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && inpaintImage(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
        </button>
        {#if gallery.selectedImage && !gallery.selectedImage.is_upscaled}
          <button
            title={locale.t("gallery.upscale")}
            class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
            onclick={() => gallery.selectedImage && upscaleImage(gallery.selectedImage)}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
          </button>
        {/if}
        <button
          title={locale.t("gallery.remix")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && applyMetadataToGeneration(gallery.selectedImage, "remix")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0114.13-3.36L23 10M1 14l5.37 4.36A9 9 0 0020.49 15"/></svg>
        </button>

        <!-- Separator -->
        <div class="w-px h-5 bg-neutral-700/60 mx-0.5"></div>

        <!-- Reuse group -->
        <button
          title={locale.t("gallery.interrogate_tags")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && interrogateFromGallery(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/></svg>
        </button>
        <button
          title={locale.t("gallery.reuse_settings")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && applyMetadataToGeneration(gallery.selectedImage, "settings")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/></svg>
        </button>
        <button
          title={locale.t("gallery.reuse_seed")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && applyMetadataToGeneration(gallery.selectedImage, "seed")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2v20"/><path d="M5 7h7"/><path d="M5 12h7"/><path d="M5 17h7"/></svg>
        </button>

        <!-- Separator -->
        <div class="w-px h-5 bg-neutral-700/60 mx-0.5"></div>

        <!-- Export group -->
        <button
          title={locale.t('gallery.save_as')}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          disabled={gallery.saving}
          onclick={() => gallery.selectedImage && gallery.saveImageAs(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
        </button>
        {#if generation.manualSaveMode && gallery.selectedImage && !gallery.selectedImage.gallery_filename}
          <button
            title={locale.t('gallery.save_to_folder')}
            class="flex items-center justify-center w-8 h-8 rounded-lg bg-indigo-700/80 hover:bg-indigo-600 text-neutral-100 transition-colors"
            onclick={() => gallery.selectedImage && saveToDir(gallery.selectedImage)}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
          </button>
        {/if}
        <button
          title={locale.t('gallery.copy_clipboard')}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && gallery.copyToClipboard(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
        </button>

        <!-- Separator -->
        <div class="w-px h-5 bg-neutral-700/60 mx-0.5"></div>

        <!-- Delete (destructive) -->
        <button
          title={locale.t("gallery.delete")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-red-900/60 hover:bg-red-800 text-red-400 hover:text-red-300 transition-colors"
          onclick={() => gallery.selectedImage && gallery.deleteImage(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        </button>
      </div>
      {/if}

      {#if !gallery.selectedImage && gallery.lightboxUrl}
        <div class="absolute bottom-6 left-1/2 -translate-x-1/2 z-10 flex items-center gap-1.5 bg-neutral-900/70 backdrop-blur-sm rounded-xl px-2 py-1.5 border border-neutral-700/50">
          <button
            title={locale.t("gallery.inpaint")}
            class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
            onclick={inpaintLightboxPreview}
          >
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
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

{#if generationDoneToast}
  {#key generationDoneToast.id}
    <div class="fixed bottom-5 right-4 z-80 w-[min(22rem,calc(100vw-2rem))] md:right-5">
      <div
        class="generation-done-toast flex items-center gap-3 rounded-xl border border-neutral-700 bg-neutral-900/95 p-2 shadow-2xl shadow-black/40 backdrop-blur-sm {generationDoneToast.leaving ? 'generation-done-toast-out' : 'generation-done-toast-in'}"
      >
        <button
          type="button"
          class="flex min-w-0 flex-1 items-center gap-3 rounded-lg p-1 text-left transition-colors hover:bg-neutral-800/70 focus:outline-none focus:ring-2 focus:ring-indigo-500"
          onclick={openGenerateFromDoneToast}
          aria-label={locale.t("generation.toast.image_ready")}
        >
          <img
            src={generationDoneToast.imageUrl}
            alt=""
            class="h-14 w-14 shrink-0 rounded-lg border border-neutral-700 object-cover bg-neutral-950"
          />
          <span class="min-w-0">
            <span class="block truncate text-sm font-semibold text-neutral-100">
              {locale.t("generation.toast.image_ready")}
            </span>
            <span class="block truncate text-xs text-neutral-400">{locale.t("nav.generate")}</span>
          </span>
        </button>
        <button
          type="button"
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-neutral-200 focus:outline-none focus:ring-2 focus:ring-indigo-500"
          onclick={dismissGenerationDoneToast}
          aria-label={locale.t("common.dismiss_notification")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
        </button>
      </div>
    </div>
  {/key}
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

<!-- Gallery context menu -->
<ContextMenu
  items={contextMenuItems}
  x={contextMenuX}
  y={contextMenuY}
  visible={showContextMenu}
  onclose={() => { showContextMenu = false; }}
/>

<!-- Interrogate modal (from gallery/lightbox) -->

<!-- Artist tag conflict dialog -->
{#if artistInsertPending}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-[300] flex items-center justify-center bg-black/60"
    onclick={(e) => { if (e.target === e.currentTarget) artistInsert.dismiss(); }}
    onkeydown={(e) => { if (e.key === 'Escape') artistInsert.dismiss(); }}
  >
    <div class="w-96 max-w-full rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
      {#if artistInsertPending.duplicate}
        <h2 class="mb-1 text-sm font-semibold text-neutral-100">Tag already in prompt</h2>
        <p class="mb-3 text-xs text-neutral-400">
          <span class="font-mono text-indigo-300">{artistInsertPending.tag}</span> is already in your prompt.
        </p>
        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-neutral-500"
            onclick={() => artistInsert.dismiss()}
          >
            OK
          </button>
        </div>
      {:else}
        <h2 class="mb-1 text-sm font-semibold text-neutral-100">Artist tag already in prompt</h2>
        <p class="mb-3 text-xs text-neutral-400">
          Your prompt already contains
          {#each artistInsertPending.existingTags as t, i}
            <span class="font-mono text-red-400">{t}</span>{i < artistInsertPending.existingTags.length - 1 ? ', ' : ''}
          {/each}.
          Add <span class="font-mono text-indigo-300">{artistInsertPending.tag}</span> alongside, or replace?
        </p>
        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-neutral-500"
            onclick={() => artistInsert.dismiss()}
          >
            Cancel
          </button>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-indigo-500"
            onclick={() => applyArtistTag(artistInsertPending!.tag, 'add')}
          >
            Add alongside
          </button>
          <button
            type="button"
            class="rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500"
            onclick={() => applyArtistTag(artistInsertPending!.tag, 'replace')}
          >
            Replace
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<!-- Interrogate modal (from gallery/lightbox) -->
{#if showInterrogateModal}
  <InterrogateModal
    result={interrogateResult}
    loading={interrogateLoading}
    stage={interrogateStage}
    downloadProgress={interrogateDownloadProgress}
    imagePreviewUrl={interrogateImageUrl}
    error={interrogateError}
    onclose={() => { showInterrogateModal = false; interrogateResult = null; interrogateImageUrl = null; interrogateError = null; }}
  />
{/if}

<!-- Dir picker overlay — shown when manual save mode is on and 2+ save dirs configured -->
{#if dirPickerImage}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-[200] flex items-center justify-center bg-black/60"
    onclick={(e) => { if (e.target === e.currentTarget) dirPickerImage = null; }}
    onkeydown={(e) => { if (e.key === 'Escape') dirPickerImage = null; }}
  >
    <div class="bg-neutral-900 border border-neutral-700 rounded-xl shadow-2xl p-5 w-80 max-w-full">
      <h2 class="text-sm font-semibold text-neutral-100 mb-3">{locale.t('gallery.dir_picker_title')}</h2>
      <div class="space-y-2">
        {#each generation.autoSaveDirs.filter(Boolean) as dir}
          <button
            class="w-full text-left px-3 py-2.5 rounded-lg bg-neutral-800 hover:bg-indigo-700 border border-neutral-700 hover:border-indigo-500 text-sm text-neutral-200 hover:text-white transition-colors truncate"
            onclick={() => {
              const img = dirPickerImage;
              dirPickerImage = null;
              if (img) gallery.saveImageToDir(img, dir);
            }}
          >
            {dir}
          </button>
        {/each}
      </div>
      <button
        class="mt-3 w-full px-3 py-1.5 rounded-lg bg-neutral-800 hover:bg-neutral-700 text-xs text-neutral-400 hover:text-neutral-200 transition-colors"
        onclick={() => { dirPickerImage = null; }}
      >
        {locale.t('common.cancel')}
      </button>
    </div>
  </div>
{/if}
