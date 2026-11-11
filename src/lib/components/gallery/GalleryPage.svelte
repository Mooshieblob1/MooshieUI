<script lang="ts">
  import { tick } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { portal } from "../../utils/portal.js";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { gallery, isVideoImage, GALLERY_MAX_RATING, GALLERY_SOURCE_OWN } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { directorTools, directorToolsAvailable } from "../../stores/directorTools.svelte.js";
  import { naiImageEnhance, naiImageEnhanceAvailable } from "../../stores/naiImageEnhance.svelte.js";
  import { lazyThumbnail } from "../../utils/lazyThumbnail.js";
  import { uploadOutputImageForGenerationInput } from "../../utils/galleryActions.js";
  import { prepareOutputImageForEditMode } from "../../utils/editImagePreparation.js";
  import { uploadImageBytes } from "../../utils/api.js";
  import { formatGenerationTime } from "../../utils/localeFormat.js";
  import { UPSCALE_FACTOR } from "../../utils/novelaiEnhance.js";
  import { isTauri } from "../../utils/ipc.js";
  import { buildGalleryRows, rowOffsets, selectionRange, visibleRowRange } from "../../utils/galleryGrid.js";
  import type { GalleryViewMode } from "../../stores/gallery.svelte.js";
  import type { OutputImage } from "../../types/index.js";

  interface Props {
    onSwitchToGenerate?: () => void;
    /** Open the 3D page with this image as the source. */
    onMake3D?: (image: OutputImage) => void;
  }

  let { onSwitchToGenerate, onMake3D }: Props = $props();

  let newBoardName = $state("");
  // Detailed view only: its rows vary in height, so it grows as you scroll
  // instead of being windowed like the grid.
  let galleryRenderLimit = $state(48);
  // The search box writes to the store after a short pause, so a large
  // gallery is not re-filtered on every keystroke.
  let searchInput = $state(gallery.searchQuery);
  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  let showSearchHelp = $state(false);
  let dirPickerImage = $state<OutputImage | null>(null);
  // Touch screens have no hover, so a tile's action tray stays hidden until its
  // Actions toggle opens it. One tile at a time.
  let touchActionsImage = $state<OutputImage | null>(null);

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

  /** Compare button label: pin the first image, then pick the second (issue #517). */
  function comparePinTitle(image: OutputImage): string {
    if (!gallery.comparePin) return locale.t("gallery.compare.pin");
    if (gallery.comparePin === image) return locale.t("gallery.compare.unpin");
    return locale.t("gallery.compare.with_pinned");
  }

  /**
   * Visibility for a tile's overlays (action tray, save-to-folder button). A
   * hidden overlay must not take taps, or a tap near the tile's edge on a touch
   * screen hits an invisible button, delete included (issue #736). Overlays
   * show on mouse hover, on keyboard focus inside the tile, or while the tile's
   * touch Actions toggle is open.
   */
  function tileOverlayClass(image: OutputImage): string {
    return touchActionsImage === image
      ? "opacity-100"
      : "opacity-0 pointer-events-none group-hover:opacity-100 group-hover:pointer-events-auto group-has-[:focus-visible]:opacity-100 group-has-[:focus-visible]:pointer-events-auto";
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
    if (mode === "video") return locale.t("gallery.mode.video");
    return locale.t("gallery.mode.unknown");
  }

  function formatBytes(bytes: number | undefined): string {
    if (!bytes || bytes <= 0) return "-";
    return locale.formatBytes(bytes);
  }

  /** m:ss for the grid duration badge. 5.2 -> "0:05", 65 -> "1:05". */
  function formatDuration(seconds: number): string {
    const total = Math.max(0, Math.round(seconds));
    const m = Math.floor(total / 60);
    const s = total % 60;
    return `${m}:${String(s).padStart(2, "0")}`;
  }

  function viewColumns(view: GalleryViewMode): number {
    if (view === "huge") return Math.max(2, gallery.imagesPerRow - 2);
    if (view === "small") return Math.min(10, gallery.imagesPerRow + 2);
    return gallery.imagesPerRow;
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
    gallery.boardFilter = name;
    newBoardName = "";
  }

  function onSearchInput(value: string) {
    searchInput = value;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      gallery.searchQuery = value;
      searchTimer = null;
    }, 150);
  }

  function clearSearch() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = null;
    searchInput = "";
    gallery.searchQuery = "";
  }

  /** Short label for a linked folder: its last path segment. */
  function folderLabel(folder: string): string {
    return folder.split(/[\\/]/).filter(Boolean).pop() ?? folder;
  }

  async function loadImageForMode(image: OutputImage, mode: "img2img" | "inpainting") {
    try {
      const prepared = await prepareOutputImageForEditMode(image, mode);
      const response = await uploadImageBytes(prepared.uploadBytes, prepared.uploadFilename);
      generation.inputImage = response.name;
      canvas.clearMask();
      generation.mode = mode;
      generation.upscaleEnabled = false;
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

  /** A toast for a change to the boards, with an Undo that puts them back. */
  function boardChangeToast(message: string, before: ReturnType<typeof gallery.boardSnapshot>) {
    gallery.showToast(message, "success", {
      durationMs: 10000,
      actionLabel: locale.t("gallery.undo"),
      onAction: () => {
        gallery.restoreBoards(before);
        gallery.clearToast();
      },
    });
  }

  async function sortGalleryByArtist() {
    const before = gallery.boardSnapshot();
    const result = await gallery.autoSortByArtist(connection.artistGalleryManifestUrl);
    if (result.sorted === 0 && result.scanned > 0) {
      gallery.showToast(locale.t("gallery.sort_by_artist_none"), "info");
    } else if (result.sorted > 0) {
      boardChangeToast(
        locale.t("gallery.sort_by_artist_done", { sorted: String(result.sorted), boards: String(result.boards.length) }),
        before,
      );
    }
  }

  const artistBoards = $derived(gallery.artistBoards);
  const characterBoards = $derived(gallery.characterBoards);

  /** Take an automatic sort back: its boards go, their images return to Unsorted. */
  function removeSortedBoards(boards: string[]) {
    if (boards.length === 0) return;
    const before = gallery.boardSnapshot();
    gallery.removeBoards(boards);
    boardChangeToast(locale.t("gallery.sort_removed", { boards: String(boards.length) }), before);
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

  const visibleImages = $derived(gallery.visibleImages);
  const knownCharacters = $derived(gallery.knownCharacters);
  /** Counted only while the filter panel shows them. */
  const characterCounts = $derived(knownCharacters.length > 0 && !gallery.filtersCollapsed ? gallery.characterCounts : {});
  const characterFilterActive = $derived(gallery.characterFilter !== "all" && knownCharacters.includes(gallery.characterFilter));

  function sortShownByCharacter() {
    const before = gallery.boardSnapshot();
    const result = gallery.sortByCharacter(selection.size > 0 ? selectedImages() : orderedImages);
    if (result.sorted > 0) {
      boardChangeToast(
        locale.t("gallery.sort_by_character_done", { sorted: String(result.sorted), boards: String(result.boards.length) }),
        before,
      );
    } else {
      gallery.showToast(locale.t("gallery.sort_by_character_none"), "info");
    }
  }

  /** Over this many images a scan asks first: the tagger takes about a second each. */
  const TAG_SCAN_CONFIRM = 100;

  async function scanForCharacters() {
    if (gallery.tagScan) {
      gallery.stopTagScan();
      return;
    }
    const scope = selection.size > 0 ? selectedImages() : orderedImages;
    const count = scope.filter((image) => gallery.needsTagScan(image)).length;
    if (count === 0) {
      gallery.showToast(locale.t("gallery.character_scan_none"), "info");
      return;
    }
    if (count > TAG_SCAN_CONFIRM && !confirm(locale.t("gallery.character_scan_confirm", { count: String(count) }))) return;
    const result = await gallery.scanTags(scope);
    if (result.error) gallery.showToast(locale.t("gallery.character_scan_failed", { error: result.error }), "error");
    else gallery.showToast(locale.t("gallery.character_scan_done", { count: String(result.scanned) }), "success");
  }

  const filtersActive = $derived(
    gallery.searchQuery.trim() !== "" ||
      gallery.boardFilter !== "all" ||
      gallery.sourceFilter !== "all" ||
      gallery.favouritesOnly ||
      gallery.minRating > 0 ||
      gallery.nsfwFilter !== "all" ||
      gallery.duplicatesOnly ||
      characterFilterActive,
  );

  /** A filter inside the foldable panel is narrowing the gallery (the search box stays visible either way). */
  const panelFiltersActive = $derived(
    gallery.boardFilter !== "all" ||
      gallery.sourceFilter !== "all" ||
      gallery.favouritesOnly ||
      gallery.minRating > 0 ||
      gallery.nsfwFilter !== "all" ||
      gallery.duplicatesOnly ||
      characterFilterActive,
  );

  /** Groups get a header row: Group By is on, or the page shows sets of repeated generations. */
  const showGroupHeaders = $derived(gallery.groupBy !== "none" || gallery.duplicatesOnly);

  const groupedGalleryImages = $derived.by(() => {
    if (gallery.duplicatesOnly) {
      // One group per repeated generation, oldest copy first.
      const sets = new Map<string, OutputImage[]>();
      for (const image of visibleImages) {
        const key = gallery.duplicateKey(image) ?? "";
        const bucket = sets.get(key) ?? [];
        bucket.push(image);
        sets.set(key, bucket);
      }
      return Array.from(sets.values()).map((images) => {
        images.sort((a, b) => (a.generated_at_ms ?? 0) - (b.generated_at_ms ?? 0));
        return {
          label: locale.t("gallery.duplicate_group", { seed: images[0]?.metadata?.seed ?? "", count: String(images.length) }),
          images,
        };
      });
    }
    if (gallery.groupBy !== "none") {
      const grouped = new Map<string, OutputImage[]>();
      for (const image of visibleImages) {
        const key =
          gallery.groupBy === "date"
            ? formatDateGroup(image.generated_at_ms)
            : gallery.groupBy === "month"
              ? formatMonthGroup(image.generated_at_ms)
              : gallery.groupBy === "mode"
                ? modeLabel(image.generation_mode)
                : gallery.groupBy === "board"
                  ? gallery.getBoard(image)
                  : gallery.groupBy === "character"
                    ? (gallery.charactersIn(image).join(", ") || locale.t("gallery.no_character"))
                  : (image.prompt_id || locale.t("gallery.no_prompt_id"));
        const bucket = grouped.get(key) ?? [];
        bucket.push(image);
        grouped.set(key, bucket);
      }
      return Array.from(grouped.entries()).map(([label, images]) => ({ label, images }));
    }
    return [{ label: locale.t("gallery.all_images"), images: visibleImages }];
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

  const gridColumns = $derived(viewColumns(gallery.viewMode));
  const thumbSize = $derived(gridColumns <= 3 ? 480 : 384);

  // ---------------------------------------------------------------------------
  // Windowed grid
  // ---------------------------------------------------------------------------

  const GRID_GAP = 12;
  const GRID_HEADER_HEIGHT = 32;
  /** Rows kept rendered above and below the viewport, in pixels. */
  const GRID_OVERSCAN = 600;

  let scroller = $state<HTMLElement | null>(null);
  let listEl: HTMLElement | null = null;
  let listWidth = $state(0);
  /** How far the viewport's top edge is into the grid; negative while the filters are still on screen. */
  let viewTop = $state(0);
  let viewHeight = $state(0);
  let measureFrame = 0;

  function measureGrid() {
    measureFrame = 0;
    if (!scroller || !listEl) return;
    viewTop = scroller.getBoundingClientRect().top - listEl.getBoundingClientRect().top;
    viewHeight = scroller.clientHeight;
    listWidth = listEl.clientWidth;
  }

  function scheduleMeasure() {
    if (!measureFrame) measureFrame = requestAnimationFrame(measureGrid);
  }

  function gridList(node: HTMLElement) {
    listEl = node;
    const observer = new ResizeObserver(scheduleMeasure);
    observer.observe(node);
    if (scroller) observer.observe(scroller);
    measureGrid();
    return {
      destroy() {
        observer.disconnect();
        if (listEl === node) listEl = null;
      },
    };
  }

  const tileHeight = $derived.by(() => {
    const width = Math.max(0, (listWidth - GRID_GAP * (gridColumns - 1)) / gridColumns);
    return Math.round(gallery.viewMode === "huge" ? (width * 3) / 4 : width);
  });
  const gridRows = $derived(buildGalleryRows(groupedGalleryImages, gridColumns, showGroupHeaders));
  const gridOffsets = $derived(rowOffsets(gridRows, tileHeight + GRID_GAP, GRID_HEADER_HEIGHT));
  const gridHeight = $derived(gridOffsets[gridOffsets.length - 1] ?? 0);
  const gridRange = $derived(visibleRowRange(gridOffsets, viewTop, viewHeight, GRID_OVERSCAN));

  // Anything that moves the grid without scrolling or resizing it (the search
  // help opening, the expiry banner) needs a fresh measurement.
  $effect(() => {
    void showSearchHelp;
    void gallery.filtersCollapsed;
    void boardRename;
    void gallery.hasExpiry;
    void filtersActive;
    void gallery.viewMode;
    void gridRows.length;
    void tick().then(scheduleMeasure);
  });

  $effect(() => () => {
    if (measureFrame) cancelAnimationFrame(measureFrame);
  });

  // ---------------------------------------------------------------------------
  // Multi-select and bulk actions
  // ---------------------------------------------------------------------------

  // Board names are trimmed, so a leading space can never be a real board.
  const NEW_BOARD_OPTION = " new";
  const selection = new SvelteSet<OutputImage>();
  /** Where a Shift+click range starts: the last image selected on its own. */
  let selectionAnchor: OutputImage | null = null;
  /** Text of the bulk bar's new board field; null while it shows the board list. */
  let bulkNewBoard = $state<string | null>(null);

  /** Every shown image in on-screen order, groups included. */
  const orderedImages = $derived(groupedGalleryImages.flatMap((group) => group.images));

  function selectedImages(): OutputImage[] {
    return [...selection];
  }

  function setSelected(images: OutputImage[], selected: boolean) {
    for (const image of images) {
      if (selected) selection.add(image);
      else selection.delete(image);
    }
  }

  function toggleSelected(image: OutputImage) {
    setSelected([image], !selection.has(image));
    selectionAnchor = image;
  }

  function selectRangeTo(image: OutputImage) {
    setSelected(selectionRange(orderedImages, selectionAnchor, image), true);
    if (!selectionAnchor || !orderedImages.includes(selectionAnchor)) selectionAnchor = image;
  }

  function selectAll() {
    setSelected(orderedImages, true);
  }

  function clearSelection() {
    selection.clear();
    selectionAnchor = null;
    bulkNewBoard = null;
  }

  /** A tile's image: Shift+click selects a range, Ctrl/Cmd+click one image, a plain click opens it. */
  function onTileClick(e: MouseEvent, image: OutputImage) {
    if (e.shiftKey) {
      e.preventDefault();
      selectRangeTo(image);
    } else if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      toggleSelected(image);
    } else {
      void gallery.openLightbox(image);
    }
  }

  function onSelectBoxClick(e: MouseEvent, image: OutputImage) {
    e.stopPropagation();
    if (e.shiftKey) selectRangeTo(image);
    else toggleSelected(image);
  }

  const allSelectedFavourite = $derived(selection.size > 0 && selectedImages().every((image) => gallery.isFavourite(image)));
  const allSelectedNsfw = $derived(selection.size > 0 && selectedImages().every((image) => gallery.isNsfw(image)));
  /** The two selected stills, when exactly two are selected. */
  const comparePair = $derived.by((): [OutputImage, OutputImage] | null => {
    if (selection.size !== 2) return null;
    const [a, b] = selectedImages();
    return a && b && !isVideoImage(a) && !isVideoImage(b) ? [a, b] : null;
  });

  function bulkMove(board: string) {
    const images = selectedImages();
    gallery.setBoardMany(images, board);
    gallery.showToast(
      locale.t("gallery.bulk.toast_moved", {
        count: String(images.length),
        board: board === "Unsorted" ? locale.t("gallery.unsorted") : board,
      }),
      "success",
    );
  }

  function bulkBoardChosen(value: string) {
    if (value === NEW_BOARD_OPTION) bulkNewBoard = "";
    else if (value) bulkMove(value);
  }

  function bulkMoveToNewBoard() {
    const name = bulkNewBoard?.trim();
    if (!name) return;
    bulkMove(name);
    bulkNewBoard = null;
  }

  /** Deleting more images than this at once asks first, Undo or not. */
  const BULK_DELETE_CONFIRM = 10;

  /**
   * Delete with an Undo toast instead of a confirm. Originals in a linked
   * folder still ask first: those are the user's own files, not copies.
   */
  function deleteWithUndo(images: OutputImage[]) {
    if (images.length === 0) return;
    const linked = images.filter((image) => image.linked_folder);
    if (linked.length > 0) {
      const message = images.length === 1
        ? locale.t("gallery.delete_linked_confirm", { folder: linked[0]!.linked_folder! })
        : locale.t("gallery.bulk.delete_confirm_linked", { count: String(images.length), linked: String(linked.length) });
      if (!confirm(message)) return;
    } else if (images.length > BULK_DELETE_CONFIRM) {
      if (!confirm(locale.t("gallery.bulk.delete_confirm", { count: String(images.length) }))) return;
    }
    gallery.deleteImagesWithUndo(images);
  }

  function bulkDelete() {
    deleteWithUndo(selectedImages());
  }

  // ---------------------------------------------------------------------------
  // Board rename and delete
  // ---------------------------------------------------------------------------

  /** Text of the rename field; null while the board filter shows its list. */
  let boardRename = $state<string | null>(null);
  const boardCounts = $derived(gallery.boardCounts);
  const boardIsEditable = $derived(gallery.boardFilter !== "all" && gallery.boardFilter !== "Unsorted");

  /** More boards than this and the chip row shows the busiest few plus a searchable list. */
  const BOARD_CHIP_LIMIT = 10;
  let boardPickerOpen = $state(false);
  let boardPickerQuery = $state("");
  const chipBoards = $derived.by(() => {
    const boards = gallery.boards;
    if (boards.length <= BOARD_CHIP_LIMIT) return boards;
    const busiest = [...boards].sort((a, b) => (boardCounts[b] ?? 0) - (boardCounts[a] ?? 0)).slice(0, BOARD_CHIP_LIMIT - 2);
    const shown = new Set(busiest);
    if (boardIsEditable) shown.add(gallery.boardFilter);
    return boards.filter((board) => shown.has(board));
  });
  const boardPickerMatches = $derived.by(() => {
    const query = boardPickerQuery.trim().toLowerCase();
    return query ? gallery.boards.filter((board) => board.toLowerCase().includes(query)) : gallery.boards;
  });

  function pickBoard(board: string) {
    gallery.boardFilter = board;
    boardPickerOpen = false;
    boardPickerQuery = "";
  }

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  /** Closes the board list on a click anywhere outside it. */
  function boardPickerDismiss(node: HTMLElement) {
    const onDown = (e: PointerEvent) => {
      if (!node.contains(e.target as Node)) boardPickerOpen = false;
    };
    document.addEventListener("pointerdown", onDown, true);
    return { destroy: () => document.removeEventListener("pointerdown", onDown, true) };
  }

  function commitBoardRename() {
    if (boardRename !== null) gallery.renameBoard(gallery.boardFilter, boardRename);
    boardRename = null;
  }

  function deleteCurrentBoard() {
    const board = gallery.boardFilter;
    if ((boardCounts[board] ?? 0) > 0 && !confirm(locale.t("gallery.board_delete_confirm", { board }))) return;
    gallery.deleteBoard(board);
  }

  // A selection only ever holds images that are on the page: one that is
  // filtered out or deleted elsewhere leaves it, so a bulk delete can never
  // take an image the user cannot see.
  $effect(() => {
    const shown = orderedImages;
    if (selection.size === 0) return;
    const visible = new Set(shown);
    for (const image of selection) {
      if (!visible.has(image)) selection.delete(image);
    }
  });

  // ---------------------------------------------------------------------------
  // Repeated generations
  // ---------------------------------------------------------------------------

  /** In every set of repeated generations, select all but the oldest copy. */
  function selectDuplicateExtras() {
    selection.clear();
    for (const group of groupedGalleryImages) setSelected(group.images.slice(1), true);
  }

  // ---------------------------------------------------------------------------
  // Export
  // ---------------------------------------------------------------------------

  let exportProgress = $state<{ done: number; total: number } | null>(null);

  async function bulkExport() {
    if (exportProgress) return;
    const images = selectedImages();
    if (images.length === 0) return;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const dir = await open({ directory: true, multiple: false, title: locale.t("gallery.bulk.export_title") });
      if (typeof dir !== "string" || !dir) return;
      exportProgress = { done: 0, total: images.length };
      const saved = await gallery.exportImagesToDir(images, dir, (done, total) => {
        exportProgress = { done, total };
      });
      const failed = images.length - saved;
      gallery.showToast(
        failed > 0
          ? locale.t("gallery.bulk.toast_export_failed", { count: String(failed) })
          : locale.t("gallery.bulk.toast_exported", { count: String(saved) }),
        failed > 0 ? "error" : "success",
      );
    } catch (e) {
      console.error("Failed to export images:", e);
      gallery.showToast(locale.t("gallery.toast.failed_save"), "error");
    } finally {
      exportProgress = null;
    }
  }

  // ---------------------------------------------------------------------------
  // Drag onto a board chip
  // ---------------------------------------------------------------------------

  const DRAG_TYPE = "application/x-mooshieui-gallery";
  /** The images being dragged: the selection when the dragged tile is part of it, else that tile. */
  let dragImages: OutputImage[] = [];
  let dragActive = $state(false);
  let dragOverBoard = $state<string | null>(null);

  function onTileDragStart(e: DragEvent, image: OutputImage) {
    dragImages = selection.has(image) ? selectedImages() : [image];
    dragActive = true;
    e.dataTransfer?.setData(DRAG_TYPE, String(dragImages.length));
  }

  function onTileDragEnd() {
    dragActive = false;
    dragOverBoard = null;
    dragImages = [];
  }

  function onChipDragOver(e: DragEvent, board: string) {
    if (!dragActive) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dragOverBoard = board;
  }

  function onChipDrop(e: DragEvent, board: string) {
    if (!dragActive || dragImages.length === 0) return;
    e.preventDefault();
    const images = dragImages;
    onTileDragEnd();
    gallery.setBoardMany(images, board);
    gallery.showToast(
      locale.t("gallery.bulk.toast_moved", {
        count: String(images.length),
        board: board === "Unsorted" ? locale.t("gallery.unsorted") : board,
      }),
      "success",
    );
  }

  // ---------------------------------------------------------------------------
  // Keyboard cursor
  // ---------------------------------------------------------------------------

  /** The image the arrow keys are on. Shown as a ring; Space selects it, Enter opens it. */
  let cursor = $state<OutputImage | null>(null);

  /** Scroll the grid so the row holding `image` is fully on screen. */
  function revealInGrid(image: OutputImage) {
    if (!scroller || !listEl) return;
    const row = gridRows.findIndex((r) => r.kind === "tiles" && r.images.includes(image));
    if (row === -1) return;
    measureGrid();
    const top = gridOffsets[row]!;
    const bottom = gridOffsets[row + 1]!;
    const listTop = scroller.scrollTop - viewTop;
    // The bulk bar covers the bottom of the viewport while a selection is active.
    const covered = selection.size > 0 ? 72 : 8;
    if (top < viewTop + 8) scroller.scrollTop = listTop + top - 8;
    else if (bottom > viewTop + viewHeight - covered) scroller.scrollTop = listTop + bottom - viewHeight + covered;
  }

  function moveCursor(delta: number, extend: boolean) {
    const order = orderedImages;
    if (order.length === 0) return;
    let index = cursor ? order.indexOf(cursor) : -1;
    if (index === -1) {
      // Start on the first tile that is on screen.
      const first = gridRows.slice(gridRange.start, gridRange.end).find(
        (row, i) => row.kind === "tiles" && gridOffsets[gridRange.start + i]! >= viewTop,
      );
      index = Math.max(0, order.indexOf(first?.images[0] ?? order[0]!));
    } else {
      index = Math.max(0, Math.min(order.length - 1, index + delta));
    }
    const next = order[index]!;
    if (extend) {
      if (!selectionAnchor) selectionAnchor = cursor ?? next;
      selectRangeTo(next);
    }
    cursor = next;
    if (gallery.viewMode === "details") {
      // The detailed list grows as it scrolls; make sure the row exists first.
      if (index >= galleryRenderLimit) galleryRenderLimit = index + 48;
      void tick().then(() => document.querySelector("[data-gallery-cursor]")?.scrollIntoView({ block: "nearest" }));
    } else {
      revealInGrid(next);
    }
  }

  function isTypingTarget(target: EventTarget | null): boolean {
    return target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName));
  }

  /** Ctrl/Cmd+A selects everything shown, Escape clears, Delete deletes the selection. */
  function onGalleryKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || gallery.lightboxOpen || gallery.compareOpen || dirPickerImage || exportProgress) return;
    if (isTypingTarget(e.target)) return;
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "a") {
      if (orderedImages.length === 0) return;
      e.preventDefault();
      selectAll();
    } else if ((selection.size > 0 || cursor) && e.key === "Escape") {
      e.preventDefault();
      clearSelection();
      cursor = null;
    } else if (selection.size > 0 && e.key === "Delete") {
      e.preventDefault();
      bulkDelete();
    } else if (!e.ctrlKey && !e.metaKey && !e.altKey && e.key.startsWith("Arrow")) {
      const perRow = gallery.viewMode === "details" ? 1 : gridColumns;
      const delta = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : e.key === "ArrowUp" ? -perRow : perRow;
      e.preventDefault();
      moveCursor(delta, e.shiftKey);
    } else if (cursor && orderedImages.includes(cursor) && !(e.target instanceof HTMLButtonElement)) {
      // A focused button keeps Space and Enter for itself.
      if (e.key === " ") {
        e.preventDefault();
        toggleSelected(cursor);
      } else if (e.key === "Enter") {
        e.preventDefault();
        void gallery.openLightbox(cursor);
      }
    }
  }

  if (typeof window !== "undefined") gallery.loadViewPrefs();

  $effect(() => {
    void gallery.sortBy;
    void gallery.sortDir;
    void gallery.groupBy;
    void gallery.boardFilter;
    void gallery.searchQuery;
    void gallery.sourceFilter;
    void gallery.favouritesOnly;
    void gallery.minRating;
    void gallery.nsfwFilter;
    void gallery.characterFilter;
    void gallery.duplicatesOnly;
    galleryRenderLimit = 48;
  });

  $effect(() => {
    void gallery.imagesPerRow;
    void gallery.sortBy;
    void gallery.sortDir;
    void gallery.groupBy;
    void gallery.boardFilter;
    void gallery.viewMode;
    void gallery.favouritesOnly;
    void gallery.minRating;
    void gallery.nsfwFilter;
    void gallery.blurNsfw;
    void gallery.sourceFilter;
    void gallery.filtersCollapsed;
    void gallery.characterFilter;
    gallery.saveViewPrefs();
  });

  $effect(() => () => {
    if (searchTimer) clearTimeout(searchTimer);
  });
