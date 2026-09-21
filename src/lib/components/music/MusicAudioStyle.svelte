<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { getAuthUser } from "../../utils/ipc.js";
  import { getMusicAudioStyleCapabilities, getMusicAudioStyle } from "../../utils/api.js";
  import { describeAudio, audioRangeValid, type AudioStyleCapabilities, type AudioStyleProfile, type AudioStyleTarget } from "../../utils/musicAudioStyle.js";
  import { musicId } from "../../utils/musicSettings.js";
  import { audioTime } from "../../utils/musicAudio.js";
  import { stopScorePreview } from "../../utils/musicPreview.js";

  let enabled = $state(false);
  let source = $state("cover");
  let file = $state.raw<File | null>(null);
  let capabilities = $state<AudioStyleCapabilities | null>(null);
  let checking = $state(false);
  let analyzing = $state(false);
  let profile = $state<AudioStyleProfile | null>(null);
  let draft = $state("");
  let error = $state("");
  let uploadError = $state(false);
  let section = $state(false), from = $state(0), to = $state(30);
  let previewUrl = $state(""), sourceDuration = $state(0), loadingPreview = $state(false);
  let player: HTMLAudioElement | undefined = $state();
  let previewBlob: Blob | null = null, previewSequence = 0, auditioning = false;
  let profileTarget: AudioStyleTarget | null = null, profileModel = "";
  let profileName = $state(""), saving = $state(false), saved = $state(false);
  let job: string | null = null;
  let sequence = 0, checkSequence = 0;
  let active = true;
  let baseline = "";
  let owner: string | null = null;
  let cache: { source: File | string; target: string; backend: string; profile: AudioStyleProfile } | null = null;
  let undo = $state<{ before: string; after: string; owner: string | null; song: string | undefined } | null>(null);
  const locked = $derived(music.busy || promptAssistant.isGenerating);
  const chosen = $derived(source === "cover" ? music.reviewSource : source === "upload" ? file : music.results.find(song => song.prompt_id === source));
  const sourceIdentity = $derived(chosen instanceof File ? chosen : chosen?.prompt_id);
  const inputClass = "w-full min-w-0 rounded-md bg-neutral-900 px-3 py-2.5 text-sm text-neutral-200 focus:outline-2 focus:outline-indigo-400 disabled:opacity-40";
  const buttonClass = "touch-target rounded-md px-3 text-xs text-indigo-300 hover:bg-neutral-900 disabled:opacity-40";
  const rangeValid = $derived(!section || audioRangeValid(from, to, sourceDuration || 86400));
  const snapshot = () => JSON.stringify([music.params.style, music.params.lyrics, music.params.max_duration, music.params.abc, music.params.planning, music.params.cover, music.selectedResult?.prompt_id, locale.intlTag, section ? [from, to] : null]);
  function reset() {
    sequence++;
    if (job) void getMusicAudioStyle(job, true).catch(() => {});
    job = null; analyzing = false; profile = null; draft = ""; error = ""; saved = false;
  }
  function releasePreview() {
    previewSequence++; player?.pause(); auditioning = false;
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    previewUrl = ""; previewBlob = null; sourceDuration = 0; loadingPreview = false;
  }
  onDestroy(() => { active = false; checkSequence++; reset(); releasePreview(); cache = null; file = null; });
  let previousAccount = getAuthUser();
  let previousSong = music.selectedResult?.prompt_id;
  $effect(() => {
    const params = music.params;
    const song = music.selectedResult?.prompt_id;
    const account = getAuthUser();
    untrack(() => {
      if (account !== previousAccount || song !== previousSong) {
        reset(); releasePreview(); undo = null; cache = null; file = null; uploadError = false; profileName = "";
        if (account !== previousAccount) { enabled = false; capabilities = null; checkSequence++; checking = false; }
      }
      previousAccount = account; previousSong = song;
    });
    void params;
  });
  $effect(() => {
    // Read only source identities: archiving a result replaces its metadata object.
    const identity = sourceIdentity, mode = source;
    untrack(() => {
      reset(); releasePreview(); cache = null; section = false; from = 0; to = 30; profileName = "";
      if (mode !== "upload") { file = null; uploadError = false; }
      if (identity instanceof File && enabled) { previewBlob = identity; previewUrl = URL.createObjectURL(identity); }
    });
    void identity;
  });
  async function refresh() {
    const request = ++checkSequence, account = getAuthUser();
    checking = true; error = "";
    try {
      const result = await getMusicAudioStyleCapabilities();
      if (active && request === checkSequence && account === getAuthUser()) capabilities = result;
    } catch (cause) { if (active && request === checkSequence) error = String(cause); }
    finally { if (active && request === checkSequence) checking = false; }
  }
  function toggle() {
    reset();
    if (enabled) void refresh();
    else { checkSequence++; checking = false; cache = null; file = null; uploadError = false; releasePreview(); }
  }
  async function loadPreview() {
    const selected = chosen, identity = sourceIdentity, account = getAuthUser(), token = ++previewSequence;
    if (!selected || loadingPreview) return;
    loadingPreview = true; error = "";
    try {
      const blob = selected instanceof File ? selected : await music.archive(selected);
      if (!active || !enabled || token !== previewSequence || identity !== sourceIdentity || account !== getAuthUser()) return;
      previewBlob = blob; previewUrl = URL.createObjectURL(blob);
    } catch (cause) { if (token === previewSequence && account === getAuthUser()) error = String(cause); }
    finally { if (token === previewSequence) loadingPreview = false; }
  }
  function metadata() {
    if (!player || !Number.isFinite(player.duration) || player.duration <= 0) return;
    sourceDuration = player.duration;
    to = Math.min(to, Math.floor(sourceDuration * 100) / 100);
  }
  async function playSection() {
    if (!player || !rangeValid) return;
    auditioning = section; player.currentTime = section ? from : 0;
    try { await player.play(); } catch (cause) { error = String(cause); }
  }
  async function saveProfile() {
    if (!profile || !profileTarget || !profileName.trim() || !draft.trim() || saving || owner !== getAuthUser()) return;
    const request = sequence, account = owner;
    saving = true; error = "";
    try {
      const ok = await music.saveStyleProfile({ version: 1, id: musicId(), name: profileName.trim(), createdAt: Date.now(),
        profile, style: draft, target: profileTarget, model: profileModel });
      if (active && request === sequence && account === getAuthUser()) {
        if (!ok) error = music.libraryError || locale.t("music.library_error"); else saved = true;
      }
    } catch (cause) { if (request === sequence && account === getAuthUser()) error = String(cause); }
    finally { saving = false; }
  }
  function upload(event: Event) {
    reset(); cache = null; uploadError = false;
    const input = event.currentTarget as HTMLInputElement;
    const next = input.files?.[0] ?? null;
    input.value = "";
    if (next && (!next.size || next.size > 64 * 1024 * 1024)) { file = null; uploadError = true; return; }
    file = next;
  }
  // The enclosing generation form can explicitly request a style for an empty
  // field. Manual analysis still produces a reviewable preview only.
  export function isEnabled() { return enabled; }
  export function isAnalyzing() { return analyzing || checking; }
  export async function prepareForGeneration(isCurrent: () => boolean): Promise<boolean> {
    if (!enabled || music.params.style.trim() || locked) return false;
    if (!chosen) { error = locale.t("music.audio_style_no_cover"); return false; }
    if (!capabilities?.available) { error = capabilities?.error || locale.t("music.assistant_setup"); return false; }
    if (!await analyze()) return false;
    if (!active || !enabled || !profile || !isCurrent()) {
      if (active && enabled) error = locale.t("music.assistant_changed");
      return false;
    }
    apply();
    return !!music.params.style.trim() && music.params.style === draft.trim();
  }
  async function analyze(): Promise<boolean> {
    const selected = chosen, selectedBackend = capabilities?.backend_id;
    if (!enabled || !selected || !selectedBackend || !capabilities?.available || analyzing || locked || !rangeValid) return false;
    const request = ++sequence, account = getAuthUser(), before = snapshot();
    const identity = selected instanceof File ? selected : selected.prompt_id;
    const target: AudioStyleTarget = { instrumental: !music.params.lyrics.trim(), max_duration: music.params.max_duration, language: locale.intlTag,
      ...(section ? { source_start: from, source_end: to } : {}) };
    const key = JSON.stringify(target);
    const current = () => active && enabled && request === sequence && account === getAuthUser() && identity === sourceIdentity && before === snapshot();
    analyzing = true; error = ""; profile = null; draft = "";
    try {
      // Never silently switch the destination displayed when Analyze was clicked.
      const fresh = await getMusicAudioStyleCapabilities();
      if (!current()) return false;
      if (!fresh.available || fresh.backend_id !== selectedBackend) {
        capabilities = fresh; error = fresh.error || locale.t("music.audio_style_provider_changed"); return false;
      }
      let result: AudioStyleProfile | null;
      if (cache?.source === identity && cache.target === key && cache.backend === selectedBackend) result = cache.profile;
      else {
        const blob = previewBlob ?? (selected instanceof File ? selected : await music.archive(selected));
        if (!current()) return false;
        result = await describeAudio(blob, target, selectedBackend, current, id => {
          if (current()) job = id;
          else void getMusicAudioStyle(id, true).catch(() => {});
        });
      }
      if (!current() || !result) return false;
      cache = { source: identity, target: key, backend: selectedBackend, profile: result };
      profile = result; draft = result.style; baseline = before; owner = account;
      profileTarget = target; profileModel = fresh.model || ""; saved = false;
      return true;
    } catch (cause) {
      if (current()) error = String(cause).includes("music_audio_size") ? locale.t("music.audio_style_size")
        : String(cause).includes("invalid_audio_style") ? locale.t("music.assistant_invalid_result") : String(cause);
      return false;
    } finally {
      if (active && request === sequence) {
        if (account === getAuthUser() && before !== snapshot()) error = locale.t("music.assistant_changed");
        analyzing = false; job = null;
      }
    }
  }
  function apply() {
    if (!profile || !draft.trim() || locked || owner !== getAuthUser()) return;
    if (baseline !== snapshot()) { error = locale.t("music.assistant_changed"); return; }
    undo = { before: music.params.style, after: draft.trim(), owner, song: music.selectedResult?.prompt_id };
    music.params.style = undo.after; music.saveSettings(); baseline = snapshot(); error = "";
  }
  function undoStyle() {
    if (!undo || locked || undo.owner !== getAuthUser() || undo.song !== music.selectedResult?.prompt_id || music.params.style !== undo.after) return;
    music.params.style = undo.before; music.saveSettings(); undo = null;
  }
