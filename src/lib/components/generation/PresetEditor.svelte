<script lang="ts">
  import { promptPresets, presetSlug } from "../../stores/promptPresets.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import PromptTextarea from "./PromptTextarea.svelte";

  interface Props {
    presetId: string;
    onclose: () => void;
  }

  let { presetId, onclose }: Props = $props();

  const preset = $derived(promptPresets.getById(presetId));
  const choiceCount = $derived(promptPresets.countChoices(presetId));
  const slug = $derived(preset ? presetSlug(preset.name) : "");
  const inlineToken = $derived(slug ? `@preset:${slug}` : "");

  function setName(v: string) {
    if (!preset) return;
    promptPresets.update(preset.id, { name: v });
  }

  // Local mirror bound to PromptTextarea; writes back to the store on change.
  // The component is remounted per-preset (parent gates with {#if editingPresetId}),
  // so a one-shot initializer + forward effect is sufficient.
  let contentLocal = $state("");
  let contentInitialized = $state(false);

  $effect(() => {
    if (!contentInitialized && preset) {
      contentLocal = preset.content;
      contentInitialized = true;
    }
  });

  $effect(() => {
    if (contentInitialized && preset && contentLocal !== preset.content) {
      promptPresets.update(preset.id, { content: contentLocal });
    }
  });
</script>

<div
  class="fixed inset-0 z-205 flex items-center justify-center bg-black/80 backdrop-blur-sm"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t("generation.presets.edit_title")}
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label={locale.t("common.close")}
    onclick={onclose}
  ></button>

  {#if preset}
    <div class="relative z-10 w-full max-w-2xl max-h-[92vh] overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
      <div class="mb-4 flex items-start justify-between gap-3">
        <div>
          <h2 class="text-sm font-semibold text-neutral-100">{locale.t("generation.presets.edit_title")}</h2>
          <p class="text-[11px] text-neutral-500">
            {locale.t("generation.presets.edit_desc")}
          </p>
        </div>
        <button
          type="button"
          class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
          onclick={onclose}
          aria-label={locale.t("common.close")}
        >✕</button>
      </div>

      <div class="space-y-4">
        <div>
          <label for="pst-name" class="mb-1 block text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("generation.presets.name_label")}</label>
          <input
            id="pst-name"
            type="text"
            value={preset.name}
            oninput={(e) => setName((e.currentTarget as HTMLInputElement).value)}
            placeholder={locale.t("generation.presets.name_placeholder")}
            class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
          />
          {#if inlineToken}
            <p class="mt-1 text-[10px] text-neutral-500">
              {locale.t("generation.presets.inline_token")} <button
                type="button"
                class="rounded border border-indigo-500/40 bg-indigo-500/10 px-1.5 py-0.5 font-mono text-[10px] text-indigo-200 hover:border-indigo-400 hover:bg-indigo-500/20"
                title={locale.t("generation.presets.copy_inline_token")}
                onclick={() => navigator.clipboard?.writeText(inlineToken)}
              >{inlineToken}</button>
              {locale.t("generation.presets.inline_token_help")}
            </p>
          {/if}
        </div>

        <div>
          <div class="mb-1 flex items-center justify-between">
            <label for="pst-content" class="text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("generation.presets.content_label")}</label>
            <span class="text-[10px] text-neutral-500">{choiceCount === 1 ? locale.t("generation.presets.wildcard_option", { count: choiceCount }) : locale.t("generation.presets.wildcard_options", { count: choiceCount })}</span>
          </div>
          <PromptTextarea
            bind:value={contentLocal}
            placeholder={locale.t("generation.presets.content_placeholder")}
            rows={8}
            minHeight="min-h-40"
          />
          <p class="mt-1 text-[10px] text-neutral-500">
            <span class="text-indigo-300">{locale.t("generation.presets.prepend_append_label")}</span> {locale.t("generation.presets.prepend_append_help")}
            <br />
            <span class="text-indigo-300">{locale.t("generation.presets.wildcard_label")}</span> {locale.t("generation.presets.wildcard_help")}
            <br />
            <span class="text-neutral-400">{locale.t("generation.presets.tip_label")}</span> {locale.t("generation.presets.autocomplete_tip")}
          </p>
        </div>
      </div>

      <div class="mt-4 flex justify-end">
        <button
          type="button"
          class="rounded bg-indigo-600 px-4 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
          onclick={onclose}
        >{locale.t("common.done")}</button>
      </div>
    </div>
  {:else}
    <div class="relative z-10 rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-sm text-neutral-300">
      {locale.t("generation.presets.not_found")}
      <button class="ml-3 text-indigo-400 hover:text-indigo-300" onclick={onclose}>{locale.t("common.close")}</button>
    </div>
  {/if}
</div>
