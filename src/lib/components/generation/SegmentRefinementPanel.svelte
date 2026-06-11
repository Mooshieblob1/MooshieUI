<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import {
    parseSegmentDetailPrompt,
    replaceSegmentInPrompt,
    removeSegmentFromPrompt,
    appendSegmentToPrompt,
  } from "../../utils/promptSegmentDetail.js";
  import type { DetailSegment } from "../../types/index.js";

  // Model-facing prompt fragments stay in English (same convention as quality tags).
  const suggestions = [
    { target: "eyes", prompt: "highly detailed eyes, sparkling eyes" },
    { target: "face", prompt: "highly detailed face" },
    { target: "hair", prompt: "highly detailed hair, flowing strands" },
    { target: "hands", prompt: "perfect hands, detailed fingers" },
    { target: "mouth", prompt: "detailed mouth, detailed lips" },
    { target: "background", prompt: "highly detailed background" },
  ];

  const parsed = $derived(
    generation.positivePrompt.toLowerCase().includes("<segment:")
      ? parseSegmentDetailPrompt(generation.positivePrompt)
      : { baseText: generation.positivePrompt, segments: [], ranges: [] },
  );

  let panelOpen = $state(true);

  function updateSegment(index: number, patch: Partial<DetailSegment>) {
    const current = parsed.segments[index];
    if (!current) return;
    const next = { ...current, ...patch };
    if (!next.target.trim()) return;
    generation.positivePrompt = replaceSegmentInPrompt(
      generation.positivePrompt,
      index,
      next,
    );
  }

  function removeSegment(index: number) {
    generation.positivePrompt = removeSegmentFromPrompt(generation.positivePrompt, index);
  }

  function addSuggestion(target: string, prompt: string) {
    generation.positivePrompt = appendSegmentToPrompt(
      generation.positivePrompt,
      target,
      prompt,
    );
  }
</script>

{#if parsed.segments.length > 0}
  <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-2.5 space-y-2">
    <div class="flex items-center gap-1.5">
      <button
        class="flex-1 text-left flex items-center justify-between text-xs text-neutral-400 hover:text-neutral-200 transition-colors"
        onclick={() => (panelOpen = !panelOpen)}
      >
        <span class="flex items-center gap-1.5">
          <span class="inline-block w-2 h-2 rounded-full bg-teal-400/60"></span>
          {locale.t('generation.segment.title')}
          <span class="text-[10px] text-neutral-500">({parsed.segments.length})</span>
        </span>
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {panelOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      <InfoTip text={locale.t('generation.segment.tip')} />
    </div>
    {#if panelOpen}
      <div class="space-y-1.5">
        {#each parsed.segments as seg, i (i)}
          <div class="rounded border border-teal-400/20 bg-teal-400/5 px-2 py-1.5 space-y-1.5">
            <div class="flex items-center gap-2">
              <span class="text-[10px] text-teal-300 shrink-0">#{i + 1}</span>
              <input
                type="text"
                value={seg.target}
                onchange={(e) => updateSegment(i, { target: (e.target as HTMLInputElement).value.trim() })}
                placeholder={locale.t('generation.segment.target')}
                title={locale.t('generation.segment.target_tip')}
                class="w-32 bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-[11px] text-teal-200 focus:outline-none focus:border-teal-500 transition-colors"
              />
              <input
                type="text"
                value={seg.prompt}
                onchange={(e) => updateSegment(i, { prompt: (e.target as HTMLInputElement).value.trim() })}
                placeholder={locale.t('generation.segment.prompt_placeholder')}
                class="flex-1 min-w-0 bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-[11px] text-neutral-200 focus:outline-none focus:border-teal-500 transition-colors"
              />
              <button
                class="shrink-0 px-1.5 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors"
                onclick={() => removeSegment(i)}
                title={locale.t('common.remove')}
              >
                ×
              </button>
            </div>
            <div class="grid grid-cols-2 gap-3">
              <div use:scrollCapture>
                <label class="flex items-center justify-between text-[10px] text-neutral-400 mb-0.5">
                  <span>{locale.t('generation.segment.creativity')}<InfoTip text={locale.t('generation.segment.creativity_tip')} /></span>
                  <EditableValue value={seg.creativity} min={0.05} max={1} step={0.05} decimals={2} onchange={(v) => updateSegment(i, { creativity: v })} />
                </label>
                <input
                  type="range"
                  value={seg.creativity}
                  oninput={(e) => updateSegment(i, { creativity: parseFloat((e.target as HTMLInputElement).value) })}
                  min="0.05"
                  max="1"
                  step="0.05"
                  class="w-full accent-teal-500"
                />
              </div>
              <div use:scrollCapture>
                <label class="flex items-center justify-between text-[10px] text-neutral-400 mb-0.5">
                  <span>{locale.t('generation.segment.threshold')}<InfoTip text={locale.t('generation.segment.threshold_tip')} /></span>
                  <EditableValue value={seg.threshold} min={0.05} max={0.95} step={0.05} decimals={2} onchange={(v) => updateSegment(i, { threshold: v })} />
                </label>
                <input
                  type="range"
                  value={seg.threshold}
                  oninput={(e) => updateSegment(i, { threshold: parseFloat((e.target as HTMLInputElement).value) })}
                  min="0.05"
                  max="0.95"
                  step="0.05"
                  class="w-full accent-teal-500"
                />
              </div>
            </div>
          </div>
        {/each}
      </div>
      <div class="flex flex-wrap items-center gap-1 pt-0.5">
        <span class="text-[10px] text-neutral-500 mr-0.5">{locale.t('generation.segment.suggestions')}</span>
        {#each suggestions as s (s.target)}
          <button
            type="button"
            onclick={() => addSuggestion(s.target, s.prompt)}
            class="shrink-0 inline-flex items-center gap-1 rounded-full border border-teal-500/30 bg-teal-500/5 text-teal-300/90 hover:bg-teal-500/15 hover:border-teal-500/60 px-2 py-0.5 text-[10px] transition-colors"
            title={locale.t('generation.segment.add_suggestion', { target: s.target })}
          >
            <span class="leading-none">+</span>
            <span>{s.target}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}
