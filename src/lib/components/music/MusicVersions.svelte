<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { getAuthUser } from "../../utils/ipc.js";
  import { downloadMusicFile, musicProject } from "../../utils/musicProject.js";
  import { compareMusicScores, inspectMusicScore } from "../../utils/musicScore.js";
  import { stopScorePreview } from "../../utils/musicPreview.js";
  import MusicNotation from "./MusicNotation.svelte";
  import type { MusicResult } from "../../types/music.js";
  let dialog: HTMLDialogElement | undefined = $state();
  let first = $state(""), second = $state(""), urls = $state<string[]>([]), loading = $state(false), exporting = $state(false), error = $state(""), message = $state("");
  let players: (HTMLAudioElement | undefined)[] = [], sequence = 0;
  let sync = $state(true), loop = $state(false), from = $state(0), to = $state(30);
  const songs = $derived([first, second].map(id => music.results.find(song => song.prompt_id === id)));
  const comparison = $derived.by(() => {
    const a = inspectMusicScore(songs[0]?.abc ?? "").score, b = inspectMusicScore(songs[1]?.abc ?? "").score;
    return a && b ? compareMusicScores(a,b) : null;
  });
  const button = "touch-target rounded-md px-3 text-xs text-indigo-300 hover:bg-neutral-800 disabled:opacity-40";
  function release() { sequence++; stopScorePreview(); players.forEach(p => p?.pause()); urls.forEach(URL.revokeObjectURL); urls = []; loading = false; }
  onDestroy(release);
  $effect(() => {
    const open = music.comparing, element = dialog;
    if (!element) return;
    untrack(() => {
      if (open) {
        first = music.compareIds[0] ?? music.selectedResult?.prompt_id ?? music.results[0]?.prompt_id ?? "";
        const base = music.results.find(r => r.prompt_id === first);
        second = music.compareIds[1] ?? base?.params.lineage?.parent_id ?? music.results.find(r => r.prompt_id !== first)?.prompt_id ?? "";
        if (!element.open) element.showModal();
      } else { element.close(); release(); }
    });
  });
  $effect(() => {
    const a = first, b = second, open = music.comparing;
    if (open) untrack(() => { void load(a,b); });
  });
  function close() { music.comparing = false; dialog?.close(); release(); }
  async function load(a: string, b: string) {
    release(); error = ""; message = ""; loading = true;
    const id = sequence, owner = getAuthUser();
    const selected = [a,b].map(key => music.results.find(r => r.prompt_id === key));
    try {
      const blobs = await Promise.all(selected.map(song => song ? music.archive(song) : Promise.resolve(null)));
      if (id !== sequence || owner !== getAuthUser()) return;
      urls = blobs.map(blob => blob ? URL.createObjectURL(blob) : "");
    } catch (cause) { if (id === sequence) error = String(cause); }
    finally { if (id === sequence) loading = false; }
  }
  function playing(index: number) {
    stopScorePreview();
    const player = players[index], other = players[1-index];
    if (!player) return;
    if (music.playing) void music.togglePlayback();
    if (other && !other.paused) {
      if (sync) player.currentTime = Math.min(other.currentTime, Number.isFinite(player.duration) ? player.duration : other.currentTime);
      other.pause();
    }
    if (loop && (player.currentTime < from || player.currentTime >= to)) player.currentTime = from;
  }
  function time(index: number) {
    const player = players[index];
    if (player && !player.paused && loop && to > from && player.currentTime >= to) player.currentTime = from;
  }
  async function exportProject(all: boolean) {
    if (exporting) return;
    exporting = true; error = ""; message = "";
    const owner = getAuthUser(), id = sequence;
    const selected = songs.filter((s): s is MusicResult => !!s);
    const projectId = selected[0]?.params.lineage?.project_id ?? selected[0]?.prompt_id;
    const versions = all ? music.results.filter(s => (s.params.lineage?.project_id ?? s.prompt_id) === projectId) : [...new Map(selected.map(s => [s.prompt_id,s])).values()];
    try {
      const blob = await musicProject(versions, song => music.archive(song));
      if (owner !== getAuthUser() || id !== sequence) return;
      const status = await downloadMusicFile(blob, "music-project.zip");
      if (status !== "cancelled") message = locale.t(`music.${status}`);
    } catch (cause) { if (owner === getAuthUser() && id === sequence) error = String(cause).includes("music_export_limit") ? locale.t("music.project_limit") : String(cause); }
    finally { exporting = false; }
  }
</script>

