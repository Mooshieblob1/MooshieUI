<script lang="ts">
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { activeLyricLine, lyricLines, manualLyricAlignment, type LyricLine } from "../../utils/lyricTiming.js";
  import { audioTime } from "../../utils/musicAudio.js";

  let container: HTMLDivElement | undefined = $state();
  let follow = $state(true);
  let editing = $state(false);
  let draft = $state<LyricLine[]>([]);
  let nextLine = $state(-1);
  let timingError = $state("");
  const lines = $derived(music.selectedResult?.alignment?.lines ?? lyricLines(music.selectedResult?.params.lyrics ?? ""));
  const active = $derived(activeLyricLine(lines, music.currentTime));

  function editTiming() {
    draft = lines.map(line => ({ ...line }));
    nextLine = draft.findIndex(line => !line.section && line.start === null);
    editing = true;
    follow = false;
    timingError = "";
  }
  function setStart(index: number, value: number | null) {
    draft = draft.map((line, i) => i === index ? { ...line, start: value } : line);
    timingError = "";
  }
  function mark(index: number) {
    setStart(index, Math.round(music.currentTime * 100) / 100);
    nextLine = draft.findIndex((line, i) => i > index && !line.section);
  }
  async function save() {
    try {
      const alignment = manualLyricAlignment(draft, music.duration);
      if (await music.saveAlignment(alignment)) { editing = false; follow = true; }
    } catch { timingError = locale.t("music.timing_invalid"); }
  }
  $effect(() => {
    if (!follow || editing || active < 0 || !container) return;
    const row = container.querySelector<HTMLElement>(`[data-line="${active}"]`);
    if (row) container.scrollTo({ top: Math.max(0, row.offsetTop - container.clientHeight / 2 + row.offsetHeight / 2),
      behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth" });
  });
</script>

<section class="mt-5 space-y-3" aria-label={locale.t("music.recording_lyrics")}>
  <div class="flex flex-wrap items-center justify-between gap-2">
    <h3 class="text-sm font-medium text-neutral-200">{locale.t("music.lyrics")}</h3>
    <div class="flex items-center gap-2">
      {#if !editing && music.selectedResult?.alignment?.matched}
        <button type="button" class="touch-target rounded-md px-2 text-xs text-indigo-300" aria-pressed={follow} onclick={() => { follow = !follow; }}>{locale.t("music.follow_lyrics")}</button>
      {/if}
      {#if !editing}
        <button type="button" class="touch-target rounded-md bg-neutral-800 px-3 text-xs text-neutral-200 hover:bg-neutral-700 disabled:opacity-40" disabled={music.duration <= 0 || !lines.some(line => !line.section)} onclick={editTiming}>{locale.t("music.manual_sync")}</button>
      {/if}
    </div>
  </div>
  <p class="text-xs leading-relaxed text-neutral-400">{locale.t("music.manual_sync_help")}</p>
  {#if editing}
    <div class="space-y-2 rounded-md bg-neutral-800/50 p-3">
      <div class="flex flex-wrap items-center gap-2">
        <button type="button" class="touch-target rounded-md px-3 text-sm text-neutral-200 hover:bg-neutral-700" onclick={() => music.togglePlayback()}>{locale.t(music.playing ? "music.pause" : "music.play")}</button>
        <span class="font-mono text-xs text-neutral-400">{audioTime(music.currentTime)}</span>
        <button type="button" class="touch-target rounded-md bg-indigo-500 px-3 text-sm text-[var(--theme-accent-foreground)] disabled:opacity-40" disabled={nextLine < 0 || music.currentTime >= music.duration} onclick={() => mark(nextLine)}>{locale.t("music.mark_next")}</button>
      </div>
      {#if nextLine >= 0}<p class="text-xs text-neutral-300" aria-live="polite">{draft[nextLine].text}</p>{/if}
      <div class="flex flex-wrap items-center justify-end gap-2">
        <button type="button" class="touch-target px-3 text-xs text-neutral-400" disabled={music.librarySaving} onclick={() => { draft = draft.map(line => ({ ...line, start: null, end: null })); nextLine = draft.findIndex(line => !line.section); }}>{locale.t("music.clear_timing")}</button>
        <button type="button" class="touch-target px-3 text-xs text-neutral-300" disabled={music.librarySaving} onclick={() => { editing = false; }}>{locale.t("common.cancel")}</button>
        <button type="button" class="touch-target rounded-md bg-neutral-700 px-3 text-xs text-neutral-100 disabled:opacity-40" disabled={music.librarySaving} onclick={save}>{locale.t("common.save")}</button>
      </div>
    </div>
  {/if}
  {#if timingError}<p class="text-xs text-red-300" role="alert">{timingError}</p>{/if}
  {#if music.libraryError}<p class="text-xs text-red-300" role="alert">{music.libraryError}</p>{/if}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Keyboard scrolling pauses automatic following.) -->
  <div class="relative max-h-80 space-y-1 overflow-y-auto overscroll-contain focus-visible:outline-2 focus-visible:outline-indigo-400" role="region" aria-label={locale.t("music.lyrics")} tabindex="0" bind:this={container} onwheel={() => { follow = false; }} ontouchstart={() => { follow = false; }} onkeydown={(event) => { if (["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"].includes(event.key)) follow = false; }}>
    {#each editing ? draft : lines as line, index}
      {#if line.section}
        <p data-line={index} class="px-3 pb-1 pt-3 text-[11px] font-medium uppercase tracking-wide text-neutral-500">{line.text}</p>
      {:else if editing}
        <div class="flex flex-wrap items-center gap-2 rounded-md bg-neutral-800/30 px-3 py-2">
          <p class="w-full text-sm text-neutral-300">{line.text}</p>
          <input type="number" min="0" max={music.duration} step="0.01" value={line.start ?? ""} placeholder="—" class="min-h-11 w-24 rounded-md bg-neutral-800 px-2 font-mono text-xs text-neutral-200" aria-label={locale.t("music.line_seconds", { text: line.text })} oninput={(event) => setStart(index, event.currentTarget.value === "" ? null : event.currentTarget.valueAsNumber)} />
          <button type="button" class="touch-target rounded-md px-2 text-xs text-indigo-300 hover:bg-neutral-800" onclick={() => mark(index)}>{locale.t("music.mark_now")}</button>
          {#if line.start !== null}<button type="button" class="touch-target px-2 text-xs text-neutral-400" onclick={() => setStart(index, null)}>{locale.t("common.remove")}</button>{/if}
        </div>
      {:else if line.start !== null}
        <button type="button" data-line={index} class={`min-h-11 w-full rounded-r-md border-l-2 px-3 py-2 text-left text-sm leading-relaxed focus-visible:outline-2 focus-visible:outline-indigo-400 ${index === active ? 'border-indigo-400 bg-indigo-400/5 text-indigo-300' : 'border-transparent text-neutral-300 hover:bg-neutral-800'}`} aria-current={index === active ? "true" : undefined} aria-label={locale.t("music.seek_lyric", { text: line.text, time: audioTime(line.start) })} onclick={() => music.seek(line.start!)}>{line.text}</button>
      {:else}
        <p data-line={index} class="border-l-2 border-transparent px-3 py-2 text-sm leading-relaxed text-neutral-400">{line.text}</p>
      {/if}
    {/each}
  </div>
</section>
