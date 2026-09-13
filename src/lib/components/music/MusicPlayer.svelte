<script lang="ts">
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { audioPeaks, audioTime } from "../../utils/musicAudio.js";
  import VinylRecord from "./VinylRecord.svelte";
  import MusicLyrics from "./MusicLyrics.svelte";

  const playing = $derived(music.playing);
  const playPending = $derived(music.playPending);
  const waiting = $derived(music.waiting);
  const playError = $derived(music.playError);
  const currentTime = $derived(music.currentTime);
  const duration = $derived(music.duration);
  const volume = $derived(music.volume);
  const muted = $derived(music.muted);
  const looping = $derived(music.looping);
  const title = $derived(music.title);
  let peaks = $state<number[]>([]);
  const progress = $derived(duration > 0 ? Math.min(100, Math.max(0, currentTime / duration * 100)) : 0);
  const hasAudio = $derived(!!music.audioUrl);
  const controlClass = "touch-target inline-flex items-center justify-center rounded-md transition-colors hover:bg-neutral-800 hover:text-neutral-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 disabled:pointer-events-none disabled:opacity-30";
  const togglePlayback = () => music.togglePlayback();
  const seek = (seconds: number) => music.seek(seconds);

  $effect(() => {
    const blob = music.audioBlob;
    let cancelled = false;
    peaks = [];
    if (blob) {
      void (async () => {
        try {
          // Low-rate offline decoding keeps waveform memory modest for long
          // songs and never opens an audio device or starts playback.
          const context = new OfflineAudioContext(1, 1, 8000);
          const buffer = await context.decodeAudioData(await blob.arrayBuffer());
          if (cancelled) return;
          const channels = Array.from({ length: buffer.numberOfChannels }, (_, i) => buffer.getChannelData(i));
          peaks = audioPeaks(channels);
        } catch {
          // A standard seek bar remains usable if waveform decoding is absent.
        }
      })();
    }
    return () => { cancelled = true; };
  });

</script>

