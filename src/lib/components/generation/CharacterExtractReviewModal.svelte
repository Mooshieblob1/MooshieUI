<script lang="ts" module>
  import type { SavedCharacter } from "../../stores/savedCharacters.svelte.js";
  import type { ExtractedCharacter } from "../../utils/characterExtract.js";
  import type { ExtractSourceKind } from "../../utils/characterImageExtract.js";

  export type DuplicateAction = "update" | "replace" | "variant" | "new" | "disregard";
  export type NewCharacterAction = "new" | "update" | "replace" | "variant";

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

  /** A character that matched nothing saved, with the name and optional existing target the user gave it. */
  export interface NewCharacterDecision {
    item: ExtractedCharacter;
    /** Starts as the name the prompt assistant suggested. */
    name: string;
    keep: boolean;
    /** How to apply this extracted character when keeping it. */
    action?: NewCharacterAction;
    /** Existing saved character ID when `action` is `update`, `replace`, or `variant`. */
    targetId?: string;
    /** Attach the inspected source image as this card's thumbnail/reference. */
    useImage?: boolean;
  }
</script>

<script lang="ts">
  /**
   * Review for what Extract found, before anything is saved. Each new
   * character gets a name field prefilled with the prompt assistant's
   * suggestion (or can target an existing card for Update / Replace / Variant).
   * Each matched character gets a choice: merge the new details into a saved card,
   * replace a saved card outright, keep it as a variant, save it as new, or
   * disregard it. Only Cancel or the close button leave: a stray click outside
   * does not throw away an extraction that took a while.
   */
  import { untrack } from "svelte";
  import { portal } from "../../utils/portal.js";
  import { locale } from "../../stores/locale.svelte.js";
  import CharacterPickerModal from "./CharacterPickerModal.svelte";

  interface Props {
    matches: DuplicateMatch[];
    /** Characters that matched nothing saved. */
    fresh: ExtractedCharacter[];
    /** Existing saved cards in this architecture for Update/Replace/Variant targets. */
    existingCharacters?: SavedCharacter[];
    sourceKind?: ExtractSourceKind;
    sourcePreviewUrl?: string | null;
    canSwitchToInterrogator?: boolean;
    switchingToInterrogator?: boolean;
    onSwitchToInterrogator?: () => void;
    canSwitchToPromptOnly?: boolean;
    switchingToPromptOnly?: boolean;
    onSwitchToPromptOnly?: () => void;
    onsave: (decisions: DuplicateDecision[], named: NewCharacterDecision[]) => void;
    onclose: () => void;
  }

  let {
    matches,
    fresh,
    existingCharacters = [],
    sourceKind,
    sourcePreviewUrl = null,
    canSwitchToInterrogator = false,
    switchingToInterrogator = false,
    onSwitchToInterrogator,
    canSwitchToPromptOnly = false,
    switchingToPromptOnly = false,
    onSwitchToPromptOnly,
    onsave,
    onclose,
  }: Props = $props();

  let rows = $state<DuplicateDecision[]>([]);
  let newRows = $state<NewCharacterDecision[]>([]);
  let pickerTarget = $state<
    { kind: "new"; row: NewCharacterDecision } | { kind: "dupe"; row: DuplicateDecision } | null
  >(null);

  function defaultVariantLabel(card: SavedCharacter, item: ExtractedCharacter): string {
    return `${card.name} (${item.variant || locale.t("characters.dupes.variant_default")})`;
  }

  // Re-initialize editable rows when the incoming match/fresh lists change
  // (e.g. initial open or switching from metadata to interrogator).
  $effect(() => {
    const mList = matches;
    const fList = fresh;
    pickerTarget = null;
    const hasImg = Boolean(sourcePreviewUrl);
    const singleChar = mList.length + fList.length === 1;
    const defaultTargetId = untrack(() => existingCharacters[0]?.id ?? "");
    rows = mList.map((m) => {
      const action: DuplicateAction = m.item.variant ? "variant" : "update";
      const defaultUse = hasImg && singleChar && (action !== "update" || !m.saved.thumbnail);
      return {
        ...m,
        action,
        variantName: defaultVariantLabel(m.saved, m.item),
        newName: m.item.name,
        useImage: defaultUse,
      };
    });
    newRows = fList.map((item) => ({
      item,
      name: item.name,
      keep: true,
      action: "new" as NewCharacterAction,
      targetId: defaultTargetId,
      useImage: hasImg && singleChar,
    }));
  });

  const choices: { action: DuplicateAction; key: string; tipKey?: string }[] = [
    { action: "update", key: "characters.dupes.update", tipKey: "characters.dupes.update_tip" },
    { action: "replace", key: "characters.dupes.replace", tipKey: "characters.dupes.replace_tip" },
    { action: "variant", key: "characters.dupes.variant" },
    { action: "new", key: "characters.dupes.new" },
    { action: "disregard", key: "characters.dupes.disregard" },
  ];

  const newChoices: { action: NewCharacterAction; key: string; tipKey?: string }[] = [
    { action: "new", key: "characters.dupes.new" },
    {
      action: "update",
      key: "characters.review.action_update_existing",
      tipKey: "characters.review.action_update_existing_tip",
    },
    { action: "replace", key: "characters.dupes.replace", tipKey: "characters.dupes.replace_tip" },
    { action: "variant", key: "characters.dupes.variant" },
  ];

  function findExisting(id: string | undefined): SavedCharacter | null {
    if (!id) return null;
    return existingCharacters.find((c) => c.id === id) ?? null;
  }

  function setDuplicateTarget(row: DuplicateDecision, targetId: string) {
    const next = findExisting(targetId);
    if (!next) return;
    const prevAuto = defaultVariantLabel(row.saved, row.item);
    if (!row.variantName.trim() || row.variantName === prevAuto) {
      row.variantName = defaultVariantLabel(next, row.item);
    }
    row.saved = next;
  }

  function setNewRowAction(row: NewCharacterDecision, nextAction: NewCharacterAction) {
    const prevAction = row.action ?? "new";
    const target = findExisting(row.targetId) ?? existingCharacters[0] ?? null;
    if (target) row.targetId = target.id;
    if (nextAction === "variant" && target) {
      const prevDefault = prevAction === "variant" ? defaultVariantLabel(target, row.item) : row.item.name;
      if (!row.name.trim() || row.name === prevDefault) {
        row.name = defaultVariantLabel(target, row.item);
      }
    } else if (nextAction === "new" && target) {
      const varDefault = defaultVariantLabel(target, row.item);
      if (!row.name.trim() || row.name === varDefault) {
        row.name = row.item.name;
      }
    }
    row.action = nextAction;
  }

  function setNewRowTarget(row: NewCharacterDecision, targetId: string) {
    const prevTarget = findExisting(row.targetId);
    const nextTarget = findExisting(targetId);
    if (row.action === "variant" && nextTarget) {
      const prevAuto = prevTarget ? defaultVariantLabel(prevTarget, row.item) : row.item.name;
      if (!row.name.trim() || row.name === prevAuto || row.name === row.item.name) {
        row.name = defaultVariantLabel(nextTarget, row.item);
      }
    }
    row.targetId = targetId;
  }

  const canSave = $derived(
    newRows.some((r) => {
      if (!r.keep || !r.item.prompt.trim()) return false;
      const action = r.action ?? "new";
      if (action === "update" || action === "replace") return Boolean(findExisting(r.targetId));
      return Boolean(r.name.trim()) && (action === "new" || Boolean(findExisting(r.targetId)));
    }) ||
      rows.some(
        (r) =>
          r.item.prompt.trim() &&
          (r.action === "update" ||
            r.action === "replace" ||
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
          {#if sourceKind && sourceKind !== "prompt"}
            <span class="rounded border border-neutral-700 bg-neutral-800 px-1.5 py-0.5 text-[10px] font-medium text-neutral-300">
              {locale.t(`characters.extract_source.${sourceKind}`)}
            </span>
          {/if}
          {#if sourceKind === "combined" && canSwitchToPromptOnly && onSwitchToPromptOnly}
            <button
              type="button"
              disabled={switchingToPromptOnly || switchingToInterrogator}
              onclick={onSwitchToPromptOnly}
              class="rounded border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-[10px] text-neutral-200 hover:bg-neutral-700 disabled:opacity-50"
            >
              {switchingToPromptOnly
                ? locale.t("characters.extracting")
                : locale.t("characters.use_prompt_only")}
            </button>
          {/if}
          {#if (sourceKind === "metadata" || sourceKind === "combined") && canSwitchToInterrogator && onSwitchToInterrogator}
            <button
              type="button"
              disabled={switchingToInterrogator || switchingToPromptOnly}
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
          {@const rowAction = row.action ?? "new"}
          {@const targetCard = findExisting(row.targetId)}
          <div class="rounded-lg border border-neutral-800 bg-neutral-950/40 p-2.5 {row.keep ? '' : 'opacity-50'}">
            <div class="flex flex-wrap items-center gap-2">
              <input
                type="checkbox"
                bind:checked={row.keep}
                aria-label={locale.t("characters.naming.keep", { name: row.name.trim() || row.item.name })}
                title={locale.t("characters.naming.keep", { name: row.name.trim() || row.item.name })}
                class="h-3.5 w-3.5 shrink-0 accent-indigo-500"
              />
              {#if rowAction === "new" || rowAction === "variant"}
                <input
                  type="text"
                  bind:value={row.name}
                  use:focusFirst={i === 0}
                  disabled={!row.keep}
                  maxlength={80}
                  aria-label={locale.t(rowAction === "variant" ? "characters.dupes.variant_name" : "characters.naming.name")}
                  placeholder={rowAction === "variant" ? locale.t("characters.dupes.variant_name") : row.item.name}
                  class="min-w-0 flex-1 rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 disabled:cursor-not-allowed"
                  onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); save(); } }}
                />
              {:else}
                <span class="text-xs font-medium text-neutral-200 truncate">
                  {row.item.name} → {targetCard?.name ?? ""}
                </span>
              {/if}
              {#if rowAction !== "new" && existingCharacters.length > 0 && row.keep}
                <button
                  type="button"
                  onclick={() => { pickerTarget = { kind: "new", row }; }}
                  aria-label={locale.t("characters.review.target_character")}
                  title={locale.t("characters.review.target_character")}
                  class="flex max-w-[14rem] items-center gap-1.5 rounded border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-[11px] text-neutral-100 hover:border-indigo-500/70 hover:bg-neutral-700/80"
                >
                  {#if targetCard?.thumbnail}
                    <img src={targetCard.thumbnail} alt="" class="h-4 w-4 shrink-0 rounded-sm object-cover" />
                  {/if}
                  <span class="truncate">{targetCard?.name ?? locale.t("characters.review.target_character")}</span>
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3 shrink-0 text-neutral-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
                </button>
              {/if}
            </div>
            {#if row.keep && existingCharacters.length > 0}
              <div class="mt-2 flex flex-wrap items-center gap-1.5" role="radiogroup" aria-label={row.item.name}>
                {#each newChoices as choice (choice.action)}
                  <button
                    type="button"
                    role="radio"
                    aria-checked={rowAction === choice.action}
                    title={choice.tipKey ? locale.t(choice.tipKey) : undefined}
                    class="rounded-lg border px-2 py-0.5 text-[10px] {rowAction === choice.action
                      ? 'border-indigo-400 bg-indigo-500/25 text-white'
                      : 'border-neutral-600 text-neutral-300 hover:bg-neutral-800'}"
                    onclick={() => setNewRowAction(row, choice.action)}
                  >
                    {locale.t(choice.key)}
                  </button>
                {/each}
              </div>
            {/if}
            {#if row.keep && (rowAction === "update" || rowAction === "replace") && targetCard}
              <div class="mt-2 grid gap-2 sm:grid-cols-2">
                <div class="min-w-0">
                  <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.dupes.saved")}</p>
                  <div class="max-h-32 overflow-y-auto rounded border border-neutral-800 bg-neutral-900 px-2 py-1.5 font-mono text-[11px] text-neutral-300">
                    <p class="whitespace-pre-wrap break-words">{targetCard.prompt}</p>
                    {#if targetCard.negative}
                      <p class="mt-1 whitespace-pre-wrap break-words text-neutral-500">
                        {locale.t("characters.negative_prefix")} {targetCard.negative}
                      </p>
                    {/if}
                  </div>
                </div>
                <div class="min-w-0">
                  <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("characters.dupes.found")}</p>
                  {@render editor(row.item, !row.keep)}
                </div>
              </div>
            {:else}
              {@render editor(row.item, !row.keep)}
            {/if}
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
          <div class="flex flex-wrap items-center justify-between gap-2">
            <p class="text-xs font-medium text-neutral-200">
              {locale.t("characters.dupes.looks_like", { name: row.item.name, saved: row.saved.name })}
            </p>
            {#if existingCharacters.length > 1 && (row.action === "update" || row.action === "replace" || row.action === "variant")}
              <button
                type="button"
                onclick={() => { pickerTarget = { kind: "dupe", row }; }}
                aria-label={locale.t("characters.review.target_character")}
                title={locale.t("characters.review.target_character")}
                class="flex max-w-[14rem] items-center gap-1.5 rounded border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-[11px] text-neutral-100 hover:border-indigo-500/70 hover:bg-neutral-700/80"
              >
                {#if row.saved.thumbnail}
                  <img src={row.saved.thumbnail} alt="" class="h-4 w-4 shrink-0 rounded-sm object-cover" />
                {/if}
                <span class="truncate">{row.saved.name}</span>
                <svg xmlns="http://www.w3.org/2000/svg" class="h-3 w-3 shrink-0 text-neutral-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
              </button>
            {/if}
          </div>
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
                title={choice.tipKey ? locale.t(choice.tipKey) : undefined}
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

{#if pickerTarget}
  <CharacterPickerModal
    characters={existingCharacters}
    selectedId={pickerTarget.kind === "new" ? (pickerTarget.row.targetId ?? null) : pickerTarget.row.saved.id}
    onselect={(card) => {
      if (pickerTarget?.kind === "new") setNewRowTarget(pickerTarget.row, card.id);
      else if (pickerTarget?.kind === "dupe") setDuplicateTarget(pickerTarget.row, card.id);
      pickerTarget = null;
    }}
    onclose={() => { pickerTarget = null; }}
  />
{/if}
