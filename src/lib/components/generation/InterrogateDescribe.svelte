<script lang="ts">
  /**
   * Describe the interrogated image in words: the natural-language half of
   * Interrogate, for Flux, Krea, H3 and the other prose-prompted families
   * that Danbooru tags fit poorly.
   *
   * The image never travels back through the client: the interrogation left it
   * server-side behind `imageToken`. The configured external provider answers
   * when there is one; otherwise a bundled model that can see images does, and
   * with neither this offers to install one.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import type { LlmCatalogEntry } from "../../types/index.js";

  interface Props {
    imageToken: string;
    /** Called after the description went into the prompt. */
    onapplied: () => void;
  }

  let { imageToken, onapplied }: Props = $props();

  let length = $state<"short" | "detailed">("detailed");
  let running = $state(false);
  let text = $state("");
  let error = $state<string | null>(null);
  let needsModel = $state(false);
  let installError = $state<string | null>(null);
  /** Bumped per run, so a reply for an earlier image is dropped. */
  let run = 0;

  // A new image starts over.
  $effect(() => {
    void imageToken;
    run++;
    text = "";
    error = null;
    needsModel = false;
    running = false;
  });

  /** No way to see the image yet: no provider and no bundled vision model. */
  const noRoute = $derived(
    !!promptAssistant.status && !promptAssistant.status.external_enabled && !promptAssistant.hasVisionModel,
  );
  const showInstall = $derived(needsModel || noRoute);

  async function describe() {
    const mine = ++run;
    running = true;
    error = null;
    needsModel = false;
    try {
      const result = await promptAssistant.describe(imageToken, length);
      if (mine !== run) return;
      text = result;
    } catch (e) {
      if (mine !== run) return;
      const msg = e instanceof Error ? e.message : String(e);
      if (msg.includes("no_vision_model")) {
        needsModel = true;
      } else {
        error = mapLlmError(msg);
      }
    } finally {
      if (mine === run) running = false;
    }
  }

  function variantFor(entry: LlmCatalogEntry) {
    const key = promptAssistant.defaultVariantKey(entry.id);
    return entry.variants.find((v) => `gguf:${v.quant}` === key) ?? entry.variants[0];
  }

  async function install(entry: LlmCatalogEntry) {
    const variant = variantFor(entry);
    if (!variant) return;
    installError = null;
    try {
      // Installed for describing only: the owner's assistant model stays put.
      await promptAssistant.download(entry.id, `gguf:${variant.quant}`, false);
      needsModel = false;
    } catch (e) {
      installError = String(e);
    }
  }

  function apply(replace: boolean) {
    const description = text.trim();
    if (!description) return;
    const current = generation.positivePrompt.trim();
    generation.positivePrompt = replace || !current ? description : `${current}\n${description}`;
    onapplied();
  }

  async function copy() {
    try {
      await navigator.clipboard?.writeText(text.trim());
    } catch { /* clipboard unavailable (insecure context); the text stays selectable */ }
  }

  const downloadPercent = $derived.by(() => {
    const p = promptAssistant.downloadProgress;
    return p && p.total > 0 ? Math.round((p.downloaded / p.total) * 100) : 0;
  });
</script>

<div class="rounded-xl border border-neutral-700 bg-neutral-800/40 p-3 space-y-2">
  <div class="flex flex-wrap items-center gap-2">
    <p class="text-sm font-medium text-sky-400 flex-1 min-w-0">{locale.t('generation.interrogate.describe.title')}</p>
    <div class="flex rounded-lg border border-neutral-700 overflow-hidden text-xs" role="group" aria-label={locale.t('generation.interrogate.describe.length')}>
      <button
        onclick={() => { length = "short"; }}
        aria-pressed={length === "short"}
        class="px-2 py-1 transition-colors {length === 'short' ? 'bg-neutral-700 text-neutral-100' : 'text-neutral-400 hover:text-neutral-200'}"
      >{locale.t('generation.interrogate.describe.short')}</button>
      <button
        onclick={() => { length = "detailed"; }}
        aria-pressed={length === "detailed"}
        class="px-2 py-1 transition-colors {length === 'detailed' ? 'bg-neutral-700 text-neutral-100' : 'text-neutral-400 hover:text-neutral-200'}"
      >{locale.t('generation.interrogate.describe.detailed')}</button>
    </div>
    <button
      onclick={describe}
      disabled={running || promptAssistant.isDownloading || (promptAssistant.isGenerating && !running)}
      class="px-3 py-1 text-xs rounded-lg bg-sky-700 hover:bg-sky-600 disabled:opacity-40 disabled:cursor-not-allowed text-white transition-colors"
    >{text ? locale.t('generation.interrogate.describe.again') : locale.t('generation.interrogate.describe.run')}</button>
  </div>
  <p class="text-[10px] text-neutral-500">{locale.t('generation.interrogate.describe.hint')}</p>

  {#if running}
    <div class="flex items-center gap-2 text-xs text-neutral-400">
      <svg class="animate-spin h-3.5 w-3.5 text-sky-400" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.37 0 0 5.37 0 12h4z"></path>
      </svg>
      {promptAssistant.stage === "loading_model"
        ? locale.t('generation.interrogate.describe.loading_model')
        : locale.t('generation.interrogate.describe.running')}
    </div>
  {/if}

  {#if error}
    <p class="text-xs text-red-400 break-words">{error}</p>
  {/if}

  {#if showInstall && !running}
    <div class="space-y-1.5">
      <p class="text-xs text-neutral-400">{locale.t('generation.interrogate.describe.needs_model')}</p>
      {#if promptAssistant.isDownloading}
        <div class="space-y-1">
          <div class="h-1.5 bg-neutral-700 rounded-full overflow-hidden">
            <div class="h-full bg-sky-600 rounded-full transition-all" style="width: {downloadPercent}%"></div>
          </div>
          <p class="text-[10px] text-neutral-500">{locale.t('generation.interrogate.describe.installing', { percent: downloadPercent })}</p>
        </div>
      {:else}
        <div class="flex flex-wrap gap-1.5">
          {#each promptAssistant.visionCatalog as entry (entry.id)}
            {@const variant = variantFor(entry)}
            <button
              onclick={() => install(entry)}
              title={entry.best_for}
              class="px-2 py-1 text-xs rounded-lg border border-sky-600/50 text-sky-300 hover:bg-sky-600/10 transition-colors"
            >{locale.t('generation.interrogate.describe.install', {
              name: entry.name,
              size: ((variant?.size_mb ?? 0) / 1024).toFixed(1),
            })}</button>
          {/each}
        </div>
      {/if}
      {#if installError}
        <p class="text-xs text-red-400 break-words">{installError}</p>
      {/if}
    </div>
  {/if}

  {#if text}
    <textarea
      bind:value={text}
      rows="5"
      aria-label={locale.t('generation.interrogate.describe.title')}
      class="w-full bg-neutral-900 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-sky-500 resize-y"
    ></textarea>
    <div class="flex flex-wrap justify-end gap-2">
      <button
        onclick={copy}
        class="px-3 py-1 text-xs rounded-lg bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
      >{locale.t('common.copy')}</button>
      <button
        onclick={() => apply(false)}
        class="px-3 py-1 text-xs rounded-lg bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
      >{locale.t('generation.interrogate.describe.append')}</button>
      <button
        onclick={() => apply(true)}
        class="px-3 py-1 text-xs rounded-lg bg-sky-700 hover:bg-sky-600 text-white transition-colors"
      >{locale.t('generation.interrogate.describe.replace')}</button>
    </div>
  {/if}
</div>
