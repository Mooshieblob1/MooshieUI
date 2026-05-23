<script lang="ts">
  import { characterInsert, type CharacterTagLevel } from "../../stores/characterInsert.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    onapplied?: () => void;
  }

  let { onapplied }: Props = $props();

  const pending = $derived(characterInsert.pending);
  let selectedLevel = $state<CharacterTagLevel>("name_copyright");

  $effect(() => {
    if (pending?.step === "pick_tags") {
      selectedLevel = "name_copyright";
    }
  });

  function dismiss() {
    characterInsert.dismiss();
  }

  function confirmTagLevel() {
    characterInsert.chooseTagLevel(selectedLevel);
    if (!characterInsert.pending) onapplied?.();
  }

  function applyMode(mode: "add" | "replace") {
    const level = pending?.tagLevel;
    if (!level) return;
    characterInsert.apply(level, mode);
    onapplied?.();
  }
</script>

{#if pending}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-[300] flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
    onclick={(e) => {
      if (e.target === e.currentTarget) dismiss();
    }}
    onkeydown={(e) => {
      if (e.key === "Escape") dismiss();
    }}
    role="presentation"
  >
    <div
      class="w-full max-w-lg rounded-[var(--app-panel-radius)] border border-neutral-700 bg-neutral-900 p-5 shadow-2xl"
      role="dialog"
      aria-modal="true"
      aria-labelledby="character-insert-title"
    >
      {#if pending.step === "duplicate"}
        <h2 id="character-insert-title" class="mb-1 text-sm font-semibold text-neutral-100">
          {locale.t("character_insert.duplicate_title")}
        </h2>
        <p class="mb-4 text-xs leading-relaxed text-neutral-400">
          {locale.t("character_insert.duplicate_body", { name: pending.character.name })}
        </p>
        <div class="flex justify-end">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-neutral-500"
            onclick={dismiss}
          >
            {locale.t("common.ok")}
          </button>
        </div>
      {:else if pending.step === "pick_tags"}
        <h2 id="character-insert-title" class="mb-1 text-sm font-semibold text-neutral-100">
          {locale.t("character_insert.tags_title", { name: pending.character.name })}
        </h2>
        <p class="mb-3 text-xs text-neutral-500">{locale.t("character_insert.tags_desc")}</p>

        <div class="mb-4 space-y-2">
          {#each [
            { level: "name" as const, labelKey: "character_insert.level_name", preview: characterInsert.previewInsert("name") },
            { level: "name_copyright" as const, labelKey: "character_insert.level_name_copyright", preview: characterInsert.previewInsert("name_copyright") },
            { level: "all" as const, labelKey: "character_insert.level_all", preview: characterInsert.previewInsert("all") },
          ] as opt (opt.level)}
            <label
              class="flex cursor-pointer gap-3 rounded-lg border p-3 transition-colors {selectedLevel === opt.level
                ? 'border-indigo-500 bg-indigo-950/40'
                : 'border-neutral-800 bg-neutral-950/60 hover:border-neutral-600'}"
            >
              <input
                type="radio"
                name="character-tag-level"
                value={opt.level}
                checked={selectedLevel === opt.level}
                onchange={() => {
                  selectedLevel = opt.level;
                }}
                class="mt-0.5 accent-indigo-500"
              />
              <span class="min-w-0 flex-1">
                <span class="block text-xs font-medium text-neutral-200">{locale.t(opt.labelKey)}</span>
                <span class="mt-1 block font-mono text-[11px] leading-relaxed break-words text-neutral-500">{opt.preview}</span>
              </span>
            </label>
          {/each}
        </div>

        <div class="flex justify-end gap-2">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-neutral-500"
            onclick={dismiss}
          >
            {locale.t("common.cancel")}
          </button>
          <button
            type="button"
            class="rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
            onclick={confirmTagLevel}
          >
            {pending.analysis.needsActionChoice
              ? locale.t("common.next")
              : locale.t("character_insert.insert_btn")}
          </button>
        </div>
      {:else if pending.step === "pick_action"}
        <h2 id="character-insert-title" class="mb-1 text-sm font-semibold text-neutral-100">
          {locale.t("character_insert.action_title")}
        </h2>
        <p class="mb-3 text-xs leading-relaxed text-neutral-400">
          {#if pending.analysis.isMinimalSolo}
            {locale.t("character_insert.action_minimal_body", { name: pending.character.name })}
          {:else}
            {locale.t("character_insert.action_existing_body", {
              existing: pending.analysis.existingCharacterLabel,
              name: pending.character.name,
            })}
          {/if}
        </p>
        <p class="mb-4 rounded-lg border border-neutral-800 bg-neutral-950 px-2 py-1.5 font-mono text-[11px] break-words text-neutral-500">
          {characterInsert.previewInsert(pending.tagLevel ?? "name_copyright")}
        </p>
        <p class="mb-4 text-[11px] text-neutral-500">
          {locale.t("character_insert.action_add_hint", {
            count: characterInsert.previewGirlCountAfterAdd(),
          })}
        </p>

        <div class="flex flex-wrap justify-end gap-2">
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-neutral-500"
            onclick={dismiss}
          >
            {locale.t("common.cancel")}
          </button>
          <button
            type="button"
            class="rounded-md border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500"
            onclick={() => applyMode("add")}
          >
            {locale.t("character_insert.add_character")}
          </button>
          <button
            type="button"
            class="rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
            onclick={() => applyMode("replace")}
          >
            {locale.t("character_insert.replace_character")}
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}
