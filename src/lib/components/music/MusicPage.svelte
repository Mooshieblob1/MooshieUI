<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { connection } from "../../stores/connection.svelte.js";
  import { comfyuiUpdate } from "../../stores/comfyuiUpdate.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { getConfig } from "../../utils/api.js";
  import { isBrowserMode } from "../../utils/ipc.js";
  import { insertLyricSection } from "../../utils/musicLyrics.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import type { MusicWritingTask } from "../../utils/yue2Skill.js";
  import MusicPlayer from "./MusicPlayer.svelte";
  import MusicProgress from "./MusicProgress.svelte";
  import MusicLibrary from "./MusicLibrary.svelte";
  import MusicSongDialog from "./MusicSongDialog.svelte";

  let { userRole = "admin" }: { userRole?: string } = $props();
  const canDownload = $derived(userRole === "admin" || userRole === "moderator");
  let managedRuntime = $state(false);
  let lyricsEl: HTMLTextAreaElement | undefined = $state();
  let settingsEl: HTMLDetailsElement | undefined = $state();
  let writing = $state<MusicWritingTask | null>(null);
  let writingError = $state("");
  let lyricsDialog: HTMLDialogElement | undefined = $state();
  let lyricsBriefEl: HTMLTextAreaElement | undefined = $state();
  let lyricsBrief = $state("");
  let useExistingLyrics = $state(false);
  let writingSequence = 0;
  let writingUndo = $state<Partial<Record<MusicWritingTask, { before: string; after: string }>>>({});
  let active = true;
  onDestroy(() => { active = false; });

  function openLyricsDialog() {
    writingError = "";
    useExistingLyrics = false;
    lyricsDialog?.showModal();
    lyricsBriefEl?.focus();
  }

  function closeLyricsDialog() {
    if (writing === "lyrics") writingSequence++;
    lyricsDialog?.close();
  }

  async function writeMusic(task: MusicWritingTask, brief = "") {
    if (writing || promptAssistant.isGenerating || music.busy) return;
    const sequence = ++writingSequence;
    writing = task;
    writingError = "";
    const snapshot = {
      style: music.params.style, lyrics: music.params.lyrics,
      maxDuration: music.params.max_duration, abc: music.params.abc,
      useExistingLyrics: task === "lyrics" && useExistingLyrics && !!music.params.lyrics.trim(),
    };
    try {
      if (!Number.isFinite(snapshot.maxDuration) || snapshot.maxDuration < 1 || snapshot.maxDuration > 360) {
        writingError = locale.t("music.assistant_invalid_duration");
        return;
      }
      await promptAssistant.refreshStatus();
      if (!active || sequence !== writingSequence) return;
      if (!promptAssistant.isAvailable) {
        writingError = locale.t("music.assistant_setup");
        return;
      }
      const result = await promptAssistant.writeForMusic(task, { ...snapshot, language: locale.intlTag, brief });
      if (!active || sequence !== writingSequence) return;
      // A late response must not replace edits or settings loaded while waiting.
      if (snapshot.style !== music.params.style || snapshot.lyrics !== music.params.lyrics || snapshot.maxDuration !== music.params.max_duration || snapshot.abc !== music.params.abc) {
        writingError = locale.t("music.assistant_changed");
        return;
      }
      writingUndo = { ...writingUndo, [task]: { before: music.params[task], after: result } };
      music.params[task] = result;
      music.saveSettings();
      if (task === "lyrics") lyricsDialog?.close();
    } catch (error) {
      if (active && sequence === writingSequence) writingError = String(error).includes("invalid_music_writing")
        ? locale.t("music.assistant_invalid_result") : mapLlmError(String(error));
    } finally { writing = null; }
  }

  function undoWriting(task: MusicWritingTask) {
    const previous = writingUndo[task];
    if (!previous || music.params[task] !== previous.after) return;
    music.params[task] = previous.before;
    writingUndo = { ...writingUndo, [task]: undefined };
    music.saveSettings();
  }
  async function addSection(section: "verse" | "chorus") {
    const result = insertLyricSection(music.params.lyrics, section, lyricsEl?.selectionStart ?? music.params.lyrics.length);
    if (result.text.length > 64000) {
      music.error = locale.t("music.lyrics_full");
      return;
    }
    music.params.lyrics = result.text;
    music.saveSettings();
    await tick();
    lyricsEl?.focus();
    lyricsEl?.setSelectionRange(result.cursor, result.cursor);
  }
  onMount(() => {
    if (isBrowserMode || userRole !== "admin") return;
    void getConfig().then(async (config) => {
      managedRuntime = config.server_mode === "autolaunch" && !!config.comfyui_path;
      if (managedRuntime) await comfyuiUpdate.refresh();
    }).catch((error) => { music.error = String(error); });
  });
  async function updateRuntime() {
    // App.svelte refreshes music capabilities when the restarted server is ready.
    await comfyuiUpdate.update();
  }
  const inputClass = "w-full min-w-0 rounded-md border border-transparent bg-neutral-900 px-3 py-2.5 text-sm leading-relaxed text-neutral-200 placeholder:text-neutral-500 transition-colors hover:border-indigo-400/20 focus:border-indigo-400/50 focus:outline-none disabled:opacity-50";
  onMount(() => { if (!music.busy) music.loadSettings(); void music.loadLibrary(); });
  $effect(() => { if (connection.connected) untrack(() => { void music.refresh(); }); });
