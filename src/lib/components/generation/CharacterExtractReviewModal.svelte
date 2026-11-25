<script lang="ts" module>
  import type { SavedCharacter } from "../../stores/savedCharacters.svelte.js";
  import type { ExtractedCharacter } from "../../utils/characterExtract.js";
  import type { ExtractSourceKind } from "../../utils/characterImageExtract.js";

  export type DuplicateAction = "update" | "variant" | "new" | "disregard";

  export interface DuplicateMatch {
    item: ExtractedCharacter;
    saved: SavedCharacter;
  }

  export interface DuplicateDecision extends DuplicateMatch {
    action: DuplicateAction;
    /** Card name for a variant. */
    variantName: string;
    /** Card name when it is saved as a character of its own after all. */
    newName: string;
    /** Attach the inspected source image as this card's thumbnail/reference. */
    useImage?: boolean;
  }

  /** A character that matched nothing saved, with the name the user gave it. */
  export interface NewCharacterDecision {
    item: ExtractedCharacter;
    /** Starts as the name the prompt assistant suggested. */
    name: string;
    keep: boolean;
    /** Attach the inspected source image as this card's thumbnail/reference. */
    useImage?: boolean;
  }
</script>

<script lang="ts">
  /**
   * Review for what Extract found, before anything is saved. Each new
   * character gets a name field prefilled with the prompt assistant's
   * suggestion and can be left out. Each one that looks already saved gets a
   * choice: merge the new details into the saved card, keep it as a variant
   * card, save it as a new character (the match was wrong), or disregard it.
   * What was found is editable everywhere, so a wrong tag is fixed here
   * rather than after saving. Only Cancel or the close button leave: a stray
   * click outside does not throw away an extraction that took a while.
   */
  import { portal } from "../../utils/portal.js";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    matches: DuplicateMatch[];
    /** Characters that matched nothing saved. */
    fresh: ExtractedCharacter[];
    sourceKind?: ExtractSourceKind;
    sourcePreviewUrl?: string | null;
    canSwitchToInterrogator?: boolean;
    switchingToInterrogator?: boolean;
    onSwitchToInterrogator?: () => void;
    onsave: (decisions: DuplicateDecision[], named: NewCharacterDecision[]) => void;
    onclose: () => void;
  }

  let {
    matches,
    fresh,
    sourceKind,
    sourcePreviewUrl = null,
    canSwitchToInterrogator = false,
    switchingToInterrogator = false,
    onSwitchToInterrogator,
    onsave,
    onclose,
  }: Props = $props();

  let rows = $state<DuplicateDecision[]>([]);
  let newRows = $state<NewCharacterDecision[]>([]);

  // Re-initialize editable rows when the incoming match/fresh lists change
  // (e.g. initial open or switching from metadata to interrogator).
  $effect(() => {
    const mList = matches;
    const fList = fresh;
    const hasImg = Boolean(sourcePreviewUrl);
    const singleChar = mList.length + fList.length === 1;
    rows = mList.map((m) => {
      const action: DuplicateAction = m.item.variant ? "variant" : "update";
      const defaultUse = hasImg && singleChar && (action !== "update" || !m.saved.thumbnail);
      return {
        ...m,
        action,
        variantName: `${m.saved.name} (${m.item.variant || locale.t("characters.dupes.variant_default")})`,
        newName: m.item.name,
        useImage: defaultUse,
      };
    });
    newRows = fList.map((item) => ({
      item,
      name: item.name,
      keep: true,
      useImage: hasImg && singleChar,
    }));
  });

  const choices: { action: DuplicateAction; key: string }[] = [
    { action: "update", key: "characters.dupes.update" },
    { action: "variant", key: "characters.dupes.variant" },
    { action: "new", key: "characters.dupes.new" },
    { action: "disregard", key: "characters.dupes.disregard" },
  ];

  const canSave = $derived(
    newRows.some((r) => r.keep && r.name.trim() && r.item.prompt.trim()) ||
      rows.some(
        (r) =>
          r.item.prompt.trim() &&
          (r.action === "update" ||
            (r.action === "variant" && r.variantName.trim()) ||
            (r.action === "new" && r.newName.trim())),
      ),
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

{#snippet editor(item: ExtractedCharacter, disabled: boolean)}
  <div class="mt-2 space-y-1">
    <textarea
      bind:value={item.prompt}
      {disabled}
      rows="3"
      aria-label={locale.t("characters.review.prompt")}
      placeholder={locale.t("characters.review.prompt")}
      class="block w-full resize-y rounded border border-neutral-800 bg-neutral-900 px-2 py-1.5 font-mono text-[11px] text-neutral-300 focus:outline-none focus:border-indigo-500 disabled:cursor-not-allowed"
    ></textarea>
    <textarea
      bind:value={item.negative}
      {disabled}
      rows="1"
      aria-label={locale.t("characters.review.negative")}
      placeholder={locale.t("characters.review.negative")}
      class="block w-full resize-y rounded border border-neutral-800 bg-neutral-900 px-2 py-1 font-mono text-[11px] text-neutral-500 focus:outline-none focus:border-indigo-500 focus:text-neutral-300 disabled:cursor-not-allowed"
    ></textarea>
  </div>
{/snippet}

<div
  use:portal
  class="fixed inset-0 z-210 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t(namingOnly ? "characters.naming.title" : "characters.dupes.title")}
>
  <div class="relative z-10 flex max-h-[90vh] w-full max-w-3xl flex-col rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
    <div class="mb-3 flex items-start justify-between gap-3">
      <div>
        <div class="flex flex-wrap items-center gap-2">
          <h3 class="text-sm font-semibold text-neutral-100">
            {locale.t(namingOnly ? "characters.naming.title" : "characters.dupes.title")}
          </h3>
          {#if sourceKind === "metadata" || sourceKind === "interrogator" || sourceKind === "describe"}
            <span class="rounded border border-neutral-700 bg-neutral-800 px-1.5 py-0.5 text-[10px] font-medium text-neutral-300">
              {locale.t(`characters.extract_source.${sourceKind}`)}
            </span>
          {/if}
          {#if sourceKind === "metadata" && canSwitchToInterrogator && onSwitchToInterrogator}
            <button
              type="button"
              disabled={switchingToInterrogator}
              onclick={onSwitchToInterrogator}
              class="rounded border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-[10px] text-neutral-200 hover:bg-neutral-700 disabled:opacity-50"
            >
              {switchingToInterrogator
                ? locale.t("characters.switching_to_interrogator")
                : locale.t("characters.use_interrogator_instead")}
            </button>
          {/if}
        </div>
        <p class="mt-1 text-[11px] text-neutral-500">
          {locale.t(namingOnly ? "characters.naming.desc" : "characters.dupes.desc")}
          {locale.t("characters.review.edit_hint")}
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
            {@render editor(row.item, !row.keep)}
            {#if sourcePreviewUrl && row.keep}
              <label class="mt-2 inline-flex items-center gap-1.5 text-[11px] text-neutral-300 select-none cursor-pointer">
                <input
                  type="checkbox"
                  bind:checked={row.useImage}
                  class="h-3.5 w-3.5 rounded accent-indigo-500"
                />
                <img src={sourcePreviewUrl} alt="" class="w-5 h-5 rounded object-cover border border-neutral-700 shrink-0" />
                <span>{locale.t("characters.use_as_card_picture")}</span>
              </label>
            {/if}
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
            <div class="min-w-0">
              <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.dupes.saved")}</p>
              <div class="max-h-32 overflow-y-auto rounded border border-neutral-800 bg-neutral-900 px-2 py-1.5 font-mono text-[11px] text-neutral-300">
                <p class="whitespace-pre-wrap break-words">{row.saved.prompt}</p>
                {#if row.saved.negative}
                  <p class="mt-1 whitespace-pre-wrap break-words text-neutral-500">
                    {locale.t("characters.negative_prefix")} {row.saved.negative}
                  </p>
                {/if}
              </div>
            </div>
            <div class="min-w-0">
              <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.dupes.found")}</p>
              {@render editor(row.item, row.action === "disregard")}
            </div>
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
            {:else if row.action === "new"}
              <input
                type="text"
                bind:value={row.newName}
                maxlength={80}
                aria-label={locale.t("characters.dupes.new_name")}
                placeholder={locale.t("characters.dupes.new_name")}
                class="ml-1 min-w-0 flex-1 rounded border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-[11px] text-neutral-100 focus:outline-none focus:border-indigo-500"
              />
            {/if}
          </div>
          {#if sourcePreviewUrl && row.action !== "disregard"}
            <label class="mt-2 inline-flex items-center gap-1.5 text-[11px] text-neutral-300 select-none cursor-pointer">
              <input
                type="checkbox"
                bind:checked={row.useImage}
                class="h-3.5 w-3.5 rounded accent-indigo-500"
              />
              <img src={sourcePreviewUrl} alt="" class="w-5 h-5 rounded object-cover border border-neutral-700 shrink-0" />
              <span>{locale.t("characters.use_as_card_picture")}</span>
            </label>
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
        class="px-3 py-1.5 text-xs rounded bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40 disabled:cursor-not-allowed"
        disabled={!canSave}
        onclick={save}
      >
        {locale.t("characters.dupes.save")}
      </button>
    </div>
  </div>
</div>
