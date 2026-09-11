<script lang="ts">
  /**
   * The NovelAI face detailer panel.
   *
   * Detection is always local YOLO; the engine dropdown picks who repaints each
   * crop. Under `novelai` the crop is sent back to the same NovelAI model that
   * drew the image, which is the whole point: a local checkpoint paints faces in
   * its own style. Under `local` this panel drives the existing ComfyUI pass,
   * replacing the FaceFix panel that NovelAI mode hides.
   *
   * Deliberately separate from `facefix*` on the generation store: those belong
   * to a panel the user cannot see here, so their persisted values must never
   * reach this one.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { naiV5Variant } from "../../utils/novelaiModels.js";
  import { OPUS_FREE_PIXELS } from "../../utils/novelaiCost.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import FaceDetectorPicker from "./FaceDetectorPicker.svelte";
  import { scrollCapture } from "../../utils/scrollCapture.js";

  const face = $derived(generation.novelaiSettings.face_detail);
  const isNovelAiEngine = $derived(face.detailer_engine !== "local");

  /**
   * The detailer tracks the main steps slider until the user moves this one,
   * so the displayed value is resolved rather than read straight off
   * `face.steps`. Touching either control below pins it and breaks the link.
   */
  const steps = $derived(generation.novelAiFaceDetailSteps);

  /**
   * Whether an Opus account covers this pass at all. V5 draws from the timed
   * allowance instead of Opus unlimited, so an empty battery bills like a
   * non-Opus account does.
   */
  const v5AllowanceEmpty = $derived(
    naiV5Variant(generation.checkpoint) !== null && novelai.opusAllowanceEmpty,
  );
  const billedEveryFace = $derived(!novelai.isOpus);

  /**
   * Crops from a large or upscaled frame outgrow the free window. The face is a
   * fraction of the frame, so this is a "can happen" warning rather than a
   * prediction: only the detector knows how big the faces actually are.
   */
  const cropsMayExceedFree = $derived(
    generation.upscaleEnabled ||
      generation.width * generation.height > OPUS_FREE_PIXELS,
  );

  const localCheckpoint = $derived(generation.novelaiSettings.local_checkpoint);

  function setEngine(engine: string) {
    generation.updateNovelAiFaceDetail({ detailer_engine: engine });
    // The local pass rides the same ComfyUI hand-off as the local upscale, and
    // that hand-off is gated on `local_post_process`. Picking the local engine
    // is the user asking for it; making them find a second checkbox in another
    // panel is not a decision, it is a trap.
    if (engine === "local") {
      generation.updateNovelAiSettings({ local_post_process: true });
    }
  }
</script>

