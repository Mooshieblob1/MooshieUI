<script lang="ts">
  /**
   * Review gate for a saved character the prompt assistant worked into the
   * prompt. One row per field the merge touches, each with a tick and an
   * editable result; only ticked rows are written.
   */
  import { untrack } from "svelte";
  import { locale } from "../../stores/locale.svelte.js";
  import type { CharacterMergeResult } from "../../utils/characterMerge.js";

  type FieldKey = "prompt" | "negative" | "character" | "characterNegative";

  interface Props {
    name: string;
    before: { prompt: string; negative: string };
    result: CharacterMergeResult;
    /** The character goes into a new NovelAI character box. */
    boxMode: boolean;
    /** That box takes its own UC. */
    boxNegative: boolean;
    onapply: (fields: Partial<Record<FieldKey, string>>) => void;
    oninsertplain: () => void;
    onclose: () => void;
  }

  let { name, before, result, boxMode, boxNegative, onapply, oninsertplain, onclose }: Props = $props();

  interface Row {
    key: FieldKey;
    label: string;
    before: string;
    after: string;
    selected: boolean;
  }

  function initialRows(): Row[] {
    const rows: Row[] = [
      {
        key: "prompt",
        label: locale.t(boxMode ? "characters.review.base_prompt" : "characters.review.prompt"),
        before: before.prompt,
        after: result.prompt,
        selected: true,
      },
      {
        key: "negative",
        label: locale.t("characters.review.negative"),
        before: before.negative,
        after: result.negative,
        selected: true,
      },
    ];
    if (boxMode) {
      rows.push({
        key: "character",
        label: locale.t("characters.review.character"),
        before: "",
        after: result.character,
        selected: true,
      });
      if (boxNegative) {
        rows.push({
          key: "characterNegative",
          label: locale.t("characters.review.character_negative"),
          before: "",
          after: result.characterNegative,
          selected: true,
        });
      }
    }
    return rows;
  }

  // Mounted once per review, so the props are read once on purpose.
  let rows = $state<Row[]>(untrack(initialRows));

  function changed(row: Row): boolean {
    return row.after.trim() !== row.before.trim();
  }

  const canApply = $derived(rows.some((r) => r.selected && changed(r)));

  function apply() {
    const fields: Partial<Record<FieldKey, string>> = {};
    for (const row of rows) {
      if (row.selected && changed(row)) fields[row.key] = row.after.trim();
    }
    onapply(fields);
  }
</script>

<div
  class="fixed inset-0 z-210 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t("characters.review.title", { name })}
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label={locale.t("common.cancel")}
    onclick={onclose}
  ></button>

  <div class="relative z-10 flex max-h-[90vh] w-full max-w-3xl flex-col rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
    <div class="mb-3 flex items-start justify-between gap-3">
      <div>
        <h3 class="text-sm font-semibold text-neutral-100">{locale.t("characters.review.title", { name })}</h3>
        <p class="mt-1 text-[11px] text-neutral-500">{locale.t("characters.review.desc", { name })}</p>
      </div>
      <button
        type="button"
        class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
        onclick={onclose}
        aria-label={locale.t("common.cancel")}
      >✕</button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto space-y-3">
      {#each rows as row (row.key)}
        <div class="rounded-lg border border-neutral-800 bg-neutral-950/40 p-2.5">
          <label class="flex items-center gap-2 text-xs font-medium text-neutral-200">
            <input
              type="checkbox"
              class="accent-indigo-500"
              bind:checked={row.selected}
              disabled={!changed(row)}
            />
            {row.label}
            {#if !changed(row)}
              <span class="text-[10px] font-normal text-neutral-500">{locale.t("characters.review.no_change")}</span>
            {/if}
          </label>
          {#if changed(row)}
            <div class="mt-2 grid gap-2 sm:grid-cols-2">
              <div class="min-w-0">
                <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.review.before")}</p>
                <p class="max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-neutral-900 px-2 py-1.5 font-mono text-[11px] text-neutral-400">{row.before || "—"}</p>
              </div>
              <div class="min-w-0">
                <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.review.after")}</p>
                <textarea
                  bind:value={row.after}
                  rows="5"
                  spellcheck="false"
                  aria-label={`${row.label}: ${locale.t("characters.review.after")}`}
                  disabled={!row.selected}
                  class="w-full resize-y rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 font-mono text-[11px] text-neutral-100 focus:outline-none focus:border-indigo-500 disabled:opacity-50"
                ></textarea>
              </div>
            </div>
          {/if}
        </div>
      {/each}
    </div>

    <div class="mt-4 flex flex-wrap justify-end gap-2">
      <button
        type="button"
        class="px-3 py-1.5 text-xs rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
        onclick={onclose}
      >
        {locale.t("common.cancel")}
      </button>
      <button
        type="button"
        class="px-3 py-1.5 text-xs rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800"
        title={locale.t("characters.insert_plain_tip")}
        onclick={oninsertplain}
      >
        {locale.t("characters.insert_plain")}
      </button>
      <button
        type="button"
        class="px-3 py-1.5 text-xs rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40 disabled:cursor-not-allowed"
        disabled={!canApply}
        onclick={apply}
      >
        {locale.t("characters.review.apply")}
      </button>
    </div>
  </div>
</div>