</script>

<div class="mt-2 space-y-2">
  <label class="touch-target flex items-center gap-2 text-xs text-neutral-400">
    <input type="checkbox" bind:checked={enabled} onchange={toggle} aria-controls="music-audio-style-panel" aria-expanded={enabled} />
    {locale.t("music.audio_style_enable")}
  </label>
  {#if enabled}
    <div id="music-audio-style-panel" class="space-y-3 rounded-md border border-neutral-800 p-3">
      <p class="text-xs leading-relaxed text-neutral-500">{locale.t("music.audio_style_help")}</p>
      <p class="text-xs leading-relaxed text-neutral-400">{locale.t("music.audio_style_generate_help")}</p>
      <label class="block space-y-1 text-xs text-neutral-400">
        <span>{locale.t("music.reference_recording")}</span>
        <select class={inputClass} bind:value={source}>
          <option value="cover">{locale.t("music.audio_style_cover")}</option>
          <option value="upload">{locale.t("music.audio_style_upload")}</option>
          {#each music.results as song}<option value={song.prompt_id}>{song.title || song.params.title || song.filename}</option>{/each}
        </select>
      </label>
      {#if source === "upload"}
        <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.reference_recording")}</span>
          <input type="file" accept="audio/*,.flac,.aiff,.aif,.m4a,.ogg" class={inputClass} onchange={upload} />
        </label>
        {#if file}<div class="flex flex-wrap items-center gap-2 text-xs text-neutral-400"><span class="break-all">{file.name}</span><button type="button" class={buttonClass} onclick={() => { file = null; }}>{locale.t("music.link_clear")}</button></div>{/if}
      {:else if source === "cover"}
        <p class="break-all text-xs text-neutral-400">{music.reviewSource?.name || locale.t("music.audio_style_no_cover")}</p>
      {/if}
      {#if chosen && !previewUrl}<button type="button" class={buttonClass} disabled={loadingPreview} onclick={loadPreview}>{locale.t(loadingPreview ? "common.loading" : "music.audio_preview_load")}</button>{/if}
      {#if previewUrl}
        <audio class="w-full" controls preload="metadata" src={previewUrl} bind:this={player} onloadedmetadata={metadata}
          onplay={() => { stopScorePreview(); if (music.playing) void music.togglePlayback(); }}
          ontimeupdate={() => { if (player && auditioning && player.currentTime >= to) { player.pause(); auditioning = false; } }}
          aria-label={locale.t("music.cover_source_preview")}></audio>
      {/if}
      <label class="touch-target flex items-center gap-2 text-xs text-neutral-400"><input type="checkbox" bind:checked={section} onchange={reset} />{locale.t("music.audio_section")}</label>
      {#if section}
        <div class="grid grid-cols-2 gap-2">
          <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("music.compare_from")}</span><input class={inputClass} type="number" min="0" max={sourceDuration || 86400} step="0.1" bind:value={from} oninput={reset} /></label>
          <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("music.compare_to")}</span><input class={inputClass} type="number" min="0" max={sourceDuration || 86400} step="0.1" bind:value={to} oninput={reset} /></label>
        </div>
        <div class="flex flex-wrap gap-1">
          <button type="button" class={buttonClass} disabled={!sourceDuration} onclick={() => { from = Math.floor((player?.currentTime || 0) * 10) / 10; reset(); }}>{locale.t("music.audio_start_here")}</button>
          <button type="button" class={buttonClass} disabled={!sourceDuration} onclick={() => { to = Math.floor((player?.currentTime || 0) * 10) / 10; reset(); }}>{locale.t("music.audio_end_here")}</button>
          <button type="button" class={buttonClass} disabled={!sourceDuration || !rangeValid} onclick={playSection}>{locale.t("music.audio_play_section")}</button>
        </div>
        {#if !rangeValid}<p class="text-xs text-amber-300">{locale.t("music.audio_range_error")}</p>{/if}
      {/if}
      {#if capabilities?.model}
        <p class="break-words text-xs text-neutral-400">{locale.t("music.audio_style_destination", { model: capabilities.model, destination: capabilities.destination || "" })}</p>
      {/if}
      {#if capabilities?.custom}<p class="text-xs text-neutral-500">{locale.t("music.audio_style_custom")}</p>{/if}
      {#if capabilities?.error}<p class="text-xs text-amber-300" role="status">{capabilities.error}</p>{/if}
      <div class="flex flex-wrap gap-2">
        <button type="button" class={buttonClass} disabled={checking || analyzing} onclick={refresh}>{locale.t("music.audio_style_refresh")}</button>
        <button type="button" class={buttonClass} disabled={!chosen || checking || analyzing || locked || !rangeValid || !capabilities?.available} onclick={analyze}>{locale.t(analyzing ? "music.audio_style_analyzing" : "music.audio_style_analyze")}</button>
        {#if analyzing}<button type="button" class={buttonClass} onclick={reset}>{locale.t("common.cancel")}</button>{/if}
      </div>
      {#if profile}
        <p class="text-xs text-neutral-400">{locale.t(profile.excerpt ? "music.audio_analyzed_section" : "music.audio_analyzed_full", { from: audioTime(profile.source_start_seconds ?? 0), to: audioTime(profile.source_end_seconds ?? profile.duration_seconds) })}</p>
        <p class="text-xs text-amber-300">{locale.t("music.audio_style_estimate")}</p>
        <details class="text-xs text-neutral-400" open><summary class="touch-target cursor-pointer content-center">{locale.t("music.audio_style_observations")}</summary><p class="whitespace-pre-wrap leading-relaxed">{profile.description}</p><ul class="mt-2 list-disc space-y-1 pl-5">{#each profile.estimates as detail}<li>{detail}</li>{/each}</ul></details>
        <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.audio_style_draft")}</span><textarea class={inputClass} rows="4" maxlength="1200" bind:value={draft} oninput={() => saved = false}></textarea></label>
        <button type="button" class={buttonClass} disabled={locked || !draft.trim() || draft.trim() === music.params.style} onclick={apply}>{locale.t("music.reference_apply")}</button>
        <div class="space-y-2 border-t border-neutral-800 pt-2">
          <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.profile_name")}</span><input class={inputClass} maxlength="80" bind:value={profileName} oninput={() => saved = false} /></label>
          <button type="button" class={buttonClass} disabled={saving || saved || !profileName.trim() || !draft.trim()} onclick={saveProfile}>{locale.t(saved ? "music.profile_saved" : "music.profile_save")}</button>
          <p class="text-xs text-neutral-500">{locale.t("music.profiles_storage")}</p>
        </div>
      {/if}
      {#if uploadError || error}<p class="whitespace-pre-wrap text-xs text-red-300" role="alert">{uploadError ? locale.t("music.audio_style_size") : error}</p>{/if}
    </div>
  {/if}
  {#if undo && music.params.style === undo.after}<button type="button" class={buttonClass} disabled={locked} onclick={undoStyle}>{locale.t("music.assistant_undo_style")}</button>{/if}
</div>
