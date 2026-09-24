<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { getAuthUser } from "../../utils/ipc.js";
  import { cloneMusicParams, musicId, readMusicSampling } from "../../utils/musicSettings.js";
  import { inspectMusicScore } from "../../utils/musicScore.js";
  import { musicEditChanges, type MusicEditConstraints, type MusicEditProposal, type MusicEditScope } from "../../utils/musicEdit.js";
  import type { MusicParams, MusicResult } from "../../types/music.js";
  import MusicNotation from "./MusicNotation.svelte";
  let dialog: HTMLDialogElement | undefined = $state();
  let brief = $state(""), error = $state(""), busy = $state(false), proposal = $state<MusicEditProposal | null>(null);
  let constraints = $state<MusicEditConstraints>({ melody: "both", rhythm: true, tempo: true, lyrics: true, structure: true });
  let scope = $state<MusicEditScope>("style");
  const needsScore = $derived(scope === "score" || scope === "all");
  let baseline = $state<MusicParams | null>(null);
  const changedFields = $derived(proposal && baseline ? musicEditChanges(baseline, proposal) : []);
  let source: MusicResult | undefined, draft = "", owner: string | null = null, sequence = 0;
  const button = "touch-target rounded-md px-3 text-sm text-indigo-300 hover:bg-neutral-800 disabled:opacity-40";
  const input = "min-h-11 w-full rounded-md bg-neutral-800 px-3 py-2 text-sm text-neutral-200 disabled:opacity-40";
  $effect(() => {
    const id = music.editingComposition, element = dialog;
    if (!element) return;
    untrack(() => {
      if (!id) { element.close(); return; }
      source = music.results.find(r => r.prompt_id === id);
      if (id !== "draft" && !source) return;
      baseline = source ? { ...cloneMusicParams(source.params), abc: source.abc, seed: source.seed } : cloneMusicParams(music.params);
      draft = JSON.stringify(music.params); owner = getAuthUser(); brief = ""; error = ""; proposal = null;
      constraints = { melody: "both", rhythm: true, tempo: true, lyrics: true, structure: true };
      scope = "style";
      if (!element.open) element.showModal();
    });
  });
  function close() { sequence++; busy = false; music.editingComposition = null; proposal = null; dialog?.close(); }
  onDestroy(() => { sequence++; });
  function preset(kind: "production" | "harmony" | "tempo" | "key" | "form" | "translation") {
    brief = locale.t(`music.edit_brief_${kind}`);
    constraints = { melody: kind === "key" || kind === "form" ? "none" : "both", rhythm: kind !== "form", tempo: kind !== "tempo", lyrics: kind !== "translation" && kind !== "form", structure: kind !== "form" };
    proposal = null;
    scope = kind === "production" ? "style" : kind === "translation" ? "lyrics" : kind === "harmony" ? "score" : "all";
  }
  function changeScope() {
    proposal = null; error = "";
    constraints = { melody: "both", rhythm: true, tempo: true, lyrics: scope !== "lyrics", structure: true };
  }
  async function propose() {
    if (!baseline || busy || promptAssistant.isGenerating) return;
    if (owner !== getAuthUser() || JSON.stringify(music.params) !== draft) { error = locale.t("music.assistant_changed"); return; }
    const id = ++sequence, original = cloneMusicParams(baseline), checks = { ...constraints }, intent = brief, selectedScope = scope;
    const current = () => id === sequence && owner === getAuthUser() && JSON.stringify(music.params) === draft;
    proposal = null; busy = true; error = "";
    try {
      if (needsScore && !inspectMusicScore(original.abc).score) throw new Error(locale.t("music.edit_score_needed"));
      if (source) await music.archive(source);
      await promptAssistant.refreshStatus();
      if (!current()) return;
      if (!promptAssistant.isAvailable) throw new Error(locale.t("music.assistant_setup"));
      const result = await promptAssistant.editForMusic({ params: original, brief: intent, constraints: checks, scope: selectedScope }, current);
      if (id !== sequence || owner !== getAuthUser()) return;
      if (JSON.stringify(music.params) !== draft) throw new Error(locale.t("music.assistant_changed"));
      proposal = result;
    } catch (cause) { if (id === sequence) error = String(cause); }
    finally { if (id === sequence) busy = false; }
  }
  function apply() {
    if (!proposal || !baseline || busy || music.busy) return;
    if (owner !== getAuthUser() || JSON.stringify(music.params) !== draft) { error = locale.t("music.assistant_changed"); return; }
    const before = cloneMusicParams(music.params);
    const score = inspectMusicScore(proposal.abc).score;
    const scoreChanged = proposal.abc !== baseline.abc;
    if (scoreChanged && !score) return;
    music.params = { ...cloneMusicParams(baseline), abc: proposal.abc, style: proposal.style, lyrics: proposal.lyrics,
      task: "audio", planning: scoreChanged && score ? score.voices.Vocal.chords.length ? "full" : "melody" : baseline.planning, sampling: readMusicSampling(baseline.sampling),
      lineage: { project_id: baseline.lineage?.project_id ?? source?.prompt_id ?? musicId(), parent_id: source?.prompt_id ?? baseline.lineage?.parent_id, change: proposal.summary, comparison: proposal.check } };
    music.scoreUndo = { before, after: JSON.stringify(music.params) };
    if (before.abc !== music.params.abc || before.cover !== music.params.cover) music.reviewedCoverAbc = "";
    music.saveSettings(); music.view = "generate"; close();
  }