</script>

<svelte:window onkeydown={onGalleryKeydown} />

<div bind:this={scroller} onscroll={scheduleMeasure} class="p-3 md:p-6 h-full flex flex-col overflow-y-auto will-change-scroll">
  <div class="flex shrink-0 items-center justify-between gap-3 mb-3">
    <h2 class="text-base font-semibold text-neutral-200">{locale.t("nav.gallery")}</h2>
    <button
      onclick={() => gallery.refresh()}
      disabled={gallery.loading || gallery.refreshing}
      title={locale.t("gallery.refresh")}
      aria-label={locale.t("gallery.refresh")}
      aria-busy={gallery.refreshing}
      class="w-11 h-11 shrink-0 flex items-center justify-center rounded border border-neutral-700 text-neutral-300 hover:border-neutral-500 hover:text-white disabled:opacity-50 disabled:cursor-wait transition-colors"
    >
      <RefreshCw size={18} class={gallery.refreshing ? "animate-spin motion-reduce:animate-none" : ""} />
    </button>
  </div>
  {#if gallery.loading && gallery.images.length === 0}
    <div class="flex flex-1 items-center justify-center text-neutral-500">{locale.t("gallery.loading")}</div>
  {:else if gallery.images.length === 0}
    <div class="flex flex-1 items-center justify-center text-neutral-500">{locale.t("gallery.empty_generate")}</div>
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
        <div>
          <div class="flex items-center gap-2">
            <div class="relative flex-1">
              <input
                type="text"
                value={searchInput}
                oninput={(e) => onSearchInput((e.target as HTMLInputElement).value)}
                onkeydown={(e) => { if (e.key === "Escape" && searchInput) { e.stopPropagation(); clearSearch(); } }}
                placeholder={locale.t("gallery.search.placeholder")}
                aria-label={locale.t("gallery.search.label")}
                class="w-full bg-neutral-800 border border-neutral-700 rounded-lg pl-3 pr-9 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
              />
              {#if searchInput}
                <button
                  class="absolute right-1 top-1/2 -translate-y-1/2 w-7 h-7 flex items-center justify-center rounded text-neutral-400 hover:text-neutral-100"
                  onclick={clearSearch}
                  title={locale.t("gallery.search.clear")}
                  aria-label={locale.t("gallery.search.clear")}
                >&times;</button>
              {/if}
            </div>
            <button
              class="w-9 h-9 shrink-0 flex items-center justify-center rounded-lg border text-sm transition-colors {showSearchHelp ? 'border-indigo-500 text-indigo-300' : 'border-neutral-700 text-neutral-400 hover:border-neutral-500'}"
              onclick={() => (showSearchHelp = !showSearchHelp)}
              aria-expanded={showSearchHelp}
              title={locale.t("gallery.search.help_title")}
              aria-label={locale.t("gallery.search.help_title")}
            >?</button>
            <button
              class="relative h-9 shrink-0 flex items-center gap-1.5 px-3 rounded-lg border text-xs transition-colors {gallery.filtersCollapsed ? 'border-neutral-700 text-neutral-400 hover:border-neutral-500' : 'border-indigo-500 text-indigo-300'}"
              onclick={() => (gallery.filtersCollapsed = !gallery.filtersCollapsed)}
              aria-expanded={!gallery.filtersCollapsed}
            >
              <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M4 6h16M7 12h10M10 18h4"/></svg>
              {locale.t("gallery.filters")}
              {#if gallery.filtersCollapsed && panelFiltersActive}
                <span class="absolute -top-1 -right-1 w-2.5 h-2.5 rounded-full bg-indigo-400" aria-hidden="true"></span>
              {/if}
            </button>
          </div>
          {#if showSearchHelp}
            <ul class="mt-2 space-y-0.5 text-[11px] text-neutral-400 leading-relaxed">
              <li>{locale.t("gallery.search.help_words")}</li>
              <li>{locale.t("gallery.search.help_phrase")}</li>
              <li>{locale.t("gallery.search.help_exclude")}</li>
              <li>{locale.t("gallery.search.help_fields")}</li>
              <li>{locale.t("gallery.search.help_numbers")}</li>
              <li>{locale.t("gallery.search.help_tags")}</li>
            </ul>
          {/if}
          {#if filtersActive || (gallery.hydratingMetadata && gallery.searchQuery.trim())}
            <div class="mt-1.5 flex flex-wrap gap-x-3 text-[11px] text-neutral-500">
              {#if filtersActive}
                <span>{locale.t("gallery.search.result_count", { shown: String(galleryTotalCount), total: String(gallery.images.length) })}</span>
              {/if}
              {#if gallery.hydratingMetadata && gallery.searchQuery.trim()}
                <span class="text-amber-400/80">{locale.t("gallery.search.reading_metadata")}</span>
              {/if}
            </div>
          {/if}
        </div>

        {#if !gallery.filtersCollapsed}
        <div class="grid grid-cols-1 lg:grid-cols-4 gap-3 items-end">
          <div class="lg:col-span-2">
            <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.images_per_row")} {viewColumns(gallery.viewMode)}</div>
            <input type="range" bind:value={gallery.imagesPerRow} min="2" max="8" step="1" class="w-full accent-indigo-500" disabled={gallery.viewMode === "details"} />
          </div>
          <div>
            <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.sort_by")}</div>
            <select bind:value={gallery.sortBy} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
              <option value="date">{locale.t("gallery.sort_date")}</option>
              <option value="name">{locale.t("gallery.sort_name")}</option>
              <option value="size">{locale.t("gallery.sort_size")}</option>
              <option value="rating">{locale.t("gallery.sort_rating")}</option>
            </select>
          </div>
          <div>
            <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.group_by")}</div>
            <select bind:value={gallery.groupBy} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
              <option value="none">{locale.t("gallery.group_none")}</option>
              <option value="date">{locale.t("gallery.group_date")}</option>
              <option value="month">{locale.t("gallery.group_month")}</option>
              <option value="mode">{locale.t("gallery.group_mode")}</option>
              <option value="prompt">{locale.t("gallery.group_prompt")}</option>
              <option value="board">{locale.t("gallery.group_board")}</option>
              {#if knownCharacters.length > 0}
                <option value="character">{locale.t("gallery.character_filter")}</option>
              {/if}
            </select>
          </div>
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-4 gap-3 items-end">
          <div>
            <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.board_filter")}</div>
            <div class="flex items-center gap-1.5">
              {#if boardRename !== null}
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  type="text"
                  autofocus
                  bind:value={boardRename}
                  aria-label={locale.t("gallery.board_rename")}
                  class="min-w-0 flex-1 bg-neutral-800 border border-indigo-500 rounded-lg px-2 py-2 text-sm text-neutral-100 focus:outline-none"
                  onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); commitBoardRename(); } else if (e.key === "Escape") { e.stopPropagation(); boardRename = null; } }}
                />
                <button class="w-9 h-9 shrink-0 flex items-center justify-center rounded-lg border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 disabled:opacity-50" disabled={!boardRename.trim()} title={locale.t("common.save")} aria-label={locale.t("common.save")} onclick={commitBoardRename}>✓</button>
                <button class="w-9 h-9 shrink-0 flex items-center justify-center rounded-lg border border-neutral-700 text-neutral-400 hover:border-neutral-500" title={locale.t("common.cancel")} aria-label={locale.t("common.cancel")} onclick={() => (boardRename = null)}>&times;</button>
              {:else}
                <select bind:value={gallery.boardFilter} class="min-w-0 flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
                  <option value="all">{locale.t("gallery.all_boards")}</option>
                  <option value="Unsorted">{locale.t("gallery.unsorted")} ({boardCounts["Unsorted"] ?? 0})</option>
                  {#each gallery.boards as board}
                    <option value={board}>{board} ({boardCounts[board] ?? 0})</option>
                  {/each}
                </select>
                {#if boardIsEditable}
                  <button class="w-9 h-9 shrink-0 flex items-center justify-center rounded-lg border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300" title={locale.t("gallery.board_rename")} aria-label={locale.t("gallery.board_rename")} onclick={() => (boardRename = gallery.boardFilter)}>✎</button>
                  <button class="w-9 h-9 shrink-0 flex items-center justify-center rounded-lg border border-neutral-700 text-red-300 hover:border-red-500" title={locale.t("gallery.board_delete")} aria-label={locale.t("gallery.board_delete")} onclick={deleteCurrentBoard}>&times;</button>
                {/if}
              {/if}
            </div>
          </div>
          <div class="lg:col-span-3">
            <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.create_board")}</div>
            <div class="flex items-center gap-2">
              <input type="text" bind:value={newBoardName} class="flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-100 placeholder-neutral-500" placeholder={locale.t("gallery.placeholder_board")} />
              <button class="px-3 py-2 text-xs rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors" onclick={addBoard} disabled={!newBoardName.trim()}>{locale.t("gallery.add")}</button>
            </div>
          </div>
        </div>

        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3 items-end">
          <div>
            <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.rating_filter")}</div>
            <select bind:value={gallery.minRating} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
              <option value={0}>{locale.t("gallery.rating_any")}</option>
              {#each Array.from({ length: GALLERY_MAX_RATING }, (_, i) => i + 1) as stars}
                <option value={stars}>{locale.t("gallery.rating_at_least", { stars: "★".repeat(stars) })}</option>
              {/each}
            </select>
          </div>
          <div>
            <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.nsfw_filter")}</div>
            <select bind:value={gallery.nsfwFilter} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
              <option value="all">{locale.t("gallery.nsfw_show_all")}</option>
              <option value="hide">{locale.t("gallery.nsfw_hide")}</option>
              <option value="only">{locale.t("gallery.nsfw_only")}</option>
            </select>
          </div>
          {#if knownCharacters.length > 0}
            <div>
              <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.character_filter")}</div>
              <select bind:value={gallery.characterFilter} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
                <option value="all">{locale.t("gallery.character_all")}</option>
                {#each knownCharacters as name}
                  <option value={name}>{name} ({characterCounts[name] ?? 0})</option>
                {/each}
              </select>
            </div>
          {/if}
          {#if gallery.linkedFolders.length > 0}
            <div>
              <div class="text-xs text-neutral-400 mb-1">{locale.t("gallery.source_filter")}</div>
              <select bind:value={gallery.sourceFilter} class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-2 text-sm text-neutral-200">
                <option value="all">{locale.t("gallery.source_all")}</option>
                <option value={GALLERY_SOURCE_OWN}>{locale.t("gallery.source_own")}</option>
                {#each gallery.linkedFolders as folder}
                  <option value={folder} title={folder}>{folderLabel(folder)}</option>
                {/each}
              </select>
            </div>
          {/if}
          <div class="flex flex-wrap items-center gap-2 {gallery.linkedFolders.length > 0 ? '' : 'lg:col-span-2'}">
            <label class="flex items-center gap-1.5 px-3 py-2 text-xs rounded border border-neutral-700 text-neutral-300 cursor-pointer hover:border-neutral-500">
              <input type="checkbox" class="accent-indigo-500" bind:checked={gallery.favouritesOnly} />
              {locale.t("gallery.favourites_only")}
            </label>
            <label class="flex items-center gap-1.5 px-3 py-2 text-xs rounded border border-neutral-700 text-neutral-300 cursor-pointer hover:border-neutral-500" title={locale.t("gallery.duplicates_only_tooltip")}>
              <input type="checkbox" class="accent-indigo-500" bind:checked={gallery.duplicatesOnly} />
              {locale.t("gallery.duplicates_only")}
            </label>
            {#if gallery.duplicatesOnly && galleryTotalCount > 0}
              <button class="px-3 py-2 text-xs rounded border border-indigo-700/70 text-indigo-300 hover:border-indigo-500 hover:text-indigo-200" title={locale.t("gallery.duplicates_select_extras_tooltip")} onclick={selectDuplicateExtras}>{locale.t("gallery.duplicates_select_extras")}</button>
            {/if}
            <label class="flex items-center gap-1.5 px-3 py-2 text-xs rounded border border-neutral-700 text-neutral-300 cursor-pointer hover:border-neutral-500" title={locale.t("gallery.blur_nsfw_tooltip")}>
              <input type="checkbox" class="accent-indigo-500" bind:checked={gallery.blurNsfw} />
              {locale.t("gallery.blur_nsfw")}
            </label>
          </div>
        </div>

        <div>
          <div class="text-xs text-neutral-400 mb-2">{locale.t("gallery.view")}</div>
          <div class="flex flex-wrap gap-2">
            <button onclick={() => (gallery.sortDir = gallery.sortDir === "asc" ? "desc" : "asc")} class="px-3 py-1.5 text-xs rounded border transition-colors border-neutral-700 text-neutral-300 hover:border-neutral-500">{gallery.sortDir === "asc" ? locale.t("gallery.ascending") : locale.t("gallery.descending")}</button>
            <button onclick={() => (gallery.viewMode = "huge")} class="px-3 py-1.5 text-xs rounded border transition-colors {gallery.viewMode === 'huge' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.huge_icons")}</button>
            <button onclick={() => (gallery.viewMode = "large")} class="px-3 py-1.5 text-xs rounded border transition-colors {gallery.viewMode === 'large' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.large_icons")}</button>
            <button onclick={() => (gallery.viewMode = "small")} class="px-3 py-1.5 text-xs rounded border transition-colors {gallery.viewMode === 'small' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.small_icons")}</button>
            <button onclick={() => (gallery.viewMode = "details")} class="px-3 py-1.5 text-xs rounded border transition-colors {gallery.viewMode === 'details' ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}">{locale.t("gallery.detailed_view")}</button>
            <button onclick={rescanGalleryMetadata} disabled={gallery.loading || gallery.refreshing} class="px-3 py-1.5 text-xs rounded border transition-colors border-amber-700/70 text-amber-300 hover:border-amber-500 hover:text-amber-200 disabled:opacity-50">{locale.t("gallery.rescan_metadata")}</button>
            <button onclick={sortGalleryByArtist} disabled={gallery.autoSorting} class="px-3 py-1.5 text-xs rounded border transition-colors border-indigo-700/70 text-indigo-300 hover:border-indigo-500 hover:text-indigo-200 disabled:opacity-50">{gallery.autoSorting ? locale.t("gallery.sort_by_artist_running") : locale.t("gallery.sort_by_artist")}</button>
            {#if artistBoards.length > 0}
              <button onclick={() => removeSortedBoards(artistBoards)} title={locale.t("gallery.sort_remove_tooltip")} class="px-3 py-1.5 text-xs rounded border transition-colors border-neutral-700 text-neutral-300 hover:border-red-500 hover:text-red-300">{locale.t("gallery.sort_by_artist_remove", { boards: String(artistBoards.length) })}</button>
            {/if}
            {#if knownCharacters.length > 0}
              <button onclick={sortShownByCharacter} title={locale.t("gallery.sort_by_character_tooltip")} class="px-3 py-1.5 text-xs rounded border transition-colors border-indigo-700/70 text-indigo-300 hover:border-indigo-500 hover:text-indigo-200">{locale.t("gallery.sort_by_character")}</button>
              {#if characterBoards.length > 0}
                <button onclick={() => removeSortedBoards(characterBoards)} title={locale.t("gallery.sort_remove_tooltip")} class="px-3 py-1.5 text-xs rounded border transition-colors border-neutral-700 text-neutral-300 hover:border-red-500 hover:text-red-300">{locale.t("gallery.sort_by_character_remove", { boards: String(characterBoards.length) })}</button>
              {/if}
              <button onclick={scanForCharacters} title={locale.t("gallery.character_scan_tooltip")} class="px-3 py-1.5 text-xs rounded border transition-colors {gallery.tagScan ? 'border-amber-600 text-amber-300 hover:border-amber-400' : 'border-indigo-700/70 text-indigo-300 hover:border-indigo-500 hover:text-indigo-200'}">{gallery.tagScan ? locale.t("gallery.character_scan_stop", { done: String(gallery.tagScan.done), total: String(gallery.tagScan.total) }) : locale.t("gallery.character_scan")}</button>
            {/if}
            <label class="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded border border-neutral-700 text-neutral-300 cursor-pointer hover:border-neutral-500">
              <input
                type="checkbox"
                class="accent-indigo-500"
                checked={gallery.showGenerationTime}
                onchange={(e) => gallery.setShowGenerationTime((e.target as HTMLInputElement).checked)}
              />
              {locale.t("gallery.show_generation_time")}
            </label>
          </div>
        </div>
        {/if}
      </div>

      <div class="flex flex-wrap items-center gap-1.5" role="group" aria-label={locale.t("gallery.board_filter")}>
        {@render boardChip("all", locale.t("gallery.all_boards"), gallery.images.length)}
        {@render boardChip("Unsorted", locale.t("gallery.unsorted"), boardCounts["Unsorted"] ?? 0)}
        {#each chipBoards as board (board)}
          {@render boardChip(board, board, boardCounts[board] ?? 0)}
        {/each}
        {#if chipBoards.length < gallery.boards.length}
          <div class="relative" use:boardPickerDismiss>
            <button
              type="button"
              aria-expanded={boardPickerOpen}
              onclick={() => { boardPickerOpen = !boardPickerOpen; boardPickerQuery = ""; }}
              class="flex items-center gap-1.5 px-2.5 py-1 text-xs rounded-full border transition-colors {boardPickerOpen ? 'border-indigo-500 text-indigo-200' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}"
            >
              <span>{locale.t("gallery.all_board_list")}</span>
              <span class="text-[10px] text-neutral-500 tabular-nums">{gallery.boards.length}</span>
              <svg class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
            </button>
            {#if boardPickerOpen}
              <div class="absolute left-0 top-full z-30 mt-1 w-72 rounded-lg border border-neutral-700 bg-neutral-900 shadow-xl">
                <input
                  use:focusOnMount
                  bind:value={boardPickerQuery}
                  placeholder={locale.t("gallery.board_search")}
                  aria-label={locale.t("gallery.board_search")}
                  onkeydown={(e) => {
                    if (e.key === "Escape") { e.preventDefault(); boardPickerOpen = false; }
                    else if (e.key === "Enter" && boardPickerMatches[0]) { e.preventDefault(); pickBoard(boardPickerMatches[0]); }
                  }}
                  class="w-full bg-transparent border-b border-neutral-700 px-3 py-2 text-sm text-neutral-200 placeholder-neutral-500 outline-none"
                />
                <div class="max-h-72 overflow-y-auto py-1">
                  {#each boardPickerMatches as board (board)}
                    <button
                      type="button"
                      onclick={() => pickBoard(board)}
                      class="w-full flex items-center justify-between gap-3 px-3 py-1.5 text-left text-xs transition-colors {gallery.boardFilter === board ? 'bg-indigo-500/15 text-indigo-200' : 'text-neutral-300 hover:bg-neutral-800'}"
                    >
                      <span class="truncate">{board}</span>
                      <span class="text-[10px] text-neutral-500 tabular-nums">{boardCounts[board] ?? 0}</span>
                    </button>
                  {:else}
                    <div class="px-3 py-2 text-xs text-neutral-500">{locale.t("gallery.board_search_none")}</div>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {/if}
        {#if filtersActive}
          <button
            type="button"
            onclick={() => gallery.resetFilters()}
            class="ml-auto flex items-center gap-1.5 px-2.5 py-1 text-xs rounded-full border border-amber-700/70 text-amber-300 transition-colors hover:border-amber-500 hover:text-amber-200"
          >
            <svg class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
            {locale.t("gallery.reset_filters")}
          </button>
        {/if}
      </div>

      {#if galleryTotalCount === 0}
        <div class="py-12 text-center text-sm text-neutral-500">{locale.t("gallery.no_matches")}</div>
      {/if}

      {#if gallery.viewMode === "details"}
        {#each galleryGroupsVisible as group}
          <section class="space-y-2">
            {#if showGroupHeaders}
              <div class="flex items-center gap-2">
                {@render groupBox(group.images)}
                <h3 class="text-sm text-neutral-300 font-medium">{group.label}</h3>
              </div>
            {/if}
            <div class="rounded-xl border border-neutral-800 overflow-hidden">
              <div class="grid grid-cols-[28px_72px_1fr_150px_120px_420px] gap-2 px-3 py-2 bg-neutral-900 text-[11px] uppercase tracking-wide text-neutral-500 border-b border-neutral-800">
                <div></div><div>{locale.t("gallery.col_preview")}</div><div>{locale.t("gallery.col_name")}</div><div>{locale.t("gallery.col_date")}</div><div>{locale.t("gallery.col_size")}</div><div>{locale.t("gallery.col_actions")}</div>
              </div>
              {#each group.images as image (image)}
                <div class="grid grid-cols-[28px_72px_1fr_150px_120px_420px] gap-2 px-3 py-2 items-center border-b border-neutral-900/80 last:border-b-0 [content-visibility:auto] [contain-intrinsic-size:auto_76px] {selection.has(image) ? 'bg-indigo-500/10' : ''} {cursor === image ? 'outline-2 -outline-offset-2 outline-white' : ''}" data-gallery-cursor={cursor === image ? "" : undefined}>
                  {@render selectBox(image, true)}
                  <button class="w-14 h-14 rounded border border-neutral-800 overflow-hidden" onclick={(e) => onTileClick(e, image)}>
                    <img use:lazyThumbnail={{ image, size: thumbSize }} alt={image.filename} loading="lazy" decoding="async" class="w-full h-full object-cover {gallery.shouldBlur(image) ? 'blur-md scale-110' : ''}" />
                  </button>
                  <div class="min-w-0">
                    <div class="text-sm text-neutral-200 truncate" title={image.filename}>{image.filename}</div>
                    {#if image.linked_folder}
                      <div class="text-[10px] text-neutral-500 truncate" title={image.linked_folder}>{image.linked_folder}</div>
                    {/if}
                    {#if knownCharacters.length > 0 && gallery.charactersIn(image).length > 0}
                      <div class="text-[10px] text-indigo-300/80 truncate">{gallery.charactersIn(image).join(", ")}</div>
                    {/if}
                  </div>
                  <div class="text-xs text-neutral-400">
                    {formatDate(image.generated_at_ms)}
                    {#if gallery.showGenerationTime && image.generationTimeMs != null}
                      <span class="text-amber-300/80">· {formatGenerationTime(image.generationTimeMs, locale.current)}</span>
                    {/if}
                  </div>
                  <div class="text-xs text-neutral-400">{formatBytes(image.file_size_bytes)}</div>
                  <div class="flex flex-wrap gap-1">
                    <button
                      class="px-2 py-1 text-[11px] rounded {gallery.isFavourite(image) ? 'bg-pink-700/80 hover:bg-pink-600 text-white' : 'bg-neutral-800 hover:bg-neutral-700 text-neutral-300'}"
                      aria-pressed={gallery.isFavourite(image)}
                      title={locale.t(gallery.isFavourite(image) ? "gallery.unfavourite" : "gallery.favourite")}
                      onclick={() => gallery.toggleFavourite(image)}
                    >♥</button>
                    <select
                      class="px-2 py-1 text-[11px] rounded bg-neutral-800 border border-neutral-700 text-amber-300"
                      value={gallery.getRating(image)}
                      title={locale.t("gallery.rating")}
                      aria-label={locale.t("gallery.rating")}
                      onchange={(e) => gallery.setRating(image, Number((e.target as HTMLSelectElement).value))}
                    >
                      <option value={0}>{locale.t("gallery.rating_none")}</option>
                      {#each Array.from({ length: GALLERY_MAX_RATING }, (_, i) => i + 1) as stars}
                        <option value={stars}>{"★".repeat(stars)}</option>
                      {/each}
                    </select>
                    <button
                      class="px-2 py-1 text-[11px] rounded {gallery.isNsfw(image) ? 'bg-red-900/80 hover:bg-red-800 text-red-200' : 'bg-neutral-800 hover:bg-neutral-700 text-neutral-300'}"
                      aria-pressed={gallery.isNsfw(image)}
                      title={locale.t(gallery.isNsfw(image) ? "gallery.mark_safe" : "gallery.mark_nsfw")}
                      onclick={() => gallery.toggleNsfw(image)}
                    >{locale.t("gallery.nsfw_badge")}</button>
                    <select class="px-2 py-1 text-[11px] rounded bg-neutral-800 border border-neutral-700 text-neutral-200" value={boardLabel(image)} onchange={(e) => assignBoard(image, (e.target as HTMLSelectElement).value)}>
                      <option value="Unsorted">{locale.t("gallery.unsorted")}</option>
                      {#each gallery.boards as board}
                        <option value={board}>{board}</option>
                      {/each}
                    </select>
                    {#if generation.manualSaveMode && !image.gallery_filename}
                      <button class="px-2 py-1 text-[11px] rounded bg-indigo-700 hover:bg-indigo-600 text-neutral-100" onclick={() => saveToDir(image)}>{locale.t("gallery.save_to_folder")}</button>
                    {/if}
                    {#if !isVideoImage(image)}
                      <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => img2imgImage(image)}>{locale.t("gallery.i2i")}</button>
                      <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => inpaintImage(image)}>{locale.t("gallery.inpaint")}</button>
                      {#if !image.is_upscaled}
                        <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => upscaleImage(image)}>{locale.t("gallery.upscale")}</button>
                      {/if}
                      {#if directorToolsAvailable()}
                        <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" onclick={() => directorTools.open(image, image.thumbnailUrl || image.url || null)}>{locale.t("novelai.director.action")}</button>
                      {/if}
                      {#if onMake3D}
                        <button class="px-2 py-1 text-[11px] rounded bg-[#FFCC00] hover:bg-[#FFDD4D] text-black font-semibold" title={locale.t("gallery.make_3d_title")} onclick={() => onMake3D?.(image)}>{locale.t("gallery.make_3d")}</button>
                      {/if}
                      <button class="px-2 py-1 text-[11px] rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100" onclick={() => gallery.copyToClipboard(image)}>{locale.t("gallery.copy")}</button>
                      <button class="px-2 py-1 text-[11px] rounded text-neutral-100 {gallery.comparePin === image ? 'bg-indigo-600 hover:bg-indigo-500' : 'bg-neutral-800 hover:bg-neutral-700'}" title={comparePinTitle(image)} onclick={() => gallery.toggleComparePin(image)}>{locale.t("gallery.compare.short")}</button>
                    {/if}
                    <button class="px-2 py-1 text-[11px] rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-100 disabled:opacity-50" disabled={gallery.saving} onclick={() => gallery.saveImageAs(image)}>{gallery.saving ? locale.t("gallery.saving") : locale.t("gallery.save")}</button>
                    <button class="px-2 py-1 text-[11px] rounded bg-red-900/80 hover:bg-red-800 text-neutral-100" onclick={() => deleteWithUndo([image])}>{locale.t("gallery.delete")}</button>
                  </div>
                </div>
              {/each}
            </div>
          </section>
        {/each}
        {#if galleryRenderLimit < galleryTotalCount}
          <div use:loadMoreGallery class="h-4 w-full"></div>
        {/if}
      {:else}
        <!-- Windowed grid: the list is as tall as every row, but only the rows
             near the viewport are in the DOM, so scrolling a gallery of
             thousands of images costs the same as scrolling a few dozen. -->
        <div use:gridList class="relative select-none" style="height: {gridHeight}px;">
          {#if listWidth > 0}
            <div class="absolute inset-x-0 top-0" style="transform: translateY({gridOffsets[gridRange.start] ?? 0}px);">
              {#each gridRows.slice(gridRange.start, gridRange.end) as row (row.key)}
                {#if row.kind === "header"}
                  <div class="flex items-center gap-2" style="height: {GRID_HEADER_HEIGHT}px;">
                    {@render groupBox(row.images)}
                    <h3 class="text-sm text-neutral-300 font-medium truncate">{row.label}</h3>
                  </div>
                {:else}
                  <div class="grid" style="grid-template-columns: repeat({gridColumns}, minmax(0, 1fr)); gap: {GRID_GAP}px; height: {tileHeight}px; margin-bottom: {GRID_GAP}px;">
                    {#each row.images as image (image)}
                    <div role="presentation" ondragstart={(e) => onTileDragStart(e, image)} ondragend={onTileDragEnd} class="group relative h-full rounded-lg overflow-hidden border transition-colors {selection.has(image) ? 'border-indigo-400 ring-2 ring-indigo-500' : 'border-neutral-800 hover:border-indigo-500'} {cursor === image ? 'outline-2 outline-offset-2 outline-white' : ''}">
                      <button
                        class="w-full h-full"
                        title={isVideoImage(image) ? locale.t("gallery.play_video") : image.filename}
                        onclick={(e) => onTileClick(e, image)}
                      >
                        <img use:lazyThumbnail={{ image, size: thumbSize }} alt={image.filename} decoding="async" class="w-full h-full object-cover {gallery.shouldBlur(image) ? 'blur-xl scale-110' : ''}" />
                      </button>
                      {#if gallery.shouldBlur(image)}
                        <div class="absolute inset-0 flex items-center justify-center pointer-events-none">
                          <span class="px-2 py-0.5 rounded bg-black/70 text-[11px] font-semibold text-red-300">{locale.t("gallery.nsfw_badge")}</span>
                        </div>
                      {/if}
                      {#if isVideoImage(image)}
                        <div class="absolute inset-0 flex items-center justify-center pointer-events-none">
                          <div class="w-10 h-10 rounded-full bg-black/60 flex items-center justify-center">
                            <svg class="w-5 h-5 text-white translate-x-px" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                              <path d="M8 5v14l11-7z" />
                            </svg>
                          </div>
                        </div>
                        {#if image.duration_seconds != null}
                          <div class="absolute top-1 right-1 px-1.5 py-0.5 rounded bg-black/70 text-white text-[10px] font-mono pointer-events-none">
                            {formatDuration(image.duration_seconds)}
                          </div>
                        {/if}
                      {/if}
                      {#if gallery.showGenerationTime && image.generationTimeMs != null}
                        <div class="absolute {isVideoImage(image) && image.duration_seconds != null ? 'top-7' : 'top-1'} right-1 px-1.5 py-0.5 rounded bg-black/70 text-amber-300 text-[10px] font-mono pointer-events-none">
                          {formatGenerationTime(image.generationTimeMs, locale.current)}
                        </div>
                      {/if}
                      <div class="absolute top-1 left-1 flex items-center gap-1 pointer-events-none">
                        {@render selectBox(image, selection.size > 0)}
                        <div class="px-1.5 py-0.5 rounded bg-black/70 text-[10px] text-neutral-200">{boardLabel(image)}</div>
                        {#if gallery.isFavourite(image)}
                          <div class="px-1.5 py-0.5 rounded bg-black/70 text-[10px] text-pink-400" title={locale.t("gallery.favourite")}>♥</div>
                        {/if}
                        {#if gallery.getRating(image) > 0}
                          <div class="px-1.5 py-0.5 rounded bg-black/70 text-[10px] text-amber-300" title={locale.t("gallery.rating")}>★{gallery.getRating(image)}</div>
                        {/if}
                      </div>
                      {#if generation.manualSaveMode && !image.gallery_filename}
                        <div class="absolute top-0 right-0 pt-1 pr-1 transition-opacity {tileOverlayClass(image)}">
                          <button class="w-7 h-7 flex items-center justify-center rounded bg-indigo-700/90 hover:bg-indigo-600 text-neutral-100 shadow" title={locale.t("gallery.save_to_folder")} onclick={(e) => { e.stopPropagation(); saveToDir(image); }}>
                            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
                          </button>
                        </div>
                      {/if}
                      <button
                        type="button"
                        class="hidden pointer-coarse:flex absolute bottom-1.5 right-1.5 z-10 w-7 h-7 items-center justify-center rounded shadow {touchActionsImage === image ? 'bg-indigo-600 text-neutral-100' : 'bg-black/70 text-neutral-200'}"
                        title={locale.t("gallery.col_actions")}
                        aria-label={locale.t("gallery.col_actions")}
                        aria-expanded={touchActionsImage === image}
                        onclick={(e) => { e.stopPropagation(); touchActionsImage = touchActionsImage === image ? null : image; }}
                      >{touchActionsImage === image ? "×" : "⋯"}</button>
                      <div class="absolute bottom-0 inset-x-0 flex flex-wrap justify-center items-center gap-1 px-1.5 pb-1.5 pt-6 pointer-coarse:pr-10 transition-opacity bg-linear-to-t from-black/80 to-transparent {tileOverlayClass(image)}">
                        {#if !isVideoImage(image)}
                          <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black text-[11px] font-bold shadow" title={locale.t("gallery.img2img")} onclick={(e) => { e.stopPropagation(); img2imgImage(image); }}>I2I</button>
                          <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black shadow" title={locale.t("gallery.inpaint")} onclick={(e) => { e.stopPropagation(); inpaintImage(image); }}>✎</button>
                          {#if !image.is_upscaled}
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black shadow" title={locale.t("gallery.upscale")} onclick={(e) => { e.stopPropagation(); upscaleImage(image); }}>+</button>
                          {/if}
                          {#if directorToolsAvailable()}
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black text-[11px] font-bold shadow" title={locale.t("novelai.director.action")} onclick={(e) => { e.stopPropagation(); directorTools.open(image, image.thumbnailUrl || image.url || null); }}>DT</button>
                          {/if}
                          <!-- The three NovelAI image actions, each its own button.
                               They share one modal but cost wildly different
                               amounts (about 1 Anlas, about 48, and a paid img2img
                               pass), so the cheap one must not sit behind the
                               expensive one. Same three buttons, same order, as the
                               lightbox bar. -->
                          {#if naiImageEnhanceAvailable()}
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black shadow" title={locale.t("novelai.enhance.tab_upscale", { factor: UPSCALE_FACTOR })} onclick={(e) => { e.stopPropagation(); naiImageEnhance.open(image, image.thumbnailUrl || image.url || null, "upscale"); }}><svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4h6v6"/><path d="M20 4l-7 7"/><path d="M10 20H4v-6"/><path d="M4 20l7-7"/></svg></button>
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black shadow" title={locale.t("novelai.enhance.tab_variations")} onclick={(e) => { e.stopPropagation(); naiImageEnhance.open(image, image.thumbnailUrl || image.url || null, "variations"); }}>⧉</button>
                            <button class="w-7 h-7 flex items-center justify-center rounded bg-[#FFCC00]/95 hover:bg-[#FFCC00] text-black shadow" title={locale.t("novelai.enhance.action")} onclick={(e) => { e.stopPropagation(); naiImageEnhance.open(image, image.thumbnailUrl || image.url || null); }}><svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20l9-9"/><path d="M14.5 6.5l3 3"/><path d="M13 3.5l1 2.2 2.2 1-2.2 1-1 2.2-1-2.2-2.2-1 2.2-1z"/><path d="M19 13l.7 1.6 1.6.7-1.6.7-.7 1.6-.7-1.6-1.6-.7 1.6-.7z"/></svg></button>
                          {/if}
                          <button class="w-7 h-7 flex items-center justify-center rounded bg-neutral-800/90 hover:bg-neutral-700 text-neutral-200 shadow" title={locale.t("gallery.copy")} onclick={(e) => { e.stopPropagation(); gallery.copyToClipboard(image); }}><svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M15 5.5A2.5 2.5 0 0 0 12.5 3h-7A2.5 2.5 0 0 0 3 5.5v7A2.5 2.5 0 0 0 5.5 15"/></svg></button>
                          <button class="w-7 h-7 flex items-center justify-center rounded shadow {gallery.comparePin === image ? 'bg-indigo-600 hover:bg-indigo-500 text-neutral-100' : 'bg-neutral-800/90 hover:bg-neutral-700 text-neutral-200'}" title={comparePinTitle(image)} onclick={(e) => { e.stopPropagation(); gallery.toggleComparePin(image); }}>⇄</button>
                        {/if}
                        <button class="w-7 h-7 flex items-center justify-center rounded shadow {gallery.isFavourite(image) ? 'bg-pink-700/90 hover:bg-pink-600 text-white' : 'bg-neutral-800/90 hover:bg-neutral-700 text-neutral-200'}" aria-pressed={gallery.isFavourite(image)} title={locale.t(gallery.isFavourite(image) ? "gallery.unfavourite" : "gallery.favourite")} onclick={(e) => { e.stopPropagation(); gallery.toggleFavourite(image); }}>♥</button>
                        <button class="w-7 h-7 flex items-center justify-center rounded shadow text-[9px] font-bold {gallery.isNsfw(image) ? 'bg-red-900/90 hover:bg-red-800 text-red-200' : 'bg-neutral-800/90 hover:bg-neutral-700 text-neutral-200'}" aria-pressed={gallery.isNsfw(image)} title={locale.t(gallery.isNsfw(image) ? "gallery.mark_safe" : "gallery.mark_nsfw")} onclick={(e) => { e.stopPropagation(); gallery.toggleNsfw(image); }}>{locale.t("gallery.nsfw_badge")}</button>
                        <button class="w-7 h-7 flex items-center justify-center rounded bg-neutral-800/90 hover:bg-neutral-700 text-neutral-200 shadow disabled:opacity-50" disabled={gallery.saving} title={locale.t(isVideoImage(image) ? "gallery.save_video_as" : "gallery.save_as")} onclick={(e) => { e.stopPropagation(); gallery.saveImageAs(image); }}>↓</button>
                        <button class="w-7 h-7 flex items-center justify-center rounded bg-red-900/80 hover:bg-red-800 text-red-300 hover:text-red-200 shadow" title={locale.t("gallery.delete")} onclick={(e) => { e.stopPropagation(); deleteWithUndo([image]); }}>×</button>
                      </div>
                    </div>
                    {/each}
                  </div>
                {/if}
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
  {#if selection.size > 0}
    <div class="sticky bottom-0 z-20 mt-3 flex justify-center pointer-events-none">
      <div class="pointer-events-auto flex flex-wrap items-center justify-center gap-1.5 rounded-xl border border-indigo-500/60 bg-neutral-900/95 px-3 py-2 shadow-xl shadow-black/50 backdrop-blur" role="toolbar" aria-label={locale.t("gallery.bulk.selected", { count: String(selection.size) })}>
          <span class="px-1 text-sm font-medium text-indigo-200 tabular-nums">{locale.t("gallery.bulk.selected", { count: String(selection.size) })}</span>
          {#if selection.size < galleryTotalCount}
            <button class="px-2.5 py-1.5 text-xs rounded border border-neutral-700 text-neutral-300 hover:border-neutral-500" onclick={selectAll}>{locale.t("gallery.bulk.select_all")}</button>
          {/if}
          <span class="w-px h-5 bg-neutral-700" aria-hidden="true"></span>
          {#if bulkNewBoard !== null}
            <!-- svelte-ignore a11y_autofocus -->
            <input
              type="text"
              autofocus
              bind:value={bulkNewBoard}
              placeholder={locale.t("gallery.placeholder_board")}
              class="w-36 bg-neutral-800 border border-neutral-700 rounded px-2 py-1.5 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
              onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); bulkMoveToNewBoard(); } else if (e.key === "Escape") { e.stopPropagation(); bulkNewBoard = null; } }}
            />
            <button class="px-2.5 py-1.5 text-xs rounded border border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 disabled:opacity-50" disabled={!bulkNewBoard.trim()} onclick={bulkMoveToNewBoard}>{locale.t("gallery.add")}</button>
          {:else}
            <select
              class="max-w-40 bg-neutral-800 border border-neutral-700 rounded px-2 py-1.5 text-xs text-neutral-200"
              aria-label={locale.t("gallery.bulk.move_to_board")}
              value=""
              onchange={(e) => { const el = e.currentTarget; bulkBoardChosen(el.value); el.value = ""; }}
            >
              <option value="" disabled hidden>{locale.t("gallery.bulk.move_to_board")}</option>
              <option value="Unsorted">{locale.t("gallery.unsorted")}</option>
              {#each gallery.boards as board}
                <option value={board}>{board}</option>
              {/each}
              <option value={NEW_BOARD_OPTION}>{locale.t("gallery.bulk.new_board")}</option>
            </select>
          {/if}
          <select
            class="bg-neutral-800 border border-neutral-700 rounded px-2 py-1.5 text-xs text-amber-300"
            aria-label={locale.t("gallery.bulk.set_rating")}
            value=""
            onchange={(e) => { const el = e.currentTarget; gallery.setRatingMany(selectedImages(), Number(el.value)); el.value = ""; }}
          >
            <option value="" disabled hidden>{locale.t("gallery.bulk.set_rating")}</option>
            <option value={0}>{locale.t("gallery.rating_none")}</option>
            {#each Array.from({ length: GALLERY_MAX_RATING }, (_, i) => i + 1) as stars}
              <option value={stars}>{"★".repeat(stars)}</option>
            {/each}
          </select>
          <button
            class="w-8 h-8 flex items-center justify-center rounded {allSelectedFavourite ? 'bg-pink-700/90 hover:bg-pink-600 text-white' : 'bg-neutral-800 hover:bg-neutral-700 text-neutral-200'}"
            aria-pressed={allSelectedFavourite}
            title={locale.t(allSelectedFavourite ? "gallery.unfavourite" : "gallery.favourite")}
            aria-label={locale.t(allSelectedFavourite ? "gallery.unfavourite" : "gallery.favourite")}
            onclick={() => gallery.setFavouriteMany(selectedImages(), !allSelectedFavourite)}
          >♥</button>
          <button
            class="h-8 px-2 flex items-center justify-center rounded text-[10px] font-bold {allSelectedNsfw ? 'bg-red-900/90 hover:bg-red-800 text-red-200' : 'bg-neutral-800 hover:bg-neutral-700 text-neutral-200'}"
            aria-pressed={allSelectedNsfw}
            title={locale.t(allSelectedNsfw ? "gallery.mark_safe" : "gallery.mark_nsfw")}
            onclick={() => gallery.setNsfwMany(selectedImages(), !allSelectedNsfw)}
          >{locale.t("gallery.nsfw_badge")}</button>
          {#if comparePair}
            <button class="h-8 px-2.5 text-xs rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-200" onclick={() => { const [a, b] = comparePair!; void gallery.openCompare(a, b); }}>{locale.t("gallery.compare.short")}</button>
          {/if}
          {#if isTauri}
            <button class="h-8 px-2.5 text-xs rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-200 disabled:opacity-60 tabular-nums" title={locale.t("gallery.bulk.export_title")} disabled={!!exportProgress} onclick={bulkExport}>
              {exportProgress ? locale.t("gallery.bulk.exporting", { done: String(exportProgress.done), total: String(exportProgress.total) }) : locale.t("gallery.bulk.export")}
            </button>
          {/if}
          <button class="h-8 px-2.5 text-xs rounded bg-red-900/80 hover:bg-red-800 text-red-100" disabled={!!exportProgress} onclick={bulkDelete}>{locale.t("gallery.delete")}</button>
          <span class="w-px h-5 bg-neutral-700" aria-hidden="true"></span>
          <button class="w-8 h-8 flex items-center justify-center rounded text-neutral-400 hover:text-neutral-100 hover:bg-neutral-800" title={locale.t("gallery.bulk.clear")} aria-label={locale.t("gallery.bulk.clear")} onclick={clearSelection}>&times;</button>
      </div>
    </div>
  {/if}
</div>

{#if dirPickerImage}
  <div use:portal class="fixed inset-0 z-60 flex items-center justify-center bg-black/70 p-4">
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

{#snippet selectBox(image: OutputImage, shown: boolean)}
  <button
    type="button"
    role="checkbox"
    aria-checked={selection.has(image)}
    aria-label={locale.t("gallery.bulk.select")}
    title={locale.t("gallery.bulk.select")}
    class="pointer-events-auto w-5.5 h-5.5 shrink-0 items-center justify-center rounded border shadow transition-colors {selection.has(image) ? 'flex bg-indigo-500 border-indigo-300 text-white' : `bg-black/70 border-neutral-400 text-transparent hover:border-white ${shown ? 'flex' : 'hidden group-hover:flex group-has-[:focus-visible]:flex pointer-coarse:flex'}`}"
    onclick={(e) => onSelectBoxClick(e, image)}
  ><svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg></button>
{/snippet}

{#snippet groupBox(images: OutputImage[])}
  {@const all = images.length > 0 && images.every((image) => selection.has(image))}
  <button
    type="button"
    role="checkbox"
    aria-checked={all}
    aria-label={locale.t("gallery.bulk.select_group")}
    title={locale.t("gallery.bulk.select_group")}
    class="w-5.5 h-5.5 shrink-0 flex items-center justify-center rounded border transition-colors {all ? 'bg-indigo-500 border-indigo-300 text-white' : 'border-neutral-600 text-transparent hover:border-neutral-300'}"
    onclick={() => setSelected(images, !all)}
  ><svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg></button>
{/snippet}

{#snippet boardChip(board: string, label: string, count: number)}
  <!-- A drop target for dragged tiles; "all" is a view, not a board. -->
  <button
    type="button"
    aria-pressed={gallery.boardFilter === board}
    class="flex items-center gap-1.5 px-2.5 py-1 text-xs rounded-full border transition-colors {dragOverBoard === board ? 'border-indigo-300 bg-indigo-500/30 text-white' : gallery.boardFilter === board ? 'border-indigo-500 bg-indigo-500/10 text-indigo-200' : dragActive && board !== 'all' ? 'border-dashed border-indigo-500/70 text-neutral-200' : 'border-neutral-700 text-neutral-300 hover:border-neutral-500'}"
    onclick={() => (gallery.boardFilter = board)}
    ondragover={board === "all" ? undefined : (e) => onChipDragOver(e, board)}
    ondragleave={() => { if (dragOverBoard === board) dragOverBoard = null; }}
    ondrop={board === "all" ? undefined : (e) => onChipDrop(e, board)}
  >
    <span class="max-w-40 truncate">{label}</span>
    <span class="text-[10px] text-neutral-500 tabular-nums">{count}</span>
  </button>
{/snippet}
