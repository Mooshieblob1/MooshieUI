<script lang="ts" module>
  import type { SavedCharacter } from "../../stores/savedCharacters.svelte.js";
  import type { ExtractedCharacter } from "../../utils/characterExtract.js";

  export type DuplicateAction = "update" | "variant" | "disregard";

  export interface DuplicateMatch {
    item: ExtractedCharacter;
    saved: SavedCharacter;
  }

  export interface DuplicateDecision extends DuplicateMatch {
    action: DuplicateAction;
    /** Card name for a variant. */
    variantName: string;
  }

  /** A character that matched nothing saved, with the name the user gave it. */
  export interface NewCharacterDecision {
    item: ExtractedCharacter;
    /** Starts as the name the prompt assistant suggested. */
    name: string;
    keep: boolean;
  }
</script>

<script lang="ts">
  /**
   * Review for what Extract found, before anything is saved. Each new
   * character gets a name field prefilled with the prompt assistant's
   * suggestion and can be left out. Each one that looks already saved gets a
   * choice: merge the new details into the saved card, keep it as a variant
   * card of its own, or disregard it.
   */
  import { portal } from "../../utils/portal.js";
  import { untrack } from "svelte";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    matches: DuplicateMatch[];
    /** Characters that matched nothing saved. */
    fresh: ExtractedCharacter[];
    onsave: (decisions: DuplicateDecision[], named: NewCharacterDecision[]) => void;
    onclose: () => void;
  }

  let { matches, fresh, onsave, onclose }: Props = $props();

  // Mounted once per extraction, so the props are read once on purpose. A
  // match the model labelled as a variant starts on Save as variant; any
  // other starts on Update, the "same character, newer details" reading.
  let rows = $state<DuplicateDecision[]>(
    untrack(() =>
      matches.map((m) => ({
        ...m,
        action: m.item.variant ? "variant" : "update",
        variantName: `${m.saved.name} (${m.item.variant || locale.t("characters.dupes.variant_default")})`,
      })),
    ),
  );

  let newRows = $state<NewCharacterDecision[]>(
    untrack(() => fresh.map((item) => ({ item, name: item.name, keep: true }))),
  );

  const choices: { action: DuplicateAction; key: string }[] = [
    { action: "update", key: "characters.dupes.update" },
    { action: "variant", key: "characters.dupes.variant" },
    { action: "disregard", key: "characters.dupes.disregard" },
  ];

  const canSave = $derived(
    newRows.some((r) => r.keep && r.name.trim()) ||
      rows.some((r) => r.action === "update" || (r.action === "variant" && r.variantName.trim())),
  );

  /** Only new characters: the review is about naming them. */
  const namingOnly = $derived(rows.length === 0);

  function save() {
    if (canSave) onsave(rows, newRows);
  }

  /** Focus and select the first name field, so typing replaces the suggestion. */
  function focusFirst(node: HTMLInputElement, first: boolean) {
    if (!first) return;
    node.focus();
    node.select();
  }
</script>

