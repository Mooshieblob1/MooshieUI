<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { canvas } from "../../stores/canvas.svelte.js";
  import { compare } from "../../stores/compare.svelte.js";
  import {
    generate,
    interruptGeneration,
    deleteQueueItem,
    installPipPackage,
    downloadModel,
    checkPythonImport,
  } from "../../utils/api.js";
  import { models } from "../../stores/models.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import { isBrowserMode } from "../../utils/ipc.js";
  import type { GenerationParams } from "../../types/index.js";
  import { runRegionalInpaintChain } from "../../utils/regionalInpaintChain.js";
  import {
    suppressRegionalChainGallerySave,
    clearAllRegionalChainGallerySuppress,
  } from "../../utils/regionalChainGallery.js";
  import { checkStyleTransferNodesReady } from "../../utils/styleTransferNodes.js";
  import { classifyGenerationError } from "../../utils/generationErrors.js";
  import { parseSegmentDetailPrompt, yoloTargetFilename } from "../../utils/promptSegmentDetail.js";
  import { requestGeneration, trackGeneration, submitGeneration } from "../../utils/generationSubmit.js";

  interface Props {
    canvasEditorRef?: { getRasterComposite: () => HTMLCanvasElement | null; getMaskCanvas: () => HTMLCanvasElement | null };
  }

  let { canvasEditorRef }: Props = $props();
  let errorMsg = $state<string | null>(null);
  let isSubmitting = $state(false);
  let orderedRunPromptIds = $state<string[]>([]);
  let orderedRunCancelRequested = $state(false);
  let regionalChainCancelRequested = $state(false);
  let submitRunToken = 0;
  let orderedRunToken = 0;
  let regionalChainToken = 0;
  const orderedWildcardRun = $derived(promptPresets.orderedWildcardRun);
  const orderedWildcardRunCount = $derived(compare.enabled && compare.cellCount > 1 ? 0 : (orderedWildcardRun?.count ?? 0));
  const pendingOrderedRunIds = $derived(orderedRunPromptIds.filter((id) => progress.pendingPrompts.some((prompt) => prompt.promptId === id)));

  const generateButtonTitle = $derived.by(() => {
    if (orderedWildcardRunCount > 1) {
      return locale.t("generation.generate_ordered_tip", {
        count: orderedWildcardRunCount,
        name: orderedWildcardRun?.presetName ?? "",
      });
    }
    if (progress.isGenerating) {
      return locale.t("generation.generate_queue_tip", { count: progress.queueCount });
    }
    return locale.t("generation.generate_tip");
  });

  async function ensureFacefixPythonDependency() {
    if (isBrowserMode) return;
    if (await checkPythonImport("ultralytics")) return;
    await installPipPackage("ultralytics==8.4.34");
    const importOk = await checkPythonImport("ultralytics");
    if (!importOk) {
      throw new Error(locale.t("generation.facefix.dep_check_failed"));
    }
  }

  const DETECTOR_META: Record<string, { url: string; sha256?: string }> = {
    "Anzhc Face seg 640 v4 y11n.pt": {
      url: "https://huggingface.co/Anzhc/Anzhcs_YOLOs/resolve/0319daeae9ae40752c2fb3904069cb35cc61d2ec/Anzhc%20Face%20seg%20640%20v4%20y11n.pt",
      sha256: "1e77ad7bd349babd8a4a90478bfc965348642b63a8d95d3b43ee13db42fd0a64",
    },
  };

  /** Download a YOLO detector into models/ultralytics if missing, then ensure the python dep. */
  async function ensureUltralyticsDetector(detector: string, toastKey: string): Promise<void> {
    if (!models.ultralyticsModels.includes(detector)) {
      gallery.showToast(locale.t(toastKey), "info");
      const meta = DETECTOR_META[detector];
      const url = meta?.url ?? `https://huggingface.co/Bingsu/adetailer/resolve/main/${detector}`;
      await downloadModel(url, "ultralytics", detector, undefined, meta?.sha256);
      await models.refresh();
    }
    await ensureFacefixPythonDependency();
  }

  function finishSubmitRun(runToken: number) {
    if (runToken === submitRunToken) {
      isSubmitting = false;
      progress.setRegionalChainStatus(null);
    }
  }

  function setRegionalChainStep(phase: "base" | "region" | "wait", index: number, total: number) {
    if (phase === "wait") {
      progress.setRegionalChainStatus(locale.t("generation.regional.inpaint_chain_waiting"));
      return;
    }
    const key =
      phase === "base"
        ? "generation.regional.inpaint_chain_base"
        : "generation.regional.inpaint_chain_region";
    progress.setRegionalChainStatus(
      locale.t(key, {
        current: String(index + 1),
        total: String(total),
      }),
    );
  }

  function isSequentialGenerateRun(): boolean {
    if (compare.enabled && compare.cellCount > 1) return true;
    if (orderedWildcardRunCount > 1) return true;
    const regionalPromptingSupported = generation.supportsRegionalPrompting;
    const useRegionalInpaintChain =
      regionalPromptingSupported &&
      generation.effectiveRegionalStrategy === "inpaint_chain";
    if (useRegionalInpaintChain) {
      return generation.getValidRegionalSelectionsForInpaint().length > 0;
    }
    return false;
  }

  async function handleGenerate() {
    const sequential = isSequentialGenerateRun();
    if (sequential && isSubmitting) return;
    const runToken = ++submitRunToken;
    if (sequential) isSubmitting = true;
    errorMsg = null;

    if (!generation.checkpoint) {
      errorMsg = locale.t('generation.error_no_checkpoint');
      if (sequential) finishSubmitRun(runToken);
      return;
    }

    if (generation.styleTransferEnabled) {
      if (!generation.styleReferenceImage?.trim()) {
        errorMsg = locale.t("generation.style_transfer.no_reference");
        gallery.showToast(locale.t("generation.style_transfer.no_reference"), "error");
        if (sequential) finishSubmitRun(runToken);
        return;
      }
      const nodesReady = await checkStyleTransferNodesReady();
      if (!nodesReady) {
        errorMsg = locale.t("generation.style_transfer.nodes_missing_generate");
        gallery.showToast(locale.t("generation.style_transfer.nodes_missing_generate"), "error");
        if (sequential) finishSubmitRun(runToken);
        return;
      }
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
          throw new Error(locale.t("canvas.editor_not_ready"));
        }
        await canvas.syncToGeneration(
          () => canvasEditorRef.getRasterComposite(),
          () => canvasEditorRef.getMaskCanvas()
        );
      }

      if (generation.mode === "img2img" && !generation.inputImage) {
        errorMsg = locale.t('generation.error_no_image');
        return;
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
        await ensureUltralyticsDetector(detector, "generation.downloading_facefix");
        generation.facefixDetector = detector;
      }

      // Ensure YOLO detectors referenced by <segment:yolo-...> tags are ready
      // (deduplicated so each detector is checked once). Unknown models that
      // fail to download are skipped with a warning — the segment node passes
      // through unchanged when its model is missing.
      const segmentDetectors = new Set(
        parseSegmentDetailPrompt(generation.positivePrompt)
          .segments.map((s) => yoloTargetFilename(s.target))
          .filter((name): name is string => name !== null),
      );
      for (const detector of segmentDetectors) {
        try {
          await ensureUltralyticsDetector(detector, "generation.segment.downloading_detector");
        } catch (e) {
          console.warn("[segment] detector unavailable:", detector, e);
          gallery.showToast(
            locale.t("generation.segment.detector_unavailable", { name: detector }),
            "warning",
          );
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

      const regionalPromptingSupported = generation.supportsRegionalPrompting;
      const useRegionalInpaintChain =
        regionalPromptingSupported &&
        generation.effectiveRegionalStrategy === "inpaint_chain";
      const validRegions = useRegionalInpaintChain
        ? generation.getValidRegionalSelectionsForInpaint()
        : generation.regionalPrompts.filter(
            (r) => r.text.trim() && r.width > 0 && r.height > 0,
          );
      const configuredRegions = validRegions.length;

      if (configuredRegions > 0 && !regionalPromptingSupported) {
        gallery.showToast(locale.t("generation.regional.dropped_warning"), "warning");
        console.warn(
          "[regional] Generate skipped",
          configuredRegions,
          "GUI region(s); architecture=",
          generation.modelFamily,
        );
      }

      const skippedEmpty = generation.regionalPrompts.length - configuredRegions;
      if (skippedEmpty > 0 && configuredRegions > 0) {
        gallery.showToast(
          locale.t("generation.regional.empty_skipped_warning", { count: String(skippedEmpty) }),
          "warning",
        );
      }

      console.log(
        "[generate] output_format:",
        generation.outputFormat,
        "output_bit_depth:",
        generation.outputBitDepth,
        "regional:",
        generation.effectiveRegionalStrategy,
      );
      generation.saveCurrentPromptToHistory();

      if (useRegionalInpaintChain && configuredRegions > 0) {
        const chainToken = ++regionalChainToken;
        regionalChainCancelRequested = false;
        gallery.showToast(locale.t("generation.regional.inpaint_chain_started"), "info");
        clearAllRegionalChainGallerySuppress();
        try {
          const chainResult = await runRegionalInpaintChain(validRegions, {
            submit: async (chainParams, ctx) => {
              const result = await requestGeneration(chainParams);
              if (!ctx.isFinalOutput) {
                suppressRegionalChainGallerySave(result.prompt_id);
              }
              trackGeneration(chainParams, result);
              return { promptId: result.prompt_id, seed: result.seed };
            },
            onStep: ({ phase, index, total }) => {
              setRegionalChainStep(phase, index, total);
              if (phase === "base") {
                gallery.showToast(
                  locale.t("generation.regional.inpaint_chain_base", {
                    current: "1",
                    total: String(total),
                  }),
                  "info",
                );
              } else {
                gallery.showToast(
                  locale.t("generation.regional.inpaint_chain_region", {
                    current: String(index + 1),
                    total: String(total),
                  }),
                  "info",
                );
              }
            },
            onWaitingForOutput: () => setRegionalChainStep("wait", 0, 0),
            shouldCancel: () =>
              regionalChainCancelRequested || chainToken !== regionalChainToken,
          });
          console.log(
            "[regional] Inpaint chain finished:",
            chainResult.regionCount,
            "region(s)",
          );
        } catch (chainError) {
          if (regionalChainCancelRequested || chainToken !== regionalChainToken) {
            console.log("[regional] Inpaint chain cancelled");
          } else {
            throw chainError;
          }
        } finally {
          clearAllRegionalChainGallerySuppress();
        }
      } else {
        const params = generation.toParams();
        const sentRegions = params.positive_regions?.length ?? 0;
        if (sentRegions > 0) {
          console.log("[regional] Sending", sentRegions, "region(s) via conditioning");
        }
        await submitGeneration(params);
      }
      generation.saveSettings();
    } catch (e) {
      if (runToken === submitRunToken) {
        console.error("Generation failed:", e);
        const message = e instanceof Error ? e.message : String(e);
        // Classify submission failures (stale model cache, incompatible VAE,
        // OOM, missing node, etc.) into actionable guidance. Fall back to the
        // raw message only when nothing matched.
        const classified = classifyGenerationError(message);
        errorMsg =
          classified.messageKey === "generation.toast.failed"
            ? locale.t("generation.error_failed_message", { message })
            : locale.t(classified.messageKey, classified.params);
      }
    } finally {
      if (sequential) finishSubmitRun(runToken);
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

      // Anima models produce poor results below 1024 — clamp to 1024² area
      // preserving aspect ratio. The single-image path does this on the store;
      // the grid path builds params per cell, so mirror it here.
      if (generation.isAnima && (params.width < 1024 || params.height < 1024)) {
        const ratio = params.width / params.height;
        const area = 1024 * 1024;
        params.width = Math.round(Math.sqrt(area * ratio) / 8) * 8;
        params.height = Math.round(Math.sqrt(area / ratio) / 8) * 8;
      }

      // Use shared seed for random seeds so the grid is consistent
      if (params.seed === "-1") {
        params.seed = String(sharedSeed);
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
    title={generateButtonTitle}
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