<div class="space-y-3">
  <!-- Enable toggle -->
  <div class="flex items-center justify-between">
    <label class="text-xs text-neutral-400">{locale.t('generation.nai_face_detail.title')}<InfoTip text={locale.t('generation.nai_face_detail.tip')} /></label>
    <button
      class="relative w-10 h-5 rounded-full transition-colors {face.enabled ? 'bg-indigo-600' : 'bg-neutral-700'}"
      onclick={() => generation.updateNovelAiFaceDetail({ enabled: !face.enabled })}
      role="switch"
      aria-checked={face.enabled}
      aria-label={locale.t('generation.nai_face_detail.title')}
    >
      <span
        class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {face.enabled ? 'translate-x-5' : ''}"
      ></span>
    </button>
  </div>

  {#if face.enabled}
    <!-- Engine -->
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.nai_face_detail.engine')}<InfoTip text={locale.t('generation.nai_face_detail.engine_tip')} /></label>
      <select
        value={face.detailer_engine}
        onchange={(e) => setEngine((e.target as HTMLSelectElement).value)}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        <option value="novelai">{locale.t('generation.nai_face_detail.engine_novelai')}</option>
        <option value="local">{locale.t('generation.nai_face_detail.engine_local')}</option>
      </select>
    </div>

    <!-- Detector (shared with the FaceFix panel) -->
    <FaceDetectorPicker
      value={face.detector_model}
      onchange={(filename) => generation.updateNovelAiFaceDetail({ detector_model: filename ?? "" })}
    />

    <div class="grid grid-cols-2 gap-3">
      <!-- Detector confidence -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.nai_face_detail.threshold')}<InfoTip text={locale.t('generation.nai_face_detail.threshold_tip')} /></span>
          <EditableValue value={face.threshold} min={0.1} max={0.9} step={0.05} decimals={2} onchange={(v) => generation.updateNovelAiFaceDetail({ threshold: v })} />
        </label>
        <input
          type="range"
          min="0.1"
          max="0.9"
          step="0.05"
          value={face.threshold}
          oninput={(e) => generation.updateNovelAiFaceDetail({ threshold: Number(e.currentTarget.value) })}
          class="w-full accent-indigo-500"
        />
      </div>

      <!-- Crop padding -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.nai_face_detail.padding')}<InfoTip text={locale.t('generation.nai_face_detail.padding_tip')} /></span>
          <EditableValue value={face.padding} min={1} max={3} step={0.1} decimals={1} suffix="x" onchange={(v) => generation.updateNovelAiFaceDetail({ padding: v })} />
        </label>
        <input
          type="range"
          min="1"
          max="3"
          step="0.1"
          value={face.padding}
          oninput={(e) => generation.updateNovelAiFaceDetail({ padding: Number(e.currentTarget.value) })}
          class="w-full accent-indigo-500"
        />
      </div>

      <!-- Strength -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.nai_face_detail.strength')}<InfoTip text={locale.t('generation.nai_face_detail.strength_tip')} /></span>
          <EditableValue value={face.strength} min={0.05} max={1} step={0.05} decimals={2} onchange={(v) => generation.updateNovelAiFaceDetail({ strength: v })} />
        </label>
        <input
          type="range"
          min="0.05"
          max="1"
          step="0.05"
          value={face.strength}
          oninput={(e) => generation.updateNovelAiFaceDetail({ strength: Number(e.currentTarget.value) })}
          class="w-full accent-indigo-500"
        />
      </div>

      <!-- Steps -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.nai_face_detail.steps')}<InfoTip text={locale.t('generation.nai_face_detail.steps_tip')} /></span>
          <EditableValue value={steps} min={1} max={50} step={1} onchange={(v) => generation.setNovelAiFaceDetailSteps(v)} />
        </label>
        <input
          type="range"
          min="1"
          max="50"
          step="1"
          value={steps}
          oninput={(e) => generation.setNovelAiFaceDetailSteps(Number(e.currentTarget.value))}
          class="w-full accent-indigo-500"
        />
      </div>
    </div>

    {#if isNovelAiEngine && steps > 28 && face.anlas_policy === "fit_free"}
      <p class="text-[11px] text-neutral-500">{locale.t('generation.nai_face_detail.steps_clamped')}</p>
    {/if}

    <!-- Guide size -->
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.nai_face_detail.guide_size')}<InfoTip text={locale.t('generation.nai_face_detail.guide_size_tip')} /></span>
        <EditableValue value={face.guide_size} min={256} max={1024} step={64} suffix="px" onchange={(v) => generation.updateNovelAiFaceDetail({ guide_size: v })} />
      </label>
      <input
        type="range"
        min="256"
        max="1024"
        step="64"
        value={face.guide_size}
        oninput={(e) => generation.updateNovelAiFaceDetail({ guide_size: Number(e.currentTarget.value) })}
        class="w-full accent-indigo-500"
      />
    </div>

    <div class="grid grid-cols-2 gap-3">
      <!-- Max faces -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.nai_face_detail.max_faces')}<InfoTip text={locale.t('generation.nai_face_detail.max_faces_tip')} /></span>
          <EditableValue value={face.max_faces} min={0} max={10} step={1} onchange={(v) => generation.updateNovelAiFaceDetail({ max_faces: v })} />
        </label>
        <input
          type="range"
          min="0"
          max="10"
          step="1"
          value={face.max_faces}
          oninput={(e) => generation.updateNovelAiFaceDetail({ max_faces: Number(e.currentTarget.value) })}
          class="w-full accent-indigo-500"
        />
      </div>

      <!-- Feather -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.nai_face_detail.feather')}<InfoTip text={locale.t('generation.nai_face_detail.feather_tip')} /></span>
          <EditableValue value={face.feather} min={0} max={64} step={1} suffix="px" onchange={(v) => generation.updateNovelAiFaceDetail({ feather: v })} />
        </label>
        <input
          type="range"
          min="0"
          max="64"
          step="1"
          value={face.feather}
          oninput={(e) => generation.updateNovelAiFaceDetail({ feather: Number(e.currentTarget.value) })}
          class="w-full accent-indigo-500"
        />
      </div>
    </div>

    <!-- Face prompt -->
    <div class="border-t border-neutral-800 pt-3 space-y-2">
      <div>
        <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.nai_face_detail.prompt_mode')}<InfoTip text={locale.t('generation.nai_face_detail.prompt_mode_tip')} /></label>
        <select
          value={face.prompt_mode}
          onchange={(e) => generation.updateNovelAiFaceDetail({ prompt_mode: (e.target as HTMLSelectElement).value })}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
        >
          <option value="auto">{locale.t('generation.nai_face_detail.prompt_auto')}</option>
          <option value="generic">{locale.t('generation.nai_face_detail.prompt_generic')}</option>
          <option value="custom">{locale.t('generation.nai_face_detail.prompt_custom')}</option>
        </select>
      </div>

      {#if face.prompt_mode === "custom"}
        <textarea
          rows="2"
          value={face.custom_prompt}
          oninput={(e) => generation.updateNovelAiFaceDetail({ custom_prompt: e.currentTarget.value })}
          placeholder={locale.t('generation.nai_face_detail.custom_prompt_placeholder')}
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors resize-y"
        ></textarea>
      {/if}

      {#if face.prompt_mode === "auto"}
        <div use:scrollCapture>
          <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
            <span>{locale.t('generation.nai_face_detail.tagger_threshold')}<InfoTip text={locale.t('generation.nai_face_detail.tagger_threshold_tip')} /></span>
            <EditableValue value={face.tagger_threshold} min={0.2} max={0.7} step={0.05} decimals={2} onchange={(v) => generation.updateNovelAiFaceDetail({ tagger_threshold: v })} />
          </label>
          <input
            type="range"
            min="0.2"
            max="0.7"
            step="0.05"
            value={face.tagger_threshold}
            oninput={(e) => generation.updateNovelAiFaceDetail({ tagger_threshold: Number(e.currentTarget.value) })}
            class="w-full accent-indigo-500"
          />
        </div>
        {#if isNovelAiEngine}
          <p class="text-[11px] text-neutral-500">{locale.t('generation.nai_face_detail.tagger_note')}</p>
        {:else}
          <p class="text-[11px] text-amber-400/90">{locale.t('generation.nai_face_detail.local_auto_note')}</p>
        {/if}
      {/if}
    </div>

    {#if isNovelAiEngine}
      <!-- Anlas -->
      <div class="border-t border-neutral-800 pt-3 space-y-2">
        <div>
          <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.nai_face_detail.anlas_policy')}<InfoTip text={locale.t('generation.nai_face_detail.anlas_policy_tip')} /></label>
          <select
            value={face.anlas_policy}
            onchange={(e) => generation.updateNovelAiFaceDetail({ anlas_policy: (e.target as HTMLSelectElement).value })}
            class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
          >
            <option value="fit_free">{locale.t('generation.nai_face_detail.policy_fit_free')}</option>
            <option value="allow_paid">{locale.t('generation.nai_face_detail.policy_allow_paid')}</option>
          </select>
        </div>

        {#if face.anlas_policy === "fit_free"}
          <p class="text-[11px] text-neutral-500">{locale.t('generation.nai_face_detail.free_note')}</p>
        {:else}
          <p class="text-[11px] text-amber-400/90">{locale.t('generation.nai_face_detail.paid_warning')}</p>
        {/if}

        {#if cropsMayExceedFree}
          <p class="text-[11px] text-amber-400/90">{locale.t('generation.nai_face_detail.upscale_warning')}</p>
        {/if}

        {#if billedEveryFace}
          <p class="text-[11px] text-amber-400/90">{locale.t('generation.nai_face_detail.non_opus_notice')}</p>
        {:else if v5AllowanceEmpty}
          <p class="text-[11px] text-amber-400/90">{locale.t('generation.nai_face_detail.v5_allowance_notice')}</p>
        {/if}
      </div>
    {:else}
      <!-- The local engine borrows the NovelAI panel's checkpoint rather than
           offering a second picker for the same eight fields. -->
      <div class="border-t border-neutral-800 pt-3 space-y-1">
        <p class="text-xs text-neutral-400">
          {locale.t('generation.nai_face_detail.local_checkpoint')}
          <span class="text-neutral-200">{localCheckpoint ?? locale.t('generation.nai_face_detail.local_checkpoint_none')}</span>
        </p>
        {#if localCheckpoint}
          <p class="text-[11px] text-neutral-500">{locale.t('generation.nai_face_detail.local_checkpoint_hint')}</p>
        {:else}
          <p class="text-[11px] text-amber-400/90">{locale.t('generation.nai_face_detail.local_checkpoint_unset')}</p>
        {/if}
      </div>
    {/if}
  {/if}
</div>
