<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { musicCover } from "../../stores/musicCover.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { COVER_EXAMPLE, coverScoreError, coverSourceMaxDuration, melodyOnlyAbc } from "../../utils/musicCover.js";
  import { getAuthUser } from "../../utils/ipc.js";
  import { prepareCoverScore, type ScoreVoiceSelection } from "../../utils/musicScore.js";
  import type { MusicParams } from "../../types/music.js";

  let { canDownload = false }: { canDownload?: boolean } = $props();
  let retainedVoice = $state<ScoreVoiceSelection>("both");

  let source = $state<File | null>(null);
  let sourceUrl = $state("");
  let sourceLength = $state<number | null>(null);
  let durationError = $state("");
  let sourceTooLong = $state(false);
  let sourceLimitBefore = 0;
  let sourceOwner: string | null = null;
  let error = $state("");
  let undo = $state<{ before: string; after: string; planning: MusicParams["planning"]; afterPlanning: MusicParams["planning"] } | null>(null);
  let importing = $state(false);
  let active = true;
  const scoreError = $derived(coverScoreError(music.params.abc, music.params.planning === "full"));
  const locked = $derived(music.busy || musicCover.busy || importing);
  const inputClass = "w-full min-w-0 rounded-md bg-neutral-900 px-3 py-2.5 text-sm text-neutral-200 focus:outline-2 focus:outline-indigo-400 disabled:opacity-40";
  const buttonClass = "touch-target rounded-md px-3 text-xs text-indigo-300 hover:bg-neutral-900 disabled:opacity-40";
  onMount(() => { void musicCover.refresh(); });
  onDestroy(() => { active = false; if (sourceUrl) URL.revokeObjectURL(sourceUrl); });

  function replaceScore(abc: string, planning = music.params.planning) {
    const nextPlanning = planning === "off" ? "melody" : planning;
    undo = { before: music.params.abc, after: abc, planning: music.params.planning, afterPlanning: nextPlanning };
    music.params.abc = abc;
    music.params.planning = nextPlanning;
    music.reviewedCoverAbc = "";
    music.saveSettings();
  }
  async function importScore(file?: File) {
    if (!file) return;
    error = "";
    if (file.size > 131072) { error = locale.t("music.cover_score_limit"); return; }
    const before = music.params.abc;
    importing = true;
    try {
      const abc = (await file.text()).replace(/^\uFEFF/, "");
      if (active && music.params.abc === before && music.params.cover) replaceScore(abc);
    } catch (e) { if (active) error = String(e); }
    finally { importing = false; }
  }
  function chooseSource(file?: File) {
    error = "";
    if (!file) return;
    if (!file.size || file.size > 64 * 1024 * 1024) { error = locale.t("music.cover_audio_limit"); return; }
    if (sourceUrl) URL.revokeObjectURL(sourceUrl);
    source = file;
    music.reviewSource = file;
    sourceUrl = URL.createObjectURL(file);
    sourceLength = null; durationError = ""; sourceTooLong = false;
    sourceLimitBefore = music.params.max_duration;
    sourceOwner = getAuthUser();
  }
  function readSourceDuration(media: HTMLMediaElement) {
    // Old files, late account responses and repeated duration events must not
    // reset a maximum the user already extended after this import.
    if (!active || media.getAttribute("src") !== sourceUrl || sourceOwner !== getAuthUser() || sourceLength !== null) return;
    const seconds = media.duration;
    sourceTooLong = Number.isFinite(seconds) && seconds > 360;
    const limit = coverSourceMaxDuration(seconds);
    if (limit === null) {
      durationError = locale.t(sourceTooLong ? "music.cover_duration_limit" : "music.cover_duration_unavailable");
      return;
    }
    sourceLength = limit; durationError = "";
    if (!music.busy && !musicCover.busy && music.params.max_duration === sourceLimitBefore) useSourceLength();
  }
  function useSourceLength() {
    if (sourceLength === null || sourceOwner !== getAuthUser() || locked) return;
    music.params.max_duration = sourceLength;
    music.saveSettings();
  }
  function sourceDurationFailed(media: Element) {
    if (active && media.getAttribute("src") === sourceUrl && sourceOwner === getAuthUser() && sourceLength === null) {
      durationError = locale.t("music.cover_duration_unavailable");
    }
  }
  function useTranscription() {
    try {
      error = "";
      const abc = prepareCoverScore(musicCover.candidate, retainedVoice, musicCover.candidateMode === "full");
      replaceScore(abc, musicCover.candidateMode);
    } catch (cause) { error = String(cause); }
  }
  function selectMelody() {
    try { error = ""; replaceScore(prepareCoverScore(music.params.abc, retainedVoice, music.params.planning === "full")); }
    catch (cause) { error = String(cause); }
  }
