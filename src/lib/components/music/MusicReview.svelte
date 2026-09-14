<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { getAuthUser } from "../../utils/ipc.js";
  import { getMusicReviewCapabilities, hashMusicReviewAudio, transcribeMusicReview } from "../../utils/api.js";
  import { audioTime, blobBase64 } from "../../utils/musicAudio.js";
  import { stopScorePreview } from "../../utils/musicPreview.js";
  import { analyzeMusicReview, validateMusicTranscript, type MusicReviewRecord, type MusicTranscript } from "../../utils/musicReview.js";
  import type { MusicResult } from "../../types/music.js";

  let dialog: HTMLDialogElement | undefined = $state();
  let song = $state<MusicResult | null>(null), record = $state<MusicReviewRecord | null>(null);
  let sourceFile = $state.raw<File | null>(null), sourceUrl = $state(""), coverUrl = $state("");
  let sourcePlayer: HTMLAudioElement | undefined = $state(), coverPlayer: HTMLAudioElement | undefined = $state();
  let sameLyrics = $state(true), busy = $state(false), loading = $state(false), error = $state(""), stage = $state("");
  let sourceMatches = $state(false);
  let owner: string | null = null, sequence = 0, sourceSequence = 0;
  let coverBlob = $state.raw<Blob | null>(null);
  let sourceTranscript: MusicTranscript | null = null, coverTranscript: MusicTranscript | null = null;
  const button = "touch-target rounded-md px-3 text-xs text-indigo-300 hover:bg-neutral-800 disabled:opacity-40";
  const current = (id: number) => id === sequence && owner === getAuthUser() && music.reviewingSong === song?.prompt_id;
  const seconds = (value: number | null) => value === null ? locale.t("music.review_unassessed") : locale.t("music.review_seconds", { value: `${value > 0 ? "+" : ""}${locale.formatDecimal(value, 2)}` });

  function release() {
    sourcePlayer?.pause(); coverPlayer?.pause();
    if (sourceUrl) URL.revokeObjectURL(sourceUrl);
    if (coverUrl) URL.revokeObjectURL(coverUrl);
    sourceUrl = ""; coverUrl = ""; coverBlob = null; sourceFile = null;
    record = null; song = null; sourceTranscript = null; coverTranscript = null; sourceMatches = false;
  }
  function close() { sequence++; sourceSequence++; busy = false; loading = false; music.reviewingSong = null; release(); dialog?.close(); }
  onDestroy(() => { sequence++; sourceSequence++; release(); });
  $effect(() => {
    const id = music.reviewingSong, element = dialog;
    if (!element) return;
    untrack(() => {
      if (!id) { sequence++; sourceSequence++; release(); element.close(); return; }
      const selected = music.results.find(item => item.prompt_id === id);
      if (!selected) return;
      release(); sequence++; sourceSequence++; busy = false; error = ""; stage = ""; loading = true;
      song = selected; owner = getAuthUser();
      record = selected.review?.version === 1 && selected.review.lyrics === selected.params.lyrics ? selected.review : null;
      coverTranscript = record?.cover ?? null; sourceTranscript = null; sourceMatches = false;
      sameLyrics = record?.report.sameLyricsAndTempo ?? true;
      if (!element.open) element.showModal();
      void prepare(sequence, selected, music.reviewSource);
    });
  });

  async function prepare(id: number, selected: MusicResult, file: File | null) {
    try {
      const blob = await music.archive(selected);
      if (!current(id)) return;
      coverBlob = blob; coverUrl = URL.createObjectURL(blob);
      if (record) {
        const hash = await hashMusicReviewAudio(await blobBase64(blob));
        if (!current(id)) return;
        if (hash !== record.cover.audio_sha256) { record = null; coverTranscript = null; }
      }
      if (file) await chooseSource(file);
    } catch (cause) { if (current(id)) error = String(cause); }
    finally { if (current(id)) loading = false; }
  }
  async function chooseSource(file?: File) {
    if (!file || busy) return;
    if (!file.size || file.size > 64 * 1024 * 1024) { error = locale.t("music.cover_audio_limit"); return; }
    const id = sequence, selected = ++sourceSequence;
    sourcePlayer?.pause(); if (sourceUrl) URL.revokeObjectURL(sourceUrl);
    sourceFile = file; sourceUrl = URL.createObjectURL(file); sourceTranscript = null; sourceMatches = false; error = "";
    // Saved timestamps can only play against the exact recording they describe.
    if (record?.source) {
      try {
        const hash = await hashMusicReviewAudio(await blobBase64(file));
        if (!current(id) || selected !== sourceSequence) return;
        if (hash === record.source.audio_sha256) { sourceTranscript = record.source; sourceMatches = true; }
      } catch (cause) { if (current(id) && selected === sourceSequence) error = String(cause); }
    }
  }
  async function duration(blob: Blob) {
    if (!blob.size || blob.size > 64 * 1024 * 1024) throw new Error(locale.t("music.cover_audio_limit"));
    const url = URL.createObjectURL(blob), audio = new Audio();
    try {
      return await new Promise<number>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(locale.t("music.review_duration_error"))), 10000);
        audio.onloadedmetadata = () => {
          clearTimeout(timer);
          if (Number.isFinite(audio.duration) && audio.duration > 0 && audio.duration <= 360) resolve(audio.duration);
          else reject(new Error(locale.t("music.review_duration_error")));
        };
        audio.onerror = () => { clearTimeout(timer); reject(new Error(locale.t("music.review_duration_error"))); };
        audio.preload = "metadata"; audio.src = url;
      });
    } finally { audio.removeAttribute("src"); audio.load(); URL.revokeObjectURL(url); }
  }
  async function save(id: number, value: MusicReviewRecord) {
    if (!current(id) || !song) return;
    if (!await music.updateSong(song.prompt_id, { review: value })) throw new Error(music.libraryError || locale.t("music.library_error"));
  }
  async function explain(id: number, value: MusicReviewRecord, selected: MusicResult) {
    stage = "music.review_explaining";
    await promptAssistant.refreshStatus();
    if (!current(id)) return;
    if (!promptAssistant.isAvailable) throw new Error(locale.t("music.assistant_setup"));
    const explanation = await promptAssistant.reviewForMusic(value, selected, locale.current, () => current(id));
    if (!current(id)) return;
    record = { ...value, explanation }; await save(id, record);
  }
  async function review(explanationOnly = false) {
    if (!song || !coverBlob || busy || loading || promptAssistant.isGenerating) return;
    const id = sequence, selected = song, file = sourceFile, same = sameLyrics;
    busy = true; error = "";
    try {
      if (explanationOnly && record) { await explain(id, record, selected); return; }
      if (same && !file && !record?.source) throw new Error(locale.t("music.review_source_required"));
      stage = "music.review_preparing";
      const ready = await getMusicReviewCapabilities();
      if (!current(id)) return;
      if (!ready) throw new Error(locale.t("music.review_setup"));
      if (!coverTranscript) await duration(coverBlob);
      if (same && file && !sourceTranscript) await duration(file);
      if (!current(id)) return;
      if (same && file && !sourceTranscript) {
        stage = "music.review_transcribing_source";
        const encoded = await blobBase64(file);
        if (!current(id)) return;
        const result = validateMusicTranscript(await transcribeMusicReview(encoded));
        if (!current(id)) return;
        sourceTranscript = result;
      }
      if (!coverTranscript) {
        stage = "music.review_transcribing_cover";
        const encoded = await blobBase64(coverBlob);
        if (!current(id)) return;
        const result = validateMusicTranscript(await transcribeMusicReview(encoded));
        if (!current(id)) return;
        coverTranscript = result;
      }
      const reference = same ? sourceTranscript ?? (file ? null : record?.source ?? null) : null;
      record = { version: 1, createdAt: Date.now(), lyrics: selected.params.lyrics, sourceName: file?.name ?? record?.sourceName ?? "",
        source: reference, cover: coverTranscript, report: analyzeMusicReview(selected.params.lyrics, coverTranscript, reference, same) };
      sourceMatches = !!file && !!sourceTranscript && sourceTranscript.audio_sha256 === record.source?.audio_sha256;
      // Save measurements before calling the LLM. A failed explanation can be
      // retried without paying to transcribe the recordings a second time.
      await save(id, record);
      if (current(id)) await explain(id, record, selected);
    } catch (cause) { if (current(id)) error = String(cause); }
    finally { if (current(id)) { busy = false; stage = ""; } }
  }
  function exclusive(which: "source" | "cover") {
    if (music.playing) music.togglePlayback(); stopScorePreview();
    if (which === "source") coverPlayer?.pause(); else sourcePlayer?.pause();
  }
  function seek(which: "source" | "cover", start: number | null) {
    const player = which === "source" ? sourcePlayer : coverPlayer;
    if (!player || start === null || (which === "source" && !sourceMatches)) return;
    exclusive(which); player.currentTime = Math.max(0, start - 0.25);
    void player.play().catch(() => { error = locale.t("music.playback_error"); });
  }
