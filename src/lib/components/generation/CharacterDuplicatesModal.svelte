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
</script>

<script lang="ts">
  /**
   * Review for Extract when some of what it found is already saved. Each match
   * gets a choice: merge the new details into the saved card, keep it as a
   * variant card of its own, or disregard it. Characters that matched nothing
   * are saved alongside.
   */
  import { untrack } from "svelte";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    matches: DuplicateMatch[];
    /** Names of the characters that matched nothing and are saved anyway. */
    fresh: string[];
    onsave: (decisions: DuplicateDecision[]) => void;
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

  const choices: { action: DuplicateAction; key: string }[] = [
    { action: "update", key: "characters.dupes.update" },
    { action: "variant", key: "characters.dupes.variant" },
    { action: "disregard", key: "characters.dupes.disregard" },
  ];

  const canSave = $derived(
    fresh.length > 0 ||
      rows.some((r) => r.action === "update" || (r.action === "variant" && r.variantName.trim())),
  );
</script>

<div
  class="fixed inset-0 z-210 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t("characters.dupes.title")}
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
        <h3 class="text-sm font-semibold text-neutral-100">{locale.t("characters.dupes.title")}</h3>
        <p class="mt-1 text-[11px] text-neutral-500">{locale.t("characters.dupes.desc")}</p>
      </div>
      <button
        type="button"
        class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
        onclick={onclose}
        aria-label={locale.t("common.cancel")}
      >✕</button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto space-y-3">
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

    {#if fresh.length > 0}
      <p class="mt-3 text-[11px] text-neutral-400">
        {locale.t("characters.dupes.also_new", { count: String(fresh.length), names: fresh.join(", ") })}
      </p>
    {/if}

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
        onclick={() => onsave(rows)}
      >
        {locale.t("characters.dupes.save")}
      </button>
    </div>
  </div>
</div>
