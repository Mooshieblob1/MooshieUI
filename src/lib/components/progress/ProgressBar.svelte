<script lang="ts">
  import { progress } from "../../stores/progress.svelte.js";
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
        class="h-full rounded-full transition-all duration-200 {progress.wasUpscaled && progress.samplingPass >= 2 ? 'bg-emerald-500' : 'bg-indigo-500'}"
        style="width: {progress.percentage}%"
      ></div>
    </div>
  </div>
{/if}
