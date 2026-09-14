<script lang="ts">
  import { onDestroy } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { inspectMusicScore, scoreMidi, type ScoreVoiceSelection } from "../../utils/musicScore.js";
  import { playScore, scoreSvg } from "../../utils/musicPreview.js";
  import { downloadMusicFile } from "../../utils/musicProject.js";
  import { audioTime } from "../../utils/musicAudio.js";
  let { abc, ceiling, onpreview }: { abc: string; ceiling?: number; onpreview?: () => void } = $props();
  let voices = $state<ScoreVoiceSelection>("both"), chords = $state(false), playing = $state(false), pending = $state(false), seconds = $state(0), error = $state("");
  let stop: (() => void) | null = null, sequence = 0;
  let page = $state(0);
  const inspection = $derived(inspectMusicScore(abc));
  const pages = $derived(Math.max(1, Math.ceil((inspection.score?.voices.Vocal.bars.length ?? 0) / 32)));
  const currentPage = $derived(Math.min(page, pages - 1));
  const svg = $derived(inspection.score ? scoreSvg(inspection.score, `${locale.t("music.score_notation")} (${currentPage + 1}/${pages})`, currentPage) : "");
  const button = "touch-target rounded-md px-3 text-xs text-indigo-300 hover:bg-neutral-800 disabled:opacity-40";
  function halt() { sequence++; stop?.(); stop = null; playing = false; pending = false; }
  onDestroy(halt);
  $effect(() => { abc; voices; chords; halt(); });
  async function preview() {
    if (playing || pending) { halt(); return; }
    if (!inspection.score) return;
    error = ""; pending = true; const id = ++sequence;
    try {
      onpreview?.();
      if (music.playing) await music.togglePlayback();
      const cleanup = await playScore(inspection.score, voices, chords, time => { if (id === sequence) seconds = time; }, () => { if (id === sequence) { playing = false; stop = null; } });
      if (id !== sequence) { cleanup(); return; }
      stop = cleanup; playing = true;
    } catch (cause) { if (id === sequence) error = String(cause); }
    finally { if (id === sequence) pending = false; }
  }
  async function download(kind: "abc" | "mid" | "svg") {
    error = "";
    try {
      const score = inspection.score;
      if (!score) return;
      const blob = kind === "mid" ? new Blob([scoreMidi(score, voices, chords).slice().buffer as ArrayBuffer], { type: "audio/midi" }) : new Blob([kind === "svg" ? svg : abc], { type: kind === "svg" ? "image/svg+xml" : "text/plain" });
      await downloadMusicFile(blob, kind === "svg" && pages > 1 ? `score-page-${currentPage + 1}.svg` : `score.${kind}`);
    } catch (cause) { error = String(cause); }
  }
</script>

{#if abc.trim()}
  <div class="space-y-2">
    {#if inspection.score}
      {@const score = inspection.score}
      <p class="text-xs text-neutral-400">{locale.t("music.score_stats", { bpm: score.bpm, key: score.key, bars: score.voices.Vocal.bars.length, duration: audioTime(score.seconds) })}</p>
      {#if ceiling && score.seconds > ceiling}<p class="text-xs text-amber-300">{locale.t("music.score_ceiling")}</p>{/if}
      <details>
        <summary class="touch-target cursor-pointer content-center text-sm text-indigo-300">{locale.t("music.score_preview")}</summary>
        <div class="space-y-3 pb-3">
          <div class="flex flex-wrap items-center gap-2">
            <select class="min-h-11 rounded-md bg-neutral-800 px-2 text-xs text-neutral-200" bind:value={voices} aria-label={locale.t("music.cover_parts")}><option value="both">{locale.t("music.score_both")}</option><option value="Vocal">{locale.t("music.score_vocal")}</option><option value="Ins">{locale.t("music.score_instrumental")}</option></select>
            <label class="touch-target flex items-center gap-2 text-xs text-neutral-400"><input type="checkbox" bind:checked={chords} />{locale.t("music.score_chords")}</label>
            <button type="button" class={button} onclick={preview}>{locale.t(playing || pending ? "music.score_stop" : "music.score_listen")}</button>
            {#if playing}<span class="font-mono text-xs text-neutral-400">{audioTime(seconds)}</span>{/if}
          </div>
          <p class="text-xs text-neutral-500">{locale.t("music.score_preview_help")}</p>
          <div class="max-h-80 overflow-auto rounded-md bg-white"><img class="min-w-[640px] w-full" src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`} alt={locale.t("music.score_notation")} /></div>
          {#if pages > 1}<div class="flex items-center gap-2"><button type="button" class={button} disabled={currentPage === 0} onclick={() => page = currentPage - 1} aria-label={locale.t("music.score_previous_page")}>←</button><span class="text-xs text-neutral-400">{currentPage + 1}/{pages}</span><button type="button" class={button} disabled={currentPage + 1 >= pages} onclick={() => page = currentPage + 1} aria-label={locale.t("music.score_next_page")}>→</button></div>{/if}
        </div>
      </details>
      <div class="flex flex-wrap gap-1">{#each (["abc", "mid", "svg"] as const) as kind}<button type="button" class={button} onclick={() => download(kind)}>{locale.t(`music.export_${kind}`)}</button>{/each}</div>
    {:else}<p class="whitespace-pre-wrap text-xs text-amber-300" role="status">{locale.t("music.score_invalid")} {inspection.error}</p>{/if}
    {#if error}<p class="text-xs text-red-300" role="alert">{error}</p>{/if}
  </div>
{/if}
