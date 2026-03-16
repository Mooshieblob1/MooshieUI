<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";

  let posRef: HTMLTextAreaElement;
  let negRef: HTMLTextAreaElement;

  function autoResize(el: HTMLTextAreaElement) {
    el.style.height = "auto";
    el.style.height = el.scrollHeight + "px";
  }
</script>

<div class="space-y-3">
  <div>
    <label class="block text-xs text-neutral-400 mb-1">Positive Prompt</label>
    <textarea
      bind:this={posRef}
      bind:value={generation.positivePrompt}
      oninput={() => autoResize(posRef)}
      placeholder="A beautiful landscape, masterpiece, best quality..."
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 resize-none focus:outline-none focus:border-indigo-500 transition-colors min-h-[72px]"
      rows="3"
    ></textarea>
  </div>

  <div>
    <label class="block text-xs text-neutral-400 mb-1">Negative Prompt</label>
    <textarea
      bind:this={negRef}
      bind:value={generation.negativePrompt}
      oninput={() => autoResize(negRef)}
      placeholder="lowres, bad anatomy, worst quality..."
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 resize-none focus:outline-none focus:border-indigo-500 transition-colors min-h-[56px]"
      rows="2"
    ></textarea>
  </div>
</div>
