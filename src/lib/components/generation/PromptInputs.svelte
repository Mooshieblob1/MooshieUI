<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import PromptTextarea from "./PromptTextarea.svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  const sortedPromptHistory = $derived(
    [...generation.promptHistory].sort((a, b) => {
      if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
      return b.createdAt - a.createdAt;
    }).slice(0, 12)
  );

  function historyLabel(ts: number): string {
    return new Date(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
</script>

<div class="space-y-3">
  {#if generation.stylePresetsEnabled}
    <div>
      <label class="block text-xs text-neutral-400 mb-1">Style Preset<InfoTip text="Fooocus-style presets that automatically inject prompt modifiers. Great for fast starts: pick a style first, then write your subject prompt." /></label>
      <select
        bind:value={generation.stylePreset}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each generation.stylePresetOptions as preset}
          <option value={preset.id}>{preset.label}</option>
        {/each}
      </select>
    </div>
  {/if}

  {#if generation.isAnima}
    <div class="flex justify-end">
      <span class="shrink-0 text-[10px] px-2 py-0.5 rounded-full bg-emerald-600/20 text-emerald-400 border border-emerald-600/30">
        Quality prompts applied
      </span>
    </div>
  {/if}

  <div>
    <label class="block text-xs text-neutral-400 mb-1">Positive Prompt<InfoTip text="Describe what you want to see in the image. Use commas to separate concepts. More specific prompts give better results — include style, subject, lighting, and quality tags." /></label>
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
    <label class="block text-xs text-neutral-400 mb-1">Negative Prompt<InfoTip text="Describe what you don't want in the image. Common negatives include 'lowres', 'bad anatomy', 'blurry', 'worst quality'. Helps steer the AI away from common artifacts." /></label>
    <PromptTextarea
      bind:value={generation.negativePrompt}
      placeholder="lowres, bad anatomy, worst quality..."
      rows={3}
      minHeight="min-h-18"
    />
  </div>

  {#if sortedPromptHistory.length > 0}
    <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-2.5 space-y-2">
      <div class="flex items-center justify-between">
        <label class="text-xs text-neutral-400">Prompt History & Favorites<InfoTip text="Recent prompts are auto-saved when you generate. Click to reload, star favorites to pin them to the top." /></label>
      </div>
      <div class="space-y-1.5 max-h-56 overflow-y-auto pr-1">
        {#each sortedPromptHistory as entry}
          <div class="rounded border border-neutral-800 bg-neutral-900/80 p-2">
            <button
              class="w-full text-left"
              onclick={() => generation.applyPromptHistoryEntry(entry.id)}
              title="Load prompt"
            >
              <p class="text-[11px] text-neutral-200 max-h-8 overflow-hidden">{entry.positivePrompt || "(empty positive prompt)"}</p>
              {#if entry.negativePrompt}
                <p class="text-[10px] text-neutral-500 mt-0.5 whitespace-nowrap overflow-hidden text-ellipsis">Negative: {entry.negativePrompt}</p>
              {/if}
            </button>
            <div class="mt-1.5 flex items-center justify-between gap-2">
              <span class="text-[10px] text-neutral-500">{historyLabel(entry.createdAt)}</span>
              <div class="flex items-center gap-1">
                <button
                  class="px-1.5 py-0.5 text-[10px] rounded border transition-colors {entry.favorite ? 'border-amber-500 text-amber-300 bg-amber-500/10' : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-300'}"
                  onclick={() => generation.togglePromptFavorite(entry.id)}
                  title={entry.favorite ? "Unfavorite" : "Favorite"}
                >
                  ★
                </button>
                <button
                  class="px-1.5 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors"
                  onclick={() => generation.removePromptHistoryEntry(entry.id)}
                  title="Remove"
                >
                  Remove
                </button>
              </div>
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>