</script>

{#snippet writingActions(task: MusicWritingTask)}
  <div class="flex flex-wrap items-center gap-1">
    {#if writingUndo[task] && music.params[task] === writingUndo[task]?.after}
      <button type="button" class="touch-target rounded-md px-2 text-xs text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200 disabled:opacity-40" disabled={!!writing} onclick={() => undoWriting(task)} aria-label={locale.t(`music.assistant_undo_${task}`)}>{locale.t("prompt_assistant.undo")}</button>
    {/if}
    <button type="button" class="touch-target inline-flex items-center gap-1.5 rounded-md px-2 text-xs text-indigo-300 transition-colors hover:bg-neutral-900 hover:text-indigo-200 focus-visible:outline-2 focus-visible:outline-indigo-400 disabled:opacity-40" disabled={!!writing || promptAssistant.isGenerating || music.busy} onclick={() => task === "lyrics" ? openLyricsDialog() : writeMusic(task)} title={locale.t("music.assistant_backend")} aria-busy={writing === task}>
      <svg class={`h-3.5 w-3.5 ${writing === task ? 'motion-safe:animate-pulse' : ''}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m12 3 2.4 6.6L21 12l-6.6 2.4L12 21l-2.4-6.6L3 12l6.6-2.4L12 3ZM20 2v4m-2-2h4"/></svg>
      {locale.t(writing === task ? `music.assistant_writing_${task}` : `music.assistant_${task}`)}
    </button>
  </div>
{/snippet}

<dialog bind:this={lyricsDialog} class="m-auto max-h-[85vh] w-[calc(100%-2rem)] max-w-lg overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-neutral-200 shadow-2xl backdrop:bg-black/70" aria-labelledby="music-brief-title" oncancel={(event) => { event.preventDefault(); closeLyricsDialog(); }} onclose={() => { if (writing === "lyrics") writingSequence++; }}>
  <form class="space-y-4" onsubmit={(event) => { event.preventDefault(); if (lyricsBrief.trim()) void writeMusic("lyrics", lyricsBrief.trim()); }}>
    <div class="flex items-center justify-between gap-3">
      <h2 id="music-brief-title" class="text-lg font-semibold">{locale.t("music.assistant_lyrics")}</h2>
      <button type="button" class="touch-target rounded-md px-3 text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200" aria-label={locale.t("common.close")} onclick={closeLyricsDialog}>✕</button>
    </div>
    <div class="space-y-2">
      <label for="music-brief" class="block text-sm font-medium">{locale.t("music.lyrics_topic")}</label>
      <textarea id="music-brief" class={inputClass} rows="5" maxlength="2000" bind:this={lyricsBriefEl} bind:value={lyricsBrief} placeholder={locale.t("music.lyrics_topic_placeholder")} aria-describedby="music-brief-help" disabled={writing === "lyrics"} required></textarea>
      <p id="music-brief-help" class="text-xs leading-relaxed text-neutral-400">{locale.t("music.lyrics_topic_help", { seconds: music.params.max_duration })}</p>
    </div>
    <div>
      <label class="touch-target flex items-center gap-2 text-sm has-disabled:opacity-40">
        <input type="checkbox" class="h-4 w-4 accent-indigo-500" bind:checked={useExistingLyrics} disabled={writing === "lyrics" || !music.params.lyrics.trim()} aria-describedby="music-existing-lyrics-help" />
        {locale.t("music.lyrics_use_existing")}
      </label>
      <p id="music-existing-lyrics-help" class="text-xs leading-relaxed text-neutral-400">{locale.t("music.lyrics_use_existing_help")}</p>
    </div>
    {#if writingError}<p class="whitespace-pre-wrap break-words text-sm text-red-300" role="alert">{writingError}</p>{/if}
    <div class="flex justify-end gap-2">
      <button type="button" class="touch-target rounded-md px-4 text-sm text-neutral-300 hover:bg-neutral-800" onclick={closeLyricsDialog}>{locale.t("common.cancel")}</button>
      <button type="submit" class="touch-target rounded-md bg-indigo-500 px-4 text-sm font-medium text-[var(--theme-accent-foreground)] hover:bg-indigo-400 disabled:opacity-40" disabled={!!writing || promptAssistant.isGenerating || music.busy || !lyricsBrief.trim()} aria-busy={writing === "lyrics"}>{locale.t(writing === "lyrics" ? "music.assistant_writing_lyrics" : "music.assistant_lyrics")}</button>
    </div>
  </form>
</dialog>

<MusicSongDialog />
<div class="flex h-full min-h-0 flex-col">
  <nav class="flex shrink-0 items-center gap-2 border-b border-neutral-800 px-4 py-2" aria-label={locale.t("music.studio_title")}>
    {#each (["generate", "library"] as const) as view}
      <button type="button" class={`touch-target rounded-full px-5 text-sm font-medium transition-colors ${music.view === view ? 'bg-neutral-200 text-neutral-950' : 'text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200'}`} aria-current={music.view === view ? "page" : undefined} onclick={() => { music.view = view; }}>{locale.t(view === "generate" ? "music.compose" : "music.library")}</button>
    {/each}
    {#if music.busy && music.view === "library"}<span class="ml-auto text-xs text-indigo-300" role="status">{locale.t("music.generating")}</span>{/if}
  </nav>
{#if music.view === "library"}
  <MusicLibrary />
{:else}
<div class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-5 pb-[max(env(safe-area-inset-bottom),1rem)] md:px-6">
  <div class="mx-auto max-w-6xl">
    <header class="mb-6 flex items-center justify-between gap-4">
      <div class="flex items-baseline gap-3 border-l-[3px] border-indigo-400 pl-3">
        <h1 class="text-xl font-semibold tracking-tight text-neutral-100 sm:text-2xl">{locale.t("music.studio_title")}</h1>
        <span class="text-xs text-neutral-500">YuE2</span>
      </div>
      <button class="touch-target inline-flex shrink-0 items-center justify-center gap-2 rounded-sm px-2 text-xs text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200 focus-visible:outline-2 focus-visible:outline-indigo-400 disabled:opacity-40" aria-label={locale.t("music.refresh")} title={locale.t("music.refresh")} disabled={!connection.connected || music.refreshing} onclick={() => music.refresh()}>
        <svg class={`h-3.5 w-3.5 ${music.refreshing ? 'motion-safe:animate-spin' : ''}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 13 2l1 4M4 12l1 4a8 8 0 0 0 13 2"/></svg>
        <span>{locale.t("music.refresh")}</span>
      </button>
    </header>

    {#if !connection.connected}
      <p class="mb-5 text-sm text-neutral-400">{locale.t("music.connect")}</p>
    {:else if music.capabilities && music.capabilities.missing_nodes.length > 0}
      <div class="mb-5 space-y-2 border-l-2 border-amber-500 pl-3 text-sm text-amber-200">
        <p>{locale.t("music.needs_comfyui")}</p>
        <a class="inline-block underline" href="https://github.com/Comfy-Org/ComfyUI/pull/16250" target="_blank" rel="noreferrer">{locale.t("music.support_details")}</a>
      </div>
    {/if}

    {#if managedRuntime && (comfyuiUpdate.updateAvailable || (music.capabilities?.missing_nodes.length ?? 0) > 0)}
      <div class="mb-5 space-y-2 border-l-2 border-indigo-400 pl-3 text-sm text-neutral-300">
        <p>{locale.t("music.managed_update_help")}</p>
        <button class="touch-target rounded-sm bg-indigo-500 px-4 text-[var(--theme-accent-foreground)] disabled:opacity-40" disabled={comfyuiUpdate.updating || music.busy || music.downloading} onclick={updateRuntime}>{locale.t("music.update_runtime")}</button>
        {#if comfyuiUpdate.progress}<p role="status">{comfyuiUpdate.progress}</p>{/if}
        {#if comfyuiUpdate.error}<p class="text-red-300" role="alert">{comfyuiUpdate.error}</p>{/if}
      </div>
    {/if}

    <div class="grid items-start gap-7 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] lg:gap-8">
      <form class="min-w-0 space-y-4" onsubmit={(event) => { event.preventDefault(); if (!writing) void music.generate(); }} onchange={() => music.saveSettings()}>
        <label class="block space-y-2 text-sm font-medium text-neutral-200"><span>{locale.t("music.song_title")}</span><input class={inputClass} type="text" maxlength="200" bind:value={music.params.title} placeholder={locale.t("music.title_optional")} /></label>
        <div>
          <div class="flex flex-wrap items-center justify-between gap-x-3">
            <label for="music-style" class="text-sm font-medium text-neutral-200">{locale.t("music.style")}</label>
            {@render writingActions("style")}
          </div>
          <textarea id="music-style" class={inputClass} rows="4" maxlength="16000" bind:value={music.params.style} placeholder={locale.t("music.style_placeholder")} required></textarea>
        </div>
        <div>
          <div class="flex flex-wrap items-center justify-between gap-x-3">
            <label for="music-lyrics" class="text-sm font-medium text-neutral-200">{locale.t("music.lyrics")}</label>
            <div class="flex flex-wrap items-center gap-1">
              {@render writingActions("lyrics")}
              {#each (["verse", "chorus"] as const) as section}
                <button type="button" class="touch-target inline-flex items-center gap-1 rounded-md px-2 text-xs text-neutral-400 transition-colors hover:bg-neutral-900 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => addSection(section)} aria-label={locale.t(`music.add_${section}`)}>
                  <svg class="h-3 w-3" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="M8 3v10M3 8h10"/></svg>
                  {locale.t(`music.section_${section}`)}
                </button>
              {/each}
            </div>
          </div>
          <textarea id="music-lyrics" class={`${inputClass} min-h-48 leading-7`} rows="8" maxlength="64000" bind:this={lyricsEl} bind:value={music.params.lyrics} placeholder={locale.t("music.lyrics_placeholder")} aria-describedby="music-writing-help" required></textarea>
          <p id="music-writing-help" class="mt-2 text-xs leading-relaxed text-neutral-500">{locale.t("music.lyrics_dialog_help")}</p>
          {#if music.params.abc.trim()}<p class="mt-2 text-xs leading-relaxed text-amber-300">{locale.t("music.assistant_score_help")}</p>{/if}
        </div>
        {#if writingError}<p class="whitespace-pre-wrap break-words border-l-2 border-red-400 pl-3 text-xs leading-relaxed text-red-300" role="alert">{writingError}</p>{/if}
        <div class="space-y-1.5">
          <div class="flex items-center justify-between gap-3">
            <label for="music-duration" class="text-sm font-medium text-neutral-300">{locale.t("music.max_length")}</label>
            <div class="flex items-center gap-2">
              <input id="music-duration" class={`${inputClass} max-w-20 text-right font-mono tabular-nums`} type="number" min="1" max="360" step="1" bind:value={music.params.max_duration} aria-describedby="music-duration-unit music-duration-help" required />
              <span id="music-duration-unit" class="text-xs text-neutral-500">{locale.t("music.seconds")}</span>
            </div>
          </div>
          <p id="music-duration-help" class="text-xs leading-relaxed text-neutral-500">{locale.t("music.duration_brief")}</p>
        </div>

        <details class="group/settings" bind:this={settingsEl} open={music.capabilities?.checkpoints.length === 0}>
          <summary class="flex min-h-11 cursor-pointer list-none items-center gap-2 rounded-md text-sm text-neutral-400 transition-colors hover:text-neutral-200 focus-visible:outline-2 focus-visible:outline-indigo-400 [&::-webkit-details-marker]:hidden">
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><path d="M4 7h5m4 0h7M4 17h9m4 0h3M9 4v6m4 4v6"/></svg>
            {locale.t("music.settings_short")}
            <svg class="ml-auto h-4 w-4 transition-transform group-open/settings:rotate-180 motion-reduce:transition-none" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="m4 6 4 4 4-4"/></svg>
          </summary>
          <div class="space-y-3 pb-3">
            <label class="block space-y-1.5 text-xs text-neutral-400">
              <span>{locale.t("music.checkpoint")}</span>
              <select class={inputClass} bind:value={music.params.checkpoint} disabled={music.busy}>
                <option value="" disabled>{locale.t("music.select_model")}</option>
                {#each music.capabilities?.checkpoints ?? [] as checkpoint}<option value={checkpoint}>{checkpoint}</option>{/each}
              </select>
            </label>
            <div class="grid grid-cols-2 gap-3">
              <label class="space-y-1.5 text-xs text-neutral-400"><span>{locale.t("music.seed")}</span><input class={inputClass} type="text" inputmode="text" bind:value={music.params.seed} oninvalid={() => { if (settingsEl) settingsEl.open = true; }} required /></label>
              <label class="space-y-1.5 text-xs text-neutral-400"><span>{locale.t("generation.steps.title")}</span><input class={inputClass} type="number" min="1" max="100" step="1" bind:value={music.params.steps} oninvalid={() => { if (settingsEl) settingsEl.open = true; }} required /></label>
            </div>
            <label class="block space-y-1.5 text-xs text-neutral-400">
              <span>{locale.t("music.planning")}</span>
              <select class={inputClass} bind:value={music.params.planning}>
                <option value="full">{locale.t("music.plan_full")}</option><option value="melody">{locale.t("music.plan_melody")}</option><option value="off">{locale.t("music.plan_off")}</option>
              </select>
            </label>
            <label class="block space-y-1.5 text-xs text-neutral-400">
              <span>{locale.t("music.abc")}</span>
              <textarea class={`${inputClass} font-mono text-xs`} rows="4" maxlength="128000" bind:value={music.params.abc} placeholder={locale.t("music.abc_help")}></textarea>
            </label>
            <details open={music.capabilities?.checkpoints.length === 0}>
              <summary class="min-h-11 cursor-pointer content-center text-xs text-neutral-400">{locale.t("music.setup")}</summary>
              <div class="space-y-3 pb-2 text-xs leading-relaxed text-neutral-400">
                <p>{locale.t("music.models_help")}</p>
                <p>{locale.t("music.hardware")}</p>
                <p>{locale.t("music.license")}</p>
                <a class="inline-block text-indigo-300 underline" href="https://huggingface.co/Comfy-Org/YuE2" target="_blank" rel="noreferrer">{locale.t("music.model_page")}</a>
                {#if canDownload}
                  <div class="flex flex-wrap gap-2">
                    <button type="button" class="touch-target rounded-sm bg-neutral-800 px-3 text-neutral-200 disabled:opacity-40" disabled={music.downloading || !connection.connected} onclick={() => music.downloadCheckpoint("int8_convrot")}>{locale.t("music.download_int8")}</button>
                    <button type="button" class="touch-target rounded-sm bg-neutral-800 px-3 text-neutral-200 disabled:opacity-40" disabled={music.downloading || !connection.connected} onclick={() => music.downloadCheckpoint("bf16")}>{locale.t("music.download_bf16")}</button>
                  </div>
                {/if}
                {#if music.downloading}<p role="status">{locale.t("music.downloading")}</p>{/if}
              </div>
            </details>
          </div>
        </details>
        <div class="flex items-center gap-3">
          <button type="submit" class="touch-target inline-flex min-h-12 flex-1 items-center justify-center gap-2.5 rounded-md bg-indigo-500 px-4 text-sm font-semibold text-[var(--theme-accent-foreground)] transition-colors hover:bg-indigo-400 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 disabled:opacity-40" disabled={!connection.connected || !music.ready || music.busy || !!writing}>
            <svg class={`h-4 w-4 ${music.busy ? 'motion-safe:animate-pulse' : ''}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 17V6l10-2v11M9 10l10-2"/><ellipse cx="6" cy="17" rx="3" ry="2"/><ellipse cx="16" cy="15" rx="3" ry="2"/></svg>
            {locale.t(music.busy ? "music.generating" : "music.generate")}
          </button>
          {#if music.job}<button type="button" class="touch-target rounded-sm px-3 text-sm text-neutral-400 hover:bg-neutral-900 disabled:opacity-40" disabled={music.cancelling} onclick={() => music.cancel()}>{locale.t("common.cancel")}</button>{/if}
        </div>
        <MusicProgress />
        {#if music.error}<p class="whitespace-pre-wrap break-words border-l-2 border-red-400 pl-3 text-xs leading-relaxed text-red-300" role="alert">{music.error}</p>{/if}
      </form>

      <section class="min-w-0 space-y-5 lg:sticky lg:top-0" aria-label={locale.t("music.results")}>
        <MusicPlayer />
        {#if music.selectedResult}
          <div class="space-y-1">
            {#if music.selectedResult.abc}
              <details><summary class="min-h-11 cursor-pointer content-center text-xs text-neutral-400">{locale.t("music.abc")}</summary><pre class="max-h-64 overflow-auto whitespace-pre-wrap break-words text-xs leading-relaxed text-neutral-400">{music.selectedResult.abc}</pre><button type="button" class="touch-target my-2 rounded-sm px-3 text-xs text-indigo-300 hover:bg-neutral-900" onclick={() => music.reuseScore()}>{locale.t("music.reuse_score")}</button></details>
            {/if}
          </div>
        {/if}
        {#if music.results.length > 0}
          <div>
            <h2 class="mb-3 flex items-baseline gap-2 text-sm font-medium text-neutral-200">{locale.t("music.results")} <span class="font-mono text-xs text-neutral-500">{music.results.length}</span></h2>
            <ul class="max-h-80 space-y-1 overflow-y-auto">
              {#each music.results as result, index (result.prompt_id)}
                {@const selected = music.selectedResult?.prompt_id === result.prompt_id}
                {@const resultTitle = music.resultTitle(result)}
                <li>
                  <button type="button" class={`group flex min-h-14 w-full items-center gap-3 rounded-r-md border-l-2 px-3 py-2 text-left transition-colors hover:bg-neutral-900 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400 disabled:opacity-40 ${selected ? 'border-indigo-400 bg-neutral-900' : 'border-transparent'}`} disabled={music.loadingAudio} onclick={() => music.play(result)} aria-current={selected ? "true" : undefined} aria-label={locale.t("music.load_song", { title: resultTitle })}>
                    <span class={`w-5 shrink-0 font-mono text-xs ${selected ? 'text-indigo-300' : 'text-neutral-500'}`} aria-hidden="true">{String(music.results.length - index).padStart(2, "0")}</span>
                    <span class="min-w-0 flex-1"><span class={`block truncate text-sm ${selected ? 'text-indigo-300' : 'text-neutral-300'}`}>{resultTitle}</span><span class="mt-0.5 block truncate text-xs text-neutral-500">{result.params.style}</span></span>
                    {#if selected}
                      <svg class="h-4 w-4 shrink-0 text-indigo-300" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="m3 8 3 3 7-7"/></svg>
                    {:else}<span class="shrink-0 text-[10px] text-neutral-500">FLAC</span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
            <p class="mt-3 text-[11px] leading-relaxed text-neutral-500">{locale.t("music.library_local")}</p>
          </div>
        {/if}
      </section>
    </div>
  </div>
</div>
{/if}
</div>
