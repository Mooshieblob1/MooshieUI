<script lang="ts">
  import { portal } from "../../utils/portal.js";
  import { locale } from "../../stores/locale.svelte.js";
  import type { ActivePreciseRefItem } from "../../utils/characterPreciseRefLogic.js";
  import { PRECISE_REFERENCE_COST } from "../../utils/novelaiCost.js";
  import { novelAiBase64ToSrc } from "../../utils/novelaiImage.js";
  import { novelAiReferenceLabelKey } from "../../utils/novelaiModels.js";

  interface Props {
    anchor: HTMLElement | null;
    items: readonly ActivePreciseRefItem[];
    hint?: string;
    onDismiss?: () => void;
  }

  let { anchor, items, hint, onDismiss }: Props = $props();

  const CARD_W = 260;

  let coords = $state<{ left: number; top: number; above: boolean } | null>(null);

  const visible = $derived(Boolean(anchor && anchor.isConnected && items.length > 0));

  function updateCoords() {
    if (!anchor || !anchor.isConnected) {
      coords = null;
      return;
    }
    const rect = anchor.getBoundingClientRect();
    const above = rect.top >= 150;
    const centerLeft = rect.left + rect.width / 2 - CARD_W / 2;
    const maxLeft = Math.max(8, window.innerWidth - CARD_W - 8);
    coords = {
      left: Math.max(8, Math.min(maxLeft, centerLeft)),
      top: above ? rect.top - 8 : rect.bottom + 8,
      above,
    };
  }

  $effect(() => {
    void anchor;
    void items.length;
    if (!visible) {
      coords = null;
      return;
    }
    updateCoords();
    const dismiss = () => onDismiss?.();
    window.addEventListener("scroll", dismiss, { passive: true, capture: true });
    window.addEventListener("pointerdown", dismiss, { passive: true });
    window.addEventListener("resize", updateCoords, { passive: true });
    return () => {
      window.removeEventListener("scroll", dismiss, { capture: true });
      window.removeEventListener("pointerdown", dismiss);
      window.removeEventListener("resize", updateCoords);
    };
  });
</script>

{#if visible && coords}
  <div
    use:portal
    role="tooltip"
    class="fixed z-[95] pointer-events-none w-[260px] rounded-lg border border-amber-500/50 bg-neutral-950/95 p-2 shadow-2xl backdrop-blur-sm {coords.above
      ? '-translate-y-full'
      : ''}"
    style="left: {coords.left}px; top: {coords.top}px;"
  >
    <div class="flex items-center justify-between gap-2 text-[11px]">
      <span class="flex items-center gap-1.5 font-semibold text-amber-200">
        <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400" aria-hidden="true"></span>
        <span>{locale.t("generation.novelai.reference.hover_active_on")}</span>
      </span>
      <span class="shrink-0 rounded bg-amber-500/20 px-1 py-px font-mono text-[10px] text-amber-200">
        {locale.t("characters.references.anlas_badge", {
          anlas: String(items.length * PRECISE_REFERENCE_COST),
        })}
      </span>
    </div>
    <div class="mt-1.5 space-y-1.5">
      {#each items as item (item.index)}
        {@const modeLabel = locale.t(novelAiReferenceLabelKey(item.baseType))}
        {@const primaryLabel = item.characterName ?? modeLabel}
        {@const secondaryBase = item.characterName
          ? modeLabel
          : locale.t("generation.novelai.reference.hover_manual")}
        {@const secondaryLabel = item.preferSameOutfit
          ? `${secondaryBase} · ${locale.t("generation.novelai.reference.prefer_same_outfit")}`
          : secondaryBase}
        <div class="flex items-center gap-2">
          <img
            src={novelAiBase64ToSrc(item.image)}
            alt=""
            class="h-7 w-7 shrink-0 rounded border border-amber-500/35 object-cover"
          />
          <div class="min-w-0 flex-1">
            <div class="truncate text-[11px] font-semibold text-amber-100">{primaryLabel}</div>
            <div class="truncate text-[10px] text-neutral-400">{secondaryLabel}</div>
          </div>
          <span class="shrink-0 font-mono text-[10px] tabular-nums text-amber-300">
            +{PRECISE_REFERENCE_COST}
          </span>
        </div>
      {/each}
    </div>
    {#if hint}
      <div class="mt-1.5 border-t border-neutral-800 pt-1 text-[10px] text-neutral-400">{hint}</div>
    {/if}
  </div>
{/if}
