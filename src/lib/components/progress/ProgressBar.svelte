<script lang="ts">
  import { progress } from "../../stores/progress.svelte.js";
  import { isBrowserMode } from "../../utils/ipc.js";
  import { locale } from "../../stores/locale.svelte.js";
</script>

{#if progress.isGenerating}
  <div class="space-y-2">
    <div class="flex items-center justify-between text-xs text-neutral-400">
      <span>
        {progress.phaseLabel}
        {#if progress.totalSteps > 0}
          <span class="text-neutral-500 ml-1">{progress.currentStep}/{progress.totalSteps}</span>
        {/if}
      </span>
      {#if progress.totalSteps > 0}
        <span>{Math.round(progress.percentage)}%</span>
      {/if}
    </div>
    <div class="w-full h-2 bg-neutral-800 rounded-full overflow-hidden">
      <div
        class="h-full rounded-full transition-[width] duration-200 {progress.wasUpscaled && progress.samplingPass >= 2 ? 'bg-emerald-500' : 'bg-indigo-500'}"
        style="width: {progress.percentage}%"
      ></div>
    </div>
  </div>
{/if}

{#if isBrowserMode && progress.serverProgress !== null && !progress.isGenerating}
  <div class="space-y-1 {progress.isGenerating ? 'mt-2' : ''}">
    <div class="flex items-center justify-between text-xs text-neutral-500">
      <span class="flex items-center gap-1.5">
        <span class="inline-block w-1.5 h-1.5 rounded-full bg-violet-500 animate-pulse"></span>
        {locale.t('progress.server_generating')}
      </span>
      <span>{Math.round((progress.serverProgress.value / progress.serverProgress.max) * 100)}%</span>
    </div>
    <div class="w-full h-1 bg-neutral-800 rounded-full overflow-hidden">
      <div
        class="h-full rounded-full transition-[width] duration-200 bg-violet-500"
        style="width: {(progress.serverProgress.value / progress.serverProgress.max) * 100}%"
      ></div>
    </div>
  </div>
{/if}