</script>

<dialog bind:this={dialog} class="m-auto max-h-[90vh] w-[calc(100%-2rem)] max-w-3xl overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-neutral-200 shadow-2xl backdrop:bg-black/70" aria-labelledby="music-composition-title" onclose={close}>
  <form class="space-y-4" onsubmit={event => { event.preventDefault(); void propose(); }}>
    <div class="flex items-center justify-between gap-3"><h2 id="music-composition-title" class="text-lg font-semibold">{locale.t("music.edit_composition")}</h2><button type="button" class={button} onclick={close}>{locale.t("common.close")}</button></div>
    <p class="text-xs leading-relaxed text-neutral-400">{locale.t("music.edit_help")}</p>
    <div class="flex flex-wrap gap-1">{#each (["production", "harmony", "tempo", "key", "form", "translation"] as const) as kind}<button type="button" class={button} disabled={busy} onclick={() => preset(kind)}>{locale.t(`music.edit_${kind}`)}</button>{/each}</div>
    <label class="block space-y-1 text-sm"><span>{locale.t("music.edit_scope")}</span><select class={input} bind:value={scope} onchange={changeScope} disabled={busy}>{#each (["style", "lyrics", "score", "all"] as const) as mode}<option value={mode}>{locale.t("music.edit_scope_" + mode)}</option>{/each}</select></label>
    <p class="text-xs text-neutral-400">{locale.t("music.edit_scope_help")}</p>
    <label class="block space-y-2 text-sm"><span>{locale.t("music.edit_intent")}</span><textarea class={input} rows="3" maxlength="2000" bind:value={brief} disabled={busy} oninput={() => proposal = null} required></textarea></label>
    {#if scope !== "style"}
    <fieldset class="grid gap-2 sm:grid-cols-2" disabled={busy} onchange={() => proposal = null}>
      <legend class="mb-2 text-sm text-neutral-400">{locale.t("music.edit_preserve")}</legend>
      {#if needsScore}<label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("music.edit_melody")}</span><select class={input} bind:value={constraints.melody}><option value="both">{locale.t("music.score_both")}</option><option value="Vocal">{locale.t("music.score_vocal")}</option><option value="Ins">{locale.t("music.score_instrumental")}</option><option value="none">{locale.t("music.edit_unlocked")}</option></select></label>{/if}
      {#each (["rhythm", "tempo", "lyrics", "structure"] as const).filter(key => needsScore || key === "lyrics" || key === "structure") as key}<label class="touch-target flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={constraints[key]} />{locale.t(`music.edit_keep_${key}`)}</label>{/each}
    </fieldset>
    {/if}
    <button type="submit" class={`${button} bg-indigo-500/15`} disabled={busy || promptAssistant.isGenerating || !brief.trim() || music.busy}>{locale.t(busy ? "music.edit_working" : "music.edit_propose")}</button>
    {#if error}<p class="whitespace-pre-wrap text-xs text-red-300" role="alert">{error}</p>{/if}
    {#if proposal}
      <section class="space-y-3 border-t border-neutral-700 pt-3">
        <p class="text-sm text-neutral-200">{proposal.summary}</p>
        <p class="text-xs text-emerald-300">{locale.t("music.edit_checks_passed")}</p>
        {#if changedFields.includes("abc")}<MusicNotation abc={proposal.abc} ceiling={baseline?.max_duration} />{/if}
        {#if !changedFields.length}<p class="text-xs text-neutral-400">{locale.t("music.edit_no_changes")}</p>{/if}
        {#each changedFields as field}
          <details open={field !== "abc"} class="text-xs"><summary class="touch-target cursor-pointer content-center">{locale.t("music." + field)}</summary>
            <div class="grid gap-3 sm:grid-cols-2"><div><p class="mb-1 text-neutral-500">{locale.t("music.edit_before")}</p><pre class="max-h-48 overflow-auto whitespace-pre-wrap break-words">{baseline?.[field]}</pre></div><div><p class="mb-1 text-indigo-300">{locale.t("music.edit_after")}</p><pre class="max-h-48 overflow-auto whitespace-pre-wrap break-words">{proposal[field]}</pre></div></div>
          </details>
        {/each}
        <details><summary class="touch-target cursor-pointer content-center text-sm text-neutral-400">{locale.t("music.plan_context")}</summary><pre class="max-h-64 overflow-auto whitespace-pre-wrap text-xs">{proposal.style}{"\n\n"}{proposal.lyrics}{"\n\n"}{proposal.abc}</pre></details>
        <button type="button" class={`${button} bg-indigo-500/15`} disabled={music.busy || !changedFields.length} onclick={apply}>{locale.t("music.edit_apply")}</button>
      </section>
    {/if}
  </form>
</dialog>
