<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
</script>

<div class="space-y-3">
  <div>
    <label class="block text-xs text-neutral-400 mb-1">Positive Prompt</label>
    {#if generation.isAnima}
      <div class="text-[10px] text-amber-400/80 mb-1">Tip: Start artist tags with @ (e.g. @artist_name)</div>
    {/if}
    <textarea
      bind:value={generation.positivePrompt}
      placeholder={generation.isAnima ? "1girl, long hair, @artist_name, ..." : "A beautiful landscape, masterpiece, best quality..."}
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 resize-y focus:outline-none focus:border-indigo-500 transition-colors min-h-25"
      rows="4"
    ></textarea>
  </div>

  <div>
    <label class="block text-xs text-neutral-400 mb-1">Negative Prompt</label>
    <textarea
      bind:value={generation.negativePrompt}
      placeholder="lowres, bad anatomy, worst quality..."
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 resize-y focus:outline-none focus:border-indigo-500 transition-colors min-h-18"
      rows="3"
    ></textarea>
  </div>
</div>
