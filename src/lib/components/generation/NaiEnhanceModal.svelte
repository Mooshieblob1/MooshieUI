<script lang="ts">
  /**
   * The whole NovelAI V5 rewrite flow, in one app-wide modal.
   *
   * Stage one takes what the user wants written. It is a blank box rather than
   * the prompt box's contents because V5 is asked for scenes as often as it is
   * asked to tidy up tags, and a prefilled box only offers the second.
   *
   * Stage two is a review gate. The other two enhance paths overwrite one
   * textarea and offer an undo, which is proportionate to what they change. A
   * V5 rewrite touches the base prompt, the undesired content and every
   * character box in one go, so nothing reaches the generation store until the
   * user confirms the rows.
   *
   * The `enhanceForNai` call lives here and not in the store: `naiEnhance` is a
   * feature store and may not import the prompt assistant.
   */
  import { naiEnhance } from "../../stores/naiEnhance.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { naiLanguageInfo, NAI_LANGUAGES, resolveNaiLanguage } from "../../utils/naiLanguage.js";
  import type { NaiLanguageChoice } from "../../utils/naiLanguage.js";
  import { NAI_VARIANT_BUDGET } from "../../utils/naiPrompt.js";
  import { naiV5Variant } from "../../utils/novelaiModels.js";
  import { estimatePromptTokens } from "../../utils/promptTokens.js";
  import { mapLlmError } from "../../utils/llmError.js";

  const pending = $derived(naiEnhance.pending);
  const variant = $derived(naiV5Variant(generation.checkpoint));

  const languageLabel = $derived(pending ? naiLanguageInfo(pending.language).label : "");

  /**
   * The whole selection, counted the way NovelAI counts it.
   *
   * Per-row bars answer "is this box heavy"; this answers the only question
   * that decides whether the image renders, because the budget is a property of
   * the request and not of any one field.
   */
  const selectedTokens = $derived(estimatePromptTokens(naiEnhance.selectedText));

  // The rewrite is written for one variant's budget and syntax, so a model
  // switch while a result is on screen invalidates it. The input stage survives:
  // what the user typed is worth just as much under the other variant.
  $effect(() => {
    if (naiEnhance.stage === "review" && naiEnhance.pending?.variant !== variant) {
      naiEnhance.dismiss();
    }
  });

  async function run() {
    const text = naiEnhance.input.trim();
    const v = variant;
    if (!text || !v || naiEnhance.busy) return;
    const language = resolveNaiLanguage(generation.naiEnhanceLanguage, text);
    const existing = generation.novelaiSettings.characters;
    naiEnhance.busy = true;
    try {
      const result = await promptAssistant.enhanceForNai(text, {
        variant: v,
        qualityToggle: generation.novelaiSettings.quality_toggle,
        ucPreset: generation.novelaiSettings.uc_preset,
        characterCount: existing.length,
        language,
      });
      // Cancelling closes the modal but cannot recall the request, so a late
      // answer to a dismissed one is dropped rather than popped back up.
      if (naiEnhance.stage !== "input") return;
      if (!result.parsed.base.trim()) {
        gallery.showToast(locale.t("prompt_assistant.couldnt_enhance"), "error");
        return;
      }
      naiEnhance.showReview({
        variant: v,
        language,
        budget: NAI_VARIANT_BUDGET[v],
        note: result.parsed.note,
        problems: result.problems,
        // Every row starts ticked: the rewrite is one coherent answer, and
        // half-applying it leaves a base prompt describing characters that the
        // character boxes no longer match.
        base: { before: generation.positivePrompt, after: result.parsed.base, selected: true },
        uc: { before: generation.negativePrompt, after: result.parsed.uc, selected: true },
        characters: result.parsed.characters.map((after, i) => ({
          before: existing[i]?.prompt ?? "",
          after,
          selected: true,
          targetIndex: i < existing.length ? i : null,
        })),
      });
    } catch (e) {
      console.error("NovelAI V5 prompt rewrite failed:", e);
      gallery.showToast(mapLlmError(String(e)), "error");
    } finally {
      naiEnhance.busy = false;
    }
  }

  function close() {
    naiEnhance.dismiss();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }

  function onInputKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      run();
    }
  }
</script>

<svelte:window onkeydown={naiEnhance.isOpen ? onKeydown : undefined} />

