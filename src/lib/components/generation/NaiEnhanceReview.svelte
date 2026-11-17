<script lang="ts">
  /**
   * The per-field review of a V5 Enhance answer, inside the chat panel.
   *
   * A V5 answer touches the base prompt, the undesired content and every
   * character box at once, so nothing reaches the generation store until the
   * user confirms the rows: a diff with a checkbox per field, where only the
   * ticked rows are written. The outfit tools (overlap chips, Isolate outfits,
   * the opposite-outfit UC) work on the answer before it is applied.
   */
  import { enhanceChat } from "../../stores/enhanceChat.svelte.js";
  import { enhancerSessions } from "../../stores/enhancerSessions.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { redoLast } from "../../utils/enhanceChatRunner.js";
  import {
    findOutfitOverlaps,
    hasOppositeOutfitUc,
    similarBodyPairs,
    type OutfitOverlap,
  } from "../../utils/naiOutfits.js";
  import type { NaiCanonPriority } from "../../utils/naiSavedCharacters.js";
  import { naiV5Variant } from "../../utils/novelaiModels.js";
  import { estimatePromptTokens } from "../../utils/promptTokens.js";

  const pending = $derived(enhanceChat.pending);

  /**
   * The whole selection, counted the way NovelAI counts it.
   *
   * Per-row bars answer "is this box heavy"; this answers the only question
   * that decides whether the image renders, because the budget is a property of
   * the request and not of any one field.
   */
  const selectedTokens = $derived(estimatePromptTokens(enhanceChat.selectedText));

  /** Character rows that will still be boxes after apply, by row index. */
  const liveRows = $derived(
    pending ? pending.characters.flatMap((c, i) => (c.removes ? [] : [i])) : [],
  );
  const liveBoxes = $derived(pending ? liveRows.map((i) => pending.characters[i].after) : []);
  const liveUcs = $derived(pending ? liveRows.map((i) => pending.characters[i].ucAfter) : []);
  /** Clothing and hair tags written in more than one place, the outfit leak risk. */
  const overlaps = $derived(pending ? findOutfitOverlaps(pending.base.after, liveBoxes) : []);
  /** Alike bodies in different clothes: where opposite-outfit UC matters most. */
  const similarBodies = $derived(similarBodyPairs(liveBoxes).length > 0);
  const oppositeUcDone = $derived(hasOppositeOutfitUc(liveBoxes, liveUcs));

  /** Where else an overlapping tag is written, for its chip's tooltip. */
  function overlapPlaces(o: OutfitOverlap, except: number | null): string {
    const places = [
      ...(o.inBase && except !== null ? [locale.t("prompt_assistant.nai_field_base")] : []),
      ...o.boxes
        .filter((k) => k !== except)
        .map((k) => {
          const row = pending?.characters[liveRows[k]];
          return locale.t("prompt_assistant.nai_field_character", {
            index: String((row?.targetIndex ?? liveRows[k]) + 1),
          });
        }),
    ];
    return places.join(", ");
  }

  type Chip = { tag: string; title: string; onclick: () => void };

  /** Chips for the base row: tags it restates from a box. */
  const baseChips = $derived<Chip[]>(
    overlaps
      .filter((o) => o.inBase)
      .map((o) => ({
        tag: o.tag,
        title: locale.t("prompt_assistant.nai_overlap_base_tip", { places: overlapPlaces(o, null) }),
        onclick: () => enhanceChat.resolveOverlap(o.tag, null),
      })),
  );

  /** Chips for one character row: its tags that are also written elsewhere. */
  function rowChips(row: number): Chip[] {
    const k = liveRows.indexOf(row);
    if (k === -1) return [];
    return overlaps
      .filter((o) => o.boxes.includes(k) && (o.inBase || o.boxes.length > 1))
      .map((o) => ({
        tag: o.tag,
        title: locale.t("prompt_assistant.nai_overlap_box_tip", { places: overlapPlaces(o, k) }),
        onclick: () => enhanceChat.resolveOverlap(o.tag, row),
      }));
  }

  function isolate() {
    if (!enhanceChat.isolatePending()) {
      gallery.showToast(locale.t("prompt_assistant.nai_isolate_nothing"), "info");
    }
  }

  /** The canon redo is offered on the newest answer, the one a redo replaces. */
  const canRedo = $derived.by(() => {
    const at = pending?.answerAt;
    if (at === undefined || !enhanceChat.meta[at]?.request) return false;
    const turns = enhancerSessions.turns("nai");
    return turns[turns.length - 1]?.at === at;
  });

  /** Ask the answer again with the other side winning where a saved canon character's card disagrees with canon. */
  async function rerunWithCanon(priority: NaiCanonPriority) {
    const at = pending?.answerAt;
    if (at === undefined || !canRedo || enhanceChat.busy || enhanceChat.canonPriority === priority) return;
    enhanceChat.backToChat();
    await redoLast("nai", at, priority);
  }

  // A review is written for one model's boxes, so leaving V5 while it is on
  // screen sends the panel back to the conversation.
  $effect(() => {
    if (enhanceChat.view === "review" && pending && !naiV5Variant(generation.checkpoint)) {
      enhanceChat.backToChat();
    }
  });
