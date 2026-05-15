<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { compare } from "../../stores/compare.svelte.js";
  import { generate, interruptGeneration, deleteQueueItem, installPipPackage, downloadModel } from "../../utils/api.js";
  import { models } from "../../stores/models.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import { isBrowserMode } from "../../utils/ipc.js";
  import type { GenerationParams } from "../../types/index.js";

  interface Props {
    canvasEditorRef?: { getRasterComposite: () => HTMLCanvasElement | null; getMaskCanvas: () => HTMLCanvasElement | null };
  }

  let { canvasEditorRef }: Props = $props();
  let errorMsg = $state<string | null>(null);
  let isSubmitting = $state(false);
  let orderedRunPromptIds = $state<string[]>([]);
  let orderedRunCancelRequested = $state(false);
  let submitRunToken = 0;
  let orderedRunToken = 0;
  const orderedWildcardRun = $derived(promptPresets.orderedWildcardRun);
  const orderedWildcardRunCount = $derived(compare.enabled && compare.cellCount > 1 ? 0 : (orderedWildcardRun?.count ?? 0));
  const pendingOrderedRunIds = $derived(orderedRunPromptIds.filter((id) => progress.pendingPrompts.some((prompt) => prompt.promptId === id)));

  async function requestGeneration(params: GenerationParams) {
    const result = await generate(params);
    params.seed = result.seed;
    return result;
  }

  function trackGeneration(params: GenerationParams, result: Awaited<ReturnType<typeof requestGeneration>>): string {
    progress.enqueue(result.prompt_id, params.upscale_enabled, params.mode, params);
    if (result.queue_position != null && result.queue_total != null) {
      progress.updateQueuePosition(result.prompt_id, result.queue_position, result.queue_total);
    }
    return result.prompt_id;
  }

  async function submitGeneration(params: GenerationParams): Promise<string> {
    return trackGeneration(params, await requestGeneration(params));
  }

  function finishSubmitRun(runToken: number) {
    if (runToken === submitRunToken) {
      isSubmitting = false;
    }
  }

  async function handleGenerate() {
    if (isSubmitting) return;
    const runToken = ++submitRunToken;
    isSubmitting = true;
    errorMsg = null;

    if (!generation.checkpoint) {
      errorMsg = locale.t('generation.error_no_checkpoint');
      finishSubmitRun(runToken);
      return;
    }

    try {
      // If compare grid has multiple cells, generate all cells
      if (compare.enabled && compare.cellCount > 1) {
        await handleGridGenerate();
        return;
      }

      // If canvas mode is active, export canvas content before generating
      if (canvas.isCanvasMode) {
        if (!canvasEditorRef) {
          throw new Error("Canvas editor is not ready yet. Please try again.");
        }
        await canvas.syncToGeneration(
          () => canvasEditorRef.getRasterComposite(),
          () => canvasEditorRef.getMaskCanvas()
        );
      }

      if (generation.mode === "inpainting") {
        if (!generation.inputImage) {
          errorMsg = locale.t('generation.error_no_image');
          return;
        }
        if (!generation.maskImage) {
          errorMsg = locale.t('generation.error_no_mask');
          return;
        }
      }

      // Ensure face fix dependencies are ready when enabled
      if (generation.facefixEnabled) {
        const detector = generation.facefixDetector || "Anzhc Face seg 640 v4 y11n.pt";
        if (!models.ultralyticsModels.includes(detector)) {
          gallery.showToast(locale.t('generation.downloading_facefix'), "info");
          const detectorMeta: Record<string, { url: string; sha256?: string }> = {
            "Anzhc Face seg 640 v4 y11n.pt": {
              url: "https://huggingface.co/Anzhc/Anzhcs_YOLOs/resolve/0319daeae9ae40752c2fb3904069cb35cc61d2ec/Anzhc%20Face%20seg%20640%20v4%20y11n.pt",
              sha256: "1e77ad7bd349babd8a4a90478bfc965348642b63a8d95d3b43ee13db42fd0a64",
            },
          };
          const meta = detectorMeta[detector];
          const url = meta?.url ?? `https://huggingface.co/Bingsu/adetailer/resolve/main/${detector}`;
          await downloadModel(
            url,
            "ultralytics",
            detector,
            undefined,
            meta?.sha256,
          );
          generation.facefixDetector = detector;
          await models.refresh();
        }
        if (!isBrowserMode) {
          await installPipPackage("ultralytics==8.4.34");
        }
      }

      // Anima models produce poor results below 1024 — clamp to 1024² area preserving aspect ratio
      if (generation.isAnima && (generation.width < 1024 || generation.height < 1024)) {
        const ratio = generation.width / generation.height;
        const area = 1024 * 1024;
        generation.width = Math.round(Math.sqrt(area * ratio) / 8) * 8;
        generation.height = Math.round(Math.sqrt(area / ratio) / 8) * 8;
      }

      if (orderedWildcardRunCount > 1) {
        await handleOrderedWildcardGenerate(orderedWildcardRunCount);
        return;
      }

      const params = generation.toParams();
      console.log("[generate] output_format:", params.output_format, "output_bit_depth:", params.output_bit_depth);
      generation.saveCurrentPromptToHistory();
      await submitGeneration(params);
      generation.saveSettings();
    } catch (e) {
      if (runToken === submitRunToken) {
        console.error("Generation failed:", e);
        errorMsg = String(e);
      }
    } finally {
      finishSubmitRun(runToken);
    }
  }

  async function handleOrderedWildcardGenerate(count: number) {
    const run = orderedWildcardRun;
    if (!run) return;
    const choices = promptPresets.wildcardChoices(run.presetId);
    if (choices.length === 0) return;

    generation.saveCurrentPromptToHistory();
    const runToken = ++orderedRunToken;
    orderedRunPromptIds = [];
    orderedRunCancelRequested = false;
    for (let i = 0; i < count; i++) {
      if (orderedRunCancelRequested || runToken !== orderedRunToken) break;
      const choiceIndex = (run.nextIndex + i) % choices.length;
      const fixedPresetChoices = new Map([[run.presetId, choices[choiceIndex]]]);
      const params = generation.toParams({ fixedPresetChoices });
      console.log(
        "[generate] ordered_wildcard:",
        `${i + 1}/${count}`,
        "output_format:",
        params.output_format,
        "output_bit_depth:",
        params.output_bit_depth,
      );
      let result: Awaited<ReturnType<typeof requestGeneration>>;
      try {
        result = await requestGeneration(params);
      } catch (e) {
        if (orderedRunCancelRequested || runToken !== orderedRunToken) break;
        throw e;
      }
      if (orderedRunCancelRequested || runToken !== orderedRunToken) {
        try { await interruptGeneration(result.prompt_id); } catch { /* already gone */ }
        break;
      }
      const promptId = trackGeneration(params, result);
      orderedRunPromptIds = [...orderedRunPromptIds, promptId];
      if (orderedRunCancelRequested || runToken !== orderedRunToken) {
        await cancelPromptIds([promptId], true);
        break;
      }
      promptPresets.setOrderedWildcardIndex(run.presetId, choiceIndex + 1);
    }
    if (runToken === orderedRunToken) {
      generation.saveSettings();
    }
  }

  /** Left-click: skip the current ordered item, otherwise cancel the current generation. */
  async function handleCancelCurrent() {
    if (pendingOrderedRunIds.length > 0) {
      await handleSkipOrderedPrompt();
      return;
    }
    const promptId = progress.activePromptId ?? progress.pendingPrompts[0]?.promptId;
    await interruptGeneration(promptId ?? undefined);
    if (promptId) progress.removePrompt(promptId);
  }

  async function handleSkipOrderedPrompt() {
    const promptId = progress.activePromptId && pendingOrderedRunIds.includes(progress.activePromptId)
      ? progress.activePromptId
      : pendingOrderedRunIds[0];
    if (!promptId) return;
    await cancelPromptIds([promptId], true);
  }

  async function cancelPromptIds(idsToCancel: string[], interruptActive = false) {
    if (idsToCancel.length === 0) return;
    const activePromptId = progress.activePromptId && idsToCancel.includes(progress.activePromptId)
      ? progress.activePromptId
      : null;

    if (interruptActive && activePromptId) {
      await interruptGeneration(activePromptId);
    }
    for (const promptId of idsToCancel) {
      if (promptId === activePromptId) continue;
      try { await deleteQueueItem(promptId); } catch { /* already removed */ }
    }
    for (const promptId of idsToCancel) {
      progress.removePrompt(promptId);
    }
    orderedRunPromptIds = orderedRunPromptIds.filter((id) => !idsToCancel.includes(id));
  }

  /** Right-click: cancel current + clear the entire queue. */
  async function handleCancelAll(e: MouseEvent) {
    e.preventDefault();
    orderedRunCancelRequested = true;
    submitRunToken++;
    orderedRunToken++;
    progress.cancelAll();
    orderedRunPromptIds = [];
    compare.clearGridBatch();
    try {
      await interruptGeneration();
    } catch (e) {
      console.error("Failed to cancel queued generations:", e);
    } finally {
      isSubmitting = false;
    }
  }

  /** Generate all grid cells sequentially with a shared seed, then stitch into a grid. */
  async function handleGridGenerate() {
    compare.saveActiveCell();
    const savedIndex = compare.activeIndex;

    // Resolve one seed for all cells that have random (-1) seed
    const sharedSeed = Math.floor(Math.random() * Number.MAX_SAFE_INTEGER);
    const failedCells: number[] = [];

    // Sort cells by model to minimize expensive ComfyUI model swaps
    const cellOrder = compare.cells.map((cell, i) => ({ cell, index: i }));
    cellOrder.sort((a, b) => {
      const modelA = a.cell.diffusionModel ?? a.cell.checkpoint;
      const modelB = b.cell.diffusionModel ?? b.cell.checkpoint;
      return modelA.localeCompare(modelB);
    });

    // Track results by original cell index so the grid stitches in the right order
    const resultsByIndex = new Map<number, { promptId: string; cell: typeof compare.cells[0] }>();

    for (const { cell, index } of cellOrder) {
      compare.applyToGeneration(cell);

      const params = generation.toParams();

      // Use shared seed for random seeds so the grid is consistent
      if (params.seed < 0) {
        params.seed = sharedSeed;
      }

      try {
        const result = await generate(params);
        params.seed = result.seed;
        progress.enqueue(result.prompt_id, params.upscale_enabled, params.mode, params);
        resultsByIndex.set(index, { promptId: result.prompt_id, cell });
      } catch (e) {
        console.error(`Grid cell ${index + 1} failed:`, e);
        failedCells.push(index);
      }
    }

    // Build arrays in original cell order for correct grid stitching
    const promptIds: string[] = [];
    const successSnapshots: typeof compare.cells = [];
    for (let i = 0; i < compare.cellCount; i++) {
      const entry = resultsByIndex.get(i);
      if (entry) {
        promptIds.push(entry.promptId);
        successSnapshots.push(entry.cell);
      }
    }

    if (promptIds.length >= 2) {
      compare.startGridBatch(promptIds, compare.rows, compare.cols, successSnapshots, failedCells);
    }

    // Restore active cell params
    const activeSnap = compare.cells[savedIndex];
    if (activeSnap) compare.applyToGeneration(activeSnap);

    if (failedCells.length > 0) {
      errorMsg = locale.t('compare.grid_cells_failed', { cells: failedCells.map(i => i + 1).join(', ') });
    }

    generation.saveSettings();
  }

  const canGenerate = $derived(!!generation.checkpoint);

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      handleGenerate();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex gap-3">
  <button
    onclick={handleGenerate}
    disabled={!canGenerate}
    title={orderedWildcardRunCount > 1 ? locale.t('generation.generate_ordered_tip', { count: orderedWildcardRunCount, name: orderedWildcardRun?.presetName ?? '' }) : ''}
    class="flex-1 py-3 rounded-xl font-semibold text-sm transition-colors
      {canGenerate
        ? 'bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg shadow-indigo-600/20'
        : 'bg-neutral-800 text-neutral-500 cursor-not-allowed'}"
  >
    {#if progress.queueCount > 0}
      {locale.t('generation.generate_queue', { count: progress.queueCount })}
    {:else if orderedWildcardRunCount > 1}
      {locale.t('generation.generate_ordered', { count: orderedWildcardRunCount })}
    {:else}
      {locale.t('generation.generate')}
    {/if}
  </button>

  {#if progress.isGenerating}
    <button
      onclick={handleCancelCurrent}
      oncontextmenu={handleCancelAll}
      class="px-5 py-3 rounded-xl font-semibold text-sm bg-red-700 hover:bg-red-600 text-white transition-colors"
      title={locale.t('generation.cancel_hint')}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <line x1="18" y1="6" x2="6" y2="18"></line>
        <line x1="6" y1="6" x2="18" y2="18"></line>
      </svg>
    </button>
  {:else}
    <button
      disabled
      title={locale.t('generation.cancel_hint')}
      class="px-5 py-3 rounded-xl font-semibold text-sm bg-neutral-800 text-neutral-600 cursor-not-allowed transition-colors"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <line x1="18" y1="6" x2="6" y2="18"></line>
        <line x1="6" y1="6" x2="18" y2="18"></line>
      </svg>
    </button>
  {/if}
</div>

{#if errorMsg}
  <p class="text-xs text-red-400 text-center mt-1">{errorMsg}</p>
{/if}
