<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { getAuthUser } from "../../utils/ipc.js";
  import { audioStyleTargetKey } from "../../utils/musicAudioStyle.js";
  let selected = $state(""), name = $state(""), draft = $state(""), error = $state(""), busy = $state(false);
  let estimates = $state<string[]>([]), previewKey = $state("");
  let sequence = 0, active = true, owner = getAuthUser(), baseline = "";
  let undo = $state<{ before: string; after: string; owner: string | null; song?: string } | null>(null);
  const profile = $derived(music.styleProfiles.find(p => p.id === selected));
  const target = () => ({ instrumental: !music.params.lyrics.trim(), max_duration: music.params.max_duration, language: locale.intlTag });
  const targetKey = $derived(audioStyleTargetKey(target()));
  const mismatch = $derived(profile && previewKey !== targetKey);
  const locked = $derived(music.busy || busy || promptAssistant.isGenerating);
  const snapshot = () => JSON.stringify([music.params, music.selectedResult?.prompt_id, locale.intlTag]);
  const input = "w-full min-w-0 rounded-md bg-neutral-900 px-3 py-2.5 text-sm text-neutral-200";
  const button = "touch-target rounded-md px-3 text-xs text-indigo-300 hover:bg-neutral-900 disabled:opacity-40";
  onDestroy(() => { active = false; sequence++; });
  $effect(() => {
    const current = profile;
    untrack(() => {
      sequence++; busy = false; error = ""; owner = getAuthUser(); baseline = snapshot();
      name = current?.name ?? ""; draft = current?.style ?? ""; estimates = current?.profile.estimates ?? [];
      previewKey = current ? audioStyleTargetKey(current.target) : "";
    });
  });
  let previousSong = music.selectedResult?.prompt_id;
  $effect(() => {
    const params = music.params, song = music.selectedResult?.prompt_id;
    untrack(() => {
      if (owner !== getAuthUser() || previousSong !== song) { sequence++; selected = ""; busy = false; undo = null; draft = ""; error = ""; }
      previousSong = song;
    }); void params;
  });
  async function adapt() {
    if (!profile || locked) return;
    const request = ++sequence, account = getAuthUser(), before = snapshot(), id = profile.id;
    const current = () => active && request === sequence && account === getAuthUser() && before === snapshot() && selected === id;
    busy = true; error = "";
    try {
      await promptAssistant.refreshStatus();
      if (!current()) return;
      if (!promptAssistant.isAvailable) throw new Error(locale.t("music.assistant_setup"));
      const result = await promptAssistant.adaptSavedMusicStyle(profile, target(), current);
      if (!current()) return;
      draft = result.style; estimates = result.estimates; previewKey = targetKey; baseline = before; owner = account;
    } catch (cause) { if (current()) error = String(cause); }
    finally { if (active && request === sequence) { busy = false; if (account === getAuthUser() && before !== snapshot()) error = locale.t("music.assistant_changed"); } }
  }
  function apply() {
    if (!profile || !draft.trim() || mismatch || locked || owner !== getAuthUser()) return;
    if (baseline !== snapshot()) { error = locale.t("music.assistant_changed"); return; }
    undo = { before: music.params.style, after: draft.trim(), owner, song: music.selectedResult?.prompt_id };
    music.params.style = undo.after; music.saveSettings(); baseline = snapshot();
  }
  function undoStyle() {
    if (!undo || locked || undo.owner !== getAuthUser() || undo.song !== music.selectedResult?.prompt_id || music.params.style !== undo.after) return;
    music.params.style = undo.before; music.saveSettings(); undo = null; baseline = snapshot();
  }
  async function rename() {
    if (!profile || !name.trim() || locked || owner !== getAuthUser()) return;
    const record = profile, account = owner; busy = true; error = "";
    try { if (!await music.saveStyleProfile({ ...record, name: name.trim() }) && account === getAuthUser()) error = music.libraryError; }
    finally { if (account === getAuthUser()) busy = false; }
  }
  async function remove() {
    if (!profile || locked || owner !== getAuthUser()) return;
    const id = profile.id, account = owner; busy = true; error = "";
    try { if (await music.deleteStyleProfile(id)) { if (account === getAuthUser()) selected = ""; } else if (account === getAuthUser()) error = music.libraryError; }
    finally { if (account === getAuthUser()) busy = false; }
  }
</script>
<details class="mt-2 space-y-2">
  <summary class="touch-target cursor-pointer content-center text-xs text-neutral-400">{locale.t("music.profiles_title")} ({music.styleProfiles.length})</summary>
  <div class="space-y-3 rounded-md border border-neutral-800 p-3">
    <p class="text-xs text-neutral-500">{locale.t("music.profiles_storage")}</p>
    <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.profiles_choose")}</span><select class={input} bind:value={selected} disabled={busy}><option value="">{locale.t("music.profiles_choose")}</option>{#each music.styleProfiles as p}<option value={p.id}>{p.name}</option>{/each}</select></label>
    {#if profile}
      <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.profile_name")}</span><input class={input} maxlength="80" bind:value={name} /></label>
      <div class="flex flex-wrap gap-1"><button type="button" class={button} disabled={locked || !name.trim()} onclick={rename}>{locale.t("music.profile_rename")}</button><button type="button" class={button} disabled={locked} onclick={remove}>{locale.t("common.delete")}</button></div>
      <p class="text-xs text-neutral-500">{profile.model} · {new Date(profile.createdAt).toLocaleDateString(locale.intlTag)} · {profile.target.max_duration}s · {locale.t(profile.target.instrumental ? "music.profile_instrumental" : "music.profile_vocal")}</p>
      <details class="text-xs text-neutral-400"><summary class="touch-target cursor-pointer content-center">{locale.t("music.audio_style_observations")}</summary><p>{profile.profile.description}</p><ul class="list-disc pl-4">{#each estimates as detail}<li>{detail}</li>{/each}</ul></details>
      {#if mismatch}<p class="text-xs text-amber-300">{locale.t("music.profile_adapt_help")}</p>{/if}
      <button type="button" class={button} disabled={locked} onclick={adapt}>{locale.t(busy ? "music.reference_describing" : "music.profile_adapt")}</button>
      <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.audio_style_draft")}</span><textarea class={input} rows="4" maxlength="1200" bind:value={draft}></textarea></label>
      <button type="button" class={button} disabled={locked || mismatch || !draft.trim() || draft.trim() === music.params.style} onclick={apply}>{locale.t("music.reference_apply")}</button>
    {/if}
    {#if undo && music.params.style === undo.after}<button type="button" class={button} disabled={locked} onclick={undoStyle}>{locale.t("music.assistant_undo_style")}</button>{/if}
    {#if error}<p class="text-xs text-red-300" role="alert">{error}</p>{/if}
  </div>
</details>
