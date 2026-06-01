<script lang="ts">
  import { generation, DEFAULT_ANIMA_POSITIVE_QUALITY, DEFAULT_ANIMA_NEGATIVE_QUALITY, DEFAULT_ILLUSTRIOUS_POSITIVE_QUALITY, DEFAULT_ILLUSTRIOUS_NEGATIVE_QUALITY, DEFAULT_PONY_POSITIVE_QUALITY, DEFAULT_PONY_NEGATIVE_QUALITY, DEFAULT_NANOSAUR_POSITIVE_QUALITY, DEFAULT_NANOSAUR_NEGATIVE_QUALITY } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    showWarning?: boolean;
  }

  let { showWarning = true }: Props = $props();

  function resetDefaults() {
    generation.customAnimaPositiveQuality = DEFAULT_ANIMA_POSITIVE_QUALITY;
    generation.customAnimaNegativeQuality = DEFAULT_ANIMA_NEGATIVE_QUALITY;
    generation.customIllustriousPositiveQuality = DEFAULT_ILLUSTRIOUS_POSITIVE_QUALITY;
    generation.customIllustriousNegativeQuality = DEFAULT_ILLUSTRIOUS_NEGATIVE_QUALITY;
    generation.customPonyPositiveQuality = DEFAULT_PONY_POSITIVE_QUALITY;
    generation.customPonyNegativeQuality = DEFAULT_PONY_NEGATIVE_QUALITY;
    generation.customNanosaurPositiveQuality = DEFAULT_NANOSAUR_POSITIVE_QUALITY;
    generation.customNanosaurNegativeQuality = DEFAULT_NANOSAUR_NEGATIVE_QUALITY;
    generation.saveSettings();
  }
</script>

<div class="rounded-lg border border-amber-500/20 bg-neutral-950/50 p-3 space-y-3">
  {#if showWarning}
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-2">
        <svg class="w-3.5 h-3.5 text-amber-400/80 shrink-0" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
        <p class="text-[10px] text-amber-400/80">{locale.t('settings.performance.quality_tags_warning')}</p>
      </div>
      <button
        type="button"
        onclick={resetDefaults}
        class="text-[10px] text-indigo-400 hover:text-indigo-300 transition-colors cursor-pointer whitespace-nowrap ml-3"
      >
        {locale.t('settings.performance.reset_defaults')}
      </button>
    </div>
  {/if}

  <!-- Anima Tags -->
  <div class="space-y-1.5">
    <p class="text-[10px] text-neutral-500 font-medium uppercase tracking-wide">{locale.t('settings.performance.anima')}</p>
    <div>
      <label for="anima-pos-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.positive')}</label>
      <textarea
        id="anima-pos-quality"
        bind:value={generation.customAnimaPositiveQuality}
        onblur={() => generation.saveSettings()}
        rows="2"
        class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
        placeholder={locale.t('settings.prompt.positive_example')}
      ></textarea>
    </div>
    <div>
      <label for="anima-neg-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.negative')}</label>
      <textarea
        id="anima-neg-quality"
        bind:value={generation.customAnimaNegativeQuality}
        onblur={() => generation.saveSettings()}
        rows="2"
        class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
        placeholder={locale.t('settings.prompt.negative_example')}
      ></textarea>
    </div>
  </div>

  <!-- Illustrious/NoobAI Tags -->
  <div class="space-y-1.5">
    <p class="text-[10px] text-neutral-500 font-medium uppercase tracking-wide">{locale.t('settings.performance.illustrious')}</p>
    <div>
      <label for="illustrious-pos-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.positive')}</label>
      <textarea
        id="illustrious-pos-quality"
        bind:value={generation.customIllustriousPositiveQuality}
        onblur={() => generation.saveSettings()}
        rows="2"
        class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
        placeholder={locale.t('settings.prompt.positive_example')}
      ></textarea>
    </div>
    <div>
      <label for="illustrious-neg-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.negative')}</label>
      <textarea
        id="illustrious-neg-quality"
        bind:value={generation.customIllustriousNegativeQuality}
        onblur={() => generation.saveSettings()}
        rows="2"
        class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
        placeholder={locale.t('settings.prompt.negative_bad_example')}
      ></textarea>
    </div>
  </div>

  <!-- Pony Tags -->
  <div class="space-y-1.5">
    <p class="text-[10px] text-neutral-500 font-medium uppercase tracking-wide">{locale.t('settings.performance.pony')}</p>
    <div>
      <label for="pony-pos-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.positive')}</label>
      <textarea
        id="pony-pos-quality"
        bind:value={generation.customPonyPositiveQuality}
        onblur={() => generation.saveSettings()}
        rows="2"
        class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
        placeholder={locale.t('settings.prompt.positive_example')}
      ></textarea>
    </div>
    <div>
      <label for="pony-neg-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.negative')}</label>
      <textarea
        id="pony-neg-quality"
        bind:value={generation.customPonyNegativeQuality}
        onblur={() => generation.saveSettings()}
        rows="2"
        class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
        placeholder={locale.t('settings.prompt.negative_example')}
      ></textarea>
    </div>
  </div>

  <!-- Nanosaur Tags -->
  <div class="space-y-1.5">
    <p class="text-[10px] text-neutral-500 font-medium uppercase tracking-wide">{locale.t('settings.performance.nanosaur')}</p>
    <div>
      <label for="nanosaur-pos-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.positive')}</label>
      <textarea
        id="nanosaur-pos-quality"
        bind:value={generation.customNanosaurPositiveQuality}
        onblur={() => generation.saveSettings()}
        rows="2"
        class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
        placeholder={locale.t('settings.prompt.positive_example')}
      ></textarea>
    </div>
    <div>
      <label for="nanosaur-neg-quality" class="text-[10px] text-neutral-500">{locale.t('settings.performance.negative')}</label>
      <textarea
        id="nanosaur-neg-quality"
        bind:value={generation.customNanosaurNegativeQuality}
        onblur={() => generation.saveSettings()}
        rows="2"
        class="w-full mt-0.5 px-2 py-1.5 bg-neutral-900 border border-neutral-700 rounded-lg text-xs text-neutral-200 placeholder:text-neutral-600 focus:outline-none focus:border-indigo-500/50 resize-y"
        placeholder={locale.t('settings.prompt.negative_example')}
      ></textarea>
    </div>
  </div>
</div>
