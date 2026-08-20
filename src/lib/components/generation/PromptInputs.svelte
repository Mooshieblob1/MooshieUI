<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { artistFavourites } from "../../artist-gallery/favourites.svelte.js";
  import { detectArtistsInPrompt } from "../../artist-gallery/detection.js";
  import { styles } from "../../stores/styles.svelte.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import PromptTextarea from "./PromptTextarea.svelte";
  import ExtraPromptBoxList from "./ExtraPromptBoxList.svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import { parseScheduledPrompt, hasRegionalTags, hasSchedulingTags } from "../../utils/promptSchedule.js";
  import { joinPromptBoxes } from "../../utils/promptSanitize.js";
  import { estimatePromptTokens } from "../../utils/promptTokens.js";
  import SegmentRefinementPanel from "./SegmentRefinementPanel.svelte";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import PromptAssistantSetupModal from "./PromptAssistantSetupModal.svelte";
  import PromptComposeModal from "./PromptComposeModal.svelte";
  import H3PromptGuide from "../video/H3PromptGuide.svelte";
  import { buildH3Context } from "../../utils/h3Prompt.js";

  interface Props {
    showHistory?: boolean;
    onOpenRegionalPrompt?: () => void;
  }

  let { showHistory = true, onOpenRegionalPrompt }: Props = $props();

  // NovelAI-style combined prompt view: one box with a Positive/Negative
  // switcher. Persisted (UI layout pref → localStorage, not generation
  // settings); the active tab itself is session-only.
  const COMBINED_KEY = "mooshieui.prompts.combined.v1";
  let combinedMode = $state(localStorage.getItem(COMBINED_KEY) === "true");
  let activeTab = $state<"positive" | "negative">("positive");
  $effect(() => {
    const val = String(combinedMode);
    try { localStorage.setItem(COMBINED_KEY, val); } catch {}
  });

  // H3 reads prose in its own trained section format, not danbooru tags, and it
  // has no regional conditioning to steer. So video mode drops the whole
  // tag-shaped toolbar - quality tags, the region editor, compose, autocomplete -
  // rather than leaving affordances that misfire or do nothing.
  const isVideoMode = $derived(generation.mode === "video");

  const hasPositiveSchedule = $derived(hasSchedulingTags(generation.positivePrompt));
  const regionalPromptingSupported = $derived(generation.supportsRegionalPrompting);
  const hasRegionalPrompting = $derived(
    hasRegionalTags(generation.positivePrompt) || generation.regionalPrompts.length > 0,
  );
  const qualityTagsSupported = $derived(
    !isVideoMode &&
      generation.autoQualityTags &&
      (generation.isAnima || generation.isIllustrious || generation.isPony || generation.isNanosaur),
  );
  const hasNegativeSchedule = $derived(hasSchedulingTags(generation.negativePrompt));
  const hasAnySchedule = $derived(hasPositiveSchedule || hasNegativeSchedule);

  // Combined token estimate across the main positive box and any extra boxes,
  // shown as a badge only when extra positive boxes exist. Uses the same
  // chronological concatenation that toParams sends.
  const combinedPositiveTokens = $derived(
    generation.extraPositiveBoxes.length > 0
      ? estimatePromptTokens(
          joinPromptBoxes([
            generation.positivePrompt,
            ...generation.extraPositiveBoxes.map((b) => b.content),
          ]),
        )
      : 0,
  );
  const positiveSegments = $derived(hasPositiveSchedule ? parseScheduledPrompt(generation.positivePrompt).segments : []);
  const negativeSegments = $derived(hasNegativeSchedule ? parseScheduledPrompt(generation.negativePrompt).segments : []);
  let schedulePanelOpen = $state(true);

  /** Artist tags detected in the current positive prompt. */
  const detectedArtists = $derived.by(() => {
    // Avoid fetching the ~6 MB artist index just for prompt heart chips.
    // If the gallery loads it elsewhere later, these chips still light up.
    if (!gallery.artistIndexReady || gallery.artistTagIndex.size === 0) return [];
    return detectArtistsInPrompt(generation.positivePrompt, gallery.artistTagIndex);
  });

  const sortedPromptHistory = $derived(
    [...generation.promptHistory].sort((a, b) => {
      if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
      return b.createdAt - a.createdAt;
    }).slice(0, 12)
  );
  let historySectionOpen = $state(true);

  function historyLabel(ts: number): string {
    return new Date(ts).toLocaleString(locale.intlTag, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  let undoSnapshot = $state<string | null>(null);
  let showUndo = $state(false);
  let undoTimer: ReturnType<typeof setTimeout> | null = null;
  let _pendingAction = $state<"enhance" | "enhance_h3" | "compose" | null>(null);

  async function onEnhanceClick() {
    if (!promptAssistant.isAvailable) {
      _pendingAction = "enhance";
      promptAssistant.setupModalOpen = true;
      return;
    }
    await runEnhance();
  }

  async function runEnhance() {
    const current = generation.positivePrompt?.trim();
    if (!current) return;
    try {
      const result = await promptAssistant.enhance(current, generation.modelFamily);
      if (result && result.trim()) {
        undoSnapshot = generation.positivePrompt;
        generation.positivePrompt = result;
        generation.saveSettings();
        triggerUndo();
      } else {
        gallery.showToast(locale.t("prompt_assistant.couldnt_enhance"), "error");
      }
    } catch (e) {
      console.error("Prompt enhance failed:", e);
      gallery.showToast(mapLlmError(String(e)), "error");
    }
  }

  async function onEnhanceH3Click() {
    if (!promptAssistant.isAvailable) {
      _pendingAction = "enhance_h3";
      promptAssistant.setupModalOpen = true;
      return;
    }
    await runEnhanceH3();
  }

  async function runEnhanceH3() {
    const current = generation.positivePrompt?.trim();
    if (!current) return;
    try {
      const result = await promptAssistant.enhanceForH3(
        current,
        buildH3Context({
          variant: generation.videoVariant,
          frames: generation.videoFrameLength,
          hasFirstFrame: !!generation.videoFirstFrame,
          hasLastFrame: !!generation.videoEffectiveLastFrame,
          referenceImageCount: generation.videoRefImageFilenames.length,
        }),
        generation.videoFirstFrame,
      );
      if (!result.text) {
        gallery.showToast(locale.t("prompt_assistant.couldnt_enhance"), "error");
        return;
      }
      undoSnapshot = generation.positivePrompt;
      generation.positivePrompt = result.text;
      generation.saveSettings();
      triggerUndo();
      // Applied either way: a near-miss rewrite is still a better starting point
      // than the prose it replaced, and undo is one click away. The warning names
      // the rule the model broke so the guide below shows what to fix by hand.
      if (!result.ok) {
        gallery.showToast(
          locale.t("prompt_assistant.h3_format_warning", {
            rule: result.rule ?? "",
          }),
          "warning",
          { durationMs: 12000 },
        );
      } else if (result.idle) {
        // Idle mode is invisible until it fires, which is the point. Saying so
        // once is what keeps "the pose never changes" from reading as a failure.
        gallery.showToast(locale.t("prompt_assistant.h3_idle_applied"), "info", {
          durationMs: 8000,
        });
      }
    } catch (e) {
      console.error("H3 prompt rewrite failed:", e);
      gallery.showToast(mapLlmError(String(e)), "error");
    }
  }

  function onComposeClick() {
    if (!promptAssistant.isAvailable) {
      _pendingAction = "compose";
      promptAssistant.setupModalOpen = true;
      return;
    }
    promptAssistant.composeModalOpen = true;
  }

  function triggerUndo() {
    showUndo = true;
    if (undoTimer) clearTimeout(undoTimer);
    undoTimer = setTimeout(() => (showUndo = false), 10000);
  }

  function undoEnhance() {
    if (undoSnapshot !== null) {
      generation.positivePrompt = undoSnapshot;
      generation.saveSettings();
      undoSnapshot = null;
    }
    showUndo = false;
  }

  function mapLlmError(msg: string): string {
    if (msg.includes("busy_generation")) return locale.t("prompt_assistant.busy_generation");
    if (msg.includes("no_model")) return locale.t("prompt_assistant.no_model");
    // Surface the real backend reason (llama-server crash tail, missing shared
    // library, health timeout, etc.) instead of a generic message — the detailed
    // string is what makes a failed enhance diagnosable, especially on headless
    // server deployments where the only signal the user sees is this toast.
    const detail = msg.replace(/^Error:\s*/, "").trim();
    return detail
      ? `${locale.t("prompt_assistant.error_generic")}: ${detail}`
      : locale.t("prompt_assistant.error_generic");
  }

  function onSetupInstalled() {
    const action = _pendingAction;
    _pendingAction = null;
    if (action === "enhance") runEnhance();
    else if (action === "enhance_h3") runEnhanceH3();
    else if (action === "compose") promptAssistant.composeModalOpen = true;
  }

</script>

<div class="space-y-2">
  {#if generation.stylePresetsEnabled}
    <div>
      <label class="block text-xs text-neutral-400 mb-1">{locale.t('generation.prompts.style_preset')}<InfoTip text={locale.t('generation.prompts.style_preset_tip')} /></label>
      <select
        bind:value={generation.stylePreset}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each generation.stylePresetOptions as preset (preset.id)}
          <option value={preset.id}>{preset.label}</option>
        {/each}
      </select>
    </div>
  {/if}

  <div>
    <div class="flex items-center justify-between gap-2 mb-1">
      <div class="flex items-center gap-1.5 shrink-0">
        {#if combinedMode}
          <div class="flex gap-1 bg-neutral-900 rounded-lg p-1">
            <button
              type="button"
              class="px-2.5 py-0.5 text-xs rounded-md transition-colors {activeTab === 'positive' ? 'bg-neutral-700 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
              onclick={() => (activeTab = 'positive')}
            >{locale.t('generation.prompts.tab_positive')}</button>
            <button
              type="button"
              class="inline-flex items-center gap-1 px-2.5 py-0.5 text-xs rounded-md transition-colors {activeTab === 'negative' ? 'bg-neutral-700 text-white' : 'text-neutral-400 hover:text-neutral-200'}"
              onclick={() => (activeTab = 'negative')}
            >
              {locale.t('generation.prompts.tab_negative')}
              {#if generation.disablesNegativePrompt}
                <span class="inline-block h-1.5 w-1.5 rounded-full bg-amber-400" aria-hidden="true" title={locale.t('generation.prompts.negative_disabled_for_model')}></span>
              {/if}
            </button>
          </div>
        {:else}
          <label class="text-xs text-neutral-400">{locale.t('generation.prompts.positive')}<InfoTip text={locale.t('generation.prompts.positive_tip')} /></label>
        {/if}
      </div>
      <div class="flex items-center justify-end gap-1.5 flex-wrap min-w-0">
      {#if combinedPositiveTokens > 0}
        <span
          class="shrink-0 text-[10px] px-2 py-0.5 rounded-full bg-neutral-800 text-neutral-300 border border-neutral-700 tabular-nums"
          title={locale.t('generation.prompts.extra_box_combined_tip')}
        >{locale.t('generation.prompts.extra_box_combined_tokens', { count: String(combinedPositiveTokens) })}</span>
      {/if}
      {#if qualityTagsSupported}
        <span class="shrink-0 text-[10px] px-2 py-0.5 rounded-full bg-emerald-600/20 text-emerald-400 border border-emerald-600/30">{locale.t('generation.prompts.quality_applied')}</span>
      {/if}
      {#each styles.activeStyles as activeStyle (activeStyle.id)}
        <button
          type="button"
          onclick={() => styles.deactivate(activeStyle.id)}
          class="shrink-0 inline-flex items-center gap-1 rounded-full border border-indigo-500/50 bg-indigo-500/10 text-indigo-200 hover:bg-red-500/15 hover:border-red-500/50 hover:text-red-200 px-2 py-0.5 text-[10px] transition-colors"
          title={locale.t('generation.prompts.style_deactivate_title', { count: activeStyle.artists.length, weight: locale.formatDecimal(activeStyle.overallWeight, 2) })}
          aria-label={locale.t('generation.prompts.style_deactivate_aria', { name: activeStyle.name })}
        >
          {#if activeStyle.thumbnail}
            <img src={activeStyle.thumbnail} alt="" class="h-3.5 w-3.5 rounded-sm object-cover" />
          {:else}
            <span class="inline-block h-1.5 w-1.5 rounded-full bg-indigo-400" aria-hidden="true"></span>
          {/if}
          <span class="leading-none">✦</span>
          <span class="max-w-28 truncate">{activeStyle.name}</span>
          <span class="font-mono text-[9px] text-indigo-300/80">×{locale.formatDecimal(activeStyle.overallWeight, 2)}</span>
        </button>
      {/each}
      {#each promptPresets.activeEntries as entry (entry.preset.id)}
        {@const icon = entry.mode === "prepend" ? "↑" : entry.mode === "append" ? "↓" : entry.mode === "wildcard_ordered" ? "1→" : "🎲"}
        {@const modeLabel = entry.mode === "wildcard_ordered" ? locale.t('generation.prompts.preset_mode_wildcard_ordered') : entry.mode}
        <button
          type="button"
          onclick={() => promptPresets.deactivate(entry.preset.id)}
          class="shrink-0 inline-flex items-center gap-1 rounded-full border border-indigo-500/50 bg-indigo-500/10 text-indigo-200 hover:bg-red-500/15 hover:border-red-500/50 hover:text-red-200 px-2 py-0.5 text-[10px] transition-colors"
          title={locale.t('generation.prompts.preset_deactivate_title', { mode: modeLabel })}
          aria-label={locale.t('generation.prompts.preset_deactivate_aria', { name: entry.preset.name })}
        >
          <span class="leading-none">⚡</span>
          <span class="max-w-28 truncate">{entry.preset.name}</span>
          <span class="font-mono text-[9px] text-indigo-300/80">{icon}</span>
        </button>
      {/each}
      {#each detectedArtists as hit (hit.slug)}
        {@const isFav = artistFavourites.isFavourite(hit.slug)}
        {@const favCat = artistFavourites.categoryOf(hit.slug)}
        {@const displayName = hit.tag.replace(/^@/, "").replace(/\\([()\[\]])/g, "$1")}
        <button
          type="button"
          onclick={() => artistFavourites.toggle(hit.slug)}
          class="shrink-0 inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] transition-colors {isFav ? 'border-red-500/50 bg-red-500/10 text-red-300 hover:bg-red-500/20' : 'border-neutral-700 bg-neutral-800/60 text-neutral-400 hover:border-red-500/60 hover:text-red-300'}"
          title={isFav ? locale.t('generation.prompts.favourite_remove_title', { tag: hit.tag }) : locale.t('generation.prompts.favourite_add_title', { tag: hit.tag })}
          aria-label={isFav ? locale.t('generation.prompts.favourite_remove_aria', { name: displayName }) : locale.t('generation.prompts.favourite_add_aria', { name: displayName })}
        >
          {#if favCat}
            <span class="h-2 w-2 rounded-full border border-black/20" style="background-color: {favCat.color}" aria-hidden="true"></span>
          {/if}
          <span class="leading-none">{isFav ? '♥' : '♡'}</span>
          <span class="font-mono max-w-28 truncate">@{displayName}</span>
        </button>
      {/each}
      {#if combinedMode && activeTab === "negative"}
        {@render combineToggle()}
      {/if}
      </div>
    </div>
    {#if combinedMode && activeTab === "negative"}
      {@render negativeFields()}
    {:else}
    <div class="mb-1 flex items-center justify-between">
      <div class="flex items-center gap-1.5">
        <!-- One enhance button, two rewrites. The image one grounds on danbooru
             tags, which is exactly wrong for H3 prose, so video mode swaps it
             out rather than offering both. -->
        <button
          class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
          disabled={promptAssistant.isGenerating || !generation.positivePrompt?.trim()}
          title={isVideoMode
            ? locale.t("prompt_assistant.enhance_h3_tooltip")
            : locale.t("prompt_assistant.enhance_tooltip")}
          onclick={isVideoMode ? onEnhanceH3Click : onEnhanceClick}
        >
          {#if promptAssistant.isGenerating}
            <span class="inline-block animate-spin">⟳</span>
          {:else}
            ✨
          {/if}
          {isVideoMode
            ? locale.t("prompt_assistant.enhance_h3")
            : locale.t("prompt_assistant.enhance")}
        </button>
        {#if !isVideoMode}
          <!-- Compose builds a tag list from a description. Nothing downstream of
               it fits H3, which wants the prose the user already wrote. -->
          <button
            class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800 disabled:opacity-40"
            disabled={promptAssistant.isGenerating}
            title={locale.t("prompt_assistant.compose_tooltip")}
            onclick={onComposeClick}
          >
            ✍ {locale.t("prompt_assistant.compose")}
          </button>
        {/if}
        {#if showUndo}
          <button
            class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-indigo-400 hover:bg-neutral-800"
            onclick={undoEnhance}
          >
            ↩ {locale.t("prompt_assistant.undo")}
          </button>
        {/if}
        {#if promptAssistant.stage === "loading_model"}
          <span class="text-[10px] text-neutral-400">{locale.t("prompt_assistant.loading_model")}</span>
        {/if}
      </div>
      <div class="flex items-center gap-1.5">
        {#if !isVideoMode}
          <button
            type="button"
            disabled={!regionalPromptingSupported}
            onclick={() => {
              if (!regionalPromptingSupported) {
                gallery.showToast(locale.t("generation.regional.unsupported"), "warning");
                return;
              }
              onOpenRegionalPrompt?.();
            }}
            class="rounded-lg border px-2 py-0.5 text-[10px] transition-colors disabled:cursor-not-allowed {regionalPromptingSupported
              ? 'border-neutral-700 bg-neutral-900 text-neutral-300 hover:border-indigo-500 hover:text-indigo-200'
              : 'border-neutral-800 bg-neutral-950 text-neutral-500'}"
            title={!regionalPromptingSupported ? locale.t("generation.regional.unsupported") : undefined}
          >
            {locale.t("generation.regional.button", { count: String(generation.regionalPrompts.length) })}
          </button>
        {/if}
        {@render combineToggle()}
      </div>
    </div>
    {#if generation.isAnima}
      <div class="text-[10px] text-amber-400/80 mb-1">{locale.t('generation.prompts.anima_artist_tip')}</div>
    {/if}
    <PromptTextarea
      bind:value={generation.positivePrompt}
      placeholder={generation.isAnima ? locale.t("generation.prompts.positive_placeholder_anima") : locale.t("generation.prompts.positive_placeholder")}
      rows={4}
      minHeight="min-h-25"
      storageKey="mooshieui.promptHeight.positive"
      tagAssist={!isVideoMode}
      highlightLoraWords={true}
    />
    {#if !isVideoMode && hasRegionalPrompting && !regionalPromptingSupported}
      <p class="mt-1 text-[10px] text-amber-300">
        {locale.t("generation.regional.unsupported")}
      </p>
    {/if}
    <ExtraPromptBoxList side="positive" />
    {/if}
  </div>

  {#snippet combineToggle()}
    <button
      type="button"
      onclick={() => (combinedMode = !combinedMode)}
      class="shrink-0 text-neutral-400 hover:text-neutral-200 transition-colors p-0.5"
      title={combinedMode ? locale.t('generation.prompts.combined_mode_split') : locale.t('generation.prompts.combined_mode_toggle')}
      aria-label={combinedMode ? locale.t('generation.prompts.combined_mode_split') : locale.t('generation.prompts.combined_mode_toggle')}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><line x1="12" y1="4" x2="12" y2="20"/></svg>
    </button>
  {/snippet}

  {#snippet negativeFields()}
  <div class="transition-opacity {generation.disablesNegativePrompt ? 'opacity-40 pointer-events-none' : ''}">
    <label class="block text-xs text-neutral-400 mb-1">
      {locale.t('generation.prompts.negative')}<InfoTip text={locale.t('generation.prompts.negative_tip')} />
      {#if generation.disablesNegativePrompt}
        <span class="ml-1 text-[10px] text-amber-400">({locale.t('generation.prompts.negative_disabled_for_model')})</span>
      {/if}
    </label>
    <PromptTextarea
      bind:value={generation.negativePrompt}
      placeholder={locale.t('generation.prompts.negative_placeholder')}
      rows={3}
      minHeight="min-h-18"
      storageKey="mooshieui.promptHeight.negative"
      tagAssist={!isVideoMode}
    />
    <ExtraPromptBoxList side="negative" />
  </div>
  {/snippet}

  {#if !combinedMode}
    {@render negativeFields()}
  {/if}

  {#if hasAnySchedule}
    <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-2.5 space-y-2">
      <button
        class="w-full text-left flex items-center justify-between text-xs text-neutral-400 hover:text-neutral-200 transition-colors"
        onclick={() => (schedulePanelOpen = !schedulePanelOpen)}
      >
        <span class="flex items-center gap-1.5">
          <span class="inline-block w-2 h-2 rounded-full bg-amber-400/60"></span>
          {locale.t('generation.prompts.scheduling')}
          <span class="text-[10px] text-neutral-500">({locale.t('generation.prompts.scheduling_segments', { count: String(positiveSegments.length + negativeSegments.length) })})</span>
        </span>
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {schedulePanelOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      {#if schedulePanelOpen}
        <div class="space-y-1.5">
          {#each positiveSegments as seg, i}
            <div class="flex items-center gap-2 rounded border border-amber-400/20 bg-amber-400/5 px-2 py-1.5">
              <span class="text-[10px] text-amber-300 shrink-0">+{i + 1}</span>
              <div class="flex-1 min-w-0">
                <p class="text-[11px] text-neutral-200 truncate">{seg.text}</p>
                <div class="mt-1 h-1.5 w-full rounded-full bg-neutral-800 overflow-hidden">
                  <div
                    class="h-full rounded-full bg-amber-400/50"
                    style="margin-left: {seg.start * 100}%; width: {(seg.end - seg.start) * 100}%;"
                  ></div>
                </div>
              </div>
              <span class="text-[10px] text-neutral-500 shrink-0">{Math.round(seg.start * 100)}%–{Math.round(seg.end * 100)}%</span>
            </div>
          {/each}
          {#each negativeSegments as seg, i}
            <div class="flex items-center gap-2 rounded border border-amber-400/20 bg-amber-400/5 px-2 py-1.5">
              <span class="text-[10px] text-amber-300 shrink-0">-{i + 1}</span>
              <div class="flex-1 min-w-0">
                <p class="text-[11px] text-neutral-200 truncate">{seg.text}</p>
                <div class="mt-1 h-1.5 w-full rounded-full bg-neutral-800 overflow-hidden">
                  <div
                    class="h-full rounded-full bg-amber-400/50"
                    style="margin-left: {seg.start * 100}%; width: {(seg.end - seg.start) * 100}%;"
                  ></div>
                </div>
              </div>
              <span class="text-[10px] text-neutral-500 shrink-0">{Math.round(seg.start * 100)}%–{Math.round(seg.end * 100)}%</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <SegmentRefinementPanel />

  {#if isVideoMode}
    <H3PromptGuide />
  {/if}

  {#if showHistory && sortedPromptHistory.length > 0}
    <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-2.5 space-y-2">
      <div class="flex items-center justify-between">
        <button
          class="w-full text-left flex items-center justify-between text-xs text-neutral-400 hover:text-neutral-200 transition-colors"
          onclick={() => (historySectionOpen = !historySectionOpen)}
          title={historySectionOpen ? locale.t('generation.prompts.history_collapse_title') : locale.t('generation.prompts.history_expand_title')}
        >
          <span>{locale.t('generation.prompts.history')}<InfoTip text={locale.t('generation.prompts.history_tip')} /></span>
          <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {historySectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
        </button>
      </div>
      {#if historySectionOpen}
        <div class="space-y-1.5 max-h-56 overflow-y-auto pr-1">
          {#each sortedPromptHistory as entry}
            <div class="rounded border border-neutral-800 bg-neutral-900/80 p-2">
              <button
                class="w-full text-left"
                onclick={() => generation.applyPromptHistoryEntry(entry.id)}
                title={locale.t('bottom_panel.load_prompt')}
              >
                <p class="text-[11px] text-neutral-200 max-h-8 overflow-hidden">{entry.positivePrompt || locale.t('bottom_panel.empty_prompt')}</p>
                {#if entry.negativePrompt}
                  <p class="text-[10px] text-neutral-500 mt-0.5 whitespace-nowrap overflow-hidden text-ellipsis">{locale.t('bottom_panel.neg_prefix')} {entry.negativePrompt}</p>
                {/if}
              </button>
              <div class="mt-1.5 flex items-center justify-between gap-2">
                <span class="text-[10px] text-neutral-500">{historyLabel(entry.createdAt)}</span>
                <div class="flex items-center gap-1">
                  <button
                    class="px-1.5 py-0.5 text-[10px] rounded border transition-colors {entry.favorite ? 'border-amber-500 text-amber-300 bg-amber-500/10' : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-300'}"
                    onclick={() => generation.togglePromptFavorite(entry.id)}
                    title={entry.favorite ? locale.t('bottom_panel.unfavorite') : locale.t('bottom_panel.favorite')}
                  >
                    ★
                  </button>
                  <button
                    class="px-1.5 py-0.5 text-[10px] rounded border border-neutral-700 text-neutral-400 hover:border-red-500 hover:text-red-300 transition-colors"
                    onclick={() => generation.removePromptHistoryEntry(entry.id)}
                    title={locale.t('common.remove')}
                  >
                    {locale.t('common.remove')}
                  </button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>
{#if promptAssistant.setupModalOpen}
  <PromptAssistantSetupModal
    onClose={() => (promptAssistant.setupModalOpen = false)}
    onInstalled={onSetupInstalled}
  />
{/if}
{#if promptAssistant.composeModalOpen}
  <PromptComposeModal onClose={() => (promptAssistant.composeModalOpen = false)} />
{/if}
