<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import PromptTextarea from "./PromptTextarea.svelte";
</script>

<div class="space-y-3">
  {#if generation.isAnima}
    <div class="flex justify-end">
      <span class="shrink-0 text-[10px] px-2 py-0.5 rounded-full bg-emerald-600/20 text-emerald-400 border border-emerald-600/30">
        Quality prompts applied
      </span>
    </div>
  {/if}

  <div>
    <label class="block text-xs text-neutral-400 mb-1">Positive Prompt</label>
    {#if generation.isAnima}
      <div class="text-[10px] text-amber-400/80 mb-1">Tip: Start artist tags with @ (e.g. @artist_name)</div>
    {/if}
    <PromptTextarea
      bind:value={generation.positivePrompt}
      placeholder={generation.isAnima ? "1girl, long hair, @artist_name, ..." : "A beautiful landscape, masterpiece, best quality..."}
      rows={4}
      minHeight="min-h-25"
    />
  </div>

  <div>
    <label class="block text-xs text-neutral-400 mb-1">Negative Prompt</label>
    <PromptTextarea
      bind:value={generation.negativePrompt}
      placeholder="lowres, bad anatomy, worst quality..."
      rows={3}
      minHeight="min-h-18"
    />
  </div>
</div>
