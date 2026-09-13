<script lang="ts">
  import { onMount } from "svelte";
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { audioTime } from "../../utils/musicAudio.js";

  let now = $state(Date.now());
  onMount(() => {
    const timer = setInterval(() => { now = Date.now(); }, 1000);
    return () => clearInterval(timer);
  });
  const elapsed = $derived(audioTime(Math.max(0, ((music.finishedAt || now) - music.startedAt) / 1000)));
  const heading = $derived(
    music.outcome ? locale.t(`music.outcome_${music.outcome}`)
      : music.cancelling ? locale.t("music.cancelling")
      : music.phase === "submitting" ? locale.t("music.phase_submitting")
      : music.phase === "queued" ? (music.queuePosition === null ? locale.t("music.phase_queued") : locale.t("music.queue_position", { position: music.queuePosition }))
      : locale.t(`music.stage_${music.stage}`)
  );
  // YuE2's semantic sampler reports 25 frames per second of composed audio.
  const detail = $derived(music.stage === "compose"
    ? locale.t("music.progress_audio", { seconds: (music.stageValue / 25).toFixed(1), limit: (music.stageMax / 25).toFixed(0) })
    : music.stage === "score" ? locale.t("music.progress_score", { value: music.stageValue, max: music.stageMax })
    : locale.t("music.progress_steps", { value: music.stageValue, max: music.stageMax }));
</script>

{#if music.startedAt}
  <section class="space-y-2.5 py-1" aria-label={locale.t("music.generation_progress")}>
    <div class="flex items-baseline justify-between gap-3">
      <p class={`flex items-center gap-2 text-sm font-medium ${music.outcome === "failed" ? "text-red-300" : "text-neutral-200"}`} role="status">
        {#if music.outcome === "completed"}
          <svg class="h-4 w-4 shrink-0 text-indigo-300" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="m3 8 3 3 7-7"/></svg>
        {/if}
        {heading}
      </p>
      <span class="shrink-0 font-mono text-xs tabular-nums text-neutral-400" aria-label={locale.t("music.elapsed", { time: elapsed })}>{elapsed}</span>
    </div>
    {#if music.busy}
      <div class="h-1.5 overflow-hidden rounded-sm bg-neutral-800" role="progressbar" aria-label={heading} aria-valuemin="0" aria-valuemax="100" aria-valuenow={music.stagePercent ?? undefined} aria-valuetext={music.stageMax > 0 ? detail : heading}>
        <div class={`h-full bg-indigo-400 ${music.stagePercent === null ? 'w-1/3 motion-safe:animate-pulse' : 'transition-[width] duration-300 motion-reduce:transition-none'}`} style:width={music.stagePercent === null ? undefined : `${music.stagePercent}%`}></div>
      </div>
      <ol class="flex flex-wrap gap-x-3 gap-y-1 text-[11px]">
        {#each music.stages as stage}
          {@const complete = music.completedStages.includes(stage)}
          {@const active = stage === music.stage && music.phase !== "queued" && music.phase !== "submitting"}
          <li class={`inline-flex items-center gap-1 ${active ? 'font-medium text-indigo-300' : complete ? 'text-neutral-400' : 'text-neutral-500'}`} aria-current={active ? "step" : undefined}>
            {#if complete}<span aria-label={locale.t("music.stage_complete")}>&#10003;</span>{/if}
            {locale.t(`music.step_${stage}`)}
          </li>
        {/each}
      </ol>
      {#if music.stageMax > 0}
        <p class="text-xs tabular-nums text-neutral-400">{detail}</p>
      {:else if music.phase === "running" && music.stage === "prepare"}
        <p class="text-xs text-neutral-500">{locale.t("music.preparing_help")}</p>
      {/if}
      {#if music.stage === "compose" || music.stage === "score"}
        <p class="text-[11px] leading-relaxed text-neutral-500">{locale.t("music.progress_budget_help")}</p>
      {/if}
    {/if}
  </section>
{/if}