<section class="min-w-0 rounded-md border-t-2 border-indigo-400/70 bg-neutral-900 p-4 sm:p-5" aria-label={locale.t("music.player")}>

  <div>
    <div class="mb-4 flex flex-wrap items-baseline justify-between gap-2 text-[10px]">
      <span class="font-medium uppercase tracking-[0.12em] text-neutral-400">{locale.t("music.player_brand")}</span>
      {#if hasAudio}<span class="font-mono text-neutral-500">{locale.t("music.lossless")}</span>{/if}
    </div>
    <div class="flex items-center gap-4">
      <VinylRecord {progress} spinning={playing && !waiting} />
      <div class="min-w-0 flex-1">
        <h2 class="break-words text-xl font-semibold leading-snug tracking-tight text-neutral-100">{hasAudio ? title : locale.t("music.empty_title")}</h2>
        {#if music.selectedResult}<button type="button" class="touch-target text-xs text-indigo-300 hover:underline" onclick={() => { music.editingSong = music.selectedResult!.prompt_id; }}>{locale.t("music.edit_song")}</button>{/if}
        <p class="mt-2 line-clamp-3 text-sm leading-relaxed text-neutral-400">{music.selectedResult?.params.style || locale.t("music.empty")}</p>
      </div>
    </div>

    {#if hasAudio}
      <div class="mt-4">
        <div class="relative h-14 rounded-sm focus-within:ring-2 focus-within:ring-indigo-400 focus-within:ring-offset-4 focus-within:ring-offset-neutral-900">
          {#if peaks.length > 0}
            <svg class="absolute inset-0 h-full w-full" viewBox="0 0 576 80" preserveAspectRatio="none" aria-hidden="true">
              {#each peaks as peak, index}
                <rect x={index * 6 + 1} y={40 - Math.max(2, peak * 36)} width="3" height={Math.max(4, peak * 72)} rx="0.5" class={index / peaks.length * 100 < progress ? "fill-indigo-400" : "fill-neutral-600"} />
              {/each}
            </svg>
          {:else}
            <div class="absolute inset-x-0 top-1/2 h-1 -translate-y-1/2 overflow-hidden rounded-full bg-neutral-800" aria-hidden="true"><div class="h-full rounded-full bg-indigo-400" style:width={`${progress}%`}></div></div>
          {/if}
          {#if duration > 0}<span class="pointer-events-none absolute bottom-2 top-2 w-0.5 rounded-full bg-indigo-300" style:left={`calc(${progress}% - ${progress / 100 * 2}px)`} aria-hidden="true"></span>{/if}
          <input type="range" class="absolute inset-0 h-full w-full cursor-pointer opacity-0 disabled:cursor-default" min="0" max={duration || 1} step="0.1" value={currentTime} disabled={!hasAudio || duration <= 0}
            aria-label={locale.t("music.seek")} aria-valuetext={locale.t("music.seek_time", { elapsed: audioTime(currentTime), duration: audioTime(duration) })}
            oninput={(event) => seek(Number(event.currentTarget.value))} />
        </div>
        <div class="mt-2 flex items-center justify-between gap-3 font-mono text-xs tabular-nums text-neutral-400">
          <span>{audioTime(currentTime)}</span>
          <span class="truncate font-sans text-[11px]" role="status">{music.loadingAudio ? locale.t("common.loading") : waiting && !playError ? locale.t("music.buffering") : ""}</span>
          <span>{audioTime(duration)}</span>
        </div>
      </div>

      <div class="mt-3 flex flex-wrap items-center justify-between gap-x-2 gap-y-1">
        <div class="flex items-center gap-1">
          <button type="button" class={`${controlClass} h-11 w-11 text-neutral-400`} disabled={duration <= 0} onclick={() => seek(currentTime - 10)} aria-label={locale.t("music.back_10")} title={locale.t("music.back_10")}>
            <svg class="h-5 w-5" viewBox="0 0 28 28" fill="none" aria-hidden="true"><path d="M7 9H2V4M3 9a11 11 0 1 1-1 9" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><text x="14" y="19" text-anchor="middle" fill="currentColor" font-size="10" font-family="sans-serif" font-weight="600">10</text></svg>
          </button>
          <button type="button" class="flex h-12 w-12 shrink-0 items-center justify-center rounded-full bg-indigo-500 text-[var(--theme-accent-foreground)] transition-colors hover:bg-indigo-400 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 disabled:opacity-30" disabled={!hasAudio || playPending || music.loadingAudio} onclick={togglePlayback} aria-label={locale.t(playing ? "music.pause" : "music.play")} title={locale.t(playing ? "music.pause" : "music.play")}>
            <svg class="h-5 w-5" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{#if playing}<rect x="6" y="4" width="4" height="16" rx="1"/><rect x="14" y="4" width="4" height="16" rx="1"/>{:else}<path d="M7 4.5a1 1 0 0 1 1.5-.86l12 7.5a1 1 0 0 1 0 1.72l-12 7.5A1 1 0 0 1 7 19.5z"/>{/if}</svg>
          </button>
          <button type="button" class={`${controlClass} h-11 w-11 text-neutral-400`} disabled={duration <= 0} onclick={() => seek(currentTime + 10)} aria-label={locale.t("music.forward_10")} title={locale.t("music.forward_10")}>
            <svg class="h-5 w-5" viewBox="0 0 28 28" fill="none" aria-hidden="true"><path d="M21 9h5V4M25 9a11 11 0 1 0 1 9" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><text x="14" y="19" text-anchor="middle" fill="currentColor" font-size="10" font-family="sans-serif" font-weight="600">10</text></svg>
          </button>
        </div>
        <div class="ml-auto flex items-center gap-1">
          <button type="button" class={`${controlClass} h-11 w-11 ${looping ? 'bg-indigo-500/10 text-indigo-300' : 'text-neutral-400'}`} onclick={() => music.toggleLoop()} aria-label={locale.t("music.repeat")} title={locale.t("music.repeat")} aria-pressed={looping}>
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m17 2 4 4-4 4M3 11V8a2 2 0 0 1 2-2h16M7 22l-4-4 4-4m14-1v3a2 2 0 0 1-2 2H3"/></svg>
          </button>
          <div class="flex items-center gap-1">
            <button type="button" class={`${controlClass} h-11 w-11 text-neutral-400`} onclick={() => music.toggleMute()} aria-label={locale.t(muted || volume === 0 ? "music.unmute" : "music.mute")}>
              <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m11 5-6 4H2v6h3l6 4z"/>{#if muted || volume === 0}<path d="m17 9 5 6m0-6-5 6"/>{:else}<path d="M15 8a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14"/>{/if}</svg>
            </button>
            <input type="range" min="0" max="1" step="0.01" value={muted ? 0 : volume} aria-label={locale.t("music.volume")} aria-valuetext={`${Math.round((muted ? 0 : volume) * 100)}%`} class="h-11 w-12 cursor-pointer accent-indigo-400 sm:w-20" oninput={(event) => music.setVolume(Number(event.currentTarget.value))} />
          </div>
        </div>
      </div>
    {/if}
    {#if playError}
      <div class="mt-3 flex items-center justify-between gap-3 border-l-2 border-red-400 pl-3 text-xs text-red-300" role="alert">
        <p>{locale.t("music.playback_error")}</p>
        <button type="button" class="touch-target shrink-0 rounded-sm px-3 hover:bg-neutral-800" onclick={() => music.retryPlayback()}>{locale.t("common.retry")}</button>
      </div>
    {/if}
  </div>

  {#if music.selectedResult}
    <div class="mt-4 flex flex-wrap items-center justify-between gap-3">
      <div class="min-w-0 flex-1 font-mono text-[10px] text-neutral-500">
        <p class="truncate">{locale.t("music.result_seed", { seed: music.selectedResult.seed })}</p>
      </div>
      <button type="button" class="touch-target inline-flex items-center justify-center gap-2 rounded-md bg-neutral-800 px-3 text-xs font-medium text-neutral-200 transition-colors hover:bg-neutral-700 hover:text-neutral-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 disabled:opacity-40" disabled={music.exporting || music.loadingAudio} onclick={() => music.downloadAudio()}>
        <svg class="h-4 w-4 text-indigo-300" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5"/></svg>
        {locale.t(music.exporting ? "music.saving_audio" : "music.download_audio")}
      </button>
      {#if music.exportStatus}<p class="w-full text-right text-xs text-indigo-300" role="status">{locale.t(`music.export_${music.exportStatus}`)}</p>{/if}
      {#if music.exportError}<p class="w-full whitespace-pre-wrap break-words text-sm text-red-300" role="alert">{music.exportError}</p>{/if}
      {#if !music.selectedResult.saved}<p class="w-full text-xs text-amber-300">{locale.t("music.not_saved")} <button type="button" class="touch-target px-2 underline" disabled={music.librarySaving} onclick={() => music.retryArchive()}>{locale.t("common.retry")}</button></p>{/if}
    </div>
  {/if}
  {#if music.selectedResult}
    {#key music.selectedResult.prompt_id}<MusicLyrics />{/key}
  {/if}
</section>
