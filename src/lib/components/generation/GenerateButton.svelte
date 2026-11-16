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
  import NovelAiUsage from "./NovelAiUsage.svelte";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { estimateCurrentNovelAiCost } from "../../utils/novelaiCurrentCost.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import { isBrowserMode } from "../../utils/ipc.js";
  import type { GenerationParams } from "../../types/index.js";
  import { runRegionalInpaintChain } from "../../utils/regionalInpaintChain.js";
  import {
    suppressRegionalChainGallerySave,
    clearAllRegionalChainGallerySuppress,
  } from "../../utils/regionalChainGallery.js";
  import { checkStyleTransferNodesReady } from "../../utils/styleTransferNodes.js";
  import { ensureUpscalerInstalled } from "../../utils/upscalerInstall.js";
  import { classifyGenerationError } from "../../utils/generationErrors.js";
  import { parseSegmentDetailPrompt, yoloTargetFilename } from "../../utils/promptSegmentDetail.js";
  import { requestGeneration, trackGeneration, submitGeneration } from "../../utils/generationSubmit.js";
  import { beginPausedEdit, uploadPausedEditImage } from "../../utils/pausedEdit.js";
  import { queue } from "../../stores/queue.svelte.js";
  import QueuePanel from "../ui/QueuePanel.svelte";

  interface Props {
    canvasEditorRef?: { getRasterComposite: () => HTMLCanvasElement | null; getMaskCanvas: () => HTMLCanvasElement | null };
    /** Phone dock: tighter gaps so Generate keeps its width beside the panel buttons. */
    compact?: boolean;
  }

  let { canvasEditorRef, compact = false }: Props = $props();
  let errorMsg = $state<string | null>(null);
  let isSubmitting = $state(false);
  let orderedRunPromptIds = $state<string[]>([]);
  let orderedRunCancelRequested = $state(false);
  let regionalChainCancelRequested = $state(false);
  let submitRunToken = 0;
  let orderedRunToken = 0;
  let regionalChainToken = 0;
  const orderedWildcardRun = $derived(promptPresets.orderedWildcardRun);
  const orderedWildcardRunCount = $derived(compare.active && compare.cellCount > 1 ? 0 : (orderedWildcardRun?.count ?? 0));
  const pendingOrderedRunIds = $derived(orderedRunPromptIds.filter((id) => progress.pendingPrompts.some((prompt) => prompt.promptId === id)));

  // Continuous generation: one click runs the normal single-image path again
  // and again, waiting for each image before starting the next.
  const repeatActive = $derived(generation.repeatCount !== 1 && generation.mode !== "video");
  let repeatRunning = $state(false);
  let repeatDone = $state(0);
  let repeatTotal = $state(0);
  let repeatCancelRequested = false;
  let repeatRunToken = 0;
  let repeatMenuOpen = $state(false);
  /** A continuous run gives up after this many failures in a row, even when
   *  set to keep going, so a dead connection or empty Anlas balance does not
   *  loop forever. */
  const REPEAT_MAX_CONSECUTIVE_FAILURES = 3;

  // NovelAI: the last request sent, so an identical paid request (same fixed
  // seed, same everything) asks for a second click instead of spending Anlas
  // on the same image twice.
  let lastNovelAiFingerprint: string | null = null;
  let duplicateWarning = $state(false);

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

  function willRunRegionalInpaintChain(): boolean {
    return (
      generation.supportsRegionalPrompting &&
      generation.effectiveRegionalStrategy === "inpaint_chain" &&
      generation.getValidRegionalSelectionsForInpaint().length > 0
    );
  }

  function isSequentialGenerateRun(): boolean {
    if (compare.active && compare.cellCount > 1) return true;
    if (orderedWildcardRunCount > 1) return true;
    if (repeatActive && !generation.isPaused) return true;
    return willRunRegionalInpaintChain();
  }

  /** The upscaler model this generation will load, or null when none will run.
   *  NovelAI mode upscales only through the local pass, which needs a local model. */
  function upscalerModelInUse(): string | null {
    if (generation.mode === "video" || !generation.upscaleEnabled) return null;
    if (generation.upscaleMethod !== "model" || !generation.upscaleModel) return null;
    if (
      generation.isNovelAi &&
      !(generation.novelaiSettings.local_post_process && generation.novelaiSettings.local_checkpoint)
    ) {
      return null;
    }
    return generation.upscaleModel;
  }

  async function handleEditPausedImage() {
    errorMsg = null;
    try {
      if (!(await beginPausedEdit())) {
        gallery.showToast(locale.t("generation.pause.edit_unavailable"), "error");
      }
    } catch (e) {
      console.error("Edit paused image failed:", e);
      gallery.showToast(locale.t("gallery.toast.failed_load"), "error");
    }
  }

  async function handleGenerate() {
    // Keyboard shortcuts reach this handler even when the button is disabled.
    if (generation.mode === "video" && !generation.canGenerate) return;

    const sequential = isSequentialGenerateRun();
    if (sequential && isSubmitting) return;
    const runToken = ++submitRunToken;
    if (sequential) isSubmitting = true;
    errorMsg = null;

    if (generation.mode !== "video" && !generation.checkpoint) {
      errorMsg = locale.t('generation.error_no_checkpoint');
      if (sequential) finishSubmitRun(runToken);
      return;
    }

    // Check style transfer only when it will actually be sent: an armed or
    // paused run clears it in toParams(), except the regional inpaint chain,
    // which builds its params outside the paused run and keeps it on the base
    // pass (a paused run is continued instead, so the chain never runs then).
    const styleTransferWillRun =
      generation.styleTransferEnabled &&
      (!generation.pauseResumeActive || (!generation.isPaused && willRunRegionalInpaintChain()));
    if (styleTransferWillRun) {
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

    const upscaler = upscalerModelInUse();
    if (upscaler && !(await ensureUpscalerInstalled(upscaler))) {
      if (sequential) finishSubmitRun(runToken);
      return;
    }

    try {
      // Continuing a paused run: the remaining steps sample from the paused
      // latent with the current prompt, CFG, sampler and LoRAs. Grid, ordered
      // wildcard and regional chains all start fresh images, so they do not
      // apply here.
      if (generation.resumeAppliesToMode && generation.isPaused && generation.mode === "txt2img") {
        generation.saveCurrentPromptToHistory();
        await submitGeneration(generation.toParams());
        generation.saveSettings();
        return;
      }

      // Continuing a paused run with a painted correction: the inpaint canvas
      // holds the paused preview, the user's paint and a mask. The paint is
      // composited onto the preview and blended into the paused latent under
      // the mask before the remaining steps run.
      if (generation.resumeAppliesToMode && generation.isPaused && generation.pausedEditArmed && generation.mode === "inpainting") {
        if (!canvasEditorRef) {
          throw new Error(locale.t("canvas.editor_not_ready"));
        }
        await canvas.syncToGeneration(
          () => canvasEditorRef.getRasterComposite(),
          () => canvasEditorRef.getMaskCanvas()
        );
        if (!generation.maskImage) {
          errorMsg = locale.t("generation.pause.edit_needs_mask");
          return;
        }
        const editImage = await uploadPausedEditImage(canvasEditorRef.getRasterComposite());
        if (!editImage) {
          errorMsg = locale.t("generation.error_no_image");
          return;
        }
        generation.saveCurrentPromptToHistory();
        await submitGeneration(
          generation.toParams({
            overrides: {
              mode: "txt2img",
              resume_edit_image: editImage,
              resume_edit_mask: generation.maskImage,
            },
          }),
        );
        generation.saveSettings();
        // Back to the text-to-image view; the paused run stays until discarded.
        generation.pausedEditArmed = false;
        generation.mode = "txt2img";
        canvas.isCanvasMode = false;
        return;
      }

      // If compare grid has multiple cells, generate all cells
      if (compare.active && compare.cellCount > 1) {
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

      // Anima models produce poor results below 1024, so clamp to a 1024² area
      // preserving the aspect ratio. Lock resolution means the user chose this
      // size on purpose, so a locked size is generated as is.
      if (
        generation.isAnima &&
        !generation.resolutionLocked &&
        (generation.width < 1024 || generation.height < 1024)
      ) {
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
      } else if (repeatActive) {
        await runRepeat();
      } else {
        const params = generation.toParams();
        const sentRegions = params.positive_regions?.length ?? 0;
        if (sentRegions > 0) {
          console.log("[regional] Sending", sentRegions, "region(s) via conditioning");
        }
        let fingerprint: string | null = null;
        if (generation.isNovelAi && params.seed !== "-1") {
          fingerprint = JSON.stringify(params);
          if (fingerprint === lastNovelAiFingerprint && !duplicateWarning) {
            duplicateWarning = true;
            return;
          }
        }
        duplicateWarning = false;
        await submitGeneration(params);
        lastNovelAiFingerprint = fingerprint;
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

  /** Resolves after `ms`, or early with false once `stopped()` turns true. */
  function waitOrStop(ms: number, stopped: () => boolean): Promise<boolean> {
    return new Promise((resolve) => {
      const end = Date.now() + ms;
      const tick = () => {
        if (stopped()) return resolve(false);
        if (Date.now() >= end) return resolve(true);
        setTimeout(tick, Math.min(250, end - Date.now()));
      };
      tick();
    });
  }

  /** Waits until `promptId` leaves the queue. */
  async function waitForPrompt(promptId: string, stopped: () => boolean): Promise<"done" | "failed" | "stopped"> {
    while (progress.pendingPrompts.some((p) => p.promptId === promptId)) {
      if (!(await waitOrStop(250, stopped))) return "stopped";
    }
    return progress.takeFailed(promptId) ? "failed" : "done";
  }

  async function runRepeat() {
    const token = ++repeatRunToken;
    const stopped = () => repeatCancelRequested || token !== repeatRunToken;
    repeatCancelRequested = false;
    repeatTotal = generation.repeatCount;
    repeatDone = 0;
    repeatRunning = true;
    let failuresInARow = 0;
    try {
      while (!stopped() && (repeatTotal === 0 || repeatDone < repeatTotal)) {
        if (repeatDone > 0) {
          // A failure waits at least a few seconds, so a rate limit has room to clear.
          const delayMs = Math.max(generation.repeatDelaySec * 1000, failuresInARow > 0 ? 3000 : 0);
          if (delayMs > 0 && !(await waitOrStop(delayMs, stopped))) break;
        }
        let outcome: "done" | "failed" | "stopped";
        try {
          // Fresh params each run, so random seeds and wildcards reroll.
          const promptId = await submitGeneration(generation.toParams());
          outcome = await waitForPrompt(promptId, stopped);
        } catch (e) {
          if (stopped()) break;
          console.error("[repeat] run failed to start:", e);
          const message = e instanceof Error ? e.message : String(e);
          errorMsg = locale.t("generation.error_failed_message", { message });
          outcome = "failed";
        }
        if (outcome === "stopped") break;
        repeatDone++;
        if (outcome === "failed") {
          failuresInARow++;
          if (generation.repeatStopOnError || failuresInARow >= REPEAT_MAX_CONSECUTIVE_FAILURES) break;
        } else {
          failuresInARow = 0;
        }
      }
    } finally {
      if (token === repeatRunToken) repeatRunning = false;
    }
  }

  function stopRepeat() {
    repeatCancelRequested = true;
    repeatRunToken++;
    repeatRunning = false;
  }

  function setRepeatCount(value: number) {
    generation.repeatCount = Number.isFinite(value) ? Math.max(1, Math.min(999, Math.round(value))) : 1;
    generation.saveSettings();
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
    if (repeatRunning) stopRepeat();
    if (pendingOrderedRunIds.length > 0) {
      await handleSkipOrderedPrompt();
      return;
    }
    const promptId = progress.activePromptId ?? progress.pendingPrompts[0]?.promptId;
    // Stopping a continuous run during its pause between images has nothing to interrupt.
    if (!promptId && !progress.isGenerating) return;
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
    stopRepeat();
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

  const canGenerate = $derived(generation.canGenerate);

  /**
   * Anlas the pending request is expected to cost, or null outside NovelAI mode.
   *
   * Computed here rather than in the generation store because the estimate
   * needs the Opus flag from the NovelAI store, and the hub store may not
   * import a feature store. The number is an estimate, hence the leading `~`.
   */
  // The estimate needs the Opus flag, and the account record it comes from is
  // only fetched on demand. The usage readout fetches it too, but that readout
  // sits behind a display toggle, so without this the badge quotes the full
  // price of a generation Opus covers whenever the toggle is off.
  $effect(() => {
    if (generation.isNovelAi && novelai.apiKeyConfigured) void novelai.ensureSubscription();
  });

  const anlasEstimate = $derived(estimateCurrentNovelAiCost());

  /**
   * Ctrl+Enter generates -- but only when nothing is sitting on top of the
   * panel.
   *
   * The listener is on the window, so it hears the press even while a modal
   * has focus, and a modal's own Ctrl+Enter (confirm the enhance, run the
   * tool) used to fire a background generation alongside it.  Two guards,
   * because they catch different cases: `defaultPrevented` covers a handler
   * closer to the target that already claimed the press, and the overlay
   * check covers a modal whose own listener is on the window too, where which
   * of the two runs first is decided by mount order rather than by what is on
   * top.
   *
   * `[data-modal-open]` is the opt-in: a modal that owns the keyboard while it
   * is up marks its overlay with it.
   */
  function handleKeydown(e: KeyboardEvent) {
    if (e.key !== "Enter" || !(e.ctrlKey || e.metaKey)) return;
    if (e.defaultPrevented) return;
    if (document.querySelector("[data-modal-open]")) return;
    e.preventDefault();
    handleGenerate();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if generation.showNovelaiUsage}
  <NovelAiUsage />
{/if}

<div class="relative flex items-center {compact ? 'gap-1.5' : 'gap-2'}">
  {#if !compact && generation.mode !== "video"}
    <!-- Batch size, the same value as the Sampler section's Batch slider. -->
    <div class="flex h-11 shrink-0 items-center rounded-xl border border-neutral-700" role="group" aria-label={locale.t('generation.sampler.batch')} title={locale.t('generation.sampler.batch')}>
      <button
        type="button"
        class="flex h-full w-7 items-center justify-center rounded-l-xl text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-100 disabled:opacity-30 disabled:hover:bg-transparent"
        aria-label={locale.t('generation.batch_fewer')}
        disabled={generation.batchSize <= 1}
        onclick={() => (generation.batchSize = Math.max(1, generation.batchSize - 1))}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><path d="M5 12h14"/></svg>
      </button>
      <span class="w-5 text-center font-mono text-sm tabular-nums text-neutral-100">{generation.batchSize}</span>
      <button
        type="button"
        class="flex h-full w-7 items-center justify-center rounded-r-xl text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-100 disabled:opacity-30 disabled:hover:bg-transparent"
        aria-label={locale.t('generation.batch_more')}
        disabled={generation.batchSize >= 8}
        onclick={() => (generation.batchSize = Math.min(8, generation.batchSize + 1))}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><path d="M5 12h14"/><path d="M12 5v14"/></svg>
      </button>
    </div>

    <!-- Continuous generation: how many runs one click starts. -->
    <div class="relative shrink-0">
      <button
        type="button"
        onclick={() => (repeatMenuOpen = !repeatMenuOpen)}
        class="flex h-11 items-center gap-1 rounded-xl border px-2 text-sm transition-colors
          {repeatActive
            ? 'border-indigo-500/60 bg-indigo-500/10 text-indigo-200 hover:bg-indigo-500/20'
            : 'border-neutral-700 text-neutral-400 hover:bg-neutral-800 hover:text-neutral-100'}"
        title={locale.t('generation.repeat.title')}
        aria-label={locale.t('generation.repeat.title')}
        aria-expanded={repeatMenuOpen}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/></svg>
        {#if repeatActive}
          <span class="font-mono text-xs tabular-nums">{generation.repeatCount === 0 ? "∞" : `×${generation.repeatCount}`}</span>
        {/if}
      </button>

      {#if repeatMenuOpen}
        <!-- portal-exempt: click-catcher for the anchored menu below; it must stay its sibling. -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="fixed inset-0 z-40"
          onmousedown={() => (repeatMenuOpen = false)}
          onkeydown={(e) => { if (e.key === "Escape") repeatMenuOpen = false; }}
        ></div>
        <div
          class="absolute bottom-full left-0 z-50 mb-2 w-60 space-y-3 rounded-xl border border-neutral-700 bg-neutral-900 p-3 shadow-2xl"
          role="dialog"
          aria-label={locale.t('generation.repeat.title')}
        >
          <div class="flex items-center justify-between gap-2">
            <label class="text-xs text-neutral-300" for="repeat-count">{locale.t('generation.repeat.runs')}</label>
            <input
              id="repeat-count"
              type="number"
              min="1"
              max="999"
              disabled={generation.repeatCount === 0}
              value={generation.repeatCount === 0 ? "" : generation.repeatCount}
              onchange={(e) => setRepeatCount(Number((e.target as HTMLInputElement).value))}
              class="w-20 rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1 text-right text-xs text-neutral-100 disabled:opacity-40"
            />
          </div>
          <label class="flex cursor-pointer select-none items-center gap-2 text-xs text-neutral-300">
            <input
              type="checkbox"
              class="h-3.5 w-3.5 rounded accent-indigo-500"
              checked={generation.repeatCount === 0}
              onchange={(e) => {
                generation.repeatCount = (e.target as HTMLInputElement).checked ? 0 : 1;
                generation.saveSettings();
              }}
            />
            {locale.t('generation.repeat.until_stopped')}
          </label>
          <div class="flex items-center justify-between gap-2">
            <label class="text-xs text-neutral-300" for="repeat-delay">{locale.t('generation.repeat.delay')}</label>
            <input
              id="repeat-delay"
              type="number"
              min="0"
              max="3600"
              step="1"
              value={generation.repeatDelaySec}
              onchange={(e) => {
                const v = Number((e.target as HTMLInputElement).value);
                generation.repeatDelaySec = Number.isFinite(v) ? Math.max(0, Math.min(3600, v)) : 0;
                generation.saveSettings();
              }}
              class="w-20 rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1 text-right text-xs text-neutral-100"
            />
          </div>
          <label class="flex cursor-pointer select-none items-center gap-2 text-xs text-neutral-300" title={locale.t('generation.repeat.stop_on_error_tip')}>
            <input
              type="checkbox"
              class="h-3.5 w-3.5 rounded accent-indigo-500"
              checked={generation.repeatStopOnError}
              onchange={(e) => {
                generation.repeatStopOnError = (e.target as HTMLInputElement).checked;
                generation.saveSettings();
              }}
            />
            {locale.t('generation.repeat.stop_on_error')}
          </label>
          {#if repeatActive}
            <button
              type="button"
              class="text-[11px] text-neutral-400 underline underline-offset-2 hover:text-neutral-100"
              onclick={() => setRepeatCount(1)}
            >
              {locale.t('generation.repeat.off')}
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if !compact && (progress.isGenerating || repeatRunning)}
    <!-- Stop: click skips the current image, right-click cancels everything. -->
    <button
      onclick={handleCancelCurrent}
      oncontextmenu={handleCancelAll}
      class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-red-700 text-white shadow-lg shadow-red-700/20 transition-colors hover:bg-red-600"
      title={locale.t('generation.cancel_hint')}
      aria-label={locale.t('generation.cancel_hint')}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>
    </button>
  {/if}

  <button
    onclick={handleGenerate}
    disabled={!canGenerate}
    title={generateButtonTitle}
    class="min-w-0 flex-1 rounded-xl transition-colors {compact ? 'py-3 px-2 truncate font-semibold text-sm' : 'h-11 px-3 font-bold text-[15px]'}
      {canGenerate
        ? 'bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg shadow-indigo-600/20'
        : 'bg-neutral-800 text-neutral-500 cursor-not-allowed'}"
  >
    {#if repeatRunning}
      {locale.t('generation.repeat.running', {
        current: String(Math.min(repeatDone + 1, repeatTotal || Infinity)),
        total: repeatTotal === 0 ? "∞" : String(repeatTotal),
      })}
    {:else if progress.queueCount > 0}
      {locale.t('generation.generate_queue', { count: progress.queueCount })}
    {:else if generation.resumeAppliesToMode && generation.isPaused && generation.mode === "txt2img"}
      {locale.t('generation.pause.continue', { step: String(generation.pausedEndStep), total: String(generation.steps) })}
    {:else if generation.resumeAppliesToMode && generation.isPaused && generation.pausedEditArmed && generation.mode === "inpainting"}
      {locale.t('generation.pause.continue_edit', { step: String(generation.pausedEndStep), total: String(generation.steps) })}
    {:else if orderedWildcardRunCount > 1}
      {locale.t('generation.generate_ordered', { count: orderedWildcardRunCount })}
    {:else}
      {locale.t('generation.generate')}
    {/if}
    {#if anlasEstimate !== null}
      <span class="ml-2 rounded-md bg-black/15 px-1.5 py-0.5 align-middle text-[11px] font-semibold" title={locale.t('generation.novelai.cost_tip')}>
        {locale.t('generation.novelai.cost_badge', { anlas: anlasEstimate })}
      </span>
    {/if}
  </button>

  {#if progress.queueCount > 0}
    <button
      onclick={() => queue.togglePanel()}
      title={locale.t("queue.panel.title")}
      aria-label={locale.t("queue.panel.title")}
      class="{compact ? 'py-3' : 'h-11'} px-3 rounded-xl font-semibold text-sm bg-neutral-800 hover:bg-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors relative"
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/>
        <line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/>
      </svg>
      {#if progress.queueCount > 1}
        <span class="absolute -top-1.5 -right-1.5 min-w-4 h-4 rounded-full bg-indigo-600 text-[9px] font-bold text-white flex items-center justify-center px-0.5 pointer-events-none">
          {progress.queueCount}
        </span>
      {/if}
    </button>
  {/if}

  {#if compact}
    {#if progress.isGenerating || repeatRunning}
      <button
        onclick={handleCancelCurrent}
        oncontextmenu={handleCancelAll}
        class="{compact ? 'px-3.5' : 'px-5'} py-3 rounded-xl font-semibold text-sm bg-red-700 hover:bg-red-600 text-white transition-colors"
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
        class="{compact ? 'px-3.5' : 'px-5'} py-3 rounded-xl font-semibold text-sm bg-neutral-800 text-neutral-600 cursor-not-allowed transition-colors"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>
    {/if}
  {/if}
</div>

{#if generation.isPaused && !generation.isNovelAi}
  <div class="mt-2 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[11px] text-amber-200">
    <p class="font-medium">
      {locale.t('generation.pause.banner', { step: String(generation.pausedEndStep), total: String(generation.steps) })}
    </p>
    <p class="mt-0.5 text-amber-200/70">{locale.t('generation.pause.locked_hint')}</p>
    <p class="mt-0.5 text-amber-200/70">{locale.t('generation.pause.retry_hint')}</p>
    {#if generation.pausedEditArmed && generation.mode === "inpainting"}
      <p class="mt-0.5 text-amber-100">{locale.t('generation.pause.edit_hint')}</p>
    {/if}
    <div class="mt-1 flex flex-wrap gap-3">
      {#if !(generation.pausedEditArmed && generation.mode === "inpainting")}
        <button
          type="button"
          onclick={handleEditPausedImage}
          class="underline underline-offset-2 hover:text-white transition-colors"
          title={locale.t('generation.pause.edit_tip')}
        >
          {locale.t('generation.pause.edit')}
        </button>
      {/if}
      <button
        type="button"
        onclick={() => generation.discardPausedRun()}
        class="underline underline-offset-2 hover:text-white transition-colors"
        title={locale.t('generation.pause.discard_tip')}
      >
        {locale.t('generation.pause.discard')}
      </button>
    </div>
  </div>
{/if}

{#if duplicateWarning}
  <p class="text-xs text-amber-300 text-center mt-1">{locale.t('generation.duplicate_warning')}</p>
{/if}

{#if errorMsg}
  <p class="text-xs text-red-400 text-center mt-1">{errorMsg}</p>
{/if}
