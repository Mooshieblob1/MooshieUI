<script lang="ts">
  import { onDestroy } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { musicCover } from "../../stores/musicCover.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { getAuthUser } from "../../utils/ipc.js";
  import { inspectMusicScore, setScoreTempo } from "../../utils/musicScore.js";
  import MusicNotation from "./MusicNotation.svelte";
  let error = $state(""), tempo = $state(120), importing = $state(false), active = true;
  const inspection = $derived(inspectMusicScore(music.params.abc));
  $effect(() => { if (inspection.score) tempo = inspection.score.bpm; });
  const locked = $derived(music.busy || musicCover.busy || importing);
  const button = "touch-target rounded-md px-3 text-xs text-indigo-300 hover:bg-neutral-900 disabled:opacity-40";
  const input = "min-h-11 w-full rounded-md bg-neutral-900 px-3 py-2 text-sm text-neutral-200";
  onDestroy(() => { active = false; });
  async function importScore(file?: File) {
    if (!file || locked) return;
    error = "";
    if (file.size > 131072) { error = locale.t("music.cover_score_limit"); return; }
    const before = JSON.stringify(music.params), owner = getAuthUser(); importing = true;
    try { const abc = (await file.text()).replace(/^\uFEFF/, "");
      if (active && owner === getAuthUser() && before === JSON.stringify(music.params)) music.replaceScore(abc);
      else if (active) error = locale.t("music.assistant_changed");
    } catch (cause) { if (active) error = String(cause); } finally { importing = false; }
  }
  function changeTempo() {
    try { error = ""; music.replaceScore(setScoreTempo(music.params.abc, tempo)); }
    catch (cause) { error = String(cause); }
  }
</script>

<section class="space-y-3 border-t border-neutral-800 pt-3" aria-label={locale.t("music.score_workbench")}>
  <div class="flex flex-wrap items-center justify-between gap-1">
    <h2 class="text-sm font-medium text-neutral-200">{locale.t("music.score_workbench")}</h2>
    <button type="button" class={button} disabled={locked} onclick={() => music.editingComposition = "draft"}>{locale.t("music.edit_composition")}</button>
  </div>
  {#if !music.params.cover}
    <div class="flex flex-wrap gap-2">
      <button type="button" class={button} disabled={locked || !music.ready || music.params.planning === "off"} onclick={() => music.generatePlan()}>{locale.t("music.plan_generate")}</button>
      <label class={`${button} inline-flex cursor-pointer items-center`}>{locale.t("music.cover_import")}<input class="sr-only" type="file" accept=".abc,text/plain" disabled={locked} onchange={event => { void importScore(event.currentTarget.files?.[0]); event.currentTarget.value = ""; }} /></label>
    </div>
    <p class="text-xs text-neutral-500">{locale.t("music.plan_help")}</p>
    <label class="block space-y-2 text-xs text-neutral-400"><span>{locale.t("music.abc")}</span><textarea class={`${input} font-mono text-xs`} rows="5" maxlength="128000" bind:value={music.params.abc} disabled={locked} placeholder={locale.t("music.abc_help")}></textarea></label>
  {/if}
  <MusicNotation abc={music.params.abc} ceiling={music.params.max_duration} />
  {#if inspection.score}
    <div class="flex flex-wrap items-center gap-2">
      <label class="flex items-center gap-2 text-xs text-neutral-400">{locale.t("music.score_tempo")}<input class={`${input} max-w-24`} type="number" min="20" max="400" step="1" bind:value={tempo} disabled={locked} /></label>
      <button type="button" class={button} disabled={locked} onclick={changeTempo}>{locale.t("music.score_set_tempo")}</button>
    </div>
  {/if}
  {#if music.scoreUndo}<button type="button" class={button} disabled={locked || music.scoreUndo.after !== JSON.stringify(music.params)} onclick={() => music.undoScore()}>{locale.t("prompt_assistant.undo")}</button>{/if}
  {#if music.plans.length}
    <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.saved_plans")}</span><select class={input} value="" disabled={locked} onchange={event => { music.planCandidate = music.plans.find(p => p.prompt_id === event.currentTarget.value) ?? null; event.currentTarget.value = ""; }}><option value="">{locale.t("music.plan_choose")}</option>{#each music.plans as plan}<option value={plan.prompt_id}>{plan.params.title || plan.params.style.slice(0, 80)} · {plan.seed}</option>{/each}</select></label>
  {/if}
  {#if music.planCandidate}
    {@const plan = music.planCandidate}
    <div class="space-y-2 rounded-md border border-indigo-500/40 p-3">
      <h3 class="text-sm text-indigo-300">{locale.t("music.plan_candidate")}</h3>
      {#if plan.metadata?.abc_truncated}<p class="text-xs text-amber-300">{locale.t("music.truncated_score")}</p>{/if}
      <MusicNotation abc={plan.abc} ceiling={plan.params.max_duration} />
      <details><summary class="touch-target cursor-pointer content-center text-xs text-neutral-400">{locale.t("music.plan_context")}</summary><pre class="max-h-60 overflow-auto whitespace-pre-wrap text-xs text-neutral-400">{plan.params.style}{"\n\n"}{plan.params.lyrics}{"\n\n"}{plan.abc}</pre></details>
      <button type="button" class={button} disabled={locked || !inspectMusicScore(plan.abc).score} onclick={() => music.applyPlan(plan)}>{locale.t("music.plan_apply")}</button>
      <button type="button" class={button} onclick={() => music.planCandidate = null}>{locale.t("common.close")}</button>
    </div>
  {/if}
  {#if error}<p class="text-xs text-red-300" role="alert">{error}</p>{/if}
</section>
