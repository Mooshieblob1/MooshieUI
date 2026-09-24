<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { getAuthUser } from "../../utils/ipc.js";
  import { searchMusicReference, getMusicReference } from "../../utils/api.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import type { ReferenceSong, MusicReferenceContext, MusicReferenceDraft } from "../../types/music.js";

  let enabled = $state(false);
  let title = $state("");
  let artist = $state("");
  let version = $state("");
  let matches = $state<ReferenceSong[]>([]);
  let selected = $state("");
  let reference = $state<MusicReferenceContext | null>(null);
  let draft = $state<MusicReferenceDraft | null>(null);
  let phase = $state<"search" | "describe" | null>(null);
  let error = $state("");
  let searched = $state(false);
  let baseline = "";
  let undo = $state<{ before: string; after: string; owner: string | null; song: string | undefined } | null>(null);
  let sequence = 0;
  let active = true;
  let owner: string | null = null;
  const locked = $derived(music.busy || promptAssistant.isGenerating);
  const inputClass = "w-full min-w-0 rounded-md bg-neutral-900 px-3 py-2.5 text-sm text-neutral-200 focus:outline-2 focus:outline-indigo-400 disabled:opacity-40";
  const buttonClass = "touch-target rounded-md px-3 text-xs text-indigo-300 hover:bg-neutral-900 disabled:opacity-40";
  const snapshot = () => JSON.stringify([music.params.style, music.params.lyrics, music.params.max_duration, music.params.abc, music.params.planning, music.params.cover, music.selectedResult?.prompt_id]);
  onDestroy(() => { active = false; sequence++; });
  let previousAccount = getAuthUser();
  let previousSong = music.selectedResult?.prompt_id;
  $effect(() => {
    // loadSettings replaces params when switching accounts, including an empty library.
    const params = music.params;
    const song = music.selectedResult?.prompt_id;
    const account = getAuthUser();
    untrack(() => {
      if (account !== previousAccount || song !== previousSong) {
        reset(); undo = null;
        if (account !== previousAccount) { enabled = false; title = ""; artist = ""; version = ""; }
      }
      previousAccount = account; previousSong = song;
    });
    void params;
  });

  function reset() {
    sequence++; matches = []; selected = ""; reference = null; draft = null;
    phase = null; error = ""; searched = false;
  }
  async function search() {
    if (!enabled || phase || !title.trim()) return;
    reset();
    const request = ++sequence; const account = getAuthUser();
    phase = "search";
    try {
      const result = await searchMusicReference([title, artist, version].map(s => s.trim()).filter(Boolean).join(" "));
      if (!active || request !== sequence || account !== getAuthUser()) return;
      matches = result; searched = true;
      if (result.length === 1) selected = String(result[0].id);
    } catch (cause) { if (active && request === sequence && account === getAuthUser()) error = String(cause); }
    finally { if (request === sequence) phase = null; }
  }
  async function describe() {
    const song = matches.find(s => String(s.id) === selected);
    if (!enabled || !song || phase || locked) return;
    const request = ++sequence; owner = getAuthUser();
    const account = owner; const before = snapshot();
    const current = () => active && enabled && request === sequence && account === getAuthUser() && before === snapshot();
    draft = null; reference = null; error = ""; phase = "describe";
    const context = { style: music.params.style, lyrics: music.params.lyrics, maxDuration: music.params.max_duration, abc: music.params.abc, planning: music.params.planning, cover: music.params.cover === true, language: locale.intlTag };
    try {
      await promptAssistant.refreshStatus();
      if (!current()) return;
      if (!promptAssistant.isAvailable) { error = locale.t("music.assistant_setup"); return; }
      const found = await getMusicReference(song.id);
      if (!current()) return;
      reference = found;
      const result = await promptAssistant.styleFromReference(found, context, current);
      if (!current()) return;
      draft = result; baseline = before;
    } catch (cause) {
      if (active && request === sequence && account === getAuthUser() && before === snapshot()) {
        error = String(cause).includes("music_reference_unknown") ? locale.t("music.reference_unknown")
          : String(cause).includes("invalid_music_reference") ? locale.t("music.assistant_invalid_result") : mapLlmError(String(cause));
      }
    } finally {
      if (active && request === sequence) {
        if (account === getAuthUser() && before !== snapshot()) error = locale.t("music.assistant_changed");
        phase = null;
      }
    }
  }
  function apply() {
    if (!draft || locked || owner !== getAuthUser()) return;
    if (baseline !== snapshot()) { error = locale.t("music.assistant_changed"); return; }
    undo = { before: music.params.style, after: draft.style, owner, song: music.selectedResult?.prompt_id };
    music.params.style = draft.style; music.saveSettings(); draft = null; error = "";
  }
  function undoStyle() {
    if (!undo || locked || undo.owner !== getAuthUser() || undo.song !== music.selectedResult?.prompt_id || music.params.style !== undo.after) return;
    music.params.style = undo.before; undo = null; music.saveSettings();
  }
