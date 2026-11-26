<script lang="ts">
  /**
   * Visual card-grid picker modal for choosing a saved character inside
   * modal workflows (direct character chat & extract-to-existing review).
   */
  import { architectureLabel, type SavedCharacter } from "../../stores/savedCharacters.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { portal } from "../../utils/portal.js";

  interface Props {
    characters: SavedCharacter[];
    selectedId?: string | null;
    title?: string;
    onselect: (character: SavedCharacter) => void;
    onclose: () => void;
  }

  let {
    characters,
    selectedId = null,
    title = locale.t("characters.review.target_character"),
    onselect,
    onclose,
  }: Props = $props();

  let search = $state("");

  const nameById = $derived(new Map(characters.map((c) => [c.id, c.name])));
  const multiArch = $derived(new Set(characters.map((c) => c.architecture)).size > 1);

  const ordered = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const idSet = new Set(characters.map((c) => c.id));
    const variantsByParent = new Map<string, SavedCharacter[]>();
    for (const c of characters) {
      if (c.parentId && idSet.has(c.parentId)) {
        const list = variantsByParent.get(c.parentId) ?? [];
        list.push(c);
        variantsByParent.set(c.parentId, list);
      }
    }

    const matches = (c: SavedCharacter) => {
      if (!q) return true;
      const parentName = c.parentId ? (nameById.get(c.parentId) ?? "") : "";
      return (
        c.name.toLowerCase().includes(q) ||
        c.prompt.toLowerCase().includes(q) ||
        parentName.toLowerCase().includes(q)
      );
    };

    const result: SavedCharacter[] = [];
    for (const c of characters) {
      if (c.parentId && idSet.has(c.parentId)) continue;
      const vars = (variantsByParent.get(c.id) ?? []).filter(matches);
      if (matches(c)) result.push(c);
      result.push(...vars);
    }
    return result;
  });

  function autofocus(node: HTMLInputElement) {
    queueMicrotask(() => node.focus());
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopImmediatePropagation();
      onclose();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  });
</script>

<div
  use:portal
  class="fixed inset-0 z-220 flex items-center justify-center bg-black/75 backdrop-blur-xs p-4"
  role="dialog"
  aria-modal="true"
  aria-label={title}
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label={locale.t("common.close")}
    onclick={onclose}
  ></button>
  <div
    class="relative z-10 flex max-h-[82vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl"
  >
    <div class="flex items-center justify-between gap-2 border-b border-neutral-800 px-4 py-2.5">
      <div class="flex min-w-0 items-center gap-2">
        <h3 class="truncate text-sm font-semibold text-neutral-100">{title}</h3>
        <span class="shrink-0 rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] tabular-nums text-neutral-400">
          {ordered.length}
        </span>
      </div>
      <button
        type="button"
        class="flex h-7 w-7 items-center justify-center rounded-lg text-neutral-400 hover:bg-neutral-800 hover:text-neutral-100"
        title={locale.t("common.close")}
        aria-label={locale.t("common.close")}
        onclick={onclose}
      >✕</button>
    </div>

    <div class="border-b border-neutral-800 px-4 py-2">
      <input
        type="text"
        bind:value={search}
        use:autofocus
        placeholder={locale.t("characters.search_placeholder")}
        aria-label={locale.t("characters.search_placeholder")}
        class="w-full rounded-lg border border-neutral-700 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-100 placeholder:text-neutral-500 focus:border-indigo-500 focus:outline-none"
      />
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto p-3">
      {#if ordered.length === 0}
        <p class="py-8 text-center text-xs text-neutral-500">{locale.t("characters.no_results")}</p>
      {:else}
        <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
          {#each ordered as card (card.id)}
            {@const isSelected = card.id === selectedId}
            {@const parentName = card.parentId ? nameById.get(card.parentId) : undefined}
            <button
              type="button"
              class="flex items-start gap-2.5 rounded-lg border p-2 text-left transition-colors {isSelected
                ? 'border-indigo-500 bg-indigo-500/15'
                : 'border-neutral-800 bg-neutral-950/60 hover:border-neutral-600 hover:bg-neutral-800/70'}"
              onclick={() => {
                onselect(card);
                onclose();
              }}
            >
              <div class="h-14 w-14 shrink-0 overflow-hidden rounded-md border border-neutral-700 bg-neutral-800 flex items-center justify-center text-neutral-500">
                {#if card.thumbnail}
                  <img src={card.thumbnail} alt={card.name} class="h-full w-full object-cover" />
                {:else}
                  <svg xmlns="http://www.w3.org/2000/svg" class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
                {/if}
              </div>
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-1.5">
                  <span class="truncate text-xs font-medium text-neutral-100">{card.name}</span>
                  {#if isSelected}
                    <span class="shrink-0 text-[11px] font-semibold text-indigo-300">✓</span>
                  {/if}
                </div>
                {#if parentName || multiArch}
                  <div class="mt-0.5 flex flex-wrap items-center gap-1">
                    {#if parentName}
                      <span class="truncate rounded bg-neutral-800 px-1 py-px text-[9px] text-neutral-400">
                        ↳ {parentName}
                      </span>
                    {/if}
                    {#if multiArch}
                      <span class="rounded bg-neutral-800 px-1 py-px text-[9px] text-neutral-400">
                        {architectureLabel(card.architecture)}
                      </span>
                    {/if}
                  </div>
                {/if}
                <p class="mt-0.5 line-clamp-2 text-[11px] leading-snug text-neutral-400 break-words">
                  {card.prompt || "—"}
                </p>
                {#if card.negative}
                  <p class="mt-0.5 line-clamp-1 text-[10px] text-red-300/75 break-words">
                    {locale.t("characters.negative_prefix")} {card.negative}
                  </p>
                {/if}
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>
