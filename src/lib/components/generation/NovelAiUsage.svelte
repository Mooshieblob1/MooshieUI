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
  <!-- One quiet line above Generate: Anlas, then the Opus allowance as a small bar. -->
  <div class="mb-2.5 flex items-center gap-2 text-xs">
    <span class="text-neutral-400">{locale.t('settings.novelai.anlas')}</span>
    <span class="font-mono text-neutral-200">{novelai.subscription ? novelai.anlas.toLocaleString() : '--'}</span>
    {#if allowance && !novelai.subscriptionError}
      {@const allowanceText = allowance.isEmpty
        ? locale.t('settings.novelai.allowance_empty')
        : locale.t('settings.novelai.allowance_remaining', {
            percent: allowance.percent,
            images: allowance.approxImages.toLocaleString(),
          })}
      <span class="text-neutral-600" aria-hidden="true">·</span>
      <span
        class="flex min-w-0 items-center gap-1.5"
        title="{locale.t('settings.novelai.allowance_title')}: {allowanceText}"
        aria-label="{locale.t('settings.novelai.allowance_title')}: {allowanceText}"
      >
        <span class="h-1 w-10 shrink-0 overflow-hidden rounded-full bg-neutral-800">
          <span
            class="block h-full rounded-full transition-all {allowance.isLow ? 'bg-amber-500' : allowance.isBonus ? 'bg-emerald-500' : 'bg-teal-500'}"
            style="width: {allowance.barPercent}%"
          ></span>
        </span>
        <span class="font-mono {allowance.isLow ? 'text-amber-400' : 'text-neutral-300'}">{allowance.percent}%</span>
      </span>
    {/if}
    <button
      class="ml-auto rounded p-1 text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-neutral-200 disabled:cursor-not-allowed disabled:opacity-40"
      disabled={novelai.subscriptionLoading}
      title={locale.t('settings.novelai.refresh_account')}
      aria-label={locale.t('settings.novelai.refresh_account')}
      onclick={() => { void novelai.refreshSubscription(); }}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 {novelai.subscriptionLoading ? 'animate-spin' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="23 4 23 10 17 10"/>
        <polyline points="1 20 1 14 7 14"/>
        <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/>
      </svg>
    </button>
  </div>
  {#if novelai.subscriptionError}
    <p class="-mt-1.5 mb-2 text-[11px] text-red-400">{novelai.subscriptionError}</p>
  {/if}
{/if}