</script>

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
  ucBefore: string = "",
  ucAfter: string = "",
  chips: { tag: string; title: string; onclick: () => void }[] = [],
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
    <div class="mt-2 grid gap-2 @xl:grid-cols-2">
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
        {#if chips.length > 0}
          <div class="mt-1.5 flex flex-wrap items-center gap-1">
            <span class="text-[10px] text-amber-300/80">{locale.t("prompt_assistant.nai_overlap")}</span>
            {#each chips as chip (chip.tag)}
              <button
                class="rounded-full border border-amber-500/50 bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-200 hover:bg-amber-500/25"
                title={chip.title}
                aria-label={`${chip.tag}: ${chip.title}`}
                onclick={chip.onclick}
              >
                {chip.tag}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
    {#if ucBefore.trim() || ucAfter.trim()}
      <!-- The box's own undesired content, written with its prompt. -->
      <div class="mt-2 grid gap-2 @xl:grid-cols-2">
        {#each [[ucBefore, "text-neutral-500"], [ucAfter, "text-neutral-200"]] as [text, tone], k (k)}
          <div>
            <div class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">
              {locale.t("prompt_assistant.nai_field_uc")}
            </div>
            <div
              class="max-h-24 overflow-y-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-neutral-950 p-2 text-[11px] {tone}"
            >
              {text.trim() || locale.t("prompt_assistant.nai_empty")}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/snippet}

{#if pending}
  <div class="@container">
      {#if enhanceChat.curate}
        <div
          class="mb-3 rounded-lg border border-indigo-500/40 bg-indigo-500/10 p-2 text-[11px] text-indigo-200"
        >
          {locale.t("prompt_assistant.nai_curate_notice")}
        </div>
      {/if}

      {#if pending.note}
        <div
          class="mb-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-[11px] text-amber-200"
        >
          {pending.note}
        </div>
      {/if}

      {#if pending.canon.length > 0}
        <!-- A saved canon character whose card disagrees with canon. The
             rewrite went with one side; the other is one redo away. -->
        <div class="mb-3 rounded-lg border border-sky-500/40 bg-sky-500/10 p-2 text-[11px] text-sky-100">
          <div class="font-medium">{locale.t("prompt_assistant.nai_canon_title")}</div>
          <ul class="mt-1 list-disc space-y-0.5 pl-4">
            {#each pending.canon as line, i (i)}
              <li>{line}</li>
            {/each}
          </ul>
          <div class="mt-2 flex flex-wrap items-center gap-1.5">
            <span class="text-sky-200/80">{locale.t("prompt_assistant.nai_canon_question")}</span>
            {#each [["saved", "prompt_assistant.nai_canon_keep_saved"], ["canon", "prompt_assistant.nai_canon_use_canon"]] as [choice, key] (choice)}
              <button
                class="rounded-lg border px-2 py-0.5 text-[10px] disabled:cursor-default {enhanceChat.canonPriority === choice
                  ? 'border-sky-400 bg-sky-500/30 text-white'
                  : 'border-neutral-600 text-neutral-200 hover:bg-neutral-800'}"
                aria-pressed={enhanceChat.canonPriority === choice}
                disabled={!canRedo || enhanceChat.busy || enhanceChat.canonPriority === choice}
                onclick={() => rerunWithCanon(choice as NaiCanonPriority)}
              >
                {locale.t(key)}
              </button>
            {/each}
          </div>
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
          onclick={() => enhanceChat.setAll(true)}
        >
          {locale.t("prompt_assistant.nai_select_all")}
        </button>
        <button
          class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
          onclick={() => enhanceChat.setAll(false)}
        >
          {locale.t("prompt_assistant.nai_select_none")}
        </button>
        {#if liveRows.length >= 2}
          <button
            class="ml-auto rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
            title={locale.t("prompt_assistant.nai_isolate_outfits_tip")}
            onclick={isolate}
          >
            {locale.t("prompt_assistant.nai_isolate_outfits")}
          </button>
        {/if}
      </div>

      {#if similarBodies}
        <div class="mb-3 flex items-center gap-2 text-[11px] text-sky-200">
          <span>{locale.t("prompt_assistant.nai_similar_bodies")}</span>
          {#if oppositeUcDone}
            <span class="rounded bg-sky-500/20 px-1.5 py-0.5 text-[10px] text-sky-100">
              {locale.t("prompt_assistant.nai_similar_bodies_added")}
            </span>
          {:else}
            <button
              class="rounded-lg border border-sky-500/50 px-2 py-0.5 text-[10px] text-sky-100 hover:bg-sky-500/20"
              onclick={() => enhanceChat.fillOppositeUc()}
            >
              {locale.t("prompt_assistant.nai_similar_bodies_add")}
            </button>
          {/if}
        </div>
      {/if}

      <div class="space-y-2">
        {@render row(
          locale.t("prompt_assistant.nai_field_base"),
          pending.base.before,
          pending.base.after,
          pending.base.selected,
          pending.budget,
          () => enhanceChat.toggleBase(),
          "",
          "",
          "",
          baseChips,
        )}
        {@render row(
          locale.t("prompt_assistant.nai_field_uc"),
          pending.uc.before,
          pending.uc.after,
          pending.uc.selected,
          pending.budget,
          () => enhanceChat.toggleUc(),
          "",
        )}
        {#each pending.characters as char, i (i)}
          {@render row(
            locale.t("prompt_assistant.nai_field_character", {
              // A removal row is named after the box it deletes, not its place
              // in this list, so the number matches the panel underneath.
              index: String((char.targetIndex ?? i) + 1),
            }),
            char.before,
            char.after,
            char.selected,
            pending.budget,
            () => enhanceChat.toggleCharacter(i),
            char.removes
              ? locale.t("prompt_assistant.nai_removed_character")
              : char.targetIndex === null
                ? locale.t("prompt_assistant.nai_new_character")
                : "",
            char.ucBefore,
            char.removes ? "" : char.ucAfter,
            rowChips(i),
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
            onclick={() => enhanceChat.backToChat()}
          >
            {locale.t("common.cancel")}
          </button>
          <button
            class="rounded-lg bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500 disabled:opacity-40"
            disabled={!enhanceChat.hasSelection}
            onclick={() => enhanceChat.apply()}
          >
            {locale.t("prompt_assistant.nai_apply_selected")}
          </button>
        </div>
      </div>
  </div>
{/if}