</script>

<dialog bind:this={dialog} class="m-auto max-h-[90vh] w-[calc(100%-2rem)] max-w-3xl overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-neutral-200 shadow-2xl backdrop:bg-black/70" aria-labelledby="music-review-title" onclose={close}>
  <div class="space-y-4">
    <div class="flex items-center justify-between gap-3"><h2 id="music-review-title" class="text-lg font-semibold">{locale.t("music.review")}</h2><button type="button" class={button} onclick={close}>{locale.t("common.close")}</button></div>
    {#if song}<p class="text-sm">{music.resultTitle(song)}</p>{/if}
    <p class="text-xs leading-relaxed text-neutral-400">{locale.t("music.review_help")}</p>
    <label class="block space-y-2 text-xs text-neutral-400"><span>{locale.t("music.review_source")}</span><input type="file" accept=".wav,.flac,.mp3,.ogg,.opus,.m4a,.aiff,.aif" class="block w-full min-w-0 rounded-md bg-neutral-800 p-2" disabled={busy || loading} onchange={event => { void chooseSource(event.currentTarget.files?.[0]); event.currentTarget.value = ""; }} /></label>
    <label class="touch-target flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={sameLyrics} disabled={busy} />{locale.t("music.review_same_lyrics")}</label>
    <div class="grid gap-3 sm:grid-cols-2">
      {#if sourceUrl}<div class="space-y-2"><p class="truncate text-xs text-neutral-400">{sourceFile?.name}</p><!-- svelte-ignore a11y_media_has_caption --><audio bind:this={sourcePlayer} src={sourceUrl} controls preload="metadata" class="w-full" aria-label={locale.t("music.review_source")} onplay={() => exclusive("source")}></audio></div>{/if}
      {#if coverUrl}<div class="space-y-2"><p class="text-xs text-neutral-400">{locale.t("music.review_cover")}</p><!-- svelte-ignore a11y_media_has_caption --><audio bind:this={coverPlayer} src={coverUrl} controls preload="metadata" class="w-full" aria-label={locale.t("music.review_cover")} onplay={() => exclusive("cover")}></audio></div>{/if}
    </div>
    <p class="text-xs text-neutral-400">{locale.t("music.review_disclosure")}</p>
    <div class="flex flex-wrap items-center gap-2"><button type="button" class={`${button} bg-indigo-500/15`} disabled={busy || loading || promptAssistant.isGenerating || !coverBlob || !song?.params.lyrics.trim()} onclick={() => review()}>{locale.t("music.review_start")}</button>
      {#if record}<button type="button" class={button} disabled={busy || loading || promptAssistant.isGenerating} onclick={() => review(true)}>{locale.t("music.review_explain")}</button>{/if}
      {#if busy || loading}<p role="status" class="text-xs text-indigo-300">{locale.t(stage || "common.loading")}</p>{/if}
    </div>
    {#if error}<p class="whitespace-pre-wrap text-xs text-red-300" role="alert">{error}</p>{/if}
    {#if record}
      <section class="space-y-3 border-t border-neutral-700 pt-3" aria-label={locale.t("music.review_results")}>
        <h3 class="text-sm font-semibold">{locale.t("music.review_results")}</h3>
        <p class="text-xs text-neutral-400">{locale.t("music.review_metrics", { coverage: Math.round(record.report.coverCoverage * 100), offset: seconds(record.report.offset), drift: seconds(record.report.drift) })}</p>
        <p class="text-xs text-neutral-400">{locale.t("music.review_caution")}</p>
        {#if record.source && !sourceMatches}<p class="text-xs text-amber-300">{locale.t("music.review_reattach", { name: record.sourceName })}</p>{/if}
        {#if !record.report.sameLyricsAndTempo}<p class="text-xs text-amber-300">{locale.t("music.review_changed_lyrics")}</p>{/if}
        {#if record.explanation}
          <p class="whitespace-pre-wrap text-sm">{record.explanation.summary}</p>
          {#each record.explanation.issues as issue}
            {@const evidence = record.report.lines.find(line => line.id === issue.evidence_id)}
            <div class="space-y-1 rounded-md bg-neutral-800 p-3 text-xs"><p>{issue.explanation}</p><p class="text-neutral-400">{issue.suggestion}</p>
              {#if evidence}<div class="flex flex-wrap gap-1">
                {#if evidence.cover.start !== null}<button type="button" class={button} onclick={() => seek("cover", evidence.cover.start)}>{locale.t("music.review_play_cover", { time: audioTime(evidence.cover.start) })}</button>{/if}
                {#if evidence.source?.start !== null && evidence.source?.start !== undefined}<button type="button" class={button} disabled={!sourceMatches} onclick={() => seek("source", evidence.source!.start)}>{locale.t("music.review_play_source", { time: audioTime(evidence.source.start) })}</button>{/if}
              </div>{/if}
            </div>
          {/each}
        {/if}
        <details><summary class="touch-target cursor-pointer content-center text-sm text-neutral-400">{locale.t("music.review_evidence")}</summary>
          <div class="space-y-3">
            {#each record.report.lines as line}
              <div class="rounded-md bg-neutral-800 p-3 text-xs"><p>{line.line}. {line.section} {line.lyrics}</p>
                <p class="mt-1 text-neutral-400">{locale.t("music.review_line_match", { coverage: Math.round(line.cover.coverage * 100), delta: seconds(line.delta) })}</p>
                {#if line.repeated}<p class="text-amber-300">{locale.t("music.review_repeated")}</p>{/if}
                <div class="flex flex-wrap gap-1">
                  {#if line.cover.start !== null}<button type="button" class={button} onclick={() => seek("cover", line.cover.start)}>{locale.t("music.review_play_cover", { time: audioTime(line.cover.start) })}</button>{/if}
                  {#if line.source?.start !== null && line.source?.start !== undefined}<button type="button" class={button} disabled={!sourceMatches} onclick={() => seek("source", line.source!.start)}>{locale.t("music.review_play_source", { time: audioTime(line.source.start) })}</button>{/if}
                </div>
              </div>
            {/each}
            {#if record.report.extraCoverWords.length}<p class="text-xs text-neutral-400">{locale.t("music.review_extra")}</p><div class="flex flex-wrap gap-1">{#each record.report.extraCoverWords as word}<button type="button" class={button} onclick={() => seek("cover", word.start)}>{audioTime(word.start)} {word.text}</button>{/each}</div>{/if}
            <pre class="max-h-60 overflow-auto whitespace-pre-wrap text-xs text-neutral-400">{JSON.stringify({ source: record.source, cover: record.cover }, null, 2)}</pre>
          </div>
        </details>
      </section>
    {/if}
  </div>
</dialog>
