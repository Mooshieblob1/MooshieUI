<script lang="ts">
  // Comic page builder: lay out panels, give each a prompt, generate them on
  // any model (or the whole page in one NovelAI image), check continuity and
  // assemble the page.
  import { onMount } from "svelte";
  import { comic } from "../../stores/comic.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import {
    LAYOUT_TEMPLATES,
    MAX_PANELS,
    ONE_SHOT_MAX_PANELS,
    buildTemplate,
    dividers,
    pageAspect,
    pageSize,
    panelPixelRect,
    panelRects,
    ratioFromPointer,
    type Divider,
    type LayoutTemplate,
    type PageShape,
  } from "../../utils/comicLayout.js";
  import {
    checkPanel,
    checkStory,
    draftFromBeats,
    enhancePanel,
    generateOneImage,
    generatePanels,
    previewPage,
    restoreImages,
    savePage,
    stopRun,
    undoDraft,
  } from "../../utils/comicRunner.js";
  import { naiV5Variant } from "../../utils/novelaiModels.js";
  import { portal } from "../../utils/portal.js";

  interface Props {
    onOpenGenerate: () => void;
  }
  let { onOpenGenerate }: Props = $props();

  let showTemplates = $state(false);
  let showPage = $state(false);
  let error = $state<string | null>(null);
  let saving = $state(false);
  let canvasEl = $state<HTMLDivElement | null>(null);
  let dragging = $state<Divider | null>(null);

  onMount(() => {
    comic.load();
    void restoreImages();
  });

  const aspect = $derived(pageAspect(comic.pageShape));
  const pagePx = $derived(pageSize(comic.pageShape));
  /** A panel's box with gutters, as percentages of the page: what the saved page will look like. */
  function boxStyle(r: { x: number; y: number; w: number; h: number }): string {
    const b = panelPixelRect(r, pagePx, comic.gutter);
    return `left: ${(b.x / pagePx.width) * 100}%; top: ${(b.y / pagePx.height) * 100}%; width: ${(b.w / pagePx.width) * 100}%; height: ${(b.h / pagePx.height) * 100}%;`;
  }
  const rects = $derived(comic.rects);
  const lines = $derived(dividers(comic.layout));
  const order = $derived(comic.order);
  const oneImage = $derived(comic.mode === "one_image");
  const tooManyForOneImage = $derived(oneImage && comic.enabledPanels.length > ONE_SHOT_MAX_PANELS);
  /** On NovelAI V5 the Prompt Assistant writes panels by the V5 Enhance rules, dialogue included. */
  const v5 = $derived(generation.isNovelAi && naiV5Variant(generation.checkpoint) !== null);
  const modelName = $derived((generation.checkpoint || "").split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "");
  const issueCount = $derived(
    Object.values(comic.results).reduce((n, r) => n + (r.issues?.length ?? 0), 0),
  );

  async function run(task: () => Promise<unknown>) {
    error = null;
    try {
      await task();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function generateAll() {
    void run(() => (oneImage ? generateOneImage() : generatePanels()));
  }

  async function openPage() {
    if (!oneImage) await run(previewPage);
    if (comic.pageUrl) showPage = true;
  }

  async function save() {
    saving = true;
    await run(async () => {
      await savePage();
      gallery.showToast(locale.t("comic.saved"), "success");
    });
    saving = false;
  }

  function pickTemplate(t: LayoutTemplate) {
    comic.applyTemplate(t);
    showTemplates = false;
  }

  function startDrag(event: PointerEvent, divider: Divider) {
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    comic.beginResize();
    dragging = divider;
  }

  function onDrag(event: PointerEvent) {
    if (!dragging || !canvasEl) return;
    const box = canvasEl.getBoundingClientRect();
    const pointer =
      dragging.dir === "row" ? (event.clientX - box.left) / box.width : (event.clientY - box.top) / box.height;
    comic.resize(dragging.splitId, ratioFromPointer(dragging, pointer));
    // Keep the box the divider divides; only its ratio moves.
  }

  function endDrag() {
    if (dragging) comic.persist();
    dragging = null;
  }

  function onKeydown(event: KeyboardEvent) {
    const target = event.target as HTMLElement | null;
    if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z") {
      event.preventDefault();
      comic.undo();
    }
  }

  const statusRing: Record<string, string> = {
    queued: "ring-2 ring-neutral-500/60",
    running: "ring-2 ring-indigo-400 animate-pulse",
    done: "",
    failed: "ring-2 ring-red-500",
  };

  const shapes: PageShape[] = ["portrait", "square", "landscape"];
</script>

<svelte:window onkeydown={onKeydown} />

<div class="flex h-full flex-col overflow-hidden text-neutral-200">
  <!-- Toolbar -->
  <div class="flex flex-wrap items-center gap-2 border-b border-neutral-800 px-3 py-2 text-xs">
    <h1 class="mr-1 text-sm font-medium text-neutral-100">{locale.t("comic.title")}</h1>
    <button
      class="rounded-lg border px-2.5 py-1.5 transition-colors {showTemplates ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300' : 'border-neutral-700 bg-neutral-900 hover:border-neutral-500'}"
      onclick={() => (showTemplates = !showTemplates)}
      aria-expanded={showTemplates}
    >{locale.t("comic.templates")}</button>
    <span class="h-5 w-px bg-neutral-800" aria-hidden="true"></span>
    <button
      class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
      disabled={!comic.selectedId || comic.panelCount >= MAX_PANELS}
      title={locale.t("comic.split_side_title")}
      onclick={() => comic.split("row")}
    >{locale.t("comic.split_side")}</button>
    <button
      class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
      disabled={!comic.selectedId || comic.panelCount >= MAX_PANELS}
      title={locale.t("comic.split_stack_title")}
      onclick={() => comic.split("col")}
    >{locale.t("comic.split_stack")}</button>
    <button
      class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-red-500 hover:text-red-300 disabled:opacity-40"
      disabled={!comic.selectedId || comic.panelCount <= 1}
      onclick={() => comic.removeSelected()}
    >{locale.t("comic.remove_panel")}</button>
    <button
      class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
      disabled={!comic.canUndo}
      title="Ctrl+Z"
      onclick={() => comic.undo()}
    >{locale.t("comic.undo")}</button>
    <span class="h-5 w-px bg-neutral-800" aria-hidden="true"></span>
    <label class="flex items-center gap-1.5 text-neutral-400">
      {locale.t("comic.page_shape")}
      <select
        class="rounded border border-neutral-700 bg-neutral-900 px-1.5 py-1 text-neutral-200"
        value={comic.pageShape}
        onchange={(e) => comic.setPageField("pageShape", (e.currentTarget as HTMLSelectElement).value as PageShape)}
      >
        {#each shapes as shape (shape)}
          <option value={shape}>{locale.t(`comic.shape_${shape}`)}</option>
        {/each}
      </select>
    </label>
    <label class="flex items-center gap-1.5 text-neutral-400" title={locale.t("comic.gutter_title")}>
      {locale.t("comic.gutter")}
      <input
        type="range"
        min="0"
        max="64"
        step="2"
        class="w-20 accent-indigo-500"
        value={comic.gutter}
        oninput={(e) => comic.setPageField("gutter", Number((e.currentTarget as HTMLInputElement).value))}
      />
    </label>
    <button
      class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500"
      title={locale.t("comic.reading_order_title")}
      onclick={() => comic.setPageField("rightToLeft", !comic.rightToLeft)}
    >{comic.rightToLeft ? locale.t("comic.order_rtl") : locale.t("comic.order_ltr")}</button>

    <div class="ml-auto flex flex-wrap items-center gap-2">
      <div class="flex overflow-hidden rounded-lg border border-neutral-700" role="radiogroup" aria-label={locale.t("comic.mode")}>
        <button
          role="radio"
          aria-checked={!oneImage}
          class="px-2.5 py-1.5 {!oneImage ? 'bg-indigo-600 text-white' : 'bg-neutral-900 text-neutral-300 hover:bg-neutral-800'}"
          title={locale.t("comic.mode_panels_title")}
          onclick={() => comic.setPageField("mode", "panels")}
        >{locale.t("comic.mode_panels")}</button>
        <button
          role="radio"
          aria-checked={oneImage}
          class="px-2.5 py-1.5 {oneImage ? 'bg-indigo-600 text-white' : 'bg-neutral-900 text-neutral-300 hover:bg-neutral-800'}"
          title={locale.t("comic.mode_one_image_title")}
          onclick={() => comic.setPageField("mode", "one_image")}
        >{locale.t("comic.mode_one_image")}</button>
      </div>
      {#if comic.running}
        <button class="rounded-lg bg-red-700 px-3 py-1.5 font-medium text-white hover:bg-red-600" onclick={stopRun}>
          {comic.stopRequested ? locale.t("comic.stopping") : locale.t("comic.stop")}
        </button>
      {:else}
        <button
          class="rounded-lg bg-indigo-600 px-3 py-1.5 font-medium text-white hover:bg-indigo-500 disabled:opacity-40"
          disabled={comic.enabledPanels.length === 0 || tooManyForOneImage || (oneImage && !generation.isNovelAi)}
          onclick={generateAll}
        >{oneImage ? locale.t("comic.generate_page") : locale.t("comic.generate_all")}</button>
      {/if}
      <button
        class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-neutral-500 disabled:opacity-40"
        disabled={oneImage ? !comic.pageUrl : !Object.values(comic.results).some((r) => r.url)}
        onclick={() => { void openPage(); }}
      >{locale.t("comic.view_page")}</button>
      <button
        class="rounded-lg border border-emerald-700 bg-emerald-900/40 px-2.5 py-1.5 text-emerald-200 hover:bg-emerald-800/50 disabled:opacity-40"
        disabled={saving || (oneImage ? !comic.pageUrl : !Object.values(comic.results).some((r) => r.url))}
        onclick={() => { void save(); }}
      >{locale.t("comic.save_page")}</button>
    </div>
  </div>

  <!-- Model line: the page uses whatever the Generate page is set to. -->
  <div class="flex flex-wrap items-center gap-2 border-b border-neutral-800/60 px-3 py-1.5 text-[11px] text-neutral-500">
    {#if modelName}
      <span>{locale.t("comic.using_model", { model: modelName, size: `${generation.width}×${generation.height}` })}</span>
    {:else}
      <span class="text-amber-400">{locale.t("comic.no_model")}</span>
    {/if}
    <button class="text-indigo-300 hover:underline" onclick={onOpenGenerate}>{locale.t("comic.change_model")}</button>
    {#if oneImage && !generation.isNovelAi}
      <span class="text-amber-400">{locale.t("comic.error_one_image_novelai")}</span>
    {:else if tooManyForOneImage}
      <span class="text-amber-400">{locale.t("comic.error_one_image_panels", { max: String(ONE_SHOT_MAX_PANELS) })}</span>
    {/if}
    {#if error}
      <span class="text-red-400">{error}</span>
    {/if}
  </div>

  {#if showTemplates}
    <div class="flex flex-wrap gap-2 border-b border-neutral-800 bg-neutral-900/50 px-3 py-2">
      {#each LAYOUT_TEMPLATES as t (t.id)}
        {@const preview = panelRects(buildTemplate(t, (() => { let n = 0; return () => `t${n++}`; })()))}
        <button
          class="group flex flex-col items-center gap-1 rounded-lg p-1.5 hover:bg-neutral-800"
          title={locale.t("comic.template_panels", { count: String(t.panels) })}
          onclick={() => pickTemplate(t)}
        >
          <svg viewBox="0 0 100 {Math.round(100 / aspect)}" class="h-16 border border-neutral-700 bg-neutral-100 group-hover:border-indigo-400" style="aspect-ratio: {aspect}">
            {#each [...preview.values()] as r, i (i)}
              <rect
                x={r.x * 100 + 3}
                y={(r.y * 100) / aspect + 3}
                width={Math.max(1, r.w * 100 - 6)}
                height={Math.max(1, (r.h * 100) / aspect - 6)}
                fill="#e5e7eb"
                stroke="#111827"
                stroke-width="2"
              />
            {/each}
          </svg>
          <span class="text-[10px] text-neutral-400">{t.panels}</span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="flex min-h-0 flex-1 flex-col overflow-y-auto lg:flex-row lg:overflow-hidden">
    <!-- Page setup: style, setting, cast, story -->
    <aside class="space-y-4 border-neutral-800 p-3 text-xs lg:w-72 lg:shrink-0 lg:overflow-y-auto lg:border-r">
      <section class="space-y-1.5">
        <label class="block text-neutral-400" for="comic-style">{locale.t("comic.style")}</label>
        <input
          id="comic-style"
          class="w-full rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
          placeholder={locale.t("comic.style_placeholder")}
          value={comic.style}
          oninput={(e) => comic.setPageField("style", (e.currentTarget as HTMLInputElement).value)}
        />
        <label class="block pt-1 text-neutral-400" for="comic-setting">{locale.t("comic.setting")}</label>
        <textarea
          id="comic-setting"
          rows="2"
          class="w-full resize-y rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
          placeholder={locale.t("comic.setting_placeholder")}
          value={comic.setting}
          oninput={(e) => comic.setPageField("setting", (e.currentTarget as HTMLTextAreaElement).value)}
        ></textarea>
      </section>

      <section class="space-y-2">
        <div class="flex items-center justify-between">
          <span class="text-neutral-400">{locale.t("comic.cast")}</span>
          <button class="text-indigo-300 hover:underline" onclick={() => comic.addCastMember()}>{locale.t("comic.add_cast")}</button>
        </div>
        {#if comic.cast.length === 0}
          <p class="text-[11px] text-neutral-500">{locale.t("comic.cast_empty")}</p>
        {/if}
        {#each comic.cast as member (member.id)}
          <div class="space-y-1 rounded-lg border border-neutral-800 bg-neutral-900/60 p-2">
            <div class="flex items-center gap-1.5">
              <input
                class="min-w-0 flex-1 rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
                placeholder={locale.t("comic.cast_name")}
                aria-label={locale.t("comic.cast_name")}
                value={member.name}
                oninput={(e) => comic.updateCastMember(member.id, { name: (e.currentTarget as HTMLInputElement).value })}
              />
              <button
                class="rounded px-1.5 text-neutral-500 hover:text-red-400"
                aria-label={locale.t("comic.remove_cast")}
                title={locale.t("comic.remove_cast")}
                onclick={() => comic.removeCastMember(member.id)}
              >&times;</button>
            </div>
            <textarea
              rows="2"
              class="w-full resize-y rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
              placeholder={locale.t("comic.cast_tags_placeholder")}
              aria-label={locale.t("comic.cast_tags")}
              value={member.tags}
              oninput={(e) => comic.updateCastMember(member.id, { tags: (e.currentTarget as HTMLTextAreaElement).value })}
            ></textarea>
          </div>
        {/each}
      </section>

      <section class="space-y-2">
        <span class="block text-neutral-400">{locale.t("comic.story")}</span>
        <label class="flex items-start gap-2 text-[11px] text-neutral-300">
          <input
            type="checkbox"
            class="mt-0.5 accent-indigo-500"
            checked={comic.checkContinuity}
            onchange={(e) => comic.setPageField("checkContinuity", (e.currentTarget as HTMLInputElement).checked)}
          />
          <span>{locale.t("comic.check_continuity")}</span>
        </label>
        <div class="flex flex-wrap gap-1.5">
          <button
            class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-indigo-500 disabled:opacity-40"
            disabled={!promptAssistant.isAvailable || comic.drafting}
            title={promptAssistant.isAvailable ? locale.t(v5 ? "comic.draft_title_v5" : "comic.draft_title") : locale.t("comic.assistant_needed")}
            onclick={() => { void run(draftFromBeats); }}
          >{comic.drafting ? locale.t("comic.working") : locale.t("comic.draft")}</button>
          {#if comic.draftUndo}
            <button class="rounded-lg px-2 py-1.5 text-neutral-400 hover:text-neutral-200" onclick={undoDraft}>{locale.t("comic.undo_draft")}</button>
          {/if}
          <button
            class="rounded-lg border border-neutral-700 bg-neutral-900 px-2.5 py-1.5 hover:border-indigo-500 disabled:opacity-40"
            disabled={!promptAssistant.isAvailable || comic.storyChecking}
            title={promptAssistant.isAvailable ? locale.t("comic.check_story_title") : locale.t("comic.assistant_needed")}
            onclick={() => { void run(checkStory); }}
          >{comic.storyChecking ? locale.t("comic.working") : locale.t("comic.check_story")}</button>
        </div>
        {#if comic.storyError}
          <p class="text-[11px] text-red-400">{comic.storyError}</p>
        {/if}
        {#if comic.storyIssues}
          {#if comic.storyIssues.length === 0}
            <p class="text-[11px] text-emerald-400">{locale.t("comic.story_ok")}</p>
          {:else}
            <ul class="space-y-1">
              {#each comic.storyIssues as item, i (i)}
                <li class="rounded border border-amber-500/30 bg-amber-500/10 px-2 py-1 text-[11px] text-amber-200">
                  {#if item.panel > 0}<span class="font-medium">{locale.t("comic.panel_n", { n: String(item.panel) })}:</span>{/if}
                  {item.issue}
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
        {#if issueCount > 0}
          <p class="text-[11px] text-amber-300">{locale.t("comic.issues_total", { count: String(issueCount) })}</p>
        {/if}
      </section>
    </aside>

    <!-- The page -->
    <div class="flex min-h-[420px] min-w-0 flex-1 items-center justify-center bg-neutral-950 p-4 lg:min-h-0">
      <div
        bind:this={canvasEl}
        class="relative max-h-full w-full max-w-[640px] select-none bg-white shadow-xl shadow-black/40"
        style="aspect-ratio: {aspect}"
        onpointermove={onDrag}
        onpointerup={endDrag}
        onpointercancel={endDrag}
        role="application"
        aria-label={locale.t("comic.page_label")}
      >
        {#each [...rects.entries()] as [id, r] (id)}
          {@const panel = comic.panels[id]}
          {@const result = comic.results[id]}
          {@const number = order.indexOf(id) + 1}
          <button
            class="absolute overflow-hidden border-2 border-neutral-900 bg-neutral-200 text-left transition-shadow {comic.selectedId === id ? 'outline outline-3 outline-indigo-500' : ''} {result ? statusRing[result.status] : ''} {panel?.enabled === false ? 'opacity-40' : ''}"
            style={boxStyle(r)}
            onclick={() => (comic.selectedId = id)}
            aria-pressed={comic.selectedId === id}
            aria-label={locale.t("comic.panel_n", { n: String(number) })}
          >
            {#if result?.url}
              <img src={result.url} alt="" class="h-full w-full object-cover" draggable="false" />
            {:else}
              <span class="block p-1.5 text-[10px] leading-snug text-neutral-600 line-clamp-4">{panel?.beat || panel?.prompt || ""}</span>
            {/if}
            <span class="absolute left-1 top-1 rounded-full bg-neutral-900/85 px-1.5 text-[10px] font-semibold text-white">{number}</span>
            {#if result?.status === "running"}
              <span class="absolute inset-x-0 bottom-0 bg-indigo-600/85 py-0.5 text-center text-[10px] text-white">{locale.t("comic.generating")}</span>
            {:else if result?.status === "failed"}
              <span class="absolute inset-x-0 bottom-0 bg-red-700/90 py-0.5 text-center text-[10px] text-white">{locale.t("comic.failed")}</span>
            {:else if result?.issues && result.issues.length > 0}
              <span class="absolute right-1 top-1 rounded-full bg-amber-500 px-1.5 text-[10px] font-semibold text-neutral-950" title={locale.t("comic.issues_badge_title")}>!{result.issues.length}</span>
            {:else if result?.issues && result.issues.length === 0}
              <span class="absolute right-1 top-1 rounded-full bg-emerald-600 px-1.5 text-[10px] text-white" title={locale.t("comic.check_passed")}>✓</span>
            {/if}
          </button>
        {/each}
        {#each lines as line (line.splitId)}
          <!-- A divider: drag to resize the two sides. -->
          <div
            class="absolute z-10 {line.dir === 'row' ? 'cursor-col-resize' : 'cursor-row-resize'} group"
            style={line.dir === "row"
              ? `left: calc(${line.pos * 100}% - 6px); top: ${line.box.y * 100}%; width: 12px; height: ${line.box.h * 100}%;`
              : `top: calc(${line.pos * 100}% - 6px); left: ${line.box.x * 100}%; height: 12px; width: ${line.box.w * 100}%;`}
            onpointerdown={(e) => startDrag(e, line)}
            role="separator"
            aria-orientation={line.dir === "row" ? "vertical" : "horizontal"}
            aria-label={locale.t("comic.divider")}
            tabindex="-1"
          >
            <span class="absolute rounded bg-indigo-500 opacity-0 transition-opacity group-hover:opacity-80 {dragging?.splitId === line.splitId ? 'opacity-100' : ''} {line.dir === 'row' ? 'left-[4px] top-0 h-full w-[4px]' : 'top-[4px] left-0 w-full h-[4px]'}"></span>
          </div>
        {/each}
      </div>
    </div>

    <!-- Panels in reading order -->
    <aside class="space-y-2 border-neutral-800 p-3 text-xs lg:w-96 lg:shrink-0 lg:overflow-y-auto lg:border-l">
      {#each comic.orderedPanels as panel, i (panel.id)}
        {@const result = comic.results[panel.id]}
        {@const open = comic.selectedId === panel.id}
        <div class="rounded-lg border {open ? 'border-indigo-500/70 bg-neutral-900' : 'border-neutral-800 bg-neutral-900/50'}">
          <div class="flex items-center gap-2 px-2 py-1.5">
            <button class="flex min-w-0 flex-1 items-center gap-2 text-left" onclick={() => (comic.selectedId = open ? null : panel.id)} aria-expanded={open}>
              <span class="rounded-full bg-neutral-800 px-1.5 text-[10px] font-semibold text-neutral-200">{i + 1}</span>
              <span class="min-w-0 flex-1 truncate {panel.prompt || panel.beat ? 'text-neutral-200' : 'text-neutral-500'}">{panel.beat || panel.prompt || locale.t("comic.empty_panel")}</span>
            </button>
            {#if result?.issues && result.issues.length > 0}
              <span class="rounded-full bg-amber-500 px-1.5 text-[10px] font-semibold text-neutral-950">!{result.issues.length}</span>
            {/if}
            <label class="flex items-center gap-1 text-[10px] text-neutral-400" title={locale.t("comic.panel_on_title")}>
              <input
                type="checkbox"
                class="accent-indigo-500"
                checked={panel.enabled}
                onchange={(e) => comic.updatePanel(panel.id, { enabled: (e.currentTarget as HTMLInputElement).checked })}
              />
              {locale.t("comic.panel_on")}
            </label>
          </div>
          {#if open}
            <div class="space-y-1.5 border-t border-neutral-800 px-2 pb-2 pt-1.5">
              <input
                class="w-full rounded border border-neutral-700 bg-neutral-950 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
                placeholder={locale.t("comic.beat_placeholder")}
                aria-label={locale.t("comic.beat")}
                value={panel.beat}
                oninput={(e) => comic.updatePanel(panel.id, { beat: (e.currentTarget as HTMLInputElement).value })}
              />
              <textarea
                rows="3"
                class="w-full resize-y rounded border border-neutral-700 bg-neutral-950 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
                placeholder={locale.t("comic.prompt_placeholder")}
                aria-label={locale.t("comic.prompt")}
                value={panel.prompt}
                oninput={(e) => comic.updatePanel(panel.id, { prompt: (e.currentTarget as HTMLTextAreaElement).value })}
              ></textarea>
              <textarea
                rows="2"
                class="w-full resize-y rounded border border-neutral-700 bg-neutral-950 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
                placeholder={locale.t("comic.dialogue_placeholder")}
                aria-label={locale.t("comic.dialogue")}
                value={panel.dialogue}
                oninput={(e) => comic.updatePanel(panel.id, { dialogue: (e.currentTarget as HTMLTextAreaElement).value })}
              ></textarea>
              {#if panel.dialogue.trim() && !generation.isNovelAi}
                <p class="text-[10px] text-neutral-500">{locale.t("comic.dialogue_nai_only")}</p>
              {/if}
              <div class="flex flex-wrap items-center gap-1.5">
                <button
                  class="rounded border border-indigo-700/70 bg-indigo-900/30 px-2 py-0.5 text-[11px] text-indigo-200 hover:bg-indigo-800/40 disabled:opacity-40"
                  disabled={!promptAssistant.isAvailable || comic.enhancingId !== null || comic.drafting}
                  title={promptAssistant.isAvailable ? locale.t(v5 ? "comic.enhance_panel_title_v5" : "comic.enhance_panel_title") : locale.t("comic.assistant_needed")}
                  onclick={() => { void run(() => enhancePanel(panel.id)); }}
                >{comic.enhancingId === panel.id ? locale.t("comic.working") : locale.t("comic.enhance_panel")}</button>
                {#if comic.draftUndo && comic.draftUndo[panel.id]}
                  <button class="px-1 text-[11px] text-neutral-400 hover:text-neutral-200" onclick={undoDraft}>{locale.t("comic.undo_draft")}</button>
                {/if}
              </div>
              <textarea
                rows="1"
                class="w-full resize-y rounded border border-red-900/60 bg-neutral-950 px-2 py-1 text-red-200 placeholder-neutral-600 focus:border-red-500 focus:outline-none"
                placeholder={locale.t("comic.negative_placeholder")}
                aria-label={locale.t("comic.negative")}
                value={panel.negative}
                oninput={(e) => comic.updatePanel(panel.id, { negative: (e.currentTarget as HTMLTextAreaElement).value })}
              ></textarea>
              {#if comic.cast.length > 0}
                <div class="flex flex-wrap items-center gap-1">
                  <span class="text-[10px] text-neutral-500">{locale.t("comic.in_panel")}</span>
                  {#each comic.cast as member, i (member.id)}
                    {@const on = panel.cast.includes(member.id)}
                    <button
                      class="rounded-full border px-2 py-0.5 text-[11px] {on ? 'border-indigo-500 bg-indigo-500/15 text-indigo-200' : 'border-neutral-700 text-neutral-400 hover:border-neutral-500'}"
                      aria-pressed={on}
                      onclick={() => comic.toggleCastInPanel(panel.id, member.id)}
                    >{member.name || `#${i + 1}`}</button>
                  {/each}
                </div>
              {/if}
              <input
                class="w-full rounded border border-neutral-700 bg-neutral-950 px-2 py-1 text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
                placeholder={comic.setting.trim() ? locale.t("comic.setting_override_placeholder", { setting: comic.setting.trim() }) : locale.t("comic.setting_override_empty")}
                aria-label={locale.t("comic.setting_override")}
                value={panel.setting}
                oninput={(e) => comic.updatePanel(panel.id, { setting: (e.currentTarget as HTMLInputElement).value })}
              />
              {#if result?.error}
                <p class="text-[11px] text-red-400">{result.error}</p>
              {/if}
              {#if result?.checking}
                <p class="text-[11px] text-neutral-500">{locale.t("comic.checking")}</p>
              {:else if result?.checkError}
                <p class="text-[11px] text-amber-400">{locale.t("comic.check_failed", { message: result.checkError })}</p>
              {/if}
              {#if result?.issues && result.issues.length > 0}
                <ul class="space-y-0.5">
                  {#each result.issues as issue, k (k)}
                    <li class="rounded bg-amber-500/10 px-2 py-0.5 text-[11px] text-amber-200">
                      {#if issue.kind === "changed"}
                        {locale.t(issue.who ? "comic.issue_changed" : "comic.issue_setting_changed", { who: issue.who ?? "", expected: issue.expected, got: issue.got ?? "" })}
                      {:else}
                        {locale.t(issue.who ? "comic.issue_missing" : "comic.issue_setting_missing", { who: issue.who ?? "", expected: issue.expected })}
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
              {#if !oneImage}
                <div class="flex flex-wrap gap-1.5 pt-0.5">
                  <button
                    class="rounded-lg bg-indigo-600 px-2.5 py-1 font-medium text-white hover:bg-indigo-500 disabled:opacity-40"
                    disabled={comic.running}
                    onclick={() => { void run(() => generatePanels([panel.id])); }}
                  >{result?.url ? locale.t("comic.redo_panel") : locale.t("comic.generate_panel")}</button>
                  {#if result?.url && !result.checking}
                    <button
                      class="rounded-lg border border-neutral-700 px-2.5 py-1 hover:border-neutral-500"
                      onclick={() => { void checkPanel(panel.id); }}
                    >{locale.t("comic.check_again")}</button>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      <button class="w-full rounded-lg border border-dashed border-neutral-700 py-1.5 text-neutral-500 hover:border-neutral-500 hover:text-neutral-300" onclick={() => comic.resetPanels()}>
        {locale.t("comic.start_over")}
      </button>
    </aside>
  </div>
</div>

{#if showPage && comic.pageUrl}
  <div
    use:portal
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("comic.view_page")}
    tabindex="-1"
    onclick={(e) => { if (e.target === e.currentTarget) showPage = false; }}
    onkeydown={(e) => { if (e.key === "Escape") showPage = false; }}
  >
    <div class="flex max-h-full flex-col items-center gap-2">
      <img src={comic.pageUrl} alt={locale.t("comic.page_label")} class="max-h-[85vh] max-w-full object-contain shadow-2xl" />
      <div class="flex gap-2">
        <button class="rounded-lg bg-emerald-700 px-3 py-1.5 text-sm text-white hover:bg-emerald-600 disabled:opacity-40" disabled={saving} onclick={() => { void save(); }}>{locale.t("comic.save_page")}</button>
        <button class="rounded-lg bg-neutral-800 px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-700" onclick={() => (showPage = false)}>{locale.t("comic.close")}</button>
      </div>
    </div>
  </div>
{/if}
