<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import FaceDetectorPicker from "./FaceDetectorPicker.svelte";
  import { scrollCapture } from "../../utils/scrollCapture.js";
</script>

<div class="space-y-3">
  <!-- Enable toggle -->
  <div class="flex items-center justify-between">
    <label class="text-xs text-neutral-400">{locale.t('generation.facefix.title')}<InfoTip text={locale.t('generation.facefix.tip')} /></label>
    <button
      class="relative w-10 h-5 rounded-full transition-colors {generation.facefixEnabled
        ? 'bg-indigo-600'
        : 'bg-neutral-700'}"
      onclick={() => (generation.facefixEnabled = !generation.facefixEnabled)}
      role="switch"
      aria-checked={generation.facefixEnabled}
    >
      <span
        class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {generation.facefixEnabled
          ? 'translate-x-5'
          : ''}"
      ></span>
    </button>
  </div>

  {#if generation.facefixEnabled}
    <!-- Detector Model -->
    <FaceDetectorPicker
      value={generation.facefixDetector}
      onchange={(filename) => (generation.facefixDetector = filename)}
    />

    <div class="grid grid-cols-2 gap-3">
      <!-- Denoise -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.facefix.denoise')}<InfoTip text={locale.t('generation.facefix.denoise_tip')} /></span>
          <EditableValue value={generation.facefixDenoise} min={0} max={1} step={0.05} decimals={2} onchange={(v) => generation.facefixDenoise = v} />
        </label>
        <input
          type="range"
          bind:value={generation.facefixDenoise}
          min="0"
          max="1"
          step="0.05"
          class="w-full accent-indigo-500"
        />
      </div>

      <!-- Steps -->
      <div use:scrollCapture>
        <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
          <span>{locale.t('generation.facefix.steps')}<InfoTip text={locale.t('generation.facefix.steps_tip')} /></span>
          <EditableValue value={generation.facefixSteps} min={1} max={50} step={1} onchange={(v) => generation.facefixSteps = v} />
        </label>
        <input
          type="range"
          bind:value={generation.facefixSteps}
          min="1"
          max="50"
          step="1"
          class="w-full accent-indigo-500"
        />
      </div>
    </div>

    <!-- Guide Size -->
    <div use:scrollCapture>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>{locale.t('generation.facefix.guide_size')}<InfoTip text={locale.t('generation.facefix.guide_size_tip')} /></span>
        <EditableValue value={generation.facefixGuideSize} min={256} max={1024} step={64} suffix="px" onchange={(v) => generation.facefixGuideSize = v} />
      </label>
      <input
        type="range"
        bind:value={generation.facefixGuideSize}
        min="256"
        max="1024"
        step="64"
        class="w-full accent-indigo-500"
      />
    </div>

    <!-- Auto face prompt -->
    <div class="flex items-center justify-between">
      <label class="text-xs text-neutral-400">{locale.t('generation.facefix.auto_prompt')}<InfoTip text={locale.t('generation.facefix.auto_prompt_tip')} /></label>
      <button
        class="relative w-10 h-5 rounded-full transition-colors {generation.facefixAutoPrompt
          ? 'bg-indigo-600'
          : 'bg-neutral-700'}"
        onclick={() => (generation.facefixAutoPrompt = !generation.facefixAutoPrompt)}
        role="switch"
        aria-checked={generation.facefixAutoPrompt}
      >
        <span
          class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {generation.facefixAutoPrompt
            ? 'translate-x-5'
            : ''}"
        ></span>
      </button>
    </div>
  {/if}
</div>
