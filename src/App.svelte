<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { ipcInvoke, ipcListen, isTauri, isBrowserMode, startHeartbeat, getAuthToken, setAuthToken, setAuthUser, authHeaders, wasRememberMe } from "./lib/utils/ipc.js";
  import { useMobileLayout } from "./lib/utils/device.js";
  import SetupWizard from "./lib/components/setup/SetupWizard.svelte";
  import MobileApp from "./lib/components/mobile/MobileApp.svelte";
  import GenerationPage from "./lib/components/generation/GenerationPage.svelte";
  import SettingsPage from "./lib/components/settings/SettingsPage.svelte";
  import GalleryPage from "./lib/components/gallery/GalleryPage.svelte";
  import CompareViewer from "./lib/components/gallery/CompareViewer.svelte";
  import ModelHubPage from "./lib/components/modelhub/ModelHubPage.svelte";
  import { ArtistGalleryPage } from "./lib/artist-gallery/index.js";
  import { connection } from "./lib/stores/connection.svelte.js";
  import { startup } from "./lib/stores/startup.svelte.js";
  import { progress } from "./lib/stores/progress.svelte.js";
  import { gallery, isVideoImage } from "./lib/stores/gallery.svelte.js";
  import { models } from "./lib/stores/models.svelte.js";
  import { uploadImageBytes, getConfig, readImageMetadata, getQueue, recoverPromptOutputs, readTempImage, getComfyuiVersion, type ComfyUiVersionInfo } from "./lib/utils/api.js";
  import { loadOutputImageForGenerationInput, uploadOutputImageForGenerationInput, sendImageToVideoFrame, addImageToVideoReference, videoReferenceSlotsFree } from "./lib/utils/galleryActions.js";
  import { H3_MAX_REF_IMAGES } from "./lib/utils/videoParams.js";
  import { prepareOutputImageForEditMode } from "./lib/utils/editImagePreparation.js";
  import { shouldSuppressRegionalChainGallerySave, clearRegionalChainGallerySuppress } from "./lib/utils/regionalChainGallery.js";
  import { generation } from "./lib/stores/generation.svelte.js";
  import { autocomplete } from "./lib/stores/autocomplete.svelte.js";
  import { canvas } from "./lib/stores/canvas.svelte.js";
  import { accessibility } from "./lib/stores/accessibility.svelte.js";
  import { locale } from "./lib/stores/locale.svelte.js";
  import { prefsSync } from "./lib/stores/prefsSync.svelte.js";
  import type { GenerationMode, GenerationParams, OutputImage, InterrogationResult } from "./lib/types/index.js";
  import UpdateNotification from "./lib/components/updater/UpdateNotification.svelte";
  import DownloadBanner from "./lib/components/downloads/DownloadBanner.svelte";
  import { downloads } from "./lib/stores/downloads.svelte.js";
  import { compare } from "./lib/stores/compare.svelte.js";
  import { artistInsert } from "./lib/stores/artistInsert.svelte.js";
  import { characterInsert } from "./lib/stores/characterInsert.svelte.js";
  import CharacterInsertModal from "./lib/animadex/components/CharacterInsertModal.svelte";
  import type { AnimadexCharacter } from "./lib/animadex/types.js";
  import { styles as stylesStore } from "./lib/stores/styles.svelte.js";
  import { promptAssistant } from "./lib/stores/promptAssistant.svelte.js";
  import { notifications } from "./lib/stores/notifications.svelte.js";
  import NotificationBell from "./lib/components/ui/NotificationBell.svelte";
  import logoUrl from "./lib/assets/logo.png";
  import { applyTheme, getActiveThemeLogoUrl, onThemeApplied } from "./lib/utils/theme.js";
  import { serializeSegmentTags } from "./lib/utils/promptSegmentDetail.js";

  import { lazyThumbnail } from "./lib/utils/lazyThumbnail.js";
  import ContextMenu from "./lib/components/ui/ContextMenu.svelte";
  import type { ContextMenuItem } from "./lib/components/ui/ContextMenu.svelte";
  import VideoPlayer from "./lib/components/video/VideoPlayer.svelte";
  import InterrogateModal from "./lib/components/generation/InterrogateModal.svelte";
  import ExternalComfyModal from "./lib/components/ExternalComfyModal.svelte";
  import PhotopeaEditor from "./lib/components/PhotopeaEditor.svelte";
  import GlobalErrorModal from "./lib/components/errors/GlobalErrorModal.svelte";
  import ReportErrorModal from "./lib/components/errors/ReportErrorModal.svelte";
  import ErrorGallery from "./lib/components/errors/ErrorGallery.svelte";
  import type { FriendlyError } from "./lib/errors/types.js";
  import {
    interrogateGalleryImage,
    interrogateImage,
    interrogateImagePath,
    interrogateClipboard,
    readClipboardImageSafe,
    saveModelSidecarThumbnail,
    installCustomNode,
    loadGalleryImageDisplay,
  } from "./lib/utils/api.js";
  import {
    ARTIST_PREVIEW_RECIPE,
    artistPreviewPrompt,
    missingRecipeModels,
  } from "./lib/artist-gallery/previewRecipe.js";
  import type {
    ArtistPreviewStatus,
    ArtistPreviewVariant,
  } from "./lib/artist-gallery/previewRecipe.js";
  import { artistLocalPreviews } from "./lib/stores/artistLocalPreviews.svelte.js";
  import { submitGeneration } from "./lib/utils/generationSubmit.js";
  import InterrogateQuickModal from "./lib/components/generation/InterrogateQuickModal.svelte";
  import {
    fetchModelPreviewImageBytes,
    uploadModelPreviewImage,
    type ModelPreviewActionDetail,
  } from "./lib/utils/modelPreviewImage.js";
  import {
    checkStyleTransferNodesReady,
  } from "./lib/utils/styleTransferNodes.js";
  import { classifyGenerationError } from "./lib/utils/generationErrors.js";
  import { formatGenerationTime } from "./lib/utils/localeFormat.js";
  import {
    parseComfyServerError,
    type ComfyServerErrorPayload,
  } from "./lib/utils/comfyStartup.js";

  const appVersion = __APP_VERSION__ ?? "dev";
  let comfyuiVersionInfo = $state<ComfyUiVersionInfo | null>(null);
  const COMFYUI_OUTDATED_NOTIF_TITLE = "notifications.comfyui_outdated.title";

  async function checkComfyuiVersion() {
    try {
      const info = await getComfyuiVersion();
      comfyuiVersionInfo = info;
      // The in-app updater is desktop-only. Hosted/browser deployments ship
      // ComfyUI baked into the Docker image and update by pulling a newer
      // image, so an "outdated" notification there would be a dead end with no
      // update button. Still keep the fetched version for the sidebar badge.
      if (info.update_available && !isBrowserMode) {
        const alreadyNotified = notifications.notifications.some(
          (n) => n.local && n.i18n && n.title === COMFYUI_OUTDATED_NOTIF_TITLE && !n.read,
        );
        if (!alreadyNotified) {
          notifications.addLocalNotification({
            i18n: true,
            title: COMFYUI_OUTDATED_NOTIF_TITLE,
            body: "notifications.comfyui_outdated.body",
            params: { installed: info.installed ?? locale.t("settings.performance.comfyui_unknown"), target: info.target },
            kind: "warning",
          });
        }
      }
    } catch {
      // Non-critical — Settings panel will surface it when opened
    }
  }

  const visionSimClass = $derived(
    accessibility.visionSimulatorMode === "none"
      ? ""
      : `sim-${accessibility.visionSimulatorMode}`
  );

  let lastProgressEventAt = 0;

  /** Images received via WebSocket during generation, keyed by prompt_id. */
  let pendingOutputImages = new Map<string, Array<{ blob: Blob; url: string; tempFilename?: string; displayTempFilename?: string }>>();
  /** In-flight output_image fetch promises per prompt_id (for SSE race-condition avoidance). */
  let pendingOutputFetches = new Map<string, Promise<void>[]>();
  /** Wait for pending fetches with a hard time limit to prevent hanging.
   *  Sized to leave room for fetchTempImageWithRetry's backoff (~5.6s) plus the
   *  transfer of a large upscaled/16-bit refined image over a slow remote link. */
  const FETCH_TIMEOUT_MS = 45_000;
  const GENERATION_DONE_TOAST_VISIBLE_MS = 6_000;
  const GENERATION_DONE_TOAST_EXIT_MS = 220;
  type PrimaryPage = "generate" | "gallery" | "modelhub" | "artists" | "characters" | "settings";
  type GenerationDoneToast = {
    id: number;
    imageUrl: string;
    leaving: boolean;
  };
  async function awaitFetchesWithTimeout(fetches: Promise<void>[]): Promise<void> {
    const timeout = new Promise<void>((resolve) => setTimeout(resolve, FETCH_TIMEOUT_MS));
    await Promise.race([Promise.allSettled(fetches), timeout]);
  }
  /** Fetch a `_temp_image` URL, retrying a few times with exponential backoff.
   *  A transient failure here (e.g. a 401 while the LAN session token refreshes,
   *  or a slow remote link) would otherwise drop the final output image and
   *  leave the blurry progress preview stuck as the displayed result. The
   *  refined/upscaled image is the largest payload the app ever fetches, so it
   *  is the most likely to need more than one attempt before we give up. */
  async function fetchTempImageWithRetry(url: string, attempts = 4): Promise<Response> {
    let lastResp: Response | null = null;
    for (let i = 0; i < attempts; i++) {
      if (i > 0) {
        // Backoff: 0.8s, 1.6s, 3.2s.
        await new Promise((resolve) => setTimeout(resolve, 800 * 2 ** (i - 1)));
      }
      try {
        const resp = await fetch(url, { headers: authHeaders() });
        if (resp.ok) return resp;
        lastResp = resp;
      } catch {
        // network error — retry
      }
    }
    return lastResp ?? new Response(null, { status: 599, statusText: "fetch failed" });
  }
  let reconcileIntervalId: ReturnType<typeof setInterval> | null = null;
  let sseReconnectHandler: (() => void) | null = null;
  let modelPreviewActionHandler: ((event: Event) => void) | null = null;
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

  // Cross-store bridge (stores must not import each other; see CLAUDE.md):
  // keep the autocomplete builtin tag set in sync with the active model family.
  // notifyModelChanged is idempotent, so redundant runs are harmless. This
  // supersedes the imperative calls generation used to make into autocomplete.
  $effect(() => {
    autocomplete.notifyModelChanged(
      generation.isAnima || generation.isWan || generation.isQwen,
    );
  });

  // Model family/spec detection lives here rather than in ModelSelector, which
  // is unmounted while the Model panel is collapsed. Quality-tag injection keys
  // off generation.modelFamily, so detection has to run panel state aside.
  $effect(() => {
    const checkpoint = generation.checkpoint;
    const diffusionModel = generation.diffusionModel;
    const useSplitModel = generation.useSplitModel;
    // Physical folder when detection reclassified the model (e.g. a Flux unet
    // sitting in checkpoints/); null when the file is where it belongs.
    const sourceCategory = generation.modelSourceCategory;
    // Read eagerly: clearing a manual override must re-trigger detection, and
    // the async body below runs after the tracking window has closed.
    void generation.modelFamilyOverrides;

    if (useSplitModel && diffusionModel) {
      void generation.fetchAndApplyModelMetadata(
        sourceCategory ?? "diffusion_models",
        diffusionModel,
      );
    } else if (checkpoint) {
      void generation.fetchAndApplyModelMetadata(sourceCategory ?? "checkpoints", checkpoint);
    } else {
      generation.clearModelMetadata();
    }
  });

  // Document-level keyboard handler for lightbox (fallback for browser focus issues).
  // The compare viewer sits on top of the lightbox and owns Escape/arrows while
  // it is open, so stand down rather than closing the lightbox underneath it.
  $effect(() => {
    if (!gallery.lightboxOpen || gallery.compareOpen) return;
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

  function applyFontScale(scale: number) {
    document.documentElement.style.setProperty("--font-scale", String(scale));
  }

  function brandingHidden(): boolean {
    return document.documentElement.dataset.branding === "off";
  }

  function resolveThemeLogoUrl(): string {
    return getActiveThemeLogoUrl() ?? logoUrl;
  }

  let themeLogoUrl = $state(logoUrl);

  $effect(() => {
    themeLogoUrl = resolveThemeLogoUrl();
    return onThemeApplied(() => {
      themeLogoUrl = resolveThemeLogoUrl();
    });
  });

  async function upscaleImage(image: OutputImage) {
    try {
      generation.inputImage = await uploadOutputImageForGenerationInput(image, "refine_input.png");
      generation.mode = "img2img";
      generation.upscaleEnabled = true;
      // Skip the base img2img pass — the user wants to upscale this image
      // as-is, not regenerate it first.
      generation.refineOnly = true;
      currentPage = "generate";
      gallery.closeLightbox();
      gallery.showToast(locale.t("gallery.toast.loaded_upscale"), "success");
    } catch (e) {
      console.error("Failed to set up upscale:", e);
      gallery.showToast(locale.t("gallery.toast.failed_load"), "error");
    }
  }

  async function makeVideoFromImage(image: OutputImage) {
    try {
      await sendImageToVideoFrame(image);
      currentPage = "generate";
      gallery.closeLightbox();
      gallery.showToast(locale.t("gallery.toast.loaded_video_frame"), "success");
    } catch (e) {
      console.error("Failed to load image as a video frame:", e);
      gallery.showToast(locale.t("gallery.toast.failed_load"), "error");
    }
  }

  async function addImageAsVideoReference(image: OutputImage) {
    if (videoReferenceSlotsFree() === 0) {
      gallery.showToast(
        locale.t("gallery.toast.video_refs_full", { count: H3_MAX_REF_IMAGES }),
        "error",
      );
      return;
    }
    try {
      const slot = await addImageToVideoReference(image);
      currentPage = "generate";
      gallery.closeLightbox();
      gallery.showToast(
        locale.t("gallery.toast.loaded_video_reference", { index: slot }),
        "success",
      );
    } catch (e) {
      console.error("Failed to add image as a video reference:", e);
      gallery.showToast(locale.t("gallery.toast.failed_load"), "error");
    }
  }

  async function loadImageForMode(
    image: OutputImage,
    mode: "img2img" | "inpainting",
  ) {
    try {
      const prepared = await prepareOutputImageForEditMode(image, mode);
      const response = await uploadImageBytes(prepared.uploadBytes, prepared.uploadFilename);
      generation.inputImage = response.name;
      canvas.clearMask();
      generation.mode = mode;
      generation.upscaleEnabled = false;
      generation.refineOnly = false;

      if (mode === "inpainting" && prepared.normalized) {
        const normalized = prepared.normalized;
        generation.width = normalized.width;
        generation.height = normalized.height;
        progress.setLastOutputForMode("inpainting", null);

        canvas.setInpaintDrawMode("mask");
        canvas.isCanvasMode = true;
        canvas.clearStaging();
        canvas.setInpaintOriginalSource({
          previewUrl: normalized.previewUrl,
          width: normalized.width,
          height: normalized.height,
          uploadedInputName: response.name,
        });

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
        loginError = data.error ?? locale.t("auth.login_failed");
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
      changePassError = locale.t("auth.password_min_length");
      return;
    }
    if (newPass1 !== newPass2) {
      changePassError = locale.t("auth.passwords_mismatch");
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
        changePassError = data.error ?? locale.t("auth.change_password_failed");
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
  function handleVersionTap() {
    if (currentPage !== "settings") return;
    versionTapCount++;
    if (versionTapCount >= 10) {
      versionTapCount = 0;
      if (generation.devModeUnlocked) {
        generation.devModeUnlocked = false;
        generation.devMode = false;
        gallery.showToast(locale.t("app.dev_mode_disabled"), "info");
      } else {
        generation.devModeUnlocked = true;
        gallery.showToast(locale.t("app.dev_mode_unlocked"), "success");
      }
    }
  }

  function handleLightboxAreaKeydown(e: KeyboardEvent) {
    if ((e.key === "Enter" || e.key === " ") && e.target === e.currentTarget) {
      e.preventDefault();
      gallery.closeLightbox();
    }
  }

  let startupStatus = $state<string>("");
  let startupStatusKind = $state<"idle" | "manual" | "starting" | "connecting" | "error">("idle");
  let externalComfyOpen = $state(false);
  let externalComfyPayload = $state<ComfyServerErrorPayload>({ error: "" });
  let comfyServerUrl = $state("http://127.0.0.1:8188");

  let photopeaOpen = $state(false);
  let photopeaImage = $state<OutputImage | null>(null);

  /** `slug::pN` -> object URL for a locally generated preview. */
  let artistPreviewSrcs = $state<Record<string, string>>({});
  /** Non-reactive guard so the effect below does not re-trigger on its own writes. */
  const artistPreviewRequested = new Set<string>();

  // A store reacting to another store's state belongs here, not as an
  // imperative call between stores.
  $effect(() => {
    for (const [slug, slot] of Object.entries(artistLocalPreviews.previews)) {
      for (const variant of [1, 2] as const) {
        const filename = variant === 1 ? slot.p1 : slot.p2;
        if (!filename) continue;
        // Keyed by filename too, so a re-generated preview re-decodes.
        const guard = `${slug}::p${variant}::${filename}`;
        if (artistPreviewRequested.has(guard)) continue;
        artistPreviewRequested.add(guard);
        void loadGalleryImageDisplay(filename)
          .then((bytes) => {
            const blob = new Blob([new Uint8Array(bytes)], { type: "image/webp" });
            artistPreviewSrcs = {
              ...artistPreviewSrcs,
              [`${slug}::p${variant}`]: URL.createObjectURL(blob),
            };
          })
          .catch(() => {
            // The file is gone (deleted from the gallery). Drop the mapping so
            // the card falls back to its Generate button.
            artistLocalPreviews.forget(slug, variant);
          });
      }
    }
  });

  function artistPreviewStatus(
    slug: string,
    variant: ArtistPreviewVariant,
  ): ArtistPreviewStatus {
    if (artistLocalPreviews.isRunning(slug, variant)) return { state: "running" };
    const src = artistPreviewSrcs[`${slug}::p${variant}`];
    if (src) return { state: "ready", src };
    const missing = missingRecipeModels(models);
    if (missing.length > 0) return { state: "unavailable", missing };
    return { state: "idle" };
  }

  async function editInPhotopea(image: OutputImage) {
    const filename = await gallery.resolveGalleryFilename(image);
    if (!filename) {
      gallery.showToast(locale.t("gallery.persisted_only_thumb"), "warning");
      return;
    }
    photopeaImage = image.gallery_filename ? image : { ...image, gallery_filename: filename };
    photopeaOpen = true;
  }

  function showComfyStartupIssue(raw: unknown, fallbackMessage = "") {
    // Startup failed — release the lock so the error banner and settings are usable.
    startup.locked = false;
    const parsed = parseComfyServerError(raw, fallbackMessage);
    externalComfyPayload = parsed;
    externalComfyOpen = true;
    startupStatus = locale.t("app.status.failed_to_start", {
      message: parsed.error ?? fallbackMessage,
    });
    startupStatusKind = "error";
  }

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
                  : (image.prompt_id || locale.t("gallery.no_prompt_id"));
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

  // Proactive "report a problem" flow: the sidebar bug button opens the same
  // proxy-backed report modal as an error, using a synthetic user-initiated error.
  let showBugReport = $state(false);
  const userInitiatedReport: FriendlyError = {
    code: "user_report",
    title: "",
    what: "",
    why: "",
    fixes: [],
    reportable: true,
    raw: "User-initiated report (no error)",
  };
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
  let contextMenuUrl = $state<string | null>(null);
  let contextMenuX = $state(0);
  let contextMenuY = $state(0);
  let showContextMenu = $state(false);
  // Dev-only error gallery, opened with #error-gallery. Guarded so it never ships in production.
  let showErrorGallery = $state(
    import.meta.env.DEV && globalThis.location?.hash === "#error-gallery",
  );

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

  function handleCharacterInsert(character: AnimadexCharacter) {
    characterInsert.request(character);
    if (!characterInsert.pending) {
      currentPage = "generate";
    }
  }

  function buildArtistPreviewParams(tag: string, variant: ArtistPreviewVariant): GenerationParams {
    const r = ARTIST_PREVIEW_RECIPE;
    // Start from the user's current settings so backend-only fields (output
    // format, bit depth, ...) stay valid, then pin every field that can change
    // the image.
    //
    // TRADE-OFF: a generation field added in future inherits the user's
    // current value unless it is added below. If a new field can change the
    // image, pin it here.
    return {
      ...generation.toParams(),
      mode: "txt2img",
      positive_prompt: artistPreviewPrompt(tag, variant),
      negative_prompt: r.negativePrompt,
      positive_segments: [],
      negative_segments: [],
      detail_segments: [],
      positive_regions: undefined,
      use_split_model: true,
      diffusion_model: r.unet,
      clip_model: r.textEncoder,
      clip_type: r.clipType,
      vae: r.vae,
      checkpoint: "",
      model_source_category: null,
      model_architecture: r.architecture,
      is_sdxl_like: false,
      is_vpred_model: false,
      sampler_name: r.sampler,
      scheduler: r.scheduler,
      steps: r.steps,
      cfg: r.cfg,
      denoise: r.denoise,
      seed: r.seed,
      width: r.width,
      height: r.height,
      batch_size: 1,
      loras: [],
      controlnet: null,
      upscale_enabled: false,
      save_pre_upscale_image: false,
      facefix_enabled: false,
      smart_guidance: false,
      differential_diffusion: false,
      refine_only: false,
      input_image: null,
      mask_image: null,
      grow_mask_by: null,
      style_transfer_enabled: false,
      style_reference_image: null,
      edit_reference_images: [],
    };
  }

  async function handleArtistGeneratePreview(
    slug: string,
    tag: string,
    variant: ArtistPreviewVariant,
  ) {
    if (artistLocalPreviews.isRunning(slug, variant)) return;
    const missing = missingRecipeModels(models);
    if (missing.length > 0) {
      gallery.showToast(
        locale.t("artist_gallery.generate_preview_missing", { models: missing.join(", ") }),
        "error",
      );
      return;
    }
    try {
      const promptId = await submitGeneration(buildArtistPreviewParams(tag, variant));
      artistLocalPreviews.attach(promptId, slug, variant);
      gallery.showToast(locale.t("artist_gallery.preview_queued", { tag }), "info");
    } catch (err) {
      artistLocalPreviews.fail(slug, variant);
      // Same classification GenerateButton uses: turn a raw submission failure
      // (stale model cache, OOM, missing node) into actionable text, falling
      // back to the raw message when nothing matched.
      const message = err instanceof Error ? err.message : String(err);
      const classified = classifyGenerationError(message);
      const detail =
        classified.messageKey === "generation.toast.failed"
          ? message
          : locale.t(classified.messageKey, classified.params);
      gallery.showToast(locale.t("artist_gallery.preview_failed", { error: detail }), "error");
    }
  }

  function finishCharacterInsert() {
    characterInsert.dismiss();
    currentPage = "generate";
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

  async function setGalleryImageAsModelThumb(
    image: OutputImage,
    category: "checkpoints" | "loras",
    filename: string,
  ) {
    if (!image.gallery_filename) {
      gallery.showToast(locale.t("gallery.persisted_only_thumb"), "warning");
      return;
    }
    try {
      await saveModelSidecarThumbnail({
        category,
        filename,
        galleryFilename: image.gallery_filename,
      });
      gallery.showToast(locale.t("checkpoint.sidecar_saved"), "success");
    } catch (e) {
      gallery.showToast(
        locale.t("checkpoint.sidecar_failed", {
          message: e instanceof Error ? e.message : String(e),
        }),
        "error",
      );
    }
  }

  function openLightboxContextMenu(e: MouseEvent) {
    // The lightbox shows either a persisted OutputImage (full menu) or a raw
    // preview URL (save/copy/inpaint only). Restore right-click-to-save that the
    // global native-menu suppressor (#392) took away, in both desktop and browser.
    if (gallery.selectedImage) {
      contextMenuImage = gallery.selectedImage;
      contextMenuUrl = null;
    } else if (gallery.lightboxUrl) {
      contextMenuUrl = gallery.lightboxUrl;
      contextMenuImage = null;
    } else {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    contextMenuX = e.clientX;
    contextMenuY = e.clientY;
    showContextMenu = true;
  }

  /**
   * One menu entry drives the whole compare flow: pin the first image, then
   * pick a second one (or un-pin the same image to back out).
   */
  function comparePinLabel(image: OutputImage) {
    if (!gallery.comparePin) return locale.t("gallery.compare.pin");
    if (gallery.comparePin === image) return locale.t("gallery.compare.unpin");
    return locale.t("gallery.compare.with_pinned");
  }

  const contextMenuItems = $derived.by((): ContextMenuItem[] => {
    const image = contextMenuImage;
    if (!image) {
      const url = contextMenuUrl;
      if (!url) return [];
      return [
        { label: locale.t("gallery.get_tags"), action: () => void interrogateFromPreviewUrl(url) },
        { label: "", action: () => {}, separator: true },
        { label: locale.t("gallery.inpaint"), action: () => void inpaintLightboxPreview() },
        { label: "", action: () => {}, separator: true },
        { label: locale.t("gallery.save_as"), action: () => void gallery.saveBlobAs(url, `preview_${Date.now()}.png`) },
        { label: locale.t("gallery.copy"), action: () => void gallery.copyBlobToClipboard(url) },
      ];
    }
    const items: ContextMenuItem[] = [
      { label: locale.t("gallery.get_tags"), action: () => interrogateFromGallery(image) },
    ];
    if (generation.checkpoint) {
      items.push({
        label: locale.t("gallery.use_as_checkpoint_thumb"),
        action: () => void setGalleryImageAsModelThumb(image, "checkpoints", generation.checkpoint!),
      });
    }
    const thumbLora = generation.loras.find((l) => l.enabled && l.name)?.name;
    if (thumbLora) {
      items.push({
        label: locale.t("gallery.use_as_lora_thumb"),
        action: () => void setGalleryImageAsModelThumb(image, "loras", thumbLora),
      });
    }
    items.push(
      { label: "", action: () => {}, separator: true },
      { label: locale.t("gallery.img2img"), action: () => img2imgImage(image) },
      { label: locale.t("gallery.inpaint"), action: () => inpaintImage(image) },
      ...(!image.is_upscaled ? [{ label: locale.t("gallery.upscale"), action: () => upscaleImage(image) }] : []),
      ...(!isVideoImage(image)
        ? [
            { label: locale.t("gallery.make_video"), action: () => makeVideoFromImage(image) },
            {
              label: locale.t("gallery.add_video_reference"),
              action: () => addImageAsVideoReference(image),
            },
          ]
        : []),
      { label: "", action: () => {}, separator: true },
      { label: comparePinLabel(image), action: () => gallery.toggleComparePin(image) },
      { label: "", action: () => {}, separator: true },
      { label: locale.t("gallery.save_as"), action: () => gallery.saveImageAs(image) },
      { label: locale.t("gallery.copy"), action: () => gallery.copyToClipboard(image) },
      { label: "", action: () => {}, separator: true },
      { label: locale.t("gallery.delete"), action: () => gallery.deleteImage(image), destructive: true },
    );
    return items;
  });

  async function interrogateFromPreviewUrl(url: string) {
    showInterrogateModal = true;
    interrogateLoading = true;
    interrogateResult = null;
    interrogateStage = null;
    interrogateDownloadProgress = null;
    interrogateError = null;
    interrogateImageUrl = url.startsWith("data:") ? url : null;

    const unlistenDownload = await ipcListen(
      "interrogator:download_progress",
      (event) => {
        if (event.payload.done) {
          interrogateDownloadProgress = null;
        } else {
          interrogateDownloadProgress = event.payload;
        }
      },
    );

    const unlistenStage = await ipcListen("interrogator:stage", (event) => {
      interrogateStage = event.payload;
    });

    try {
      const { bytes } = await fetchModelPreviewImageBytes(url, "model_preview.png");
      const uint8 = new Uint8Array(bytes);
      let binary = "";
      for (let i = 0; i < uint8.length; i++) {
        binary += String.fromCharCode(uint8[i]);
      }
      interrogateResult = await interrogateImage(btoa(binary));
      if (!interrogateImageUrl && url.startsWith("http")) {
        interrogateImageUrl = url;
      }
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

  async function img2imgFromPreviewUrl(url: string) {
    try {
      const name = await uploadModelPreviewImage(url, "model_preview.png");
      generation.inputImage = name;
      canvas.clearMask();
      generation.maskImage = null;
      generation.mode = "img2img";
      currentPage = "generate";
      gallery.showToast(locale.t("gallery.toast.loaded_img2img"), "success");
    } catch (e) {
      console.error("Failed to load preview for img2img:", e);
      gallery.showToast(locale.t("gallery.toast.failed_load"), "error");
    }
  }

  async function setStyleReferenceFromPreviewUrl(url: string) {
    try {
      const name = await uploadModelPreviewImage(url, "style_reference.png");
      generation.styleReferenceImage = name;
      generation.styleTransferEnabled = true;
      generation.controlnetEnabled = false;
      generation.upscaleEnabled = false;
      generation.facefixEnabled = false;
      generation.mode = "txt2img";
      if (!["euler_ancestral", "euler_a"].includes(generation.samplerName)) {
        generation.samplerName = "euler_ancestral";
      }
      generation.saveSettings();
      currentPage = "generate";
      gallery.showToast(locale.t("generation.style_transfer.reference_loaded"), "success");

      // Actually ensure style transfer nodes are installed and take action when the
      // feature is activated via a preview "Style" button (e.g. on a LoRA card).
      // This is the path that previously left users with a stuck "generates but no output"
      // state. We check, and if missing we action the git installs for the two required
      // packages (the same ones the backend ensure logic uses on startup).
      try {
        const nodesReady = await checkStyleTransferNodesReady();
        if (!nodesReady) {
          gallery.showToast(locale.t("generation.style_transfer.nodes_install"), "info");
          if (!isBrowserMode) {
            // Action the installs only in desktop mode (clones the repos into the local custom_nodes).
            // In browser/LAN mode the remote ComfyUI must have the nodes pre-installed (via its server build).
            await installCustomNode(
              "https://github.com/BigStationW/ComfyUi-Untwisting-RoPE.git",
              "ComfyUi-Untwisting-RoPE",
            );
            await installCustomNode(
              "https://github.com/BigStationW/ComfyUi-Scale-Image-to-Total-Pixels-Advanced.git",
              "ComfyUi-Scale-Image-to-Total-Pixels-Advanced",
            );
          }
          // Full ComfyUI restart (via Settings or next app launch) is still needed for the
          // nodes to load; the warning banner in the Style Transfer panel will guide the user.
        }
      } catch (ensureErr) {
        console.warn("Style transfer node ensure/install during preview activation:", ensureErr);
      }
    } catch (e) {
      console.error("Failed to load style reference:", e);
      gallery.showToast(locale.t("generation.style_transfer.upload_error"), "error");
    }
  }

  async function handleModelPreviewAction(event: Event) {
    const detail = (event as CustomEvent<ModelPreviewActionDetail>).detail;
    if (!detail?.imageUrl) return;
    if (detail.type === "interrogate") {
      await interrogateFromPreviewUrl(detail.imageUrl);
    } else if (detail.type === "img2img") {
      await img2imgFromPreviewUrl(detail.imageUrl);
    } else if (detail.type === "style_reference") {
      await setStyleReferenceFromPreviewUrl(detail.imageUrl);
    }
  }

  async function interrogateFromGallery(image: OutputImage) {
    showInterrogateModal = true;
    interrogateLoading = true;
    interrogateResult = null;
    interrogateStage = null;
    interrogateDownloadProgress = null;
    interrogateError = null;
    interrogateImageUrl = image.thumbnailUrl || image.url || null;

    const unlistenDownload = await ipcListen(
      "interrogator:download_progress",
      (event) => {
        if (event.payload.done) {
          interrogateDownloadProgress = null;
        } else {
          interrogateDownloadProgress = event.payload;
        }
      }
    );

    const unlistenStage = await ipcListen("interrogator:stage", (event) => {
      interrogateStage = event.payload;
    });

    try {
      let result;
      if (image.gallery_filename) {
        result = await interrogateGalleryImage(image.gallery_filename);
      } else {
        // Session images aren't in the gallery DB yet, and their synthetic
        // filename doesn't exist in ComfyUI's output dir — resolve the pixels
        // from the in-memory session blob / temp file instead (issue #397).
        const bytes = await gallery.resolveImagePngBytes(image);
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

  // --- Quick interrogate (sidebar button + compact modal) ---
  let showInterrogateQuickModal = $state(false);
  let interrogateSidebarBtn = $state<HTMLButtonElement | undefined>();

  // One-time speech bubble pointing at the sidebar interrogate button, for
  // users who knew the feature from its pre-v1.6.0 spot in the panel list
  // (issue #488). UI layout pref → localStorage, like the combined-prompt
  // toggle. Using the button also dismisses it: the feature was found.
  const INTERROGATE_HINT_KEY = "mooshieui.hint.interrogate-sidebar.v1";
  let showInterrogateHint = $state(localStorage.getItem(INTERROGATE_HINT_KEY) !== "true");
  function dismissInterrogateHint() {
    showInterrogateHint = false;
    try { localStorage.setItem(INTERROGATE_HINT_KEY, "true"); } catch {}
  }

  /** Shared runner for the quick-interrogate flows: opens the results modal,
   *  wires the progress/stage listeners, then runs the supplied IPC call. */
  async function runQuickInterrogation(previewUrl: string | null, run: () => Promise<InterrogationResult>) {
    showInterrogateQuickModal = false;
    showInterrogateModal = true;
    interrogateLoading = true;
    interrogateResult = null;
    interrogateStage = null;
    interrogateDownloadProgress = null;
    interrogateError = null;
    if (interrogateImageUrl?.startsWith("blob:")) URL.revokeObjectURL(interrogateImageUrl);
    interrogateImageUrl = previewUrl;

    const unlistenDownload = await ipcListen(
      "interrogator:download_progress",
      (event) => {
        interrogateDownloadProgress = event.payload.done ? null : event.payload;
      },
    );
    const unlistenStage = await ipcListen("interrogator:stage", (event) => {
      interrogateStage = event.payload;
    });

    try {
      interrogateResult = await run();
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

  /** Interrogate an in-memory image file (browser file input / drag-drop). */
  async function interrogateFileQuick(file: File) {
    const previewUrl = URL.createObjectURL(file);
    const bytes = new Uint8Array(await file.arrayBuffer());
    let binary = "";
    for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
    await runQuickInterrogation(previewUrl, () => interrogateImage(btoa(binary)));
  }

  /** Interrogate an image by filesystem path (Tauri dialog / native drop). */
  async function interrogatePathQuick(path: string) {
    let previewUrl: string | null = null;
    if (isTauri) {
      try {
        const { readFile } = await import("@tauri-apps/plugin-fs");
        const bytes = await readFile(path);
        previewUrl = URL.createObjectURL(new Blob([bytes]));
      } catch {
        previewUrl = null;
      }
    }
    await runQuickInterrogation(previewUrl, () => interrogateImagePath(path));
  }

  /** Interrogate whatever image is on the clipboard. */
  async function interrogatePasteQuick() {
    if (isBrowserMode) {
      try {
        const bytes = await readClipboardImageSafe();
        if (!bytes || bytes.length === 0) {
          showInterrogateQuickModal = false;
          showInterrogateModal = true;
          interrogateLoading = false;
          interrogateResult = null;
          interrogateError = locale.t("common.no_clipboard_image");
          return;
        }
        const blob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
        const previewUrl = URL.createObjectURL(blob);
        const base64 = await new Promise<string>((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => resolve((reader.result as string).split(",")[1]);
          reader.onerror = () => reject(reader.error);
          reader.readAsDataURL(blob);
        });
        await runQuickInterrogation(previewUrl, () => interrogateImage(base64));
      } catch (e) {
        showInterrogateQuickModal = false;
        showInterrogateModal = true;
        interrogateLoading = false;
        interrogateResult = null;
        interrogateError = e instanceof Error ? e.message : String(e);
      }
      return;
    }
    await runQuickInterrogation(null, () => interrogateClipboard());
  }

  /** Tauri-only: pick an image via the native dialog, then interrogate it. */
  async function browseInterrogateQuick() {
    if (!isTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (!selected) return;
    await interrogatePathQuick(selected as string);
  }

  // Drag-hover-to-open: hovering a dragged image over the sidebar button for
  // ~1.5s pops the quick modal. Timer is shared by the HTML5 (browser) and
  // native Tauri drag paths below.
  const INTERROGATE_HOVER_MS = 1500;
  let interrogateHoverTimer: ReturnType<typeof setTimeout> | null = null;

  function startInterrogateHoverTimer() {
    if (interrogateHoverTimer || showInterrogateQuickModal) return;
    interrogateHoverTimer = setTimeout(() => {
      interrogateHoverTimer = null;
      showInterrogateQuickModal = true;
    }, INTERROGATE_HOVER_MS);
  }

  function cancelInterrogateHoverTimer() {
    if (interrogateHoverTimer) {
      clearTimeout(interrogateHoverTimer);
      interrogateHoverTimer = null;
    }
  }

  let unlistenInterrogateDragDrop: (() => void) | null = null;

  /** App-level native drag-drop wiring for the interrogate sidebar button/modal.
   *  Coexists with GenerationPage's own listener (multiple listeners are allowed;
   *  each hit-tests its own elements). */
  async function setupInterrogateTauriDragDrop() {
    if (!isTauri) return;
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const webview = getCurrentWebview();
    const scaleFactor = await getCurrentWindow().scaleFactor();

    const isOverButton = (cssX: number, cssY: number): boolean => {
      const rect = interrogateSidebarBtn?.getBoundingClientRect();
      if (!rect) return false;
      return cssX >= rect.left && cssX <= rect.right && cssY >= rect.top && cssY <= rect.bottom;
    };
    const isOverModal = (cssX: number, cssY: number): boolean => {
      const el = document.elementFromPoint(cssX, cssY);
      return !!(el as HTMLElement | null)?.closest?.("[data-interrogate-quick-modal]");
    };

    unlistenInterrogateDragDrop = await webview.onDragDropEvent(async (event) => {
      const payload = event.payload;
      if (payload.type === "enter" || payload.type === "over") {
        const cssX = payload.position.x / scaleFactor;
        const cssY = payload.position.y / scaleFactor;
        if (isOverButton(cssX, cssY)) {
          startInterrogateHoverTimer();
        } else {
          cancelInterrogateHoverTimer();
        }
      } else if (payload.type === "leave") {
        cancelInterrogateHoverTimer();
      } else if (payload.type === "drop") {
        const cssX = payload.position.x / scaleFactor;
        const cssY = payload.position.y / scaleFactor;
        cancelInterrogateHoverTimer();
        // Only claim the drop when it lands on the button or the open quick modal;
        // otherwise leave it to GenerationPage's drop handler.
        if (!isOverButton(cssX, cssY) && !(showInterrogateQuickModal && isOverModal(cssX, cssY))) return;
        const imgPath = payload.paths.find((p) => /\.(png|jpe?g|webp)$/i.test(p));
        if (!imgPath) return;
        await interrogatePathQuick(imgPath);
      }
    });
  }

  function getImageTimestamp(image: OutputImage): number {
    return image.generated_at_ms ?? 0;
  }

  function getImageSize(image: OutputImage): number {
    return image.file_size_bytes ?? 0;
  }

  function formatDate(ts: number | undefined): string {
    if (!ts) return locale.t("gallery.date.unknown");
    return locale.formatDateTime(ts);
  }

  function formatDateGroup(ts: number | undefined): string {
    if (!ts) return locale.t("gallery.date.unknown_date");
    return new Date(ts).toLocaleDateString(locale.intlTag, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  }

  function formatMonthGroup(ts: number | undefined): string {
    if (!ts) return locale.t("gallery.date.unknown_month");
    return new Date(ts).toLocaleDateString(locale.intlTag, {
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
    // Re-append <segment:...> tags in canonical closed form so reimport/remix
    // restores them (the params prompt itself is segment-stripped).
    const positiveWithSegments = params.detail_segments?.length
      ? [params.positive_prompt, serializeSegmentTags(params.detail_segments)]
          .filter(Boolean)
          .join(", ")
      : params.positive_prompt;
    const metadata: Record<string, string> = {
      positive_prompt: positiveWithSegments,
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
          // Assign the string as-is: Number() would round 63-bit seeds past 2^53.
          const seed = metadata.seed.trim();
          if (/^\d+$/.test(seed)) generation.seed = seed;
          gallery.showToast(locale.t("gallery.toast.applied_seed"), "success");
        } else {
          gallery.showToast(locale.t("gallery.toast.no_seed"), "info");
        }
        return;
      }

      // Mode before the prompts: image and video keep separate prompt buckets, so
      // switching modes swaps what is in the prompt boxes. Writing the prompt first
      // would park it in the bucket we are leaving and then overwrite it.
      if (metadata.mode === "txt2img" || metadata.mode === "img2img" || metadata.mode === "inpainting") {
        generation.mode = metadata.mode;
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
        generation.seed = "-1";
        gallery.showToast(locale.t("gallery.toast.loaded_remix"), "success");
        return;
      }

      if (metadata.seed !== undefined && /^\d+$/.test(metadata.seed.trim())) {
        generation.seed = metadata.seed.trim();
      }
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
    return locale.formatBytes(bytes);
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

  /** Recover output images from the server temp cache (browser mode) when the
   *  output_image SSE event was dropped or its fetch failed. Without this the
   *  blurry progress frame stays as the displayed result. The `_temp_image`
   *  endpoint transcodes JXL to WebP, so the blobs are always displayable. */
  async function recoverMissingOutputImages(
    promptId: string,
    images: Array<{ blob: Blob; url: string; tempFilename?: string; displayTempFilename?: string }>,
  ): Promise<void> {
    if (isTauri) return;
    try {
      const recovered = await recoverPromptOutputs(promptId);
      for (const imgRef of recovered.images) {
        const tempFn = imgRef.temp_filename?.trim();
        if (!tempFn) continue;
        progress.registerPromptOutput(promptId, tempFn);
        try {
          const resp = await fetch(
            `/internal-api/_temp_image/${encodeURIComponent(tempFn)}`,
            { headers: authHeaders() },
          );
          if (resp.ok) {
            const blob = await resp.blob();
            const url = URL.createObjectURL(blob);
            images.push({ blob, url, tempFilename: tempFn });
          }
        } catch { /* individual image fetch failed */ }
      }
    } catch { /* recovery command failed */ }
  }

  /**
   * Finalize images received via WebSocket during generation.
   * MooshieSaveImage sends PNG bytes directly over WS — no disk round-trip.
   */
  async function prepareLatestInpaintResult(image: OutputImage, sourceVersion: number) {
    try {
      const prepared = await prepareOutputImageForEditMode(image, "inpainting");
      const normalized = prepared.normalized;
      if (!normalized) return;

      const response = await uploadImageBytes(prepared.uploadBytes, prepared.uploadFilename);
      if (
        generation.mode !== "inpainting" ||
        !canvas.isCanvasMode ||
        canvas.inpaintSourceVersion !== sourceVersion
      ) {
        URL.revokeObjectURL(normalized.previewUrl);
        return;
      }
      // Display-only: preview the result without advancing the base, so the next
      // "Generate" re-rolls the original base + mask. "Apply" promotes it later.
      canvas.setPendingInpaintResult({
        previewUrl: normalized.previewUrl,
        width: normalized.width,
        height: normalized.height,
        uploadedInputName: response.name,
        owned: true,
      });
    } catch (e) {
      console.error("Failed to prepare latest inpaint result:", e);
    }
  }

  function finalizeOutputImages(
    promptId: string,
    mode: GenerationMode,
    wasUpscaled: boolean,
    params: GenerationParams | null,
    images: Array<{ blob: Blob; url: string; tempFilename?: string; displayTempFilename?: string }>,
    generationTimeMs?: number,
  ) {
    if (images.length === 0) return;

    const newImages: OutputImage[] = images.map((img, i) => {
      const ext =
        img.blob.type === "image/jxl" ? "jxl" : img.blob.type === "image/webp" ? "webp" : "png";
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
        generationTimeMs,
      };
    });

    gallery.addImages(newImages);
    progress.setLastOutputForMode(mode, newImages[0]?.url ?? null);
    if (mode === "inpainting" && generation.mode === "inpainting" && canvas.isCanvasMode && newImages[0]) {
      const sourceVersion = canvas.inpaintSourceVersion;
      void prepareLatestInpaintResult(newImages[0], sourceVersion);
    }

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

    // Route a finished artist-preview generation back to its placeholder card.
    // Must run before the style-thumbnail block below, which returns early.
    const previewTarget = artistLocalPreviews.resolve(promptId);
    if (previewTarget) {
      const firstImage = newImages[0];
      if (!firstImage) {
        artistLocalPreviews.fail(previewTarget.slug, previewTarget.variant);
      } else {
        const persistPromise = gallery.getPersistPromise(firstImage);
        if (persistPromise) {
          void persistPromise
            .then((galleryFilename) => {
              if (galleryFilename) {
                artistLocalPreviews.record(
                  previewTarget.slug,
                  previewTarget.variant,
                  galleryFilename,
                );
              } else {
                artistLocalPreviews.fail(previewTarget.slug, previewTarget.variant);
              }
            })
            .catch(() => artistLocalPreviews.fail(previewTarget.slug, previewTarget.variant));
        } else {
          artistLocalPreviews.fail(previewTarget.slug, previewTarget.variant);
        }
      }
    }

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

    notifications.addLocalNotification({
      title: locale.t("generation.toast.image_ready"),
      body: images.length > 1
        ? locale.t("generation.notification.images_ready_body", { count: images.length })
        : locale.t("generation.notification.image_ready_body"),
      kind: "success",
    });

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
    mode: GenerationMode,
  ) {
    try {
      const loadImg = (src: string) => new Promise<HTMLImageElement>((resolve, reject) => {
        const img = new Image();
        img.onload = () => resolve(img);
        img.onerror = reject;
        img.src = src;
      });

      const includeBranding = !brandingHidden();
      const logoSource = resolveThemeLogoUrl();
      const imgElements = await Promise.all(cellImages.map(({ url }) => loadImg(url)));
      const logoImg = includeBranding
        ? await loadImg(logoSource).catch(() => loadImg(logoUrl)).catch(() => null)
        : null;

      const cellW = Math.max(...imgElements.map(img => img.naturalWidth));
      const cellH = Math.max(...imgElements.map(img => img.naturalHeight));
      const gap = 4;
      const fontSize = Math.max(14, Math.round(cellW * 0.028));
      const labelFont = `600 ${fontSize}px sans-serif`;
      const labelH = fontSize + 10;

      // Reserve footer space for the watermark below the grid (when branding is enabled)
      const wmSize = Math.max(20, Math.round(cellW * 0.045));
      const wmFont = `600 ${Math.round(wmSize * 0.8)}px sans-serif`;
      const wmPad = Math.round(wmSize * 0.5);
      const footerH = includeBranding ? wmSize + wmPad * 2 + wmPad : 0;

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

      if (includeBranding && logoImg) {
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
      }

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
        generation_mode: mode,
        is_upscaled: false,
        url: gridUrl,
        file_size_bytes: gridBlob.size,
        generated_at_ms: Date.now(),
      };

      gallery.addImages([gridImage]);
      gallery.persistImages([gridImage], undefined, [gridBlob], generation.metadataMode);
      // Mirror the single-image path (finalizeOutputImages) so a completed grid
      // also surfaces a done toast / notification when off the generate page.
      showGenerationDoneToast([gridImage]);
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

    // Native drag-drop wiring for the interrogate sidebar button (Tauri only).
    setupInterrogateTauriDragDrop();

    // Suppress the native WebView context menu (the "Share", "Save As" (html),
    // "Print" and "Send link to..." entries that point at tauri.localhost and make
    // no sense in-app) everywhere except editable text, where the native
    // copy/paste/spellcheck menu is still useful. App-specific right-click menus
    // (gallery/session images, artist tags) call preventDefault themselves and are
    // unaffected — this only blocks the default menu where nothing else handles it (#392).
    window.addEventListener("contextmenu", (e) => {
      const target = e.target as HTMLElement | null;
      if (target?.closest('input, textarea, [contenteditable="true"]')) return;
      e.preventDefault();
    });

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
    notifications.startPolling();

    // Check auth for browser mode LAN access (before any ipcInvoke calls)
    const authOk = await checkAuth();
    if (!authOk) return;

    // "Rerun setup" from Settings sets a one-shot flag and reloads. check_setup
    // would otherwise auto-recover the completion marker and skip the wizard, so
    // honor the flag here and force it. Running in a fresh session also avoids
    // re-registering initApp()'s listeners; initApp runs after the wizard finishes.
    if (localStorage.getItem("mooshieui_force_setup") === "1") {
      localStorage.removeItem("mooshieui_force_setup");
      setupComplete = false;
      return;
    }

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
    // Block interaction until ComfyUI is reachable. Settings load asynchronously,
    // and a save triggered mid-load would persist in-memory defaults over the
    // restored values; model-family detection is in flight over the same window.
    startup.locked = true;
    // Never let the lock stick if no unlock path is reached (unreachable server,
    // dropped event); the banner still reports the real startup state.
    setTimeout(() => { startup.locked = false; }, 120_000);

    // Apply UI preferences (theme, font scale) immediately
    try {
      const cfg = await getConfig();
      applyTheme(cfg);
      applyFontScale(cfg.font_scale);
      autoStartEnabled = cfg.auto_start !== false;
      comfyServerUrl = cfg.server_url || `http://127.0.0.1:${cfg.server_port ?? 8188}`;
    } catch {
      // Config not ready yet, defaults are fine
    }

    // Load persisted settings
    await Promise.all([generation.loadSettings(), autocomplete.loadSettings(), locale.loadSettings()]);

    // Browser/LAN mode: pull the server-side preference snapshot (or seed it
    // from current local state) once local settings have loaded and the auth
    // token is available. No-op in Tauri desktop mode.
    if (isBrowserMode) {
      void prefsSync.loadAndApply();
    }

    // Prompt assistant: detect hardware + pre-select recommended model at launch.
    promptAssistant.init();

    // Set up event listeners BEFORE starting so we don't miss events
    await Promise.all([
      ipcListen("comfyui:connection", (event: any) => {
        console.log("Connection event:", event.payload);
        connection.connected = event.payload.connected;
        if (event.payload.connected) {
          startup.locked = false;
          startupStatus = "";
          startupStatusKind = "idle";
          models.refresh().then(() => {
            generation.applyDefaultsIfNeeded(models.checkpoints, models.vaes);
          });
        }
      }),
      ipcListen("comfyui:server_ready", async () => {
        console.log("Server ready event received");
        // Unlock here too: with zero installed checkpoints connection.connected
        // is never set, so the connection handler above would never fire.
        startup.locked = false;
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
        showComfyStartupIssue(
          event.payload,
          event.payload?.error || locale.t("app.status.unknown_error"),
        );
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
        artistLocalPreviews.failAll();
        compare.clearGridBatch();
      }),
      ipcListen("comfyui:preview", async (event: any) => {
        const data = event.payload;
        if (!progress.isGenerating) return;
        // Filter by prompt_id — reject events for other users' prompts
        if (data.prompt_id && !progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) return;
        if (data.prompt_id && progress.activePromptId && data.prompt_id !== progress.activePromptId) return;

        if (data.temp_filename) {
          // The worker WebSocket delivers preview frames as temp-file
          // references (not inline base64). On desktop there is no HTTP server,
          // so a fetch to /internal-api would resolve to the SPA shell and
          // produce a broken <img>. Read the temp file via invoke() instead,
          // mirroring the output_image handler. See issue #309.
          try {
            let blob: Blob;
            if (isTauri) {
              const rawBytes = await readTempImage(data.temp_filename);
              const mime = data.format === "png" ? "image/png" : "image/jpeg";
              blob = new Blob([new Uint8Array(rawBytes)], { type: mime });
            } else {
              // SSE/browser path: fetch image from temp endpoint
              const resp = await fetch(`/internal-api/_temp_image/${encodeURIComponent(data.temp_filename)}`, {
                headers: authHeaders(),
              });
              if (!resp.ok) return;
              blob = await resp.blob();
            }
            // The prompt may have completed while the fetch was in flight
            // (common on remote browser mode with inpaint chains). Setting
            // previewImage now would overwrite the finalized output in the
            // lightbox with a stale blurry preview frame — and nothing would
            // ever clear it. The pendingPrompts check also covers the window
            // between one queued prompt completing and the next starting,
            // when activePromptId is briefly null.
            if (
              !progress.isGenerating ||
              (data.prompt_id && !progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) ||
              (data.prompt_id && progress.activePromptId && data.prompt_id !== progress.activePromptId)
            ) {
              return;
            }
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

        const pid = data.prompt_id ?? progress.activePromptId;
        if (typeof data.temp_filename === "string" && data.temp_filename.trim() && pid) {
          progress.registerPromptOutput(pid, data.temp_filename);
        }

        if (!progress.isGenerating) return;
        // Filter by prompt_id — reject events for other users' prompts
        if (data.prompt_id && !progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) return;

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
                  // PNG and WebP are both WebView2-renderable, so the canonical
                  // file doubles as the display copy — no second temp file.
                  const rawBytes = await readTempImage(data.temp_filename);
                  const mime = data.format === "webp" ? "image/webp" : "image/png";
                  blob = new Blob([new Uint8Array(rawBytes)], { type: mime });
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
                    fetchTempImageWithRetry(
                      `/internal-api/_temp_image/${encodeURIComponent(data.temp_filename)}?raw=true`,
                    ),
                    fetchTempImageWithRetry(displayUrl),
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
                  const resp = await fetchTempImageWithRetry(
                    `/internal-api/_temp_image/${encodeURIComponent(data.temp_filename)}`,
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
            // `display_format` only accompanies JXL; a plain WebP output is its
            // own display copy, so fall back to the canonical format.
            const displayMime =
              data.display_format === "webp" || data.format === "webp" ? "image/webp" : "image/png";
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
      ipcListen("comfyui:output_video", async (event: any) => {
        // MooshieSaveVideo (WS event 102) has already moved the mp4 and its
        // poster sidecar into the gallery directory and indexed them, so the
        // payload carries a gallery filename, not bytes.
        //
        // NOTE: this event is not registered with cache_temp_event, so an SSE
        // client that connects late gets no replay. Adding the entry here is
        // what makes the video appear without a manual refresh; late clients
        // fall back to the normal loadFromDisk() listing.
        const data = event.payload;
        const videoFilename = data?.video_filename;
        if (typeof videoFilename !== "string" || !videoFilename) return;

        // Filter by prompt_id, matching the output_image listener: reject
        // events belonging to another user's prompt in browser mode.
        if (data.prompt_id && !progress.pendingPrompts.some((p: any) => p.promptId === data.prompt_id)) return;

        const durationSeconds =
          typeof data.duration_seconds === "number" ? data.duration_seconds : undefined;
        const videoFps = typeof data.fps === "number" && data.fps > 0 ? data.fps : undefined;
        const generationTimeMs = data.prompt_id ? progress.peekDurationMs(data.prompt_id) : undefined;

        await gallery.addPersistedImage(
          videoFilename,
          { duration_seconds: durationSeconds, fps: videoFps, generationTimeMs },
          true,
        );

        // Play it in the progress preview. The gallery URL is Range-served, so
        // the preview never buffers the whole clip.
        progress.lastOutputVideo = await gallery.loadFullImage(videoFilename);
        progress.lastOutputVideoFps = videoFps ?? null;
        progress.lastOutputVideoFilename = videoFilename;
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

          // Read the collected output images first so completePrompt knows
          // whether a real final image is about to replace the preview frame.
          const images = pendingOutputImages.get(promptId) ?? [];
          const item = progress.completePrompt(promptId, images.length > 0);
          promptLastActivity.delete(promptId);
          if (item) {
            for (const img of images) {
              const tempFn = img.tempFilename?.trim();
              if (tempFn) progress.registerPromptOutput(promptId, tempFn);
            }
            pendingOutputImages.delete(promptId);

            if (images.length === 0) {
              // The output_image event was dropped or its fetch failed — try
              // to recover from the server temp cache so the final image
              // replaces the blurry progress frame.
              await recoverMissingOutputImages(promptId, images);
            }

            const skipGallery = shouldSuppressRegionalChainGallerySave(promptId);
            if (skipGallery) {
              clearRegionalChainGallerySuppress(promptId);
              console.log("[regional] Skipping gallery save for chain intermediate:", promptId);
            } else {
              finalizeOutputImages(promptId, item.mode, item.wasUpscaled, item.params, images, item.durationMs);
            }

            // Track grid batch completion — stitch when all cells are done
            if (images.length > 0 && compare.isGridPrompt(promptId)) {
              const gridResult = compare.addGridResult(promptId, images[0]!);
              if (gridResult) {
                stitchGrid(gridResult.images, gridResult.rows, gridResult.cols, gridResult.cellLabels, item.mode);
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
        // Classify the failure into an actionable message. The raw ComfyUI
        // desktop payload exposes exception_message/exception_type/node_type/
        // traceback (no top-level `error`), so pass the whole payload, not just
        // data.error, or most desktop errors fall through to the generic toast.
        const classified = classifyGenerationError(data);
        const toastMsg = locale.t(classified.messageKey, classified.params);
        if (classified.clearStyleTransfer) {
          // Auto-clear style transfer state on failure so the user is not stuck unable to generate
          // normal images after enabling via a preview button (for example on a LoRA card). This addresses
          // reports of generation loading quickly with no final output.
          generation.styleTransferEnabled = false;
          generation.styleReferenceImage = null;
          generation.saveSettings();
        }
        gallery.showToast(toastMsg, "error", classified.durationMs ? { durationMs: classified.durationMs } : false);
        if (data.prompt_id) {
          pendingOutputImages.delete(data.prompt_id);
          pendingOutputFetches.delete(data.prompt_id);
          promptLastActivity.delete(data.prompt_id);
          progress.removePrompt(data.prompt_id);
          const errPreviewTarget = artistLocalPreviews.resolve(data.prompt_id);
          if (errPreviewTarget) artistLocalPreviews.fail(errPreviewTarget.slug, errPreviewTarget.variant);
          compare.clearGridBatch();
        } else {
          // No prompt_id — clear everything
          pendingOutputImages.clear();
          pendingOutputFetches.clear();
          promptLastActivity.clear();
          progress.cancelAll();
          artistLocalPreviews.failAll();
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
        const queueOrder: string[] = [];
        for (const item of [...q.queue_running, ...q.queue_pending]) {
          // ComfyUI queue entries: [number, prompt_id, ...] or {prompt_id: ...}
          const pid = Array.isArray(item)
            ? (item[1] as string)
            : (item as any)?.prompt_id;
          if (pid) {
            allPromptIds.add(pid);
            queueOrder.push(pid);
          }
        }
        const queueTotal = queueOrder.length;
        progress.resetQueuePosition();
        const now = Date.now();
        for (const p of progress.pendingPrompts) {
          // Keep queue position synced from the merged ComfyUI queue, not just
          // the internal fair-queue tracker. This avoids showing "Preparing..."
          // while this prompt is waiting behind existing external jobs.
          const queueIndex = queueOrder.indexOf(p.promptId);
          if (queueIndex >= 0) {
            progress.updateQueuePosition(p.promptId, queueIndex, queueTotal);
          }

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
            let images = pendingOutputImages.get(p.promptId) ?? [];
            const item = progress.completePrompt(p.promptId, images.length > 0);
            promptLastActivity.delete(p.promptId);
            if (item) {
              for (const img of images) {
                const tempFn = img.tempFilename?.trim();
                if (tempFn) progress.registerPromptOutput(p.promptId, tempFn);
              }
              pendingOutputImages.delete(p.promptId);

              if (images.length === 0) {
                // SSE event was likely dropped during a reconnect — the image was
                // saved to a temp file on the server and cached by the cleanup
                // reactor.  Try to recover it before giving up.
                await recoverMissingOutputImages(p.promptId, images);
              }

              if (images.length > 0) {
                const skipGallery = shouldSuppressRegionalChainGallerySave(p.promptId);
                if (skipGallery) {
                  clearRegionalChainGallerySuppress(p.promptId);
                  console.log("[regional] Skipping gallery save for chain intermediate:", p.promptId);
                } else {
                  finalizeOutputImages(p.promptId, item.mode, item.wasUpscaled, item.params, images, item.durationMs);
                }
              } else {
                const failedStyleTransfer = item.params?.style_transfer_enabled;
                if (failedStyleTransfer) {
                  // Auto-clear to recover normal generation. Prevents being stuck in a
                  // non-generating state after a style reference (for example from LoRA preview) causes
                  // the style transfer workflow to produce no output image.
                  generation.styleTransferEnabled = false;
                  generation.styleReferenceImage = null;
                  generation.saveSettings();
                }
                gallery.showToast(
                  failedStyleTransfer
                    ? locale.t("generation.style_transfer.failed_no_output")
                    : locale.t("app.generation_lost"),
                  "error",
                );
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

    modelPreviewActionHandler = (event: Event) => {
      void handleModelPreviewAction(event);
    };
    window.addEventListener("mooshie:model-preview-action", modelPreviewActionHandler);

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
            startup.locked = false;
            startupStatus = "";
            startupStatusKind = "idle";
          } catch (e) {
            startup.locked = false;
            console.error("Model refresh failed (already running):", e);
          }
        }
      } catch (e) {
        console.error("Failed to start ComfyUI:", e);
        showComfyStartupIssue(e, String(e));
      }
    } else {
      // Manual mode: nothing is starting, so the user needs the UI (and the
      // banner's start button) immediately.
      startup.locked = false;
      startupStatus = locale.t("app.status.auto_start_disabled");
      startupStatusKind = "manual";
    }

    // Load persisted gallery images from disk (independent of server status)
    gallery.loadFromDisk();

    // Skip warming the large artist index during startup so prompt input stays responsive.
    // Artist-specific affordances can wait until a gallery flow actually needs the index.

    // Verify the installed ComfyUI is on the pinned version even for users who
    // never open Settings — features like Krea 2 fail validation on older
    // ComfyUI builds, so surface a notification instead of a silent failure.
    void checkComfyuiVersion();
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
    if (modelPreviewActionHandler) {
      window.removeEventListener("mooshie:model-preview-action", modelPreviewActionHandler);
    }
    cancelInterrogateHoverTimer();
    if (unlistenInterrogateDragDrop) unlistenInterrogateDragDrop();
    clearGenerationDoneToastTimers();
  });
</script>

{#if authRequired}
  {#if mustChangePassword}
    <!-- Forced password change screen -->
    <div class="flex items-center justify-center h-full bg-neutral-950">
      <div class="w-80 space-y-4">
        <div class="mooshie-branding flex items-center justify-center gap-3 mb-6">
          <img src={themeLogoUrl} alt={locale.t("app.brand_name")} class="w-10 h-10 aspect-square object-contain rounded-lg" />
          <h1 class="text-xl font-bold text-neutral-100">{locale.t("app.brand_name")}</h1>
        </div>
        <p class="text-sm text-neutral-400 text-center">{locale.t("auth.password_reset_by_admin")}</p>
        <input
          type="password"
          bind:value={newPass1}
          placeholder={locale.t("auth.new_password_placeholder")}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
          onkeydown={(e) => { if (e.key === "Enter") document.getElementById("confirm-pass")?.focus(); }}
        />
        <input
          id="confirm-pass"
          type="password"
          bind:value={newPass2}
          placeholder={locale.t("auth.confirm_password_placeholder")}
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
          {changePassBusy ? locale.t("common.saving") : locale.t("auth.change_password")}
        </button>
      </div>
    </div>
  {:else}
  <!-- Login gate for LAN users -->
  <div class="flex items-center justify-center h-full bg-neutral-950">
    <div class="w-80 space-y-4">
      <div class="mooshie-branding flex items-center justify-center gap-3 mb-6">
        <img src={themeLogoUrl} alt={locale.t("app.brand_name")} class="w-10 h-10 aspect-square object-contain rounded-lg" />
        <h1 class="text-xl font-bold text-neutral-100">{locale.t("app.brand_name")}</h1>
      </div>
      <p class="text-sm text-neutral-400 text-center">{locale.t("auth.sign_in_continue")}</p>
      <input
        type="text"
        bind:value={loginUser}
        placeholder={locale.t("auth.username_placeholder")}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        onkeydown={(e) => { if (e.key === "Enter") handleLogin(); }}
      />
      <input
        type="password"
        bind:value={loginPass}
        placeholder={locale.t("auth.password_placeholder")}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
        onkeydown={(e) => { if (e.key === "Enter") handleLogin(); }}
      />
      <label class="flex items-center gap-2 cursor-pointer select-none">
        <input
          type="checkbox"
          bind:checked={rememberMe}
          class="w-4 h-4 rounded border-neutral-600 bg-neutral-800 text-indigo-500 focus:ring-indigo-500 focus:ring-offset-0 cursor-pointer"
        />
        <span class="text-sm text-neutral-400">{locale.t("auth.remember_me")}</span>
      </label>
      {#if loginError}
        <p class="text-xs text-red-400">{loginError}</p>
      {/if}
      <button
        class="w-full py-2 rounded-lg text-sm font-medium transition-colors cursor-pointer {loginBusy ? 'bg-neutral-700 text-neutral-500' : 'bg-indigo-600 hover:bg-indigo-500 text-white'}"
        disabled={loginBusy}
        onclick={handleLogin}
      >
        {loginBusy ? locale.t("common.saving") : locale.t("auth.sign_in")}
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
    {userRole}
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

  <!-- Sidebar column: logo panel + nav -->
  <div class="flex w-14 shrink-0 flex-col gap-1.5 self-stretch md:gap-3">
    <div
      class="theme-logo-panel flex shrink-0 items-center justify-center border-r border-neutral-800 bg-neutral-900 px-1.5 py-2 md:rounded-2xl md:border md:shadow-2xl md:shadow-black/30"
    >
      <div
        class="flex size-8 items-center justify-center rounded-lg bg-neutral-800/60 text-neutral-400"
        title={locale.t("app.theme_logo")}
      >
        <img src={themeLogoUrl} alt={locale.t("app.theme_logo")} class="size-7 rounded-md object-contain" />
      </div>
    </div>

    <nav
      class="flex min-h-0 flex-1 flex-col items-stretch gap-1.5 border-r border-neutral-800 bg-neutral-900 px-1.5 py-3 md:rounded-2xl md:border md:shadow-2xl md:shadow-black/30"
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
      title={locale.t("nav.artist_gallery")}
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

    <div class="relative mx-auto">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <button
        bind:this={interrogateSidebarBtn}
        class="w-8 h-8 rounded-lg flex items-center justify-center transition-colors {showInterrogateQuickModal
          ? 'bg-indigo-600 text-white'
          : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}"
        onclick={() => { dismissInterrogateHint(); showInterrogateQuickModal = true; }}
        ondragenter={(e) => { e.preventDefault(); startInterrogateHoverTimer(); }}
        ondragover={(e) => { e.preventDefault(); }}
        ondragleave={cancelInterrogateHoverTimer}
        title={locale.t('generation.interrogate.title')}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          class="w-4.5 h-4.5 pointer-events-none"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          ><path d="M3 7V5a2 2 0 0 1 2-2h2" /><path d="M17 3h2a2 2 0 0 1 2 2v2" /><path d="M21 17v2a2 2 0 0 1-2 2h-2" /><path d="M7 21H5a2 2 0 0 1-2-2v-2" /><circle cx="12" cy="12" r="3" /><path d="m16 16-1.5-1.5" /></svg
        >
      </button>
      {#if showInterrogateHint}
        <div
          role="status"
          class="absolute left-full top-1/2 z-50 ml-3 w-56 -translate-y-1/2 rounded-lg border border-[var(--theme-accent-500)] bg-neutral-800 p-2.5 pr-7 shadow-xl shadow-black/40"
        >
          <div
            class="absolute top-1/2 -left-1.25 h-2 w-2 -translate-y-1/2 rotate-45 border-b border-l border-[var(--theme-accent-500)] bg-neutral-800"
          ></div>
          <p class="text-xs leading-snug text-neutral-200">
            {locale.t("generation.interrogate.hint_moved")}
          </p>
          <button
            class="absolute top-1 right-1 flex h-5 w-5 items-center justify-center rounded text-neutral-400 hover:bg-neutral-700 hover:text-neutral-200"
            onclick={dismissInterrogateHint}
            aria-label={locale.t("common.close")}
          >
            <svg
              xmlns="http://www.w3.org/2000/svg"
              class="h-3 w-3"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              ><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg
            >
          </button>
        </div>
      {/if}
    </div>

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

    <NotificationBell onOpenSettings={() => (currentPage = "settings")} />

    <!-- Connection status dot -->
    <div
      class="w-3 h-3 rounded-full mb-2 mx-auto transition-colors {connection.connected
        ? 'bg-green-500'
        : startupStatus
          ? 'bg-amber-500 animate-pulse'
          : 'bg-red-500'}"
      title={connection.connected ? locale.t('nav.connected') : startupStatus || locale.t('nav.disconnected')}
    ></div>

    <!-- Report a problem -->
    <button
      class="w-8 h-8 rounded-lg flex items-center justify-center text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200 transition-colors mx-auto mb-1"
      onclick={() => (showBugReport = true)}
      title={locale.t('nav.report_bug')}
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
        ><path d="m8 2 1.88 1.88" /><path d="M14.12 3.88 16 2" /><path
          d="M9 7.13v-1a3.003 3.003 0 1 1 6 0v1"
        /><path
          d="M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6"
        /><path d="M12 20v-9" /><path d="M6.53 9C4.6 8.8 3 7.1 3 5" /><path
          d="M6 13H2"
        /><path d="M3 21c0-2.1 1.7-3.9 3.8-4" /><path d="M20.97 5c0 2.1-1.6 3.8-3.5 4" /><path
          d="M22 13h-4"
        /><path d="M17.2 17c2.1.1 3.8 1.9 3.8 4" /></svg
      >
    </button>

    {#if comfyuiVersionInfo?.installed}
      <span
        class="flex items-center justify-center gap-1 text-[10px] text-center mb-1 select-none cursor-default {comfyuiVersionInfo.update_available
          ? 'text-amber-500'
          : 'text-neutral-500'}"
        title={comfyuiVersionInfo.update_available
          ? locale.t('settings.performance.comfyui_update_note')
          : locale.t('settings.performance.comfyui_up_to_date')}
      >
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" class="w-2.5 h-2.5 shrink-0" aria-hidden="true">
          <circle cx="8" cy="8" r="7.5" fill="currentColor" fill-opacity="0.15" stroke="currentColor" stroke-width="1" />
          <text x="8" y="11.2" text-anchor="middle" font-size="9" font-weight="700" fill="currentColor">C</text>
        </svg>
        v{comfyuiVersionInfo.installed}
      </span>
    {/if}
    <span
      class="mooshie-branding text-[10px] text-neutral-500 text-center mb-2 select-none cursor-default"
      role="button"
      tabindex="0"
      onclick={handleVersionTap}
      onkeydown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          handleVersionTap();
        }
      }}
    >v{appVersion}</span>
    </nav>
  </div>

  <!-- Main content -->
  <main class="flex min-w-0 flex-1 flex-col overflow-hidden md:rounded-2xl md:border md:border-neutral-800 md:bg-neutral-900 md:p-1 md:shadow-2xl md:shadow-black/30">
    <UpdateNotification {userRole} />
    <DownloadBanner />
    <ExternalComfyModal
      open={externalComfyOpen}
      payload={externalComfyPayload}
      serverUrl={comfyServerUrl}
      onclose={() => {
        externalComfyOpen = false;
      }}
      onrestarted={() => {
        externalComfyOpen = false;
        startupStatus = locale.t("app.status.starting_comfyui");
        startupStatusKind = "starting";
      }}
    />
    <PhotopeaEditor
      open={photopeaOpen}
      image={photopeaImage}
      onclose={() => {
        photopeaOpen = false;
        photopeaImage = null;
      }}
      onsaved={(filename) => {
        void gallery.addPersistedImage(filename);
        gallery.showToast(locale.t("photopea.saved"), "success");
      }}
    />
    {#if startupStatus && !connection.connected}
      <div class="mb-1 flex shrink-0 items-center gap-2 rounded-[var(--app-panel-radius)] border border-amber-800/60 bg-amber-950/85 px-4 py-2.5 text-sm text-amber-100 shadow-lg shadow-black/20 backdrop-blur-sm">
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
    <div class="relative flex-1 overflow-hidden md:min-h-0 md:rounded-xl md:bg-neutral-950">
    {#if startup.locked}
      <div
        class="absolute inset-0 z-50 flex flex-col items-center justify-center gap-3 bg-neutral-950/70 backdrop-blur-[2px]"
        role="status"
        aria-label={locale.t("app.startup.initializing")}
      >
        <div class="h-6 w-6 animate-spin rounded-full border-2 border-indigo-400 border-t-transparent"></div>
        <span class="text-sm text-neutral-300">{locale.t("app.startup.initializing")}</span>
      </div>
    {/if}
    {#if currentPage === "generate"}
      <GenerationPage />
    {:else if currentPage === "gallery"}
      <GalleryPage onSwitchToGenerate={() => (currentPage = "generate")} />
    {:else if currentPage === "modelhub"}
      <ModelHubPage />
    {:else if currentPage === "artists"}
      <ArtistGalleryPage
        manifestUrl={connection.artistGalleryManifestUrl}
        oninsertTag={handleArtistTagInsert}
        oninsertCharacter={handleCharacterInsert}
        ongeneratePreview={handleArtistGeneratePreview}
        previewStatus={artistPreviewStatus}
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

            {#if gallery.selectedImage.generationTimeMs != null}
              <div class="mb-3 flex justify-between gap-2">
                <span class="text-[10px] text-neutral-500 uppercase tracking-wider">{locale.t("gallery.generation_time")}</span>
                <span class="text-neutral-200">{formatGenerationTime(gallery.selectedImage.generationTimeMs, locale.current)}</span>
              </div>
            {/if}

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
            class="relative w-1.5 cursor-col-resize hover:bg-indigo-500/40 active:bg-indigo-500/60 transition-colors shrink-0 flex items-center justify-center group"
            onmousedown={startMetadataResize}
          >
          </div>
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
      role="button"
      tabindex="0"
      onclick={(e) => { if (e.target === e.currentTarget) gallery.closeLightbox(); }}
      onkeydown={handleLightboxAreaKeydown}
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
          title={locale.t("gallery.lightbox.prev_title")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
        </button>
        <button
          class="absolute right-14 top-1/2 -translate-y-1/2 z-10 w-10 h-10 flex items-center justify-center rounded-full bg-black/40 hover:bg-black/70 text-white transition-colors"
          onclick={() => navigateLightbox("next")}
          title={locale.t("gallery.lightbox.next_title")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
        </button>
      {/if}

      <!-- Action buttons. For a video the player owns the bottom of the frame,
           so this bar sits above the player's control chrome (~90px tall)
           instead of on top of it. -->
      {#if gallery.selectedImage}
      <div class="absolute {gallery.lightboxIsVideo ? 'bottom-28' : 'bottom-6'} left-1/2 -translate-x-1/2 z-10 flex items-center gap-1.5 bg-neutral-900/70 backdrop-blur-sm rounded-xl px-2 py-1.5 border border-neutral-700/50">
        {#if !gallery.lightboxIsVideo}
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
          title={locale.t("gallery.make_video")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && makeVideoFromImage(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18"/><line x1="7" y1="2" x2="7" y2="22"/><line x1="17" y1="2" x2="17" y2="22"/><line x1="2" y1="12" x2="22" y2="12"/><line x1="2" y1="7" x2="7" y2="7"/><line x1="2" y1="17" x2="7" y2="17"/><line x1="17" y1="17" x2="22" y2="17"/><line x1="17" y1="7" x2="22" y2="7"/></svg>
        </button>
        <button
          title={locale.t("gallery.add_video_reference")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && addImageAsVideoReference(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/></svg>
        </button>
        <button
          title={locale.t("gallery.remix")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && applyMetadataToGeneration(gallery.selectedImage, "remix")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0114.13-3.36L23 10M1 14l5.37 4.36A9 9 0 0020.49 15"/></svg>
        </button>
        <button
          title={locale.t("gallery.edit_photopea")}
          class="flex items-center justify-center w-8 h-8 rounded-lg bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => gallery.selectedImage && editInPhotopea(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5z"/></svg>
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

        <!-- Compare group -->
        <button
          title={gallery.selectedImage ? comparePinLabel(gallery.selectedImage) : locale.t("gallery.compare.pin")}
          class="flex items-center justify-center w-8 h-8 rounded-lg transition-colors {gallery.comparePin && gallery.comparePin === gallery.selectedImage
            ? 'bg-indigo-600/80 hover:bg-indigo-500 text-neutral-100'
            : 'bg-neutral-800/80 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100'}"
          onclick={() => gallery.selectedImage && gallery.toggleComparePin(gallery.selectedImage)}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><line x1="12" y1="4" x2="12" y2="20"/><polyline points="7 10 5 12 7 14"/><polyline points="17 10 19 12 17 14"/></svg>
        </button>
        <!-- Separator (inside the guard: with no image-only group there is
             nothing to separate, so a video would show a floating rule) -->
        <div class="w-px h-5 bg-neutral-700/60 mx-0.5"></div>
        {/if}

        <!-- Export group -->
        <button
          title={locale.t(gallery.lightboxIsVideo ? "gallery.save_video_as" : "gallery.save_as")}
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

      {#if gallery.lightboxUrl && gallery.lightboxIsVideo}
        <!-- The player owns its own chrome, keyboard handling (including the
             arrow-key stopPropagation that used to live inline here), and
             export affordances. Zoom and pan stay absent: they are for stills. -->
        <VideoPlayer
          src={gallery.lightboxUrl}
          fps={gallery.selectedImage?.fps ?? 24}
          density="full"
          filename={gallery.selectedImage?.gallery_filename}
          onContextMenu={openLightboxContextMenu}
        />
      {:else if gallery.lightboxUrl}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <img
          bind:this={lbImgEl}
          src={gallery.lightboxUrl}
          alt={gallery.selectedImage?.filename ?? locale.t("gallery.no_preview")}
          class="max-w-full max-h-[85vh] object-contain select-none {lbPanning ? 'cursor-grabbing' : 'cursor-grab'}"
          draggable="false"
          style="transform-origin: center center; will-change: transform;"
          onwheel={zoomLightboxAtCursor}
          onmousedown={(e) => { if (e.button === 1) e.preventDefault(); startLightboxPan(e); }}
          onmousemove={updateLightboxPan}
          onmouseup={stopLightboxPan}
          onmouseleave={stopLightboxPan}
          onauxclick={(e) => e.preventDefault()}
          oncontextmenu={openLightboxContextMenu}
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

<!-- A/B comparison viewer (issue #517) -->
{#if gallery.compareOpen}
  <CompareViewer />
{/if}

<!-- Waiting for the second image of a comparison. Sits above the lightbox so the
     user can keep browsing there to find the counterpart. -->
{#if gallery.comparePin && !gallery.compareOpen}
  <div class="fixed bottom-4 left-1/2 -translate-x-1/2 z-70 flex items-center gap-3 rounded-xl border border-indigo-700/60 bg-neutral-900/95 px-3 py-2 shadow-2xl shadow-black/40 backdrop-blur-sm">
    {#if gallery.comparePin.url || gallery.comparePin.thumbnailUrl}
      <img
        src={gallery.comparePin.thumbnailUrl ?? gallery.comparePin.url}
        alt=""
        class="h-10 w-10 shrink-0 rounded-lg border border-neutral-700 object-cover bg-neutral-950"
      />
    {/if}
    <span class="text-xs text-neutral-200">{locale.t("gallery.compare.pick_second")}</span>
    <button
      class="px-2 py-1 text-xs rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
      onclick={() => gallery.cancelComparePin()}
    >
      {locale.t("common.cancel")}
    </button>
  </div>
{/if}

{#if generationDoneToast}
  {#key generationDoneToast.id}
    <div class="fixed bottom-5 right-4 z-10000 w-[min(22rem,calc(100vw-2rem))] md:right-5">
      <div
        class="generation-done-toast flex items-center gap-3 rounded-[var(--app-panel-radius)] border border-neutral-700 bg-neutral-900/95 p-2 shadow-2xl shadow-black/40 backdrop-blur-sm {generationDoneToast.leaving ? 'generation-done-toast-out' : 'generation-done-toast-in'}"
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
    class="fixed bottom-6 left-1/2 z-10000 flex -translate-x-1/2 animate-fade-in items-center gap-2 rounded-[var(--app-panel-radius)] border px-4 py-2.5 text-sm shadow-2xl shadow-black/40 backdrop-blur-sm
    {type === 'success' ? 'border-green-700/80 bg-green-950/95 text-green-100' : 
     type === 'error' ? 'border-red-700/80 bg-red-950/95 text-red-100' :
     'border-neutral-700 bg-neutral-900/95 text-neutral-100'}"
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

<CharacterInsertModal onapplied={finishCharacterInsert} />

<!-- Global human-readable error surface -->
<GlobalErrorModal />

{#if showBugReport}
  <ReportErrorModal error={userInitiatedReport} generic onclose={() => (showBugReport = false)} />
{/if}

<!-- Dev-only error gallery (#error-gallery), never shipped in production -->
{#if showErrorGallery}
  <ErrorGallery />
{/if}

<!-- Artist tag conflict dialog -->
{#if artistInsertPending}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-[300] flex items-center justify-center bg-black/60"
    onclick={(e) => { if (e.target === e.currentTarget) artistInsert.dismiss(); }}
    onkeydown={(e) => { if (e.key === 'Escape') artistInsert.dismiss(); }}
  >
    <div class="w-96 max-w-full rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
      {#if !artistInsertPending.duplicate}
        <h2 class="mb-1 text-sm font-semibold text-neutral-100">{locale.t("artist_insert.artist_duplicate_title")}</h2>
        <p class="mb-3 text-xs text-neutral-400">
          {locale.t("artist_insert.artist_duplicate_body", {
            existing: artistInsertPending.existingTags.join(", "),
            tag: artistInsertPending.tag,
          })}
        </p>
        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-neutral-500"
            onclick={() => artistInsert.dismiss()}
          >
            {locale.t("common.cancel")}
          </button>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 transition-colors hover:border-indigo-500"
            onclick={() => applyArtistTag(artistInsertPending!.tag, 'add')}
          >
            {locale.t("artist_insert.add_alongside")}
          </button>
          <button
            type="button"
            class="rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500"
            onclick={() => applyArtistTag(artistInsertPending!.tag, 'replace')}
          >
            {locale.t("artist_insert.replace")}
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<!-- Compact interrogate modal (from the sidebar button / drag-hover) -->
{#if showInterrogateQuickModal}
  <InterrogateQuickModal
    onclose={() => (showInterrogateQuickModal = false)}
    onpaste={interrogatePasteQuick}
    onbrowse={browseInterrogateQuick}
    onfile={interrogateFileQuick}
  />
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
    onclose={() => {
      showInterrogateModal = false;
      interrogateResult = null;
      if (interrogateImageUrl?.startsWith("blob:")) URL.revokeObjectURL(interrogateImageUrl);
      interrogateImageUrl = null;
      interrogateError = null;
    }}
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
