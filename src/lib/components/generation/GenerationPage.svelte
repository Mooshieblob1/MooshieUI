<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import PromptInputs from "./PromptInputs.svelte";
  import ModelSelector from "./ModelSelector.svelte";
  import SamplerSettings from "./SamplerSettings.svelte";
  import DimensionControls from "./DimensionControls.svelte";
  import GenerateButton from "./GenerateButton.svelte";
  import UpscaleSettings from "./UpscaleSettings.svelte";
  import FaceFixSettings from "./FaceFixSettings.svelte";
  import ControlNetSettings from "./ControlNetSettings.svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import ProgressBar from "../progress/ProgressBar.svelte";
  import PreviewImage from "../progress/PreviewImage.svelte";
  import CanvasEditor from "../canvas/CanvasEditor.svelte";
  import LayerPanel from "../canvas/layers/LayerPanel.svelte";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { open } from "@tauri-apps/plugin-dialog";
  import { readFile } from "@tauri-apps/plugin-fs";
  import { uploadImage, uploadImageBytes, loadGalleryImage, getOutputImage } from "../../utils/api.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { lazyThumbnail } from "../../utils/lazyThumbnail.js";
  import type { OutputImage } from "../../types/index.js";

  const DIMENSIONS_LAYOUT_KEY = "mooshieui.generation.dimensions.layout.v1";
  const SECTION_LAYOUT_KEY = "mooshieui.generation.sections.layout.v1";

  type SectionId =
    | "dimensions"
    | "prompts"
    | "history"
    | "sessionHistory"
    | "imageInputs"
    | "inpaintLayers"
    | "generationSettings"
    | "model"
    | "sampler"
    | "controlnet"
    | "facefix"
    | "upscaleHistory";

  type SectionSide = "left" | "right";

  const modes = [
    { id: "txt2img" as const, label: "Text to Image" },
    { id: "img2img" as const, label: "Image to Image" },
    { id: "inpainting" as const, label: "Inpainting" },
  ];

  let canvasEditorRef: CanvasEditor | undefined = $state();
  let imagePreviewUrl = $state<string | null>(null);
  let maskPreviewUrl = $state<string | null>(null);
  let uploading = $state(false);
  let imageAspect = $state<{ w: number; h: number } | null>(null);
  let dragOver = $state(false);
  let maskDragOver = $state(false);
  let promptsSectionOpen = $state(true);
  let historySectionOpen = $state(true);

  const sortedPromptHistory = $derived(
    [...generation.promptHistory]
      .sort((a, b) => {
        if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
        return b.createdAt - a.createdAt;
      })
      .slice(0, 12)
  );

  function historyLabel(ts: number): string {
    return new Date(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  let sectionSides = $state<Record<SectionId, SectionSide>>({
    dimensions: "left",
    prompts: "left",
    history: "left",
    sessionHistory: "right",
    imageInputs: "left",
    inpaintLayers: "right",
    generationSettings: "right",
    model: "right",
    sampler: "right",
    controlnet: "right",
    facefix: "right",
    upscaleHistory: "right",
  });

  let draggingSection = $state<SectionId | null>(null);
  let pendingDrop = $state<{ side: SectionSide; index: number } | null>(null);
  let dragMouseX = $state(0);
  let dragMouseY = $state(0);
  let dragOffsetX = $state(0);
  let dragOffsetY = $state(0);
  let dragWidth = $state(0);
  let dragHeight = $state(0);
  let dragCloneHtml = $state("");
  let sectionRefs: Record<string, HTMLElement | null> = {};
  let leftColumnRef = $state<HTMLElement | null>(null);
  let rightColumnRef = $state<HTMLElement | null>(null);

  const SECTION_ORDER: SectionId[] = [
    "dimensions",
    "prompts",
    "history",
    "sessionHistory",
    "imageInputs",
    "inpaintLayers",
    "generationSettings",
    "model",
    "sampler",
    "controlnet",
    "facefix",
    "upscaleHistory",
  ];

  let sectionOrder = $state<SectionId[]>([...SECTION_ORDER]);

  function normalizeSectionOrder(order: unknown): SectionId[] {
    if (!Array.isArray(order)) return [...SECTION_ORDER];
    const allowed = new Set<SectionId>(SECTION_ORDER);
    const seen = new Set<SectionId>();
    const out: SectionId[] = [];
    for (const item of order) {
      if (typeof item !== "string") continue;
      // Migrate legacy "modelSampler" → "model" + "sampler"
      if (item === "modelSampler") {
        for (const replacement of ["model", "sampler"] as SectionId[]) {
          if (!seen.has(replacement)) {
            seen.add(replacement);
            out.push(replacement);
          }
        }
        continue;
      }
      const id = item as SectionId;
      if (!allowed.has(id) || seen.has(id)) continue;
      seen.add(id);
      out.push(id);
    }
    for (const id of SECTION_ORDER) {
      if (!seen.has(id)) out.push(id);
    }
    return out;
  }

  function loadSectionPlacement() {
    try {
      const raw = localStorage.getItem(SECTION_LAYOUT_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as
          | { sides?: Partial<Record<SectionId, SectionSide>>; order?: SectionId[] }
          | Partial<Record<SectionId, SectionSide>>;

        const rawSides =
          parsed && typeof parsed === "object" && "sides" in parsed
            ? (parsed.sides ?? {})
            : (parsed as Partial<Record<SectionId, SectionSide>>);

        // Migrate legacy "modelSampler" side to both "model" and "sampler"
        const entries = Object.entries(rawSides).filter(([, side]) => side === "left" || side === "right");
        const legacyModelSampler = entries.find(([key]) => key === "modelSampler");
        if (legacyModelSampler) {
          const side = legacyModelSampler[1] as SectionSide;
          entries.push(["model", side], ["sampler", side]);
        }

        sectionSides = {
          ...sectionSides,
          ...Object.fromEntries(
            entries.filter(([key]) => key !== "modelSampler")
          ) as Partial<Record<SectionId, SectionSide>>,
        };

        if (parsed && typeof parsed === "object" && "order" in parsed) {
          sectionOrder = normalizeSectionOrder(parsed.order);
        }
        return;
      }

      const legacy = localStorage.getItem(DIMENSIONS_LAYOUT_KEY);
      if (!legacy) return;
      const parsedLegacy = JSON.parse(legacy) as { side?: SectionSide };
      if (parsedLegacy.side === "left" || parsedLegacy.side === "right") {
        sectionSides = { ...sectionSides, dimensions: parsedLegacy.side };
      }
    } catch (e) {
      console.error("Failed to load section layout:", e);
    }
  }

  function saveSectionPlacement() {
    try {
      localStorage.setItem(
        SECTION_LAYOUT_KEY,
        JSON.stringify({ sides: sectionSides, order: sectionOrder })
      );
    } catch (e) {
      console.error("Failed to save section layout:", e);
    }
  }

  if (typeof window !== "undefined") {
    loadSectionPlacement();
  }

  let layoutSaveTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    void sectionSides;
    void sectionOrder;
    if (layoutSaveTimer) clearTimeout(layoutSaveTimer);
    layoutSaveTimer = setTimeout(() => saveSectionPlacement(), 300);
  });

  function startSectionDrag(section: SectionId, e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    const el = sectionRefs[section];
    if (el) {
      const rect = el.getBoundingClientRect();
      dragOffsetX = e.clientX - rect.left;
      dragOffsetY = e.clientY - rect.top;
      dragWidth = rect.width;
      dragHeight = rect.height;
      dragCloneHtml = el.outerHTML;
    }
    draggingSection = section;
    dragMouseX = e.clientX;
    dragMouseY = e.clientY;
    const side = sectionSides[section];
    const sideSections = sectionsForSide(side);
    const idx = sideSections.indexOf(section);
    pendingDrop = { side, index: idx >= 0 ? idx : sideSections.length };
  }

  function isPendingDrop(side: SectionSide, index: number): boolean {
    return !!pendingDrop && pendingDrop.side === side && pendingDrop.index === index;
  }

  function sectionLabel(section: SectionId): string {
    if (section === "dimensions") return "Dimensions";
    if (section === "prompts") return "Prompts";
    if (section === "history") return "Prompt History & Favorites";
    if (section === "sessionHistory") return "Session History";
    if (section === "imageInputs") return "Image Inputs";
    if (section === "inpaintLayers") return "Inpainting & Layers";
    if (section === "generationSettings") return "Generation Settings";
    if (section === "model") return "Model";
    if (section === "sampler") return "Sampler";
    if (section === "facefix") return "Face Fix";
    return "Upscale";
  }

  function canShowHistorySection() {
    return sortedPromptHistory.length > 0;
  }

  function sectionVisible(section: SectionId): boolean {
    if (section === "history") return canShowHistorySection();
    if (section === "sessionHistory") return true;
    if (section === "imageInputs") return generation.mode !== "txt2img";
    if (section === "inpaintLayers") return generation.mode === "inpainting";
    if (section === "generationSettings") return generation.mode === "inpainting";
    if (section === "model") return generation.mode !== "inpainting";
    if (section === "sampler") return generation.mode !== "inpainting";
    if (section === "upscaleHistory") return generation.mode !== "inpainting";
    if (section === "controlnet") return !generation.isAnima;
    if (section === "facefix") return generation.mode !== "inpainting";
    return true;
  }

  function sectionsForSide(side: SectionSide): SectionId[] {
    return sectionOrder.filter((id) => sectionVisible(id) && sectionSides[id] === side);
  }

  const leftSections = $derived(sectionsForSide("left"));
  const rightSections = $derived(sectionsForSide("right"));
  const leftHasSections = $derived(leftSections.length > 0);
  const rightHasSections = $derived(rightSections.length > 0);
  const controlsSide = $derived(leftHasSections ? "left" : "right");

  // Sections for rendering — excludes the dragged section so drop zone indices match computeDropTarget
  const leftRenderSections = $derived(leftSections.filter((id) => id !== draggingSection));
  const rightRenderSections = $derived(rightSections.filter((id) => id !== draggingSection));

  const COLLAPSE_KEY = "mooshieui.generation.sections.collapsed.v1";

  function loadCollapseState(): Record<string, boolean> {
    try {
      const raw = localStorage.getItem(COLLAPSE_KEY);
      if (raw) return JSON.parse(raw);
    } catch {}
    return {};
  }

  const savedCollapse = typeof window !== "undefined" ? loadCollapseState() : {};

  let dimensionsSectionOpen = $state(savedCollapse.dimensions !== false);
  let imageSectionOpen = $state(savedCollapse.imageInputs !== false);
  let layersSectionOpen = $state(savedCollapse.inpaintLayers !== false);
  let sessionSectionOpen = $state(savedCollapse.sessionHistory !== false);
  let sessionPage = $state(0);
  let prevSessionCount = $state(gallery.sessionImages.length);
  const SESSION_PAGE_SIZE = 4;
  const sessionTotalPages = $derived(Math.max(1, Math.ceil(gallery.sessionImages.length / SESSION_PAGE_SIZE)));
  const sessionPageImages = $derived(gallery.sessionImages.slice(sessionPage * SESSION_PAGE_SIZE, (sessionPage + 1) * SESSION_PAGE_SIZE));
  $effect(() => {
    const count = gallery.sessionImages.length;
    if (count > prevSessionCount) {
      // New images added — jump to first page
      sessionPage = 0;
    } else if (sessionPage >= sessionTotalPages) {
      // Images deleted — clamp page
      sessionPage = Math.max(0, sessionTotalPages - 1);
    }
    prevSessionCount = count;
  });
  let controlsSectionOpen = $state(savedCollapse.generationSettings !== false);
  let modelSectionOpen = $state(savedCollapse.model !== false);
  let samplerSectionOpen = $state(savedCollapse.sampler !== false);
  let controlnetSectionOpen = $state(savedCollapse.controlnet !== false);
  let facefixSectionOpen = $state(savedCollapse.facefix !== false);
  let postSectionOpen = $state(savedCollapse.upscaleHistory !== false);

  let collapseSaveTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    const state: Record<string, boolean> = {
      dimensions: dimensionsSectionOpen,
      imageInputs: imageSectionOpen,
      inpaintLayers: layersSectionOpen,
      sessionHistory: sessionSectionOpen,
      generationSettings: controlsSectionOpen,
      model: modelSectionOpen,
      sampler: samplerSectionOpen,
      controlnet: controlnetSectionOpen,
      facefix: facefixSectionOpen,
      upscaleHistory: postSectionOpen,
    };
    if (collapseSaveTimer) clearTimeout(collapseSaveTimer);
    collapseSaveTimer = setTimeout(() => {
      try { localStorage.setItem(COLLAPSE_KEY, JSON.stringify(state)); } catch {}
    }, 300);
  });

  const MAX_INPUT_PIXELS = 1024 * 1024;

  function applyImageGeometry(width: number, height: number) {
    imageAspect = { w: width, h: height };
    generation.width = width;
    generation.height = height;

    if (canvas.isCanvasMode && (canvas.canvasWidth !== width || canvas.canvasHeight !== height)) {
      canvas.initCanvas(width, height);
    }
  }

  async function normalizeImageBytes(
    imageBytes: number[],
    fallbackFilename: string
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

  function getFilenameFromPath(path: string): string {
    const name = path.split(/[\\/]/).pop() ?? "input.png";
    return name.trim() || "input.png";
  }

  async function browseImage() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (!selected) return;

    uploading = true;
    try {
      const selectedPath = typeof selected === "string" ? selected : selected[0];
      if (!selectedPath) return;

      const bytes = Array.from(await readFile(selectedPath));
      const normalized = await normalizeImageBytes(bytes, getFilenameFromPath(selectedPath));

      if (imagePreviewUrl) URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = normalized.previewUrl;
      applyImageGeometry(normalized.width, normalized.height);
      canvas.setReferenceImage(imagePreviewUrl);

      const response = await uploadImageBytes(normalized.bytes, normalized.filename);
      generation.inputImage = response.name;
    } catch (e) {
      console.error("Failed to upload image:", e);
    } finally {
      uploading = false;
    }
  }

  async function browseMask() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (!selected) return;

    uploading = true;
    try {
      const bytes = await readFile(selected);
      const blob = new Blob([bytes], { type: "image/png" });
      if (maskPreviewUrl) URL.revokeObjectURL(maskPreviewUrl);
      maskPreviewUrl = URL.createObjectURL(blob);
      canvas.setPersistedMaskPreview(maskPreviewUrl);

      const response = await uploadImage(selected);
      generation.maskImage = response.name;
    } catch (e) {
      console.error("Failed to upload mask:", e);
    } finally {
      uploading = false;
    }
  }

  async function handleImageDrop(e: DragEvent) {
    e.preventDefault();
    dragOver = false;
    const file = e.dataTransfer?.files?.[0];
    if (!file || !file.type.startsWith("image/")) return;

    uploading = true;
    try {
      const buffer = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const normalized = await normalizeImageBytes(bytes, file.name || "dropped_image.png");

      if (imagePreviewUrl) URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = normalized.previewUrl;
      applyImageGeometry(normalized.width, normalized.height);
      canvas.setReferenceImage(imagePreviewUrl);

      const response = await uploadImageBytes(normalized.bytes, normalized.filename);
      generation.inputImage = response.name;
    } catch (e) {
      console.error("Failed to handle dropped image:", e);
    } finally {
      uploading = false;
    }
  }

  async function handleMaskDrop(e: DragEvent) {
    e.preventDefault();
    maskDragOver = false;
    const file = e.dataTransfer?.files?.[0];
    if (!file || !file.type.startsWith("image/")) return;

    uploading = true;
    try {
      const buffer = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buffer));
      const blob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
      if (maskPreviewUrl) URL.revokeObjectURL(maskPreviewUrl);
      maskPreviewUrl = URL.createObjectURL(blob);
      canvas.setPersistedMaskPreview(maskPreviewUrl);

      const response = await uploadImageBytes(bytes, file.name || "dropped_mask.png");
      generation.maskImage = response.name;
    } catch (e) {
      console.error("Failed to handle dropped mask:", e);
    } finally {
      uploading = false;
    }
  }

  async function handleImagePaste() {
    try {
      const items = await navigator.clipboard.read();
      for (const item of items) {
        const imageType = item.types.find((t) => t.startsWith("image/"));
        if (imageType) {
          const blob = await item.getType(imageType);
          const ext = imageType.split("/")[1] || "png";
          const file = new File([blob], `pasted_image.${ext}`, { type: imageType });
          uploading = true;
          const buffer = await file.arrayBuffer();
          const bytes = Array.from(new Uint8Array(buffer));
          const normalized = await normalizeImageBytes(bytes, file.name);

          if (imagePreviewUrl) URL.revokeObjectURL(imagePreviewUrl);
          imagePreviewUrl = normalized.previewUrl;
          applyImageGeometry(normalized.width, normalized.height);
          canvas.setReferenceImage(imagePreviewUrl);

          const response = await uploadImageBytes(normalized.bytes, normalized.filename);
          generation.inputImage = response.name;
          return;
        }
      }
    } catch (e) {
      console.error("Failed to paste image:", e);
    } finally {
      uploading = false;
    }
  }

  async function handleMaskPaste() {
    try {
      const items = await navigator.clipboard.read();
      for (const item of items) {
        const imageType = item.types.find((t) => t.startsWith("image/"));
        if (imageType) {
          const blob = await item.getType(imageType);
          uploading = true;
          if (maskPreviewUrl) URL.revokeObjectURL(maskPreviewUrl);
          maskPreviewUrl = URL.createObjectURL(blob);
          canvas.setPersistedMaskPreview(maskPreviewUrl);

          const buffer = await blob.arrayBuffer();
          const bytes = Array.from(new Uint8Array(buffer));
          const response = await uploadImageBytes(bytes, "pasted_mask.png");
          generation.maskImage = response.name;
          return;
        }
      }
    } catch (e) {
      console.error("Failed to paste mask:", e);
    } finally {
      uploading = false;
    }
  }

  function clearImage() {
    generation.inputImage = null;
    imageAspect = null;
    canvas.setReferenceImage(null);
    if (imagePreviewUrl) {
      URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = null;
    }
  }

  function clearMask() {
    canvas.clearMask();
    if (maskPreviewUrl) {
      URL.revokeObjectURL(maskPreviewUrl);
      maskPreviewUrl = null;
    }
  }

  async function upscaleImage(image: OutputImage) {
    try {
      let bytes: number[];
      if (image.gallery_filename) {
        bytes = await loadGalleryImage(image.gallery_filename);
      } else {
        bytes = await getOutputImage(image.filename, image.subfolder);
      }
      const response = await uploadImageBytes(bytes, image.filename);
      generation.inputImage = response.name;
      generation.mode = "img2img";
      generation.upscaleEnabled = true;
      gallery.showToast("Image loaded for upscaling", "success");
    } catch (e) {
      console.error("Failed to set up upscale:", e);
      gallery.showToast("Failed to load image", "error");
    }
  }

  async function inpaintImage(image: OutputImage) {
    try {
      let bytes: number[];
      if (image.gallery_filename) {
        bytes = await loadGalleryImage(image.gallery_filename);
      } else {
        bytes = await getOutputImage(image.filename, image.subfolder);
      }

      const normalized = await normalizeImageBytes(bytes, image.filename || "inpaint_input.png");
      const response = await uploadImageBytes(normalized.bytes, normalized.filename);
      generation.inputImage = response.name;
      canvas.clearMask();
      generation.mode = "inpainting";
      canvas.isCanvasMode = true;

      if (imagePreviewUrl) URL.revokeObjectURL(imagePreviewUrl);
      imagePreviewUrl = normalized.previewUrl;
      applyImageGeometry(normalized.width, normalized.height);
      canvas.setReferenceImage(imagePreviewUrl);

      if (canvas.layers.length === 0) {
        canvas.initCanvas(generation.width, generation.height);
      }

      gallery.showToast("Image loaded for inpainting", "success");
    } catch (e) {
      console.error("Failed to set up inpainting:", e);
      gallery.showToast("Failed to load image", "error");
    }
  }

  const LEFT_DEFAULT = 405;
  const RIGHT_DEFAULT = 338;
  let leftWidth = $state(LEFT_DEFAULT);
  let rightWidth = $state(RIGHT_DEFAULT);

  const LEFT_MIN = 280;
  const LEFT_MAX = 520;
  const RIGHT_MIN = 250;
  const RIGHT_MAX = 450;

  let dragging = $state<"left" | "right" | null>(null);
  let dragStartX = 0;
  let dragStartWidth = 0;

  function onDividerDown(side: "left" | "right", e: MouseEvent) {
    dragging = side;
    dragStartX = e.clientX;
    dragStartWidth = side === "left" ? leftWidth : rightWidth;
    e.preventDefault();
  }

  function computeDropTarget(mx: number, my: number): { side: SectionSide; index: number } | null {
    // Determine which column the cursor is over
    let side: SectionSide | null = null;
    if (leftColumnRef) {
      const r = leftColumnRef.getBoundingClientRect();
      if (mx >= r.left && mx <= r.right) side = "left";
    }
    if (!side && rightColumnRef) {
      const r = rightColumnRef.getBoundingClientRect();
      if (mx >= r.left && mx <= r.right) side = "right";
    }
    if (!side) {
      // Fallback: pick the closer column
      const lc = leftColumnRef?.getBoundingClientRect();
      const rc = rightColumnRef?.getBoundingClientRect();
      if (lc && rc) {
        const lDist = Math.abs(mx - (lc.left + lc.width / 2));
        const rDist = Math.abs(mx - (rc.left + rc.width / 2));
        side = lDist < rDist ? "left" : "right";
      } else {
        side = lc ? "left" : "right";
      }
    }

    // Use only non-dragged sections for midpoint calculation
    const allSections = side === "left" ? leftSections : rightSections;
    const sections = allSections.filter((id) => id !== draggingSection);

    // Find insertion index based on cursor Y vs section midpoints
    let index = sections.length;
    for (let i = 0; i < sections.length; i++) {
      const el = sectionRefs[sections[i]];
      if (!el) continue;
      const rect = el.getBoundingClientRect();
      const midY = rect.top + rect.height / 2;
      if (my < midY) {
        index = i;
        break;
      }
    }

    return { side, index };
  }

  function onPointerMove(e: MouseEvent) {
    if (draggingSection) {
      dragMouseX = e.clientX;
      dragMouseY = e.clientY;
      pendingDrop = computeDropTarget(e.clientX, e.clientY);
      return;
    }

    if (!dragging) return;
    const delta = e.clientX - dragStartX;
    if (dragging === "left") {
      leftWidth = Math.min(LEFT_MAX, Math.max(LEFT_MIN, dragStartWidth + delta));
    } else {
      rightWidth = Math.min(RIGHT_MAX, Math.max(RIGHT_MIN, dragStartWidth - delta));
    }
  }

  function onPointerUp() {
    if (draggingSection && pendingDrop) {
      const targetSide = pendingDrop.side;
      const targetIndex = Math.max(0, pendingDrop.index);

      sectionSides = {
        ...sectionSides,
        [draggingSection]: targetSide,
      };

      const remaining = sectionOrder.filter((id) => id !== draggingSection);
      // Use only visible sections to match computeDropTarget's index calculation
      const sideSections = remaining.filter((id) => sectionSides[id] === targetSide && sectionVisible(id));

      let insertAt = remaining.length;
      if (sideSections.length > 0) {
        if (targetIndex <= 0) {
          insertAt = remaining.indexOf(sideSections[0]);
        } else if (targetIndex >= sideSections.length) {
          insertAt = remaining.indexOf(sideSections[sideSections.length - 1]) + 1;
        } else {
          insertAt = remaining.indexOf(sideSections[targetIndex]);
        }
      }

      const next = [...remaining];
      next.splice(Math.max(0, insertAt), 0, draggingSection);
      sectionOrder = normalizeSectionOrder(next);
    }
    draggingSection = null;
    pendingDrop = null;
    dragMouseX = 0;
    dragMouseY = 0;
    dragCloneHtml = "";
    dragging = null;
  }

  function resetLeftWidth() {
    leftWidth = LEFT_DEFAULT;
  }

  function resetRightWidth() {
    rightWidth = RIGHT_DEFAULT;
  }

  $effect(() => {
    if (generation.mode !== "inpainting" && canvas.isCanvasMode) {
      canvas.isCanvasMode = false;
    }
  });