</script>

<div class="mt-2 space-y-2">
  <label class="touch-target flex items-center gap-2 text-xs text-neutral-400">
    <input type="checkbox" bind:checked={enabled} onchange={reset} aria-controls="music-reference-panel" aria-expanded={enabled} />
    {locale.t("music.reference_enable")}
  </label>
  {#if enabled}
    <div id="music-reference-panel" class="space-y-3 rounded-md border border-neutral-800 p-3">
      <p class="text-xs leading-relaxed text-neutral-500">{locale.t("music.reference_help")}</p>
      <div class="grid gap-2 sm:grid-cols-2">
        <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("music.song_title")}</span><input class={inputClass} maxlength="180" bind:value={title} oninput={reset} onkeydown={event => { if (event.key === "Enter") { event.preventDefault(); void search(); } }} /></label>
        <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("music.reference_artist")}</span><input class={inputClass} maxlength="180" bind:value={artist} oninput={reset} onkeydown={event => { if (event.key === "Enter") { event.preventDefault(); void search(); } }} /></label>
      </div>
      <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.reference_version")}</span><input class={inputClass} maxlength="120" bind:value={version} oninput={reset} onkeydown={event => { if (event.key === "Enter") { event.preventDefault(); void search(); } }} /></label>
      <button type="button" class={buttonClass} disabled={!!phase || !title.trim()} onclick={search}>{locale.t(phase === "search" ? "music.reference_searching" : "music.reference_search")}</button>
      {#if searched && !matches.length}<p class="text-xs text-amber-300" role="status">{locale.t("music.reference_empty")}</p>{/if}
      {#if matches.length}
        <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.reference_recording")}</span><select class={inputClass} bind:value={selected} onchange={() => { sequence++; phase = null; draft = null; reference = null; error = ""; }}><option value="">{locale.t("music.reference_choose")}</option>{#each matches as song}<option value={String(song.id)}>{song.title} · {song.artist} · {song.album} ({song.year})</option>{/each}</select></label>
        <button type="button" class={buttonClass} disabled={!selected || !!phase || locked} onclick={describe}>{locale.t(phase === "describe" ? "music.reference_describing" : "music.reference_describe")}</button>
      {/if}
      {#if reference}
        <p class="text-xs text-neutral-300">{reference.song.title} · {reference.song.artist}<span class="block text-neutral-500">{reference.song.album} · {reference.song.year} · {reference.song.genre}</span></p>
        <div class="flex flex-wrap gap-3 text-xs text-indigo-300" aria-label={locale.t("music.reference_sources")}>{#each reference.sources as source}<a href={source.url} target="_blank" rel="noreferrer" class="underline">{source.title}</a>{/each}</div>
        {#if reference.sources.length === 1}<p class="text-xs text-neutral-500">{locale.t("music.reference_catalog_only")}</p>{/if}
      {/if}
      {#if draft}
        <p class="text-xs text-amber-300">{locale.t("music.reference_estimate_help")}</p>
        <p class="whitespace-pre-wrap text-sm leading-relaxed text-neutral-200">{draft.style}</p>
        <details class="text-xs text-neutral-400"><summary class="touch-target cursor-pointer content-center">{locale.t("music.reference_estimates")}</summary><ul class="list-disc space-y-1 pl-5">{#each draft.estimates as detail}<li>{detail}</li>{/each}</ul></details>
        <button type="button" class={buttonClass} disabled={locked} onclick={apply}>{locale.t("music.reference_apply")}</button>
      {/if}
      {#if error}<p class="whitespace-pre-wrap text-xs text-red-300" role="alert">{error}</p>{/if}
    </div>
  {/if}
  {#if undo && music.params.style === undo.after}<button type="button" class={buttonClass} disabled={locked} onclick={undoStyle}>{locale.t("music.assistant_undo_style")}</button>{/if}
</div>
