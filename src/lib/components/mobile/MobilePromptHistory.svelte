<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  const sortedPromptHistory = $derived(
    [...generation.promptHistory].sort((a, b) => {
      if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
      return b.createdAt - a.createdAt;
    }),
  );

  function tt(key: string, fb: string) {
    const v = locale.t(key);
    return v === key ? fb : v;
  }

  function historyLabel(ts: number): string {
    return new Date(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
</script>

<div class="flex flex-col h-full min-h-0">
  <div class="px-3 pb-1.5 pt-0.5 shrink-0 flex items-center justify-between">
    <h3 class="text-[11px] font-semibold uppercase tracking-wide text-neutral-400">
      {tt("bottom_panel.tab.prompts", "Prompt history")}
    </h3>
    <span class="text-[10px] text-neutral-500">{sortedPromptHistory.length}</span>
  </div>
  {#if sortedPromptHistory.length === 0}
    <div class="flex items-center justify-center flex-1 text-neutral-500 text-xs px-4">
      <p>{tt("bottom_panel.no_prompts", "No prompts yet — generate something!")}</p>
    </div>
  {:else}
    <div class="flex-1 min-h-0 overflow-y-auto px-3 pb-3 space-y-2 no-scroll-chain">
      {#each sortedPromptHistory as entry}
        <div
          class="rounded-lg border overflow-hidden transition-colors {entry.favorite
            ? 'border-amber-500/40 bg-amber-500/5'
            : 'border-neutral-800 bg-neutral-900/60'}"
        >
          <button
            type="button"
            class="w-full text-left p-2.5"
            onclick={() => generation.applyPromptHistoryEntry(entry.id)}
          >
            <p class="text-[12px] text-neutral-200 leading-relaxed line-clamp-3">
              {entry.positivePrompt || tt("bottom_panel.empty_prompt", "(empty)")}
            </p>
            {#if entry.negativePrompt}
              <p class="text-[10px] text-neutral-500 mt-1 line-clamp-1">
                {tt("bottom_panel.neg_prefix", "Neg:")} {entry.negativePrompt}
              </p>
            {/if}
          </button>
          <div class="px-2.5 pb-2 flex items-center justify-between gap-2">
            <div class="flex items-center gap-1.5 text-[10px] text-neutral-500">
              <span>{historyLabel(entry.createdAt)}</span>
              <span class="px-1 py-0.5 rounded bg-neutral-800 text-neutral-400">{entry.mode}</span>
            </div>
            <div class="flex items-center gap-1">
              <button
                type="button"
                aria-label={entry.favorite ? "Unfavorite" : "Favorite"}
                class="px-2 py-1 text-[11px] rounded border transition-colors {entry.favorite
                  ? 'border-amber-500 text-amber-300 bg-amber-500/10'
                  : 'border-neutral-700 text-neutral-400'}"
                onclick={(e) => {
                  e.stopPropagation();
                  generation.togglePromptFavorite(entry.id);
                }}
              >★</button>
              <button
                type="button"
                aria-label="Remove"
                class="px-2 py-1 text-[11px] rounded border border-neutral-700 text-neutral-400"
                onclick={(e) => {
                  e.stopPropagation();
                  generation.removePromptHistoryEntry(entry.id);
                }}
              >×</button>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