<div
  use:portal
  class="fixed inset-0 z-210 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t(namingOnly ? "characters.naming.title" : "characters.dupes.title")}
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
        <h3 class="text-sm font-semibold text-neutral-100">
          {locale.t(namingOnly ? "characters.naming.title" : "characters.dupes.title")}
        </h3>
        <p class="mt-1 text-[11px] text-neutral-500">
          {locale.t(namingOnly ? "characters.naming.desc" : "characters.dupes.desc")}
        </p>
      </div>
      <button
        type="button"
        class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
        onclick={onclose}
        aria-label={locale.t("common.cancel")}
      >✕</button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto space-y-3">
      {#if newRows.length > 0}
        {#if !namingOnly}
          <p class="text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.naming.new_heading")}</p>
        {/if}
        {#each newRows as row, i (i)}
          <div class="rounded-lg border border-neutral-800 bg-neutral-950/40 p-2.5 {row.keep ? '' : 'opacity-50'}">
            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                bind:checked={row.keep}
                aria-label={locale.t("characters.naming.keep", { name: row.name.trim() || row.item.name })}
                title={locale.t("characters.naming.keep", { name: row.name.trim() || row.item.name })}
                class="h-3.5 w-3.5 shrink-0 accent-indigo-500"
              />
              <input
                type="text"
                bind:value={row.name}
                use:focusFirst={i === 0}
                disabled={!row.keep}
                maxlength={80}
                aria-label={locale.t("characters.naming.name")}
                placeholder={row.item.name}
                class="min-w-0 flex-1 rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 disabled:cursor-not-allowed"
                onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); save(); } }}
              />
            </div>
            <div class="mt-2 max-h-24 overflow-y-auto rounded border border-neutral-800 bg-neutral-900 px-2 py-1.5 font-mono text-[11px] text-neutral-300">
              <p class="whitespace-pre-wrap break-words">{row.item.prompt}</p>
              {#if row.item.negative}
                <p class="mt-1 whitespace-pre-wrap break-words text-neutral-500">
                  {locale.t("characters.negative_prefix")} {row.item.negative}
                </p>
              {/if}
            </div>
          </div>
        {/each}
        {#if !namingOnly}
          <p class="pt-1 text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.naming.saved_heading")}</p>
        {/if}
      {/if}
      {#each rows as row, i (i)}
        <div class="rounded-lg border border-neutral-800 bg-neutral-950/40 p-2.5">
          <p class="text-xs font-medium text-neutral-200">
            {locale.t("characters.dupes.looks_like", { name: row.item.name, saved: row.saved.name })}
          </p>
          <div class="mt-2 grid gap-2 sm:grid-cols-2">
            {#each [
              { label: locale.t("characters.dupes.saved"), prompt: row.saved.prompt, negative: row.saved.negative },
              { label: locale.t("characters.dupes.found"), prompt: row.item.prompt, negative: row.item.negative },
            ] as side (side.label)}
              <div class="min-w-0">
                <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">{side.label}</p>
                <div class="max-h-32 overflow-y-auto rounded border border-neutral-800 bg-neutral-900 px-2 py-1.5 font-mono text-[11px] text-neutral-300">
                  <p class="whitespace-pre-wrap break-words">{side.prompt}</p>
                  {#if side.negative}
                    <p class="mt-1 whitespace-pre-wrap break-words text-neutral-500">
                      {locale.t("characters.negative_prefix")} {side.negative}
                    </p>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
          <div class="mt-2 flex flex-wrap items-center gap-1.5" role="radiogroup" aria-label={row.item.name}>
            {#each choices as choice (choice.action)}
              <button
                type="button"
                role="radio"
                aria-checked={row.action === choice.action}
                class="rounded-lg border px-2 py-0.5 text-[10px] {row.action === choice.action
                  ? 'border-indigo-400 bg-indigo-500/25 text-white'
                  : 'border-neutral-600 text-neutral-300 hover:bg-neutral-800'}"
                title={choice.action === "update" ? locale.t("characters.dupes.update_tip") : undefined}
                onclick={() => { row.action = choice.action; }}
              >
                {locale.t(choice.key)}
              </button>
            {/each}
            {#if row.action === "variant"}
              <input
                type="text"
                bind:value={row.variantName}
                aria-label={locale.t("characters.dupes.variant_name")}
                placeholder={locale.t("characters.dupes.variant_name")}
                class="ml-1 min-w-0 flex-1 rounded border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-[11px] text-neutral-100 focus:outline-none focus:border-indigo-500"
              />
            {/if}
          </div>
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
        class="px-3 py-1.5 text-xs rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40 disabled:cursor-not-allowed"
        disabled={!canSave}
        onclick={save}
      >
        {locale.t("characters.dupes.save")}
      </button>
    </div>
  </div>
</div>
