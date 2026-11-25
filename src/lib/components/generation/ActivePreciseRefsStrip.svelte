<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { savedCharacters } from "../../stores/savedCharacters.svelte.js";
  import {
    describeActiveNovelAiDirectorReferences,
    formatActivePreciseRefTargets,
  } from "../../utils/characterPreciseRefLogic.js";
  import { PRECISE_REFERENCE_COST } from "../../utils/novelaiCost.js";
  import { novelAiBase64ToSrc } from "../../utils/novelaiImage.js";
  import { NAI_REF_CHARACTER, novelAiReferenceLabelKey } from "../../utils/novelaiModels.js";

  interface Props {
    compact?: boolean;
    onJumpToPreciseRef?: () => void;
  }

  let { compact = false, onJumpToPreciseRef }: Props = $props();

  const summary = $derived(
    describeActiveNovelAiDirectorReferences(
      generation.isNovelAi ? generation.novelAiModel : null,
      generation.novelaiSettings.director_references,
      generation.isNovelAiInfill,
      savedCharacters.characters,
    ),
  );

  const targets = $derived(
    formatActivePreciseRefTargets(summary.items, (baseType) =>
      locale.t(novelAiReferenceLabelKey(baseType)),
    ),
  );

  const activeCountByCharId = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const item of summary.items) {
      if (!item.characterId) continue;
      counts.set(item.characterId, (counts.get(item.characterId) ?? 0) + 1);
    }
    return counts;
  });

  const headerTip = $derived(
    summary.count > 0
      ? locale.t("generation.novelai.reference.active_header_tip", {
          count: String(summary.count),
          targets: targets.detailed,
          anlas: String(summary.anlas),
        })
      : "",
  );
</script>

{#if summary.count > 0}
  {#if compact}
    <div class="mb-1 flex items-center gap-1 overflow-x-auto">
      <button
        type="button"
        onclick={() => onJumpToPreciseRef?.()}
        title={headerTip}
        class="inline-flex min-w-0 shrink items-center gap-1.5 rounded-full border border-amber-500/50 bg-amber-500/15 px-2 py-0.5 text-[10px] font-medium text-amber-200 hover:bg-amber-500/25 transition-colors"
      >
        <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400" aria-hidden="true"></span>
        <span class="shrink-0 font-semibold">{locale.t("characters.references.pill_label")}</span>
        <span class="max-w-40 truncate">{targets.short}</span>
        <span class="shrink-0 font-mono text-[9px] text-amber-300">+{summary.anlas}</span>
      </button>
      <button
        type="button"
        onclick={() => generation.clearNovelAiDirectorReferences()}
        title={locale.t("generation.novelai.reference.clear_all")}
        aria-label={locale.t("generation.novelai.reference.clear_all")}
        class="shrink-0 rounded-full border border-amber-500/40 bg-neutral-900/70 px-1.5 py-0.5 text-[10px] text-amber-200/80 hover:border-red-500/50 hover:bg-red-500/15 hover:text-red-200 transition-colors"
      >
        ×
      </button>
    </div>
  {:else}
    <div class="mb-2 space-y-1.5 rounded-lg border border-amber-500/45 bg-amber-500/10 px-2 py-1.5">
      <div class="flex items-center justify-between gap-2 text-[11px]">
        <span class="flex min-w-0 items-center gap-1.5 font-medium text-amber-200" title={headerTip}>
          <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400" aria-hidden="true"></span>
          <span class="truncate">{locale.t("generation.novelai.reference.title")}</span>
          <span class="shrink-0 rounded bg-amber-500/20 px-1 py-px font-mono text-[10px] text-amber-200">
            {locale.t("characters.references.anlas_badge", { anlas: String(summary.anlas) })}
          </span>
        </span>
        {#if summary.count >= 2}
          <button
            type="button"
            onclick={() => generation.clearNovelAiDirectorReferences()}
            class="shrink-0 text-[10px] text-amber-200/80 hover:text-red-200 transition-colors"
          >
            {locale.t("generation.novelai.reference.clear_all")}
          </button>
        {/if}
      </div>
      <div class="flex flex-wrap gap-1">
        {#each summary.items as item (item.index)}
          {@const modeLabel = locale.t(novelAiReferenceLabelKey(item.baseType))}
          {@const targetLabel = item.characterName ?? modeLabel}
          {@const showSecondaryMode =
            Boolean(item.characterName) &&
            (item.baseType !== NAI_REF_CHARACTER ||
              (activeCountByCharId.get(item.characterId ?? "") ?? 0) > 1)}
          {@const itemTip = locale.t("generation.novelai.reference.active_strip_tip", {
            target: targetLabel,
            mode: modeLabel,
            anlas: String(PRECISE_REFERENCE_COST),
          })}
          {@const removeAria = locale.t("generation.novelai.reference.remove_aria", {
            target: targetLabel,
          })}
          <div class="inline-flex max-w-full items-center overflow-hidden rounded-full border border-amber-500/50 bg-neutral-950/60 text-[10px] text-amber-100">
            <button
              type="button"
              onclick={() => onJumpToPreciseRef?.()}
              title={itemTip}
              class="inline-flex min-w-0 items-center gap-1 py-0.5 pl-1 pr-1.5 hover:bg-amber-500/15 transition-colors"
            >
              <img src={novelAiBase64ToSrc(item.image)} alt="" class="h-4 w-4 shrink-0 rounded-sm object-cover" />
              <span class="max-w-28 truncate font-medium">{targetLabel}</span>
              {#if showSecondaryMode}
                <span class="shrink-0 text-[9px] text-amber-300/80">({modeLabel})</span>
              {/if}
            </button>
            <button
              type="button"
              onclick={() => generation.removeNovelAiDirectorReference(item.index)}
              title={locale.t("generation.novelai.reference.remove")}
              aria-label={removeAria}
              class="inline-flex h-5 items-center border-l border-amber-500/30 px-1.5 text-amber-300/80 hover:bg-red-500/20 hover:text-red-200 transition-colors"
            >
              ×
            </button>
          </div>
        {/each}
      </div>
    </div>
  {/if}
{/if}
