<script lang="ts">
  import { progress } from "../../stores/progress.svelte.js";
</script>

{#if progress.isGenerating}
  <div class="space-y-2">
    <div class="flex items-center justify-between text-xs text-neutral-400">
      <span>
        {#if progress.totalSteps > 0}
          Step {progress.currentStep} / {progress.totalSteps}
        {:else}
          Preparing...
        {/if}
      </span>
      <span>{Math.round(progress.percentage)}%</span>
    </div>
    <div class="w-full h-2 bg-neutral-800 rounded-full overflow-hidden">
      <div
        class="h-full bg-indigo-500 rounded-full transition-all duration-200"
        style="width: {progress.percentage}%"
      ></div>
    </div>
  </div>
{/if}
