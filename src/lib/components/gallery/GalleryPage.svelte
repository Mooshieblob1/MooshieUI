<script lang="ts">
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { lazyThumbnail } from "../../utils/lazyThumbnail.js";
  import { loadOutputImageForGenerationInput, uploadOutputImageForGenerationInput } from "../../utils/galleryActions.js";
  import { uploadImageBytes } from "../../utils/api.js";
  import type { OutputImage } from "../../types/index.js";

  interface Props {
    onSwitchToGenerate?: () => void;
  }

  let { onSwitchToGenerate }: Props = $props();

  const MAX_INPUT_PIXELS = 1024 * 1024;
  const GALLERY_PREFS_KEY = "mooshieui.gallery.prefs.v1";

  let galleryImagesPerRow = $state(5);
  let gallerySortBy = $state<"date" | "name" | "size">("date");
  let gallerySortDir = $state<"asc" | "desc">("desc");
  let galleryGroupBy = $state<"none" | "date" | "month" | "mode" | "prompt" | "board">("none");
  let galleryBoardFilter = $state<string>("all");
  let newBoardName = $state("");
  let galleryView = $state<"huge" | "large" | "small" | "details">("large");
  let galleryRenderLimit = $state(48);
  let dirPickerImage = $state<OutputImage | null>(null);

  function loadMoreGallery(node: HTMLElement) {
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) galleryRenderLimit += 48;
      },
      { rootMargin: "200px" },
    );
    observer.observe(node);
    return { destroy() { observer.disconnect(); } };
  }

  function getImageTimestamp(image: OutputImage): number {
    return image.generated_at_ms ?? 0;
  }

  function getImageSize(image: OutputImage): number {
    return image.file_size_bytes ?? 0;
  }

  function formatDate(ts: number | undefined): string {
    if (!ts) return "Unknown";
    return locale.formatDateTime(ts);
  }

  function formatDateGroup(ts: number | undefined): string {
    if (!ts) return "Unknown Date";
    return new Date(ts).toLocaleDateString(locale.intlTag, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  }

  function formatMonthGroup(ts: number | undefined): string {
    if (!ts) return "Unknown Month";
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

  function formatBytes(bytes: number | undefined): string {
    if (!bytes || bytes <= 0) return "-";
    return locale.formatBytes(bytes);
  }

  function viewColumns(view: "huge" | "large" | "small" | "details"): number {
    if (view === "huge") return Math.max(2, galleryImagesPerRow - 2);
    if (view === "small") return Math.min(10, galleryImagesPerRow + 2);
    return galleryImagesPerRow;
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
      if (typeof parsed.imagesPerRow === "number") galleryImagesPerRow = Math.max(2, Math.min(8, Math.round(parsed.imagesPerRow)));
      if (parsed.sortBy) gallerySortBy = parsed.sortBy;
      if (parsed.sortDir) gallerySortDir = parsed.sortDir;
      if (parsed.groupBy) galleryGroupBy = parsed.groupBy;
      if (parsed.boardFilter) galleryBoardFilter = parsed.boardFilter;
      if (parsed.view) galleryView = parsed.view;
    } catch {}
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
      return { bytes: imageBytes, previewUrl: sourceUrl, width: dims.width, height: dims.height, filename: fallbackFilename };
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
        if (!ctx) return reject(new Error("Failed to create resize context"));
        ctx.imageSmoothingEnabled = true;
        ctx.imageSmoothingQuality = "high";
        ctx.drawImage(img, 0, 0, targetWidth, targetHeight);
        out.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("Failed to encode resized image"))), "image/png");
      };
      img.onerror = () => reject(new Error("Failed to decode source image"));
      img.src = sourceUrl;
    });

    URL.revokeObjectURL(sourceUrl);
    const resizedBuffer = await resizedBlob.arrayBuffer();
    const resizedBytes = Array.from(new Uint8Array(resizedBuffer));
    const resizedPreview = URL.createObjectURL(resizedBlob);
    return { bytes: resizedBytes, previewUrl: resizedPreview, width: targetWidth, height: targetHeight, filename: fallbackFilename };
  }

  async function loadImageForMode(image: OutputImage, mode: "img2img" | "inpainting") {
    try {
      const source = await loadOutputImageForGenerationInput(
        image,
        mode === "inpainting" ? "inpaint_input.png" : "img2img_input.png",
      );
      const normalized = mode === "inpainting" ? await normalizeImageBytes(source.bytes, source.filename) : null;
      const response = await uploadImageBytes(normalized ? normalized.bytes : source.bytes, normalized ? normalized.filename : source.filename);
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
        if (canvas.layers.length === 0 || canvas.canvasWidth !== normalized.width || canvas.canvasHeight !== normalized.height) {
          canvas.initCanvas(normalized.width, normalized.height);
        }
      }
      onSwitchToGenerate?.();
      gallery.showToast(
        mode === "inpainting" ? locale.t("gallery.toast.loaded_inpaint") : locale.t("gallery.toast.loaded_img2img"),
        "success",
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

  async function upscaleImage(image: OutputImage) {
    try {
      generation.inputImage = await uploadOutputImageForGenerationInput(image, "refine_input.png");
      generation.mode = "img2img";
      generation.upscaleEnabled = true;
      onSwitchToGenerate?.();
      gallery.showToast(locale.t("gallery.toast.loaded_upscale"), "success");
    } catch (e) {
      console.error("Failed to set up upscale:", e);
      gallery.showToast(locale.t("gallery.toast.failed_load"), "error");
    }
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
        locale.t("gallery.sort_by_artist_done", { sorted: String(result.sorted), boards: String(result.boards.length) }),
        "success",
      );
    }
  }

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
    return galleryBoardFilter === "all" ? sorted : sorted.filter((image) => gallery.getBoard(image) === galleryBoardFilter);
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
    }
    return [{ label: locale.t("gallery.all_images"), images: sortedGalleryImages }];
  });

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

  const thumbSize = $derived(viewColumns(galleryView) <= 3 ? 480 : 384);

  if (typeof window !== "undefined") loadGalleryPrefs();

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
    } catch {}
  });
