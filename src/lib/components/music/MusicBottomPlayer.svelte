<script lang="ts">
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { audioTime } from "../../utils/musicAudio.js";
  let { onOpen }: { onOpen: () => void } = $props();
  const button = "touch-target inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-neutral-300 hover:bg-neutral-800 focus-visible:outline-2 focus-visible:outline-indigo-400 disabled:opacity-30";
</script>

{#if music.selectedResult}
  <footer class="shrink-0 border-t border-neutral-800 bg-neutral-950 px-3 py-2 text-neutral-200 md:px-6" aria-label={locale.t("music.player")}>
    <div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 md:grid-cols-[minmax(0,1fr)_minmax(14rem,1.3fr)_minmax(0,1fr)]">
      <button type="button" class="flex min-w-0 items-center gap-3 text-left focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={onOpen} title={locale.t("music.open_player")}>
        <span class="flex h-11 w-11 shrink-0 items-center justify-center rounded-md bg-linear-to-br from-indigo-400/30 to-neutral-800 text-xl text-indigo-300" aria-hidden="true">♫</span>
        <span class="min-w-0"><span class="block truncate text-sm font-medium">{music.title}</span><span class="block truncate text-xs text-neutral-500">{music.selectedResult.params.style}</span></span>
      </button>
      <div class="flex items-center justify-center gap-1">
        <button type="button" class={button} disabled={!music.hasPrevious || music.loadingAudio} onclick={() => music.previous()} aria-label={locale.t("music.previous")}><svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M5 4h2v16H5zm14 0v16L8 12z"/></svg></button>
        <button type="button" class={`${button} bg-indigo-500 text-[var(--theme-accent-foreground)] hover:bg-indigo-400`} disabled={music.playPending || music.loadingAudio} onclick={() => music.togglePlayback()} aria-label={locale.t(music.playing ? "music.pause" : "music.play")}>
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{#if music.playing}<path d="M6 4h4v16H6zm8 0h4v16h-4z"/>{:else}<path d="m7 4 14 8-14 8z"/>{/if}</svg>
        </button>
        <button type="button" class={button} disabled={!music.hasNext || music.loadingAudio} onclick={() => music.next()} aria-label={locale.t("music.next")}><svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M17 4h2v16h-2zM5 4l11 8-11 8z"/></svg></button>
      </div>
      <div class="hidden items-center justify-end gap-1 md:flex">
        <button type="button" class={`${button} ${music.looping ? 'text-indigo-300' : ''}`} aria-pressed={music.looping} onclick={() => music.toggleLoop()} aria-label={locale.t("music.repeat")}>↻</button>
        <button type="button" class={button} onclick={() => music.toggleMute()} aria-label={locale.t(music.muted || !music.volume ? "music.unmute" : "music.mute")}>
          <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="m11 5-6 4H2v6h3l6 4z"/>{#if music.muted || !music.volume}<path d="m17 9 5 6m0-6-5 6"/>{:else}<path d="M15 8a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14"/>{/if}</svg>
        </button>
        <input type="range" min="0" max="1" step="0.01" value={music.muted ? 0 : music.volume} aria-label={locale.t("music.volume")} class="h-11 w-20 accent-indigo-400" oninput={(event) => music.setVolume(Number(event.currentTarget.value))} />
        <button type="button" class={button} onclick={() => music.stopPlayback()} aria-label={locale.t("music.close_player")}>✕</button>
      </div>
      <div class="col-span-full flex items-center gap-2 font-mono text-[10px] tabular-nums text-neutral-400 md:col-start-2 md:row-start-2">
        <span>{audioTime(music.currentTime)}</span>
        <input type="range" class="h-6 min-w-0 flex-1 accent-indigo-400" min="0" max={music.duration || 1} step="0.1" value={music.currentTime} disabled={music.duration <= 0 || music.loadingAudio} aria-label={locale.t("music.seek")} aria-valuetext={locale.t("music.seek_time", { elapsed: audioTime(music.currentTime), duration: audioTime(music.duration) })} oninput={(event) => music.seek(Number(event.currentTarget.value))} />
        <span>{audioTime(music.duration)}</span>
      </div>
    </div>
    {#if music.loadingAudio || music.waiting}<p class="text-center text-xs text-neutral-400" role="status">{locale.t("music.buffering")}</p>{/if}
    {#if music.playError}<p class="flex items-center justify-center gap-2 text-xs text-red-300" role="alert">{locale.t("music.playback_error")} <button type="button" class="touch-target px-2 underline" onclick={() => music.retryPlayback()}>{locale.t("common.retry")}</button></p>{/if}
  </footer>
{/if}