</script>

<section class="space-y-3 border-l-2 border-indigo-400 pl-3" aria-label={locale.t("music.cover")}>
  <p class="text-xs leading-relaxed text-neutral-400">{locale.t("music.cover_help")}</p>
  <div class="grid gap-3 sm:grid-cols-2">
    <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("music.cover_fidelity")}</span><select class={inputClass} bind:value={music.params.planning} disabled={locked} onchange={() => { music.reviewedCoverAbc = ""; music.saveSettings(); }}><option value="melody">{locale.t("music.cover_free_harmony")}</option><option value="full">{locale.t("music.cover_keep_harmony")}</option></select></label>
    <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("music.cover_parts")}</span><select class={inputClass} bind:value={retainedVoice} disabled={locked}><option value="both">{locale.t("music.score_both")}</option><option value="Vocal">{locale.t("music.score_vocal")}</option><option value="Ins">{locale.t("music.score_instrumental")}</option></select></label>
  </div>
  <details>
    <summary class="touch-target cursor-pointer content-center text-sm text-neutral-300">{locale.t("music.cover_source")}</summary>
    <div class="space-y-3 pb-2">
      <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.transcription_backend")}</span><select class={inputClass} bind:value={musicCover.backend} disabled={locked}><option value="native">{locale.t("music.transcription_native")}</option><option value="separate">{locale.t("music.transcription_separate")}</option></select></label>
      {#if musicCover.backend === "native"}
        <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.transcription_model")}</span><select class={inputClass} bind:value={musicCover.encoder} disabled={locked}><option value="">{locale.t("music.select_model")}</option>{#each musicCover.nativeCapabilities?.audio_encoders ?? [] as encoder}<option value={encoder}>{encoder}</option>{/each}</select></label>
        {#if !musicCover.nativeSupported}<p class="text-xs text-amber-300">{locale.t("music.transcription_update")}</p>{/if}
        {#if canDownload}<button type="button" class={buttonClass} disabled={musicCover.downloading || locked} onclick={() => musicCover.downloadEncoder()}>{locale.t(musicCover.downloading ? "music.downloading" : "music.transcription_download")}</button>{/if}
        <p class="text-xs text-neutral-500">{locale.t("music.transcription_native_help")}</p>
      {/if}
      <label class="block space-y-2 text-xs text-neutral-400">
        <span>{locale.t("music.cover_audio")}</span>
        <input class={`${inputClass} file:mr-2 file:rounded file:border-0 file:bg-neutral-800 file:px-2 file:py-1 file:text-neutral-300`} type="file" accept=".wav,.mp3,.flac,.m4a,.ogg,.opus,.aiff,.aif,audio/*" disabled={locked} onchange={event => chooseSource(event.currentTarget.files?.[0])} />
      </label>
      {#if sourceUrl}
        {#key sourceUrl}<audio class="h-10 w-full" controls preload="metadata" src={sourceUrl} aria-label={locale.t("music.cover_source_preview")} onloadedmetadata={event => readSourceDuration(event.currentTarget)} ondurationchange={event => readSourceDuration(event.currentTarget)} onerror={event => sourceDurationFailed(event.currentTarget)}></audio>{/key}
        {#if sourceLength !== null}
          <div class="flex flex-wrap items-center gap-2">
            <p class="text-xs text-neutral-400" role="status">{locale.t("music.cover_source_length", { seconds: sourceLength })}</p>
            <button type="button" class={buttonClass} disabled={locked || music.params.max_duration === sourceLength} onclick={useSourceLength}>{locale.t("music.cover_use_source_length")}</button>
          </div>
        {/if}
        {#if durationError}<p class="text-xs text-amber-300" role="status">{durationError}</p>{/if}
      {/if}
      <div class="flex flex-wrap items-center gap-2">
        <button type="button" class={buttonClass} disabled={!source || sourceTooLong || !musicCover.configured || locked} onclick={() => { if (source && !sourceTooLong) void musicCover.transcribe(source, music.params.planning === "full" ? "full" : "melody"); }}>{locale.t("music.cover_transcribe")}</button>
        <button type="button" class={buttonClass} disabled={musicCover.refreshing} onclick={() => musicCover.refresh()}>{locale.t("music.refresh")}</button>
        {#if musicCover.configured && musicCover.backend === "separate"}<span class="text-xs text-neutral-500">{locale.t("music.cover_device", { device: musicCover.device })}</span>{/if}
      </div>
      {#if musicCover.backend === "separate"}
        <p class="text-xs text-neutral-400">{locale.t("music.transcription_separate_help")}</p>
      {/if}
      {#if !musicCover.configured && musicCover.backend === "separate"}
        <p class="text-xs leading-relaxed text-neutral-400">{locale.t("music.cover_setup")}</p>
        <a class="text-xs text-indigo-300 underline" href="https://github.com/multimodal-art-projection/YuE/blob/main/docs/covers.md" target="_blank" rel="noreferrer">{locale.t("music.cover_guide")}</a>
      {/if}
    </div>
  </details>
  {#if musicCover.busy}
    <div class="flex flex-wrap items-center gap-2 text-xs text-indigo-300" role="status">
      <span>{locale.t("music.cover_transcribing", { filename: musicCover.filename })}</span>
      <button type="button" class={buttonClass} disabled={!musicCover.jobId || musicCover.cancelling} onclick={() => musicCover.cancel()}>{locale.t("common.cancel")}</button>
    </div>
  {/if}
  {#if musicCover.error}<p class="whitespace-pre-wrap break-words text-xs text-red-300" role="alert">{musicCover.error}</p>{/if}
  {#if musicCover.candidate}
    <details class="space-y-2">
      <summary class="touch-target cursor-pointer content-center text-xs text-indigo-300">{locale.t("music.cover_transcribed_score")}</summary>
      <pre class="max-h-48 overflow-auto whitespace-pre-wrap text-xs text-neutral-400">{musicCover.candidate}</pre>
      {#each musicCover.warnings as warning}<p class="text-xs text-amber-300">{warning}</p>{/each}
      <button type="button" class={buttonClass} disabled={locked} onclick={useTranscription}>{locale.t("music.cover_use_score")}</button>
    </details>
  {/if}
  <div class="flex flex-wrap items-center gap-1">
    <label class={`${buttonClass} inline-flex cursor-pointer items-center focus-within:outline-2 focus-within:outline-indigo-400 has-disabled:opacity-40`}>
      {locale.t("music.cover_import")}
      <input class="sr-only" type="file" accept=".abc,.txt,text/plain" disabled={locked} onchange={event => { void importScore(event.currentTarget.files?.[0]); event.currentTarget.value = ""; }} />
    </label>
    <button type="button" class={buttonClass} disabled={locked} onclick={() => replaceScore(COVER_EXAMPLE)}>{locale.t("music.cover_example")}</button>
    <button type="button" class={buttonClass} disabled={locked || scoreError !== "cover_chords"} onclick={() => replaceScore(melodyOnlyAbc(music.params.abc))}>{locale.t("music.cover_remove_chords")}</button>
    <button type="button" class={buttonClass} disabled={locked || !music.params.abc.trim()} onclick={selectMelody}>{locale.t("music.cover_apply_parts")}</button>
    {#if undo && music.params.abc === undo.after && music.params.planning === undo.afterPlanning}<button type="button" class={buttonClass} disabled={locked} onclick={() => { if (undo) { music.params.abc = undo.before; music.params.planning = undo.planning; music.reviewedCoverAbc = ""; undo = null; music.saveSettings(); } }}>{locale.t("prompt_assistant.undo")}</button>{/if}
  </div>
  <label class="block space-y-2 text-sm text-neutral-200">
    <span>{locale.t("music.cover_score")}</span>
    <textarea class={`${inputClass} font-mono text-xs leading-6`} rows="8" maxlength="131072" bind:value={music.params.abc} disabled={music.busy || importing} placeholder={locale.t("music.cover_score_placeholder")} oninput={() => { music.reviewedCoverAbc = ""; }}></textarea>
  </label>
  {#if scoreError && music.params.abc.trim()}<p class="text-xs text-amber-300">{locale.t(`music.${scoreError}`)}</p>{/if}
  {#if error}<p class="text-xs text-red-300" role="alert">{error}</p>{/if}
  <label class="touch-target flex items-start gap-2 py-2 text-xs leading-relaxed text-neutral-300 has-disabled:opacity-40">
    <input type="checkbox" class="mt-0.5 h-4 w-4 shrink-0 accent-indigo-500" checked={music.coverReady} disabled={!!scoreError || locked} onchange={event => { music.reviewedCoverAbc = event.currentTarget.checked ? music.params.abc : ""; }} />
    <span>{locale.t("music.cover_review")}</span>
  </label>
</section>
