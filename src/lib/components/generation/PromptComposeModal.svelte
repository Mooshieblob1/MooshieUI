<script lang="ts">
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  let { onClose }: { onClose: () => void } = $props();

  let description = $state("");
  let length = $state<"short" | "medium" | "detailed">("medium");
  let includeArtists = $state(false);
  let result = $state("");
  let error = $state<string | null>(null);

  const isAnima = $derived(generation.modelFamily === "anima");

  async function generate() {
    if (!description.trim()) return;
    error = null;
    result = "";
    try {
      result = await promptAssistant.compose(description, generation.modelFamily, {
        length,
        include_artists: includeArtists,
      });
      if (!result.trim()) {
        error = locale.t("prompt_assistant.couldnt_compose");
      }
    } catch (e) {
      console.error("Prompt compose failed:", e);
      const msg = String(e);
      if (msg.includes("busy_generation")) {
        error = locale.t("prompt_assistant.busy_generation");
      } else if (msg.includes("no_model")) {
        error = locale.t("prompt_assistant.no_model");
      } else {
        // Surface the real backend reason instead of a generic message so
        // server-side failures are diagnosable from the UI alone.
        const detail = msg.replace(/^Error:\s*/, "").trim();
        error = detail
          ? `${locale.t("prompt_assistant.error_generic")}: ${detail}`
          : locale.t("prompt_assistant.error_generic");
      }
    }
  }

  function replace() {
    generation.positivePrompt = result;
    generation.saveSettings();
    onClose();
  }

  function append() {
    const cur = generation.positivePrompt?.trim();
    generation.positivePrompt = cur ? `${cur}, ${result}` : result;
    generation.saveSettings();
    onClose();
  }
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
  onclick={onClose}
  role="presentation"
>
  <div
    class="max-h-[85vh] w-full max-w-xl overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl"
    onclick={(e) => e.stopPropagation()}
    role="dialog"
    aria-modal="true"
  >
    <div class="mb-3 flex items-center justify-between">
      <h2 class="text-lg font-semibold text-neutral-100">
        {locale.t("prompt_assistant.compose_title")}
      </h2>
      <button
        class="rounded-lg px-2 py-1 text-neutral-400 hover:bg-neutral-800"
        onclick={onClose}
        aria-label={locale.t("common.close")}
      >
        ✕
      </button>
    </div>

    <textarea
      bind:value={description}
      rows="4"
      placeholder={locale.t("prompt_assistant.describe_placeholder")}
      class="w-full rounded-lg border border-neutral-700 bg-neutral-800 p-2 text-sm text-neutral-100"
    ></textarea>

    <div class="mt-2 flex flex-wrap items-center gap-2 text-xs text-neutral-300">
      <span>{locale.t("prompt_assistant.length")}:</span>
      {#each ["short", "medium", "detailed"] as len}
        <button
          class="rounded border px-2 py-0.5 {length === len
            ? 'border-[var(--theme-accent-500)] text-neutral-100'
            : 'border-neutral-600 text-neutral-400'}"
          onclick={() => (length = len as typeof length)}
        >
          {locale.t(`prompt_assistant.length_${len}`)}
        </button>
      {/each}
      {#if isAnima}
        <label class="ml-2 flex items-center gap-1">
          <input type="checkbox" bind:checked={includeArtists} />
          {locale.t("prompt_assistant.include_artists")}
        </label>
      {/if}
    </div>

    {#if error}
      <div class="mt-2 rounded-lg border border-red-700 bg-red-900/30 px-3 py-2 text-sm text-red-300">
        {error}
      </div>
    {/if}

    <button
      class="mt-3 w-full rounded-lg bg-[var(--theme-accent-600)] px-4 py-2 font-medium text-black hover:bg-[var(--theme-accent-500)] disabled:opacity-50"
      disabled={promptAssistant.isGenerating || !description.trim()}
      onclick={generate}
    >
      {#if promptAssistant.isGenerating}
        <span class="inline-block animate-spin">⟳</span>
        {locale.t("prompt_assistant.generating")}
      {:else}
        {locale.t("prompt_assistant.generate")}
      {/if}
    </button>

    {#if result}
      <div class="mt-3">
        <div class="rounded-lg border border-neutral-700 bg-neutral-800/50 p-2 text-sm text-neutral-200">
          {result}
        </div>
        <div class="mt-2 flex gap-2">
          <button
            class="flex-1 rounded-lg border border-neutral-600 px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-800"
            onclick={replace}
          >
            {locale.t("prompt_assistant.replace")}
          </button>
          <button
            class="flex-1 rounded-lg border border-neutral-600 px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-800"
            onclick={append}
          >
            {locale.t("prompt_assistant.append")}
          </button>
        </div>
      </div>
    {/if}
  </div>
</div>