<dialog bind:this={dialog} class="m-auto max-h-[90vh] w-[calc(100%-2rem)] max-w-5xl overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-neutral-200 shadow-2xl backdrop:bg-black/70" aria-labelledby="music-versions-title" onclose={close}>
  <div class="space-y-4">
    <div class="flex items-center justify-between gap-3"><h2 id="music-versions-title" class="text-lg font-semibold">{locale.t("music.versions")}</h2><button type="button" class={button} onclick={close}>{locale.t("common.close")}</button></div>
    <p class="text-xs text-neutral-400">{locale.t("music.compare_help")}</p>
    <div class="grid gap-5 md:grid-cols-2">
      {#each songs as song, index}
        <section class="min-w-0 space-y-3" aria-label={index === 0 ? "A" : "B"}>
          <label class="flex items-center gap-2 text-sm"><span>{index === 0 ? "A" : "B"}</span><select class="min-h-11 min-w-0 flex-1 rounded-md bg-neutral-800 px-3 text-sm" value={index === 0 ? first : second} onchange={event => { if (index === 0) first = event.currentTarget.value; else second = event.currentTarget.value; }}><option value="">{locale.t("music.choose_version")}</option>{#each music.results as result}<option value={result.prompt_id}>{music.resultTitle(result)} · {result.seed}</option>{/each}</select></label>
          {#if song}
            {#if urls[index]}<audio class="w-full" controls preload="metadata" src={urls[index]} bind:this={players[index]} onplay={() => playing(index)} ontimeupdate={() => time(index)} aria-label={`${index === 0 ? "A" : "B"}: ${music.resultTitle(song)}`}></audio>{/if}
            <p class="text-xs text-neutral-400">{song.params.style}</p>
            <p class="break-all font-mono text-[11px] text-neutral-500">{locale.t("music.seed")}: {song.seed}</p>
            {#if song.params.lineage?.parent_id}<p class="text-xs text-neutral-400">{locale.t("music.version_parent", { title: music.results.find(r => r.prompt_id === song.params.lineage?.parent_id)?.title || song.params.lineage.parent_id })}</p>{/if}
            {#if song.params.lineage?.change}<p class="text-xs text-neutral-300">{song.params.lineage.change}</p>{/if}
            {#if song.params.lineage?.comparison?.match}<p class="text-xs text-emerald-300">{locale.t("music.edit_checks_passed")}</p>{/if}
            {#if song.metadata?.abc_truncated}<p class="text-xs text-amber-300">{locale.t("music.truncated_score")}</p>{/if}
            {#if song.metadata?.semantic_truncated}<p class="text-xs text-amber-300">{locale.t("music.truncated_audio")}</p>{/if}
            <MusicNotation abc={song.abc} onpreview={() => players.forEach(player => player?.pause())} />
            <div class="flex flex-wrap gap-1"><button type="button" class={button} disabled={music.busy || loading} onclick={() => { music.useVersion(song); close(); }}>{locale.t("music.version_use")}</button><button type="button" class={button} disabled={music.busy || loading || !song.abc.trim()} onclick={() => { close(); music.editingComposition = song.prompt_id; }}>{locale.t("music.edit_composition")}</button></div>
          {/if}
        </section>
      {/each}
    </div>
    <div class="flex flex-wrap items-center gap-3 text-xs text-neutral-400">
      <label class="touch-target flex items-center gap-2"><input type="checkbox" bind:checked={sync} />{locale.t("music.compare_sync")}</label>
      <label class="touch-target flex items-center gap-2"><input type="checkbox" bind:checked={loop} disabled={!Number.isFinite(from) || !Number.isFinite(to) || from < 0 || to <= from} />{locale.t("music.compare_loop")}</label>
      <label class="flex items-center gap-2">{locale.t("music.compare_from")}<input class="min-h-11 w-20 rounded-md bg-neutral-800 px-2" type="number" min="0" step="0.1" bind:value={from} oninput={() => loop = false} /></label>
      <label class="flex items-center gap-2">{locale.t("music.compare_to")}<input class="min-h-11 w-20 rounded-md bg-neutral-800 px-2" type="number" min="0" step="0.1" bind:value={to} oninput={() => loop = false} /></label>
    </div>
    {#if comparison}<p class={`text-xs ${comparison.match ? 'text-emerald-300' : 'text-amber-300'}`}>{locale.t(comparison.match ? "music.compare_score_match" : "music.compare_score_changed")} {comparison.differences.join(" ")}</p>{/if}
    <div class="flex flex-wrap gap-2"><button type="button" class={button} disabled={loading || exporting || !songs[0]} onclick={() => exportProject(false)}>{locale.t("music.project_export")}</button><button type="button" class={button} disabled={loading || exporting || !songs[0]} onclick={() => exportProject(true)}>{locale.t("music.project_export_all")}</button></div>
    <p class="text-xs text-neutral-500">{locale.t("music.project_help")}</p>
    {#if loading || exporting}<p class="text-xs text-neutral-400" role="status">{locale.t("common.loading")}</p>{/if}
    {#if message}<p class="text-xs text-emerald-300" role="status">{message}</p>{/if}
    {#if error}<p class="text-xs text-red-300" role="alert">{error}</p>{/if}
  </div>
</dialog>