</script>

  {#snippet dragHandle(section: SectionId)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      onmousedown={(e) => startSectionDrag(section, e)}
      class="flex items-center px-3 cursor-grab active:cursor-grabbing text-neutral-600 hover:text-neutral-400"
      title="Drag to move section"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="12" r="1"/><circle cx="9" cy="5" r="1"/><circle cx="9" cy="19" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="5" r="1"/><circle cx="15" cy="19" r="1"/></svg>
    </div>
  {/snippet}

  {#snippet sectionDropZone(side: SectionSide, index: number)}
    {#if draggingSection}
      <div class="relative">
        <div
          class="h-0.5 rounded-full transition-[background-color,transform] duration-150 mx-2 {isPendingDrop(side, index)
            ? 'bg-indigo-400 shadow-[0_0_8px_rgba(99,102,241,0.5)] scale-y-[3]'
            : 'bg-transparent'}"
        ></div>
      </div>
    {/if}
  {/snippet}

  {#snippet sessionHistoryGrid()}
    {#if gallery.sessionImages.length === 0}
      <p class="text-xs text-neutral-500 italic">No images generated this session.</p>
    {:else}
      <div>
        <div class="grid grid-cols-2 gap-2">
          {#each sessionPageImages as image}
            <div class="group relative aspect-square rounded-lg overflow-hidden border border-neutral-800 hover:border-indigo-500 transition-colors">
              <button
                class="w-full h-full"
                onclick={() => gallery.openLightbox(image)}
              >
                  <img
                    use:lazyThumbnail={{ image }}
                    alt={image.filename}
                    class="w-full h-full object-cover"
                  />
              </button>
              <div
                class="absolute inset-0 bg-black/45 opacity-0 group-hover:opacity-100 transition-opacity flex items-end justify-center p-2 pointer-events-none"
                onclick={() => gallery.openLightbox(image)}
              >
                <div class="grid grid-cols-3 gap-1.5 pointer-events-auto">
                  {#if !image.is_upscaled}
                    <button
                      class="w-8 h-8 flex items-center justify-center rounded bg-indigo-900/90 hover:bg-indigo-700 text-neutral-300 text-xs"
                      title="Upscale"
                      onclick={(e) => { e.stopPropagation(); upscaleImage(image); }}
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="11" y1="8" x2="11" y2="14"/><line x1="8" y1="11" x2="14" y2="11"/></svg>
                    </button>
                  {/if}
                  <button
                    class="w-8 h-8 flex items-center justify-center rounded bg-indigo-900/90 hover:bg-indigo-700 text-neutral-300 text-xs"
                    title="Inpaint"
                    onclick={(e) => { e.stopPropagation(); inpaintImage(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
                  </button>
                  <button
                    class="w-8 h-8 flex items-center justify-center rounded bg-neutral-900/95 hover:bg-neutral-700 text-neutral-300 text-xs"
                    title="Save As"
                    onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
                  </button>
                  <button
                    class="w-8 h-8 flex items-center justify-center rounded bg-neutral-900/95 hover:bg-neutral-700 text-neutral-300 text-xs"
                    title="Copy"
                    onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                  </button>
                  <button
                    class="w-8 h-8 flex items-center justify-center rounded bg-red-900/90 hover:bg-red-800 text-neutral-300 text-xs"
                    title="Delete"
                    onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}
                  >
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                  </button>
                </div>
              </div>
            </div>
          {/each}
        </div>

        {#if sessionTotalPages > 1}
          <div class="flex items-center justify-between mt-2">
            <button
              onclick={() => { sessionPage = Math.max(0, sessionPage - 1); }}
              disabled={sessionPage === 0}
              class="px-2 py-1 text-[11px] rounded bg-neutral-800 text-neutral-300 hover:bg-neutral-700 disabled:opacity-30 disabled:pointer-events-none transition-colors"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"/></svg>
            </button>
            <span class="text-[11px] text-neutral-500 tabular-nums">{sessionPage + 1} / {sessionTotalPages}</span>
            <button
              onclick={() => { sessionPage = Math.min(sessionTotalPages - 1, sessionPage + 1); }}
              disabled={sessionPage >= sessionTotalPages - 1}
              class="px-2 py-1 text-[11px] rounded bg-neutral-800 text-neutral-300 hover:bg-neutral-700 disabled:opacity-30 disabled:pointer-events-none transition-colors"
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 18 15 12 9 6"/></svg>
            </button>
          </div>
        {/if}
      </div>
    {/if}
  {/snippet}

  {#snippet sessionHistorySection()}
    <div bind:this={sectionRefs['sessionHistory']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'sessionHistory' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("sessionHistory")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (sessionSectionOpen = !sessionSectionOpen)}
          title={sessionSectionOpen ? "Collapse Session History" : "Expand Session History"}
        >
          <span class="font-medium">Session History</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {sessionSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if sessionSectionOpen}
        <div class="px-3 pb-3 pt-1">
          {@render sessionHistoryGrid()}
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet dimensionsSection()}
    <div bind:this={sectionRefs['dimensions']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'dimensions' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("dimensions")}
        <button
          class="flex-1 flex items-center justify-between py-2 pr-3 text-xs text-neutral-300 hover:text-neutral-100 focus:outline-none"
          onclick={() => (dimensionsSectionOpen = !dimensionsSectionOpen)}
          title={dimensionsSectionOpen ? "Collapse Dimensions" : "Expand Dimensions"}
        >
          <span class="font-medium">Dimensions</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {dimensionsSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if dimensionsSectionOpen}
        <div class="px-3 pb-3 pt-1 cursor-default">
          <DimensionControls suggestedAspect={imageAspect} />
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet promptsSection()}
    <div bind:this={sectionRefs['prompts']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'prompts' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("prompts")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (promptsSectionOpen = !promptsSectionOpen)}
          title={promptsSectionOpen ? "Collapse Prompts" : "Expand Prompts"}
        >
          <span class="font-medium">Prompts</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {promptsSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if promptsSectionOpen}
        <div class="px-3 pb-3 pt-1">
          <PromptInputs showHistory={false} />
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet historySection()}
    <div bind:this={sectionRefs['history']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'history' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("history")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (historySectionOpen = !historySectionOpen)}
          title={historySectionOpen ? "Collapse History & Favorites" : "Expand History & Favorites"}
        >
          <span class="font-medium">Prompt History & Favorites</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {historySectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if historySectionOpen}
        <div class="px-3 pb-3 pt-1">
          <div class="space-y-1.5 max-h-56 overflow-y-auto pr-1">
            {#each sortedPromptHistory as entry}
              <div class="rounded border border-neutral-800 bg-neutral-900/80 p-2">
                <button
                  class="w-full text-left"
                  onclick={() => generation.applyPromptHistoryEntry(entry.id)}
                  title="Load prompt"
                >
                  <p class="text-[11px] text-neutral-200 max-h-8 overflow-hidden">{entry.positivePrompt || "(empty positive prompt)"}</p>
                  {#if entry.negativePrompt}
                    <p class="text-[10px] text-neutral-500 mt-0.5 whitespace-nowrap overflow-hidden text-ellipsis">Negative: {entry.negativePrompt}</p>
                  {/if}
                </button>
                <div class="mt-1.5 flex items-center justify-between gap-2">
                  <span class="text-[10px] text-neutral-500">{historyLabel(entry.createdAt)}</span>
                  <div class="flex items-center gap-1">
                    <button
                      class="px-1.5 py-0.5 text-[10px] rounded border transition-colors {entry.favorite ? 'border-amber-500 text-amber-300 bg-amber-500/10' : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-300'}"
                      onclick={() => generation.togglePromptFavorite(entry.id)}
                      title={entry.favorite ? "Unfavorite" : "Favorite"}
                    >
                      ★
                    </button>
                    <button
                      class="px-1.5 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors"
                      onclick={() => generation.removePromptHistoryEntry(entry.id)}
                      title="Remove"
                    >
                      Remove
                    </button>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet imageInputsSection()}
    <div bind:this={sectionRefs['imageInputs']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'imageInputs' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("imageInputs")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (imageSectionOpen = !imageSectionOpen)}
          title={imageSectionOpen ? "Collapse Image Inputs" : "Expand Image Inputs"}
        >
          <span class="font-medium">Image Inputs</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {imageSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if imageSectionOpen}
        <div class="px-3 pb-3 pt-1 space-y-3">
          {#if canvas.currentStagingImage}
            <div class="rounded-md border border-amber-700/50 bg-amber-900/20 p-2 flex items-center justify-between gap-2">
              <span class="text-[11px] text-amber-300">Staged image active. Input image controls are disabled.</span>
              <button
                class="px-2 py-1 text-[11px] rounded border border-amber-600/60 text-amber-200 hover:border-amber-400 hover:text-amber-100 transition-colors"
                onclick={() => canvas.dismissCurrentStaging()}
                title="Remove staged image"
              >
                Remove Staged
              </button>
            </div>
          {/if}

          <div class="{canvas.currentStagingImage ? 'opacity-50 pointer-events-none' : ''}">
            <p class="text-xs text-neutral-400 mb-1">Input Image</p>
            {#if imagePreviewUrl}
              <div class="relative group">
                <img
                  src={imagePreviewUrl}
                  alt="Input"
                  class="w-full rounded-lg border border-neutral-700 object-contain max-h-40"
                />
                <button
                  class="absolute top-1 right-1 w-6 h-6 flex items-center justify-center rounded bg-neutral-900/80 hover:bg-red-800 text-neutral-300 text-xs opacity-0 group-hover:opacity-100 transition-opacity"
                  onclick={clearImage}
                  title="Remove"
                >
                  &times;
                </button>
              </div>
            {:else}
              <button
                type="button"
                class="w-full bg-neutral-800 border border-dashed rounded-lg p-4 text-sm transition-colors flex flex-col items-center justify-center gap-2 cursor-pointer {dragOver
                  ? 'border-indigo-400 bg-indigo-500/10 text-indigo-300'
                  : 'border-neutral-600 text-neutral-400 hover:border-indigo-500 hover:text-indigo-400'}"
                onclick={browseImage}
                ondragover={(e) => { e.preventDefault(); dragOver = true; }}
                ondragleave={() => { dragOver = false; }}
                ondrop={handleImageDrop}
              >
                {#if uploading}
                  <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
                  Uploading...
                {:else if dragOver}
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                  Drop image here
                {:else}
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                  Browse or drop image
                {/if}
              </button>
              <button
                type="button"
                class="w-full text-xs text-neutral-500 hover:text-neutral-300 transition-colors flex items-center justify-center gap-1 mt-1"
                onclick={handleImagePaste}
              >
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" /></svg>
                Paste from clipboard
              </button>
            {/if}
          </div>

          <div>
            <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
              <span>Denoise Strength<InfoTip text="How much the AI changes the input image. 0 = no change, 1 = completely new image ignoring the input. Lower values (0.3-0.5) keep the original composition, higher values (0.6-0.8) allow more creative freedom." /></span>
              <span class="text-neutral-300">{generation.denoise.toFixed(2)}</span>
            </label>
            <input
              type="range"
              bind:value={generation.denoise}
              min="0"
              max="1"
              step="0.01"
              class="w-full accent-indigo-500"
            />
          </div>

          {#if generation.mode === "inpainting"}
            <div class="rounded-md border border-neutral-800 bg-neutral-900/70 p-2.5">
              <label class="flex items-center justify-between gap-3 text-xs text-neutral-300">
                <span class="leading-tight">Differential Diffusion<InfoTip text="Recommended for v-pred / Anima style models during inpainting unless you are using a CFG++ sampler. Helps preserve source structure while editing masked regions." /></span>
                <input
                  type="checkbox"
                  bind:checked={generation.differentialDiffusion}
                  class="accent-indigo-500 w-4 h-4 shrink-0"
                />
              </label>
            </div>
          {/if}

          {#if generation.mode === "inpainting"}
            <div>
              <div class="flex items-center justify-between mb-1">
                <p class="text-xs text-neutral-400">Mask Image</p>
                <button
                  class="px-2 py-1 text-[10px] rounded border border-neutral-700 text-neutral-300 hover:border-red-500 hover:text-red-300 transition-colors"
                  onclick={clearMask}
                  title="Remove current mask"
                >
                  Remove Mask
                </button>
              </div>
              {#if maskPreviewUrl}
                <div class="relative group">
                  <img
                    src={maskPreviewUrl}
                    alt="Mask"
                    class="w-full rounded-lg border border-neutral-700 object-contain max-h-40"
                  />
                  <button
                    class="absolute top-1 right-1 w-6 h-6 flex items-center justify-center rounded bg-neutral-900/80 hover:bg-red-800 text-neutral-300 text-xs opacity-0 group-hover:opacity-100 transition-opacity"
                    onclick={clearMask}
                    title="Remove"
                  >
                    &times;
                  </button>
                </div>
              {:else}
                <button
                  type="button"
                  class="w-full bg-neutral-800 border border-dashed rounded-lg p-4 text-sm transition-colors flex flex-col items-center justify-center gap-2 cursor-pointer {maskDragOver
                    ? 'border-indigo-400 bg-indigo-500/10 text-indigo-300'
                    : 'border-neutral-600 text-neutral-400 hover:border-indigo-500 hover:text-indigo-400'}"
                  onclick={browseMask}
                  ondragover={(e) => { e.preventDefault(); maskDragOver = true; }}
                  ondragleave={() => { maskDragOver = false; }}
                  ondrop={handleMaskDrop}
                >
                  {#if uploading}
                    <div class="w-4 h-4 border-2 border-indigo-400 border-t-transparent rounded-full animate-spin"></div>
                    Uploading...
                  {:else if maskDragOver}
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                    Drop mask here
                  {:else}
                    <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                    Browse or drop mask
                  {/if}
                </button>
                <button
                  type="button"
                  class="w-full text-xs text-neutral-500 hover:text-neutral-300 transition-colors flex items-center justify-center gap-1 mt-1"
                  onclick={handleMaskPaste}
                >
                  <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" /></svg>
                  Paste from clipboard
                </button>
              {/if}
            </div>

            <div>
              <div class="flex items-center justify-between text-xs mb-0.5">
                <span class="text-neutral-400">Grow Mask By<InfoTip text="Expands the masked area by this many pixels. Helps blend the inpainted region into the surrounding image for seamless results." /></span>
                <span class="text-neutral-300 tabular-nums">{generation.growMaskBy}px</span>
              </div>
              <input
                type="range"
                bind:value={generation.growMaskBy}
                min="0"
                max="64"
                step="1"
                class="w-full accent-indigo-500"
              />
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet inpaintLayersSection()}
    <div bind:this={sectionRefs['inpaintLayers']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'inpaintLayers' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("inpaintLayers")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (layersSectionOpen = !layersSectionOpen)}
          title={layersSectionOpen ? "Collapse Inpainting & Layers" : "Expand Inpainting & Layers"}
        >
          <span class="font-medium">Inpainting & Layers</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {layersSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if layersSectionOpen}
        <div class="px-3 pb-3 pt-1 space-y-2">
          <div class="grid grid-cols-2 gap-1">
            <button
              onclick={() => canvas.setInpaintDrawMode("mask")}
              class="px-2 py-1 text-[10px] rounded border transition-colors {canvas.inpaintDrawMode === 'mask'
                ? 'border-indigo-500 text-indigo-300 bg-indigo-500/10'
                : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200'}"
              title="Inpaint Mask Mode"
            >
              Inpaint Mask
            </button>
            <button
              onclick={() => canvas.setInpaintDrawMode("regular")}
              class="px-2 py-1 text-[10px] rounded border transition-colors {canvas.inpaintDrawMode === 'regular'
                ? 'border-indigo-500 text-indigo-300 bg-indigo-500/10'
                : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200'}"
              title="Regular Inpaint Mode"
            >
              Regular Inpaint
            </button>
          </div>

          {#if canvas.isCanvasMode}
            <LayerPanel />
          {:else}
            <div class="space-y-2">
              <p class="text-[11px] text-neutral-500">Canvas editor is off. Enable it to manage layers.</p>
              <button
                onclick={() => {
                  canvas.isCanvasMode = true;
                  if (canvas.layers.length === 0) {
                    canvas.initCanvas(generation.width, generation.height);
                  }
                }}
                class="w-full px-2 py-1.5 text-[11px] rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
                title="Enable canvas editor"
              >
                Enable Canvas Editor
              </button>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet generationSettingsSection()}
    <div bind:this={sectionRefs['generationSettings']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'generationSettings' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("generationSettings")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (controlsSectionOpen = !controlsSectionOpen)}
          title={controlsSectionOpen ? "Collapse Generation Settings" : "Expand Generation Settings"}
        >
          <span class="font-medium">Generation Settings</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {controlsSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if controlsSectionOpen}
        <div class="px-3 pb-3 pt-1 space-y-4">
          <ModelSelector />

          <div class="border-t border-neutral-800 pt-4">
            <SamplerSettings />
          </div>

          <div class="border-t border-neutral-800 pt-4">
            <UpscaleSettings />
          </div>
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet modelSection()}
    <div bind:this={sectionRefs['model']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'model' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("model")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (modelSectionOpen = !modelSectionOpen)}
          title={modelSectionOpen ? "Collapse Model" : "Expand Model"}
        >
          <span class="font-medium">Model</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {modelSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if modelSectionOpen}
        <div class="px-3 pb-3 pt-1 space-y-4">
          <ModelSelector />
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet samplerSection()}
    <div bind:this={sectionRefs['sampler']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'sampler' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("sampler")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (samplerSectionOpen = !samplerSectionOpen)}
          title={samplerSectionOpen ? "Collapse Sampler" : "Expand Sampler"}
        >
          <span class="font-medium">Sampler</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {samplerSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if samplerSectionOpen}
        <div class="px-3 pb-3 pt-1 space-y-4">
          <SamplerSettings />
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet controlnetSection()}
    <div bind:this={sectionRefs['controlnet']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'controlnet' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("controlnet")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (controlnetSectionOpen = !controlnetSectionOpen)}
          title={controlnetSectionOpen ? "Collapse ControlNet" : "Expand ControlNet"}
        >
          <span class="font-medium">ControlNet</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {controlnetSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if controlnetSectionOpen}
        <div class="px-3 pb-3 pt-1 space-y-4">
          <ControlNetSettings />
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet facefixSection()}
    <div bind:this={sectionRefs['facefix']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'facefix' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("facefix")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (facefixSectionOpen = !facefixSectionOpen)}
          title={facefixSectionOpen ? "Collapse Face Fix" : "Expand Face Fix"}
        >
          <span class="font-medium">Face Fix</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {facefixSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if facefixSectionOpen}
        <div class="px-3 pb-3 pt-1 space-y-4">
          <FaceFixSettings />
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet upscaleHistorySection()}
    <div bind:this={sectionRefs['upscaleHistory']} class="rounded-lg border border-neutral-800 bg-neutral-900/40 transition-[height,opacity] duration-150 {draggingSection === 'upscaleHistory' ? 'h-0 overflow-hidden opacity-0 m-0! p-0! border-0!' : 'opacity-100'}">
      <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
        {@render dragHandle("upscaleHistory")}
        <button
          class="flex-1 px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (postSectionOpen = !postSectionOpen)}
          title={postSectionOpen ? "Collapse Upscale" : "Expand Upscale"}
        >
          <span class="font-medium">Upscale</span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {postSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if postSectionOpen}
        <div class="px-3 pb-3 pt-1 space-y-4">
          <UpscaleSettings />
        </div>
      {/if}
    </div>
  {/snippet}

  {#snippet renderSection(section: SectionId)}
    {#if section === "dimensions"}
      {@render dimensionsSection()}
    {:else if section === "prompts"}
      {@render promptsSection()}
    {:else if section === "history"}
      {@render historySection()}
    {:else if section === "sessionHistory"}
      {@render sessionHistorySection()}
    {:else if section === "imageInputs"}
      {@render imageInputsSection()}
    {:else if section === "inpaintLayers"}
      {@render inpaintLayersSection()}
    {:else if section === "generationSettings"}
      {@render generationSettingsSection()}
    {:else if section === "model"}
      {@render modelSection()}
    {:else if section === "sampler"}
      {@render samplerSection()}
    {:else if section === "controlnet"}
      {@render controlnetSection()}
    {:else if section === "facefix"}
      {@render facefixSection()}
    {:else if section === "upscaleHistory"}
      {@render upscaleHistorySection()}
    {/if}
  {/snippet}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="flex h-full select-none {draggingSection ? 'cursor-grabbing' : ''}"
    onmousemove={onPointerMove}
    onmouseup={onPointerUp}
    onmouseleave={onPointerUp}
  >
    {#if leftHasSections || controlsSide === "left" || draggingSection}
      <div
        bind:this={leftColumnRef}
        class="overflow-y-auto overflow-x-hidden px-4 pt-4 flex flex-col gap-4 shrink-0 border-r {draggingSection && pendingDrop?.side === 'left' ? 'border-indigo-500/50' : 'border-transparent'}"
        style="width: {leftWidth}px"
      >
        {#if controlsSide === "left"}
          <div class="sticky top-0 z-10 bg-neutral-950 -mx-4 px-4 -mt-4 pt-4 pb-4">
            <div class="flex gap-1 bg-neutral-900 rounded-lg p-1">
              {#each modes as mode}
                <button
                  onclick={() => {
                    generation.mode = mode.id;
                    if (mode.id !== "inpainting") canvas.isCanvasMode = false;
                  }}
                  class="flex-1 text-xs py-1.5 rounded-md transition-colors {generation.mode === mode.id
                    ? 'bg-neutral-700 text-white'
                    : 'text-neutral-400 hover:text-neutral-200'}"
                >
                  {mode.label}
                </button>
              {/each}
            </div>

            {#if generation.mode === "inpainting"}
              <button
                onclick={() => {
                  canvas.isCanvasMode = !canvas.isCanvasMode;
                  if (canvas.isCanvasMode && canvas.layers.length === 0) {
                    canvas.initCanvas(generation.width, generation.height);
                  }
                }}
                class="flex items-center justify-between w-full px-3 py-2 mt-2 rounded-lg text-xs transition-colors {canvas.isCanvasMode
                  ? 'bg-indigo-600/20 border border-indigo-500/50 text-indigo-300'
                  : 'bg-neutral-800 border border-neutral-700 text-neutral-400 hover:text-neutral-200 hover:border-neutral-600'}"
              >
                <span class="flex items-center gap-2">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
                  Canvas Editor
                </span>
                <span class="text-[10px] {canvas.isCanvasMode ? 'text-indigo-400' : 'text-neutral-500'}">
                  {canvas.isCanvasMode ? 'ON' : 'OFF'}
                </span>
              </button>
            {/if}
          </div>
        {/if}

        {@render sectionDropZone("left", 0)}
        {#each leftRenderSections as section, i}
          {@render renderSection(section)}
          {@render sectionDropZone("left", i + 1)}
        {/each}

        {#if controlsSide === "left"}
          <div class="sticky bottom-0 mt-auto border-t border-neutral-800 bg-neutral-950 rounded-t-lg px-3 pt-3 pb-4">
            <h3 class="text-xs text-neutral-400 mb-2 font-medium">Generate</h3>
            <GenerateButton canvasEditorRef={canvasEditorRef} />
          </div>
        {/if}
      </div>
    {/if}

    {#if leftHasSections || controlsSide === "left" || draggingSection}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="w-1 mx-1 shrink-0 cursor-col-resize hover:bg-indigo-500/40 transition-colors {dragging === 'left' ? 'bg-indigo-500/60' : 'bg-neutral-800'}"
        onmousedown={(e) => onDividerDown("left", e)}
        ondblclick={resetLeftWidth}
        title="Drag to resize, double-click to reset"
      ></div>
    {/if}

    {#if canvas.isCanvasMode}
      <div class="flex-1 min-w-0 flex flex-col overflow-hidden">
        <CanvasEditor bind:this={canvasEditorRef} />
      </div>
    {:else}
      <div class="flex-1 min-w-0 p-6 flex flex-col gap-4 overflow-y-auto">
        <ProgressBar />
        <PreviewImage />
      </div>
    {/if}

    {#if rightHasSections || controlsSide === "right" || draggingSection}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="w-1 mx-1 shrink-0 cursor-col-resize hover:bg-indigo-500/40 transition-colors {dragging === 'right' ? 'bg-indigo-500/60' : 'bg-neutral-800'}"
        onmousedown={(e) => onDividerDown("right", e)}
        ondblclick={resetRightWidth}
        title="Drag to resize, double-click to reset"
      ></div>
    {/if}

    {#if rightHasSections || controlsSide === "right" || draggingSection}
      <div
        bind:this={rightColumnRef}
        class="overflow-y-auto p-4 space-y-4 shrink-0 border-l {draggingSection && pendingDrop?.side === 'right' ? 'border-indigo-500/50' : 'border-transparent'}"
        style="width: {rightWidth}px"
      >
        {#if controlsSide === "right"}
          <div class="sticky top-0 z-10 bg-neutral-950 -mx-4 px-4 -mt-4 pt-4 pb-4">
            <div class="flex gap-1 bg-neutral-900 rounded-lg p-1">
              {#each modes as mode}
                <button
                  onclick={() => {
                    generation.mode = mode.id;
                    if (mode.id !== "inpainting") canvas.isCanvasMode = false;
                  }}
                  class="flex-1 text-xs py-1.5 rounded-md transition-colors {generation.mode === mode.id
                    ? 'bg-neutral-700 text-white'
                    : 'text-neutral-400 hover:text-neutral-200'}"
                >
                  {mode.label}
                </button>
              {/each}
            </div>

            {#if generation.mode === "inpainting"}
              <button
                onclick={() => {
                  canvas.isCanvasMode = !canvas.isCanvasMode;
                  if (canvas.isCanvasMode && canvas.layers.length === 0) {
                    canvas.initCanvas(generation.width, generation.height);
                  }
                }}
                class="flex items-center justify-between w-full px-3 py-2 mt-2 rounded-lg text-xs transition-colors {canvas.isCanvasMode
                  ? 'bg-indigo-600/20 border border-indigo-500/50 text-indigo-300'
                  : 'bg-neutral-800 border border-neutral-700 text-neutral-400 hover:text-neutral-200 hover:border-neutral-600'}"
              >
                <span class="flex items-center gap-2">
                  <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/></svg>
                  Canvas Editor
                </span>
                <span class="text-[10px] {canvas.isCanvasMode ? 'text-indigo-400' : 'text-neutral-500'}">
                  {canvas.isCanvasMode ? 'ON' : 'OFF'}
                </span>
              </button>
            {/if}
          </div>
        {/if}

        {@render sectionDropZone("right", 0)}
        {#each rightRenderSections as section, i}
          {@render renderSection(section)}
          {@render sectionDropZone("right", i + 1)}
        {/each}

        {#if controlsSide === "right"}
          <div class="sticky bottom-0 mt-auto border-t border-neutral-800 bg-neutral-950 rounded-t-lg px-3 pt-3 pb-4">
            <h3 class="text-xs text-neutral-400 mb-2 font-medium">Generate</h3>
            <GenerateButton canvasEditorRef={canvasEditorRef} />
          </div>
        {/if}
      </div>
    {/if}
  </div>

  {#if draggingSection && dragCloneHtml}
    <div
      class="fixed z-[70] pointer-events-none"
      style="left: {dragMouseX - dragOffsetX}px; top: {dragMouseY - dragOffsetY}px; width: {dragWidth}px;"
    >
      <div
        class="rounded-lg border border-indigo-400/60 shadow-2xl shadow-indigo-900/30 scale-[1.02] opacity-90"
        style="filter: brightness(1.1);"
      >
        {@html dragCloneHtml}
      </div>
    </div>
  {/if}