{#snippet tokenBar(text: string, budget: number)}
  {@const tokens = estimatePromptTokens(text)}
  {@const ratio = Math.min(1, tokens / budget)}
  {@const over = tokens > budget}
  <div class="mt-1 flex items-center gap-2">
    <div class="h-1 flex-1 overflow-hidden rounded-full bg-neutral-800">
      <div
        class="h-full rounded-full transition-all {over ? 'bg-amber-400' : 'bg-indigo-500'}"
        style="width: {ratio * 100}%"
      ></div>
    </div>
    <span class="shrink-0 tabular-nums text-[10px] {over ? 'text-amber-400' : 'text-neutral-500'}">
      {tokens}/{budget}
    </span>
  </div>
{/snippet}

{#snippet row(
  label: string,
  before: string,
  after: string,
  selected: boolean,
  budget: number,
  toggle: () => void,
  hint: string,
)}
  <div
    class="rounded-lg border p-3 transition-colors {selected
      ? 'border-indigo-500/60 bg-neutral-800/40'
      : 'border-neutral-700 bg-neutral-900'}"
  >
    <label class="flex cursor-pointer items-center gap-2">
      <input type="checkbox" checked={selected} onchange={toggle} class="accent-indigo-500" />
      <span class="text-xs font-medium text-neutral-200">{label}</span>
      {#if hint}
        <span class="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-400">{hint}</span>
      {/if}
    </label>
    <div class="mt-2 grid gap-2 sm:grid-cols-2">
      <div>
        <div class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">
          {locale.t("prompt_assistant.nai_before")}
        </div>
        <div
          class="max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-neutral-950 p-2 text-[11px] text-neutral-500"
        >
          {before.trim() || locale.t("prompt_assistant.nai_empty")}
        </div>
      </div>
      <div>
        <div class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">
          {locale.t("prompt_assistant.nai_after")}
        </div>
        <div
          class="max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-neutral-950 p-2 text-[11px] text-neutral-200"
        >
          {after.trim() || locale.t("prompt_assistant.nai_empty")}
        </div>
        {@render tokenBar(after, budget)}
      </div>
    </div>
  </div>
{/snippet}

{#if naiEnhance.isOpen}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 sm:p-8"
    onclick={(e) => {
      if (e.currentTarget === e.target) close();
    }}
    role="presentation"
  >
    <!-- App wide rather than panel wide: the review is a two column diff of up
         to six fields, and the input box is easier to judge at the width the
         prompt will actually be read at. -->
    <div
      class="flex max-h-[90vh] w-full max-w-[1400px] flex-col overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-label={locale.t("prompt_assistant.nai_input_title")}
    >
      <div class="mb-3 flex items-start justify-between gap-3">
        <div>
          <h2 class="text-sm font-semibold text-neutral-100">
            {naiEnhance.stage === "review"
              ? locale.t("prompt_assistant.nai_review_title")
              : locale.t("prompt_assistant.nai_input_title")}
          </h2>
          <p class="mt-0.5 text-[11px] text-neutral-400">
            {#if naiEnhance.stage === "review" && pending}
              {locale.t("prompt_assistant.nai_review_subtitle", {
                variant:
                  pending.variant === "curated"
                    ? locale.t("prompt_assistant.nai_variant_curated")
                    : locale.t("prompt_assistant.nai_variant_full"),
                language: languageLabel,
              })}
            {:else}
              {locale.t("prompt_assistant.nai_input_subtitle")}
            {/if}
          </p>
        </div>
        <button
          class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
          onclick={close}
        >
          ✕
        </button>
      </div>

      {#if naiEnhance.stage === "input"}
        <textarea
          class="min-h-[9rem] w-full flex-1 resize-y rounded-lg border border-neutral-700 bg-neutral-950 p-3 text-xs text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
          placeholder={locale.t("prompt_assistant.nai_input_placeholder")}
          aria-label={locale.t("prompt_assistant.nai_input_title")}
          disabled={naiEnhance.busy}
          bind:value={naiEnhance.input}
          onkeydown={onInputKeydown}
        ></textarea>

        <div class="mt-2 flex flex-wrap items-center gap-2">
          <button
            class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
            disabled={naiEnhance.busy || !generation.positivePrompt?.trim()}
            onclick={() => naiEnhance.copyExistingPrompt()}
          >
            {locale.t("prompt_assistant.nai_copy_existing")}
          </button>
          <!-- V5 writes the scene body in the prompt's own language. Detection is
               unreliable on a short Latin prompt, so the guess is overridable. -->
          <select
            class="rounded-lg border border-neutral-600 bg-neutral-900 px-1.5 py-0.5 text-[10px] text-neutral-300 focus:border-indigo-500 focus:outline-none"
            title={locale.t("prompt_assistant.nai_language_tooltip")}
            aria-label={locale.t("prompt_assistant.nai_language")}
            value={generation.naiEnhanceLanguage}
            onchange={(e) => {
              generation.naiEnhanceLanguage = (e.currentTarget as HTMLSelectElement)
                .value as NaiLanguageChoice;
              generation.saveSettings();
            }}
          >
            <option value="auto">{locale.t("prompt_assistant.nai_language_auto")}</option>
            {#each NAI_LANGUAGES as lang (lang.code)}
              <option value={lang.code}>
                {lang.label}{lang.official ? "" : " *"}
              </option>
            {/each}
          </select>
        </div>

        <div class="mt-4 flex items-center justify-end gap-2">
          <button
            class="rounded-lg border border-neutral-600 px-3 py-1 text-xs text-neutral-300 hover:bg-neutral-800"
            onclick={close}
          >
            {locale.t("common.cancel")}
          </button>
          <button
            class="rounded-lg bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500 disabled:opacity-40"
            disabled={naiEnhance.busy || !naiEnhance.input.trim()}
            onclick={run}
          >
            {#if naiEnhance.busy}
              <span class="inline-block animate-spin">⟳</span>
              {locale.t("prompt_assistant.nai_working")}
            {:else}
              {locale.t("prompt_assistant.nai_run")}
            {/if}
          </button>
        </div>
      {:else if pending}
        {#if pending.note}
          <div
            class="mb-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-[11px] text-amber-200"
          >
            {pending.note}
          </div>
        {/if}

        {#if pending.problems.length > 0}
          <!-- Same bargain as the H3 warning: the rewrite is still usually better
               than the prompt it replaces, so it is offered with the broken rule
               named rather than thrown away. -->
          <div
            class="mb-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-[11px] text-amber-200"
          >
            <div class="font-medium">{locale.t("prompt_assistant.nai_format_warning")}</div>
            <ul class="mt-1 list-disc space-y-0.5 pl-4">
              {#each pending.problems as problem (problem)}
                <li>{problem}</li>
              {/each}
            </ul>
          </div>
        {/if}

        <div class="mb-3 flex items-center gap-2">
          <button
            class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
            onclick={() => naiEnhance.setAll(true)}
          >
            {locale.t("prompt_assistant.nai_select_all")}
          </button>
          <button
            class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
            onclick={() => naiEnhance.setAll(false)}
          >
            {locale.t("prompt_assistant.nai_select_none")}
          </button>
        </div>

        <div class="space-y-2">
          {@render row(
            locale.t("prompt_assistant.nai_field_base"),
            pending.base.before,
            pending.base.after,
            pending.base.selected,
            pending.budget,
            () => naiEnhance.toggleBase(),
            "",
          )}
          {@render row(
            locale.t("prompt_assistant.nai_field_uc"),
            pending.uc.before,
            pending.uc.after,
            pending.uc.selected,
            pending.budget,
            () => naiEnhance.toggleUc(),
            "",
          )}
          {#each pending.characters as char, i (i)}
            {@render row(
              locale.t("prompt_assistant.nai_field_character", { index: String(i + 1) }),
              char.before,
              char.after,
              char.selected,
              pending.budget,
              () => naiEnhance.toggleCharacter(i),
              char.targetIndex === null ? locale.t("prompt_assistant.nai_new_character") : "",
            )}
          {/each}
        </div>

        <div class="mt-4 flex items-center justify-between gap-3">
          <span class="text-[11px] text-neutral-400">
            {locale.t("prompt_assistant.nai_selection_tokens", {
              tokens: String(selectedTokens),
              budget: String(pending.budget),
            })}
          </span>
          <div class="flex items-center gap-2">
            <button
              class="rounded-lg border border-neutral-600 px-3 py-1 text-xs text-neutral-300 hover:bg-neutral-800"
              onclick={close}
            >
              {locale.t("common.cancel")}
            </button>
            <button
              class="rounded-lg bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500 disabled:opacity-40"
              disabled={!naiEnhance.hasSelection}
              onclick={() => naiEnhance.apply()}
            >
              {locale.t("prompt_assistant.nai_apply_selected")}
            </button>
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}