</script>

<div class="p-3 md:p-6 h-full overflow-y-auto will-change-scroll">
  {#if gallery.loading}
    <div class="flex items-center justify-center h-full text-neutral-500">{locale.t("gallery.loading")}</div>
  {:else if gallery.images.length === 0}
    <div class="flex items-center justify-center h-full text-neutral-500">{locale.t("gallery.empty_generate")}</div>
  {:else}
    <div class="space-y-4">
      {#if gallery.hasExpiry}
        <div class="px-4 py-3 rounded-xl bg-amber-900/30 border border-amber-700/50 text-amber-300 text-sm flex items-start gap-3">
          <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5 shrink-0 mt-0.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
          <div>
            <p class="font-medium text-amber-200">{locale.t("gallery.expiry_warning")}</p>
            <p class="text-amber-400/80 text-xs mt-1">{locale.t("gallery.expiry_hint")}</p>
            {#if gallery.expiringWithin24h > 0}
              <p class="text-amber-200 text-xs mt-1 font-semibold">{locale.t("gallery.expiry_soon", { count: String(gallery.expiringWithin24h) })}</p>
            {/if}
            {#if gallery.storageInfo}
              <p class="text-amber-400/60 text-xs mt-1">{locale.t("gallery.storage_usage")}: {gallery.storageLabel}</p>
            {/if}
          </div>
        </div>
      {/if}

      <div class="rounded-xl border border-neutral-800 bg-neutral-900/60 p-3 space-y-3">
        <div class="grid grid-cols-1 lg:grid-cols-4 gap-3 items-end">
          <div class="lg:col-span-2">
            <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.images_per_row")} {viewColumns(galleryView)}</div>
            <input type="range" bind:value={galleryImagesPerRow} min="2" max="8" step="1" class="w-full accent-indigo-500" disabled={galleryView === "details"} />
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
              <input type="text" bind:value={newBoardName} class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-100 placeholder-neutral-500" placeholder={locale.t("gallery.placeholder_board")} />
              <button class="px-3 py-2 text-xs rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors" onclick={addBoard} disabled={!newBoardName.trim()}>{locale.t("gallery.add")}</button>
            </div>
          </div>
        </div>

        <div>
          <div class="text-xs text-neutral-400 mb-2">{locale.t("gallery.view")}</div>
          <div class="flex flex-wrap gap-2">
            <button onclick={() => (gallerySortDir = gallerySortDir === "asc" ? "desc" : "asc")} class="px-3 py-1.5 text-xs rounded border transition-colors border-neutral-700 text-neutral-300 hover:border-neutral-500">{gallerySortDir === "asc" ? locale.t("gallery.ascending") : locale.t("gallery.descending")}</button>
            <button onclick={() => (galleryView = "huge")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'huge' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.huge_icons")}</button>
            <button onclick={() => (galleryView = "large")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'large' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.large_icons")}</button>
            <button onclick={() => (galleryView = "small")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'small' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.small_icons")}</button>
            <button onclick={() => (galleryView = "details")} class="px-3 py-1.5 text-xs rounded border transition-colors {galleryView === 'details' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.detailed_view")}</button>
            <button onclick={rescanGalleryMetadata} class="px-3 py-1.5 text-xs rounded border transition-colors border-amber-700/70 text-amber-300 hover:border-amber-500 hover:text-amber-200">{locale.t("gallery.rescan_metadata")}</button>
            <button onclick={sortGalleryByArtist} disabled={gallery.autoSorting} class="px-3 py-1.5 text-xs rounded border transition-colors border-indigo-700/70 text-indigo-300 hover:border-indigo-500 hover:text-indigo-200 disabled:opacity-50">{gallery.autoSorting ? locale.t("gallery.sort_by_artist_running") : locale.t("gallery.sort_by_artist")}</button>
          </div>
        </div>
      </div>

      {#each galleryGroupsVisible as group}
        <section class="space-y-2">
          {#if galleryGroupBy !== "none"}
            <h3 class="text-sm text-neutral-300 font-medium">{group.label}</h3>
          {/if}
          {#if galleryView === "details"}
            <div class="rounded-xl border border-neutral-800 overflow-hidden">
              <div class="grid grid-cols-[72px_1fr_150px_120px_320px] gap-2 px-3 py-2 bg-neutral-900 text-[11px] uppercase tracking-wide text-neutral-500 border-b border-neutral-800">
                <div>{locale.t("gallery.col_preview")}</div><div>{locale.t("gallery.col_name")}</div><div>{locale.t("gallery.col_date")}</div><div>{locale.t("gallery.col_size")}</div><div>{locale.t("gallery.col_actions")}</div>
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
                    <select class="px-2 py-1 text-[11px] rounded bg-neutral-800 border border-neutral-700 text-neutral-200" value={boardLabel(image)} onchange={(e) => assignBoard(image, (e.target as HTMLSelectElement).value)}>
                      <option value="Unsorted">{locale.t("gallery.unsorted")}</option>
                      {#each gallery.boards as board}
                        <option value={board}>{board}</option>
                      {/each}
                    </select>
                    {#if generation.manualSaveMode && !image.gallery_filename}
                      <button class="px-2 py-1 text-[11px] rounded bg-indigo-700 hover:bg-indigo-600 text-neutral-100" onclick={() => saveToDir(image)}>{locale.t("gallery.save_to_folder")}</button>
                    {/if}
                    <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => img2imgImage(image)}>{locale.t("gallery.i2i")}</button>
                    <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => inpaintImage(image)}>{locale.t("gallery.inpaint")}</button>
                    {#if !image.is_upscaled}
                      <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => upscaleImage(image)}>{locale.t("gallery.upscale")}</button>
                    {/if}
                    <button class="px-2 py-1 text-[11px] rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100 disabled:opacity-50" disabled={gallery.saving} onclick={() => gallery.saveImageAs(image)}>{gallery.saving ? locale.t("gallery.saving") : locale.t("gallery.save")}</button>
                    <button class="px-2 py-1 text-[11px] rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100" onclick={() => gallery.copyToClipboard(image)}>{locale.t("gallery.copy")}</button>
                    <button class="px-2 py-1 text-[11px] rounded bg-red-900/80 hover:bg-red-800 text-neutral-100" onclick={() => gallery.deleteImage(image)}>{locale.t("gallery.delete")}</button>
                  </div>
                </div>
              {/each}
            </div>
          {:else}
            <div class="grid gap-3" style="grid-template-columns: repeat({viewColumns(galleryView)}, minmax(0, 1fr));">
              {#each group.images as image}
                <div class="group relative rounded-lg overflow-hidden border border-neutral-800 hover:border-indigo-500 transition-colors {galleryView === 'huge' ? 'aspect-4/3' : 'aspect-square'}">
                  <button class="w-full h-full" onclick={() => gallery.openLightbox(image)}>
                    <img use:lazyThumbnail={{ image, size: thumbSize }} alt={image.filename} class="w-full h-full object-cover" />
                  </button>
                  <div class="absolute top-1 left-1 px-1.5 py-0.5 rounded bg-black/70 text-[10px] text-neutral-200 pointer-events-none">{boardLabel(image)}</div>
                  {#if generation.manualSaveMode && !image.gallery_filename}
                    <div class="absolute top-0 right-0 pt-1 pr-1 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button class="w-7 h-7 flex items-center justify-center rounded bg-indigo-700/90 hover:bg-indigo-600 text-neutral-100 shadow" title={locale.t("gallery.save_to_folder")} onclick={(e) => { e.stopPropagation(); saveToDir(image); }}>
                        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
                      </button>
                    </div>
                  {/if}
                  <div class="absolute bottom-0 inset-x-0 flex justify-center items-center gap-1 px-1.5 pb-1.5 pt-6 opacity-0 group-hover:opacity-100 transition-opacity bg-linear-to-t from-black/80 to-transparent">
                    <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black text-[11px] font-bold shadow" title={locale.t("gallery.img2img")} onclick={(e) => { e.stopPropagation(); img2imgImage(image); }}>I2I</button>
                    <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black shadow" title={locale.t("gallery.inpaint")} onclick={(e) => { e.stopPropagation(); inpaintImage(image); }}>✎</button>
                    {#if !image.is_upscaled}
                      <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black shadow" title={locale.t("gallery.upscale")} onclick={(e) => { e.stopPropagation(); upscaleImage(image); }}>+</button>
                    {/if}
                    <button class="w-7 h-7 flex items-center justify-center rounded bg-neutral-800/90 hover:bg-neutral-700 text-neutral-200 shadow disabled:opacity-50" disabled={gallery.saving} title={locale.t("gallery.save_as")} onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}>↓</button>
                    <button class="w-7 h-7 flex items-center justify-center rounded bg-neutral-800/90 hover:bg-neutral-700 text-neutral-200 shadow" title={locale.t("gallery.copy")} onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}>⧉</button>
                    <button class="w-7 h-7 flex items-center justify-center rounded bg-red-900/80 hover:bg-red-800 text-red-300 hover:text-red-200 shadow" title={locale.t("gallery.delete")} onclick={(e) => { e.stopPropagation(); gallery.deleteImage(image); }}>×</button>
                  </div>
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

{#if dirPickerImage}
  <div class="fixed inset-0 z-60 flex items-center justify-center bg-black/70 p-4">
    <div class="w-full max-w-md rounded-xl border border-neutral-700 bg-neutral-900 p-4">
      <h3 class="text-sm font-semibold text-neutral-100 mb-3">{locale.t("gallery.choose_save_folder")}</h3>
      <div class="space-y-2 max-h-64 overflow-y-auto">
        {#each generation.autoSaveDirs.filter(Boolean) as dir}
          <button class="w-full text-left px-3 py-2 rounded border border-neutral-700 hover:border-indigo-500 hover:text-indigo-300 text-sm text-neutral-200 transition-colors" onclick={() => { gallery.saveImageToDir(dirPickerImage!, dir!); dirPickerImage = null; }}>
            {dir}
          </button>
        {/each}
      </div>
      <button class="mt-3 w-full px-3 py-2 rounded border border-neutral-700 text-neutral-300 hover:border-neutral-500" onclick={() => (dirPickerImage = null)}>{locale.t("common.cancel")}</button>
    </div>
  </div>
{/if}
