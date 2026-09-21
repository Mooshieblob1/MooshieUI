<script lang="ts">
  import { untrack } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { getAuthUser } from "../../utils/ipc.js";
  import { arrangementKinds, arrangementTimeline, arrangementStyle, fitArrangement, suggestArrangement, validArrangement, type ArrangementSection } from "../../utils/musicArrangement.js";
  let sections = $state<ArrangementSection[]>([]);
  let preview = $state(""), error = $state(""), baseline = "", owner = getAuthUser();
  let undo = $state<{ before: string; after: string; song?: string; previousBase: string; previousApplied: string } | null>(null);
  let lastBase = "", lastApplied = "";
  const total = $derived(sections.reduce((sum, s) => sum + (Number.isFinite(s.seconds) ? s.seconds : 0), 0));
  const valid = $derived(validArrangement(sections, music.params.max_duration));
  const timeline = $derived(arrangementTimeline(sections));
  const snapshot = () => JSON.stringify([music.params, music.selectedResult?.prompt_id]);
  const button = "touch-target rounded-md px-3 text-xs text-indigo-300 hover:bg-neutral-900 disabled:opacity-40";
  const input = "min-h-11 w-full rounded-md bg-neutral-900 px-2 py-2 text-xs text-neutral-200";
  let previousSong = music.selectedResult?.prompt_id;
  $effect(() => {
    const params = music.params, song = music.selectedResult?.prompt_id;
    untrack(() => {
      if (owner !== getAuthUser() || song !== previousSong) { sections = []; preview = ""; undo = null; error = ""; lastBase = ""; lastApplied = ""; owner = getAuthUser(); }
      previousSong = song;
    }); void params;
  });
  function suggest() {
    try { sections = suggestArrangement(music.params.max_duration, music.params.lyrics); preview = ""; error = ""; }
    catch { error = locale.t("music.arrange_invalid"); }
  }
  function fit() {
    try { sections = fitArrangement(sections, music.params.max_duration); preview = ""; error = ""; }
    catch { error = locale.t("music.arrange_invalid"); }
  }
  function move(index: number, delta: number) {
    const other = index + delta;
    if (other < 0 || other >= sections.length) return;
    const next = [...sections]; [next[index], next[other]] = [next[other], next[index]]; sections = next; preview = "";
  }
  function prepare() {
    try {
      const base = music.params.style === lastApplied ? lastBase : music.params.style;
      preview = arrangementStyle(base, sections, music.params.max_duration, !music.params.lyrics.trim());
      baseline = snapshot(); error = "";
    } catch { error = locale.t("music.arrange_invalid"); }
  }
  function apply() {
    if (!preview || music.busy || owner !== getAuthUser()) return;
    if (baseline !== snapshot()) { error = locale.t("music.assistant_changed"); return; }
    const before = music.params.style;
    const previousBase = lastBase, previousApplied = lastApplied;
    lastBase = before === lastApplied ? lastBase : before; lastApplied = preview.trim();
    undo = { before, after: lastApplied, song: music.selectedResult?.prompt_id, previousBase, previousApplied };
    music.params.style = lastApplied; music.saveSettings(); preview = ""; error = "";
  }
  function undoPlan() {
    if (!undo || music.busy || owner !== getAuthUser() || undo.song !== music.selectedResult?.prompt_id || music.params.style !== undo.after) return;
    music.params.style = undo.before; music.saveSettings(); lastApplied = undo.previousApplied; lastBase = undo.previousBase; undo = null;
  }
</script>
<details class="space-y-3 rounded-md border border-neutral-800 p-3">
  <summary class="touch-target cursor-pointer content-center text-sm text-neutral-300">{locale.t("music.arrange_title")}</summary>
  <p class="text-xs text-neutral-500">{locale.t("music.arrange_help")}</p>
  {#if music.params.abc.trim()}<p class="text-xs text-amber-300">{locale.t("music.arrange_score_help")}</p>{/if}
  <fieldset class="space-y-3" disabled={music.busy}>
    <div class="flex flex-wrap gap-1"><button type="button" class={button} onclick={suggest}>{locale.t("music.arrange_suggest")}</button><button type="button" class={button} disabled={!sections.length} onclick={fit}>{locale.t("music.arrange_fit")}</button><button type="button" class={button} disabled={sections.length >= 12} onclick={() => { sections = [...sections, {kind: music.params.lyrics.trim() ? "verse" : "theme", seconds: 10, direction: ""}]; preview = ""; }}>{locale.t("common.add")}</button></div>
    {#if sections.length}
      <div class="flex h-3 overflow-hidden rounded bg-neutral-800" role="img" aria-label={locale.t("music.arrange_budget", {used: total, total: music.params.max_duration})}>{#each timeline as section, index}<div class={index % 2 ? "bg-indigo-400/50" : "bg-indigo-300/80"} style:width={(Number.isFinite(section.seconds) ? section.seconds / Math.max(1, total) : 0) * 100 + "%"} title={locale.t("music.arrange_" + section.kind) + " " + section.start + "–" + section.end + "s"}></div>{/each}</div>
      {#each sections as section, index}
        <div class="space-y-2 rounded border border-neutral-800 p-2">
          <div class="grid grid-cols-[minmax(0,1fr)_5rem] gap-2">
            <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("music.arrange_section")} {index + 1}</span><select class={input} bind:value={section.kind} onchange={() => preview = ""}>{#each arrangementKinds as kind}<option value={kind}>{locale.t("music.arrange_" + kind)}</option>{/each}</select></label>
            <label class="space-y-1 text-xs text-neutral-400"><span>{locale.t("music.seconds")}</span><input class={input} type="number" min="1" max="360" step="1" bind:value={section.seconds} oninput={() => preview = ""} /></label>
          </div>
          <label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.arrange_direction")}</span><input class={input} maxlength="160" bind:value={section.direction} oninput={() => preview = ""} /></label>
          <div class="flex flex-wrap gap-1"><button type="button" class={button} disabled={index === 0} onclick={() => move(index, -1)}>{locale.t("music.arrange_up")}</button><button type="button" class={button} disabled={index === sections.length - 1} onclick={() => move(index, 1)}>{locale.t("music.arrange_down")}</button><button type="button" class={button} onclick={() => { sections = sections.filter((_, i) => i !== index); preview = ""; }}>{locale.t("common.remove")}</button></div>
        </div>
      {/each}
      <p class={valid ? "text-xs text-neutral-400" : "text-xs text-amber-300"}>{locale.t("music.arrange_budget", {used: total, total: music.params.max_duration})}</p>
      {#if !valid}<p class="text-xs text-amber-300">{locale.t("music.arrange_invalid")}</p>{/if}
      <button type="button" class={button} disabled={!valid} onclick={prepare}>{locale.t("music.arrange_preview")}</button>
    {/if}
    {#if preview}<label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.audio_style_draft")}</span><textarea class={input} rows="5" maxlength="16000" bind:value={preview}></textarea></label><button type="button" class={button} disabled={!preview.trim()} onclick={apply}>{locale.t("music.reference_apply")}</button>{/if}
    {#if undo && music.params.style === undo.after}<button type="button" class={button} onclick={undoPlan}>{locale.t("music.assistant_undo_style")}</button>{/if}
    {#if error}<p class="text-xs text-red-300" role="alert">{error}</p>{/if}
  </fieldset>
</details>
