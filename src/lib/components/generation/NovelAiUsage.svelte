<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";

  // The balance only means something while NovelAI is the backend, and a
  // configured key alone is not that: the checkpoint picker decides.
  const show = $derived(generation.isNovelAi && novelai.apiKeyConfigured);

  // Fetch once per key rather than on every mount: this sits above the generate
  // button, so it remounts every time the generation page is rebuilt.
  $effect(() => {
    if (show) void novelai.ensureSubscription();
  });

  // Anlas is spent the moment a NovelAI generation runs, so a balance that is
  // never re-read is a wrong balance. Refresh on the busy-to-idle edge.
  let wasGenerating = false;
  $effect(() => {
    const busy = progress.isGenerating;
    if (wasGenerating && !busy && generation.isNovelAi) void novelai.refreshSubscription();
    wasGenerating = busy;
  });

  const allowance = $derived(novelai.opusAllowance);
</script>

{#if show}
  <div class="mb-2 rounded-lg border border-neutral-800 bg-neutral-900/60 px-2.5 py-1.5 space-y-1">
    <div class="flex items-center justify-between gap-2">
      <span class="text-[10px] text-neutral-400">{locale.t('settings.novelai.anlas')}</span>
      <div class="flex items-center gap-1.5">
        <span class="text-xs font-mono text-neutral-200">
          {novelai.subscription ? novelai.anlas.toLocaleString() : '--'}
        </span>
        <button
          class="p-0.5 rounded text-neutral-500 hover:text-neutral-200 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          disabled={novelai.subscriptionLoading}
          title={locale.t('settings.novelai.refresh_account')}
          aria-label={locale.t('settings.novelai.refresh_account')}
          onclick={() => { void novelai.refreshSubscription(); }}
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 {novelai.subscriptionLoading ? 'animate-spin' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="23 4 23 10 17 10"/>
            <polyline points="1 20 1 14 7 14"/>
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/>
          </svg>
        </button>
      </div>
    </div>

    {#if novelai.subscriptionError}
      <p class="text-[10px] text-red-400">{novelai.subscriptionError}</p>
    {:else if allowance}
      <div class="h-1 w-full rounded-full bg-neutral-800 overflow-hidden">
        <div
          class="h-full rounded-full transition-all {allowance.isLow ? 'bg-amber-500' : 'bg-teal-500'}"
          style="width: {allowance.percent}%"
        ></div>
      </div>
      <p class="text-[10px] {allowance.isLow ? 'text-amber-400' : 'text-neutral-500'}">
        {#if allowance.isEmpty}
          {locale.t('settings.novelai.allowance_empty')}
        {:else}
          {locale.t('settings.novelai.allowance_remaining', {
            percent: allowance.percent,
            images: allowance.approxImages.toLocaleString(),
          })}
        {/if}
      </p>
    {/if}
  </div>
{/if}
