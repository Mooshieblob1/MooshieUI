<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import {
    buildH3Context,
    h3FormatOf,
    h3InstructionLine,
    h3ManualTemplate,
  } from "../../utils/h3Prompt.js";
  import { H3_FPS } from "../../utils/videoParams.js";

  /**
   * The in-app half of H3 prompting: what the model expects, annotated, plus a
   * skeleton to fill in by hand. It mirrors whatever the video panel is set to
   * right now, so the labelled example is the one that would actually be sent
   * (reference mode has six sections and states the style before `[Shot 1]`;
   * every other task type has three fields and states it after).
   */

  /** One annotated field: the literal name H3 reads, and what belongs in it. */
  interface GuideSection {
    field: string;
    descKey: string;
  }

  const BASE_SECTIONS: GuideSection[] = [
    { field: "integrated_multimodal_description", descKey: "generation.video.h3_guide.section_integrated" },
    { field: "overall_soundscape", descKey: "generation.video.h3_guide.section_soundscape" },
    { field: "non_diegetic_music", descKey: "generation.video.h3_guide.section_music" },
  ];

  const REF_SECTIONS: GuideSection[] = [
    { field: "subject_definitions", descKey: "generation.video.h3_guide.section_subjects" },
    { field: "summary", descKey: "generation.video.h3_guide.section_summary" },
    { field: "retention_analysis", descKey: "generation.video.h3_guide.section_retention" },
    { field: "detailed_description", descKey: "generation.video.h3_guide.section_detailed" },
    { field: "overall_soundscape", descKey: "generation.video.h3_guide.section_soundscape" },
    { field: "non_diegetic_music", descKey: "generation.video.h3_guide.section_music" },
  ];

  const RULE_KEYS = [
    "generation.video.h3_guide.rule_shots",
    "generation.video.h3_guide.rule_camera",
    "generation.video.h3_guide.rule_dialogue",
    "generation.video.h3_guide.rule_text",
  ];

  let open = $state(false);
  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;

  const ctx = $derived(
    buildH3Context({
      variant: generation.videoVariant,
      frames: generation.videoFrameLength,
      hasFirstFrame: !!generation.videoFirstFrame,
      hasLastFrame: !!generation.videoLastFrame,
      referenceImageCount: generation.videoRefImageFilenames.length,
    }),
  );
  const isRef = $derived(h3FormatOf(ctx.taskType) === "ref");
  const sections = $derived(isRef ? REF_SECTIONS : BASE_SECTIONS);
  const instruction = $derived(h3InstructionLine(ctx));

  async function copyTemplate() {
    try {
      await navigator.clipboard.writeText(h3ManualTemplate(ctx));
      copied = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 2000);
    } catch (e) {
      console.error("H3 template copy failed:", e);
      gallery.showToast(locale.t("generation.video.h3_guide.copy_failed"), "error");
    }
  }
</script>

<div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-2.5 space-y-2">
  <button
    class="w-full text-left flex items-center justify-between text-xs text-neutral-400 hover:text-neutral-200 transition-colors"
    onclick={() => (open = !open)}
    title={open
      ? locale.t("common.collapse", { section: locale.t("generation.video.h3_guide.title") })
      : locale.t("common.expand", { section: locale.t("generation.video.h3_guide.title") })}
  >
    <span>{locale.t("generation.video.h3_guide.title")}</span>
    <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {open ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
  </button>

  {#if open}
    <p class="text-[11px] leading-relaxed text-neutral-400">
      {locale.t("generation.video.h3_guide.intro")}
    </p>
    <p class="text-[11px] text-neutral-500">
      {locale.t("generation.video.h3_guide.applies", {
        task: ctx.taskType.toUpperCase(),
        frames: String(ctx.frames),
        fps: String(H3_FPS),
        seconds: ctx.durationSeconds,
      })}
    </p>

    {#if instruction}
      <div class="rounded border border-neutral-800 bg-neutral-950/60 p-2 space-y-1">
        <p class="text-[10px] font-medium text-indigo-300">
          {locale.t("generation.video.h3_guide.instruction_line")}
        </p>
        <p class="text-[11px] text-neutral-400">
          {locale.t("generation.video.h3_guide.section_instruction")}
        </p>
        <pre class="overflow-x-auto whitespace-pre-wrap break-words font-mono text-[10px] leading-relaxed text-neutral-300">{instruction}</pre>
      </div>
    {/if}

    <div class="space-y-1.5">
      {#each sections as section (section.field)}
        <div class="rounded border border-neutral-800 bg-neutral-950/60 p-2 space-y-0.5">
          <p class="font-mono text-[10px] text-indigo-300">{section.field}:</p>
          <p class="text-[11px] leading-relaxed text-neutral-400">{locale.t(section.descKey)}</p>
        </div>
      {/each}
    </div>

    <div class="space-y-1">
      <p class="text-[10px] font-medium text-neutral-300">
        {locale.t("generation.video.h3_guide.rules_title")}
      </p>
      <ul class="space-y-1 pl-3">
        {#each RULE_KEYS as key (key)}
          <li class="list-disc text-[11px] leading-relaxed text-neutral-400">{locale.t(key)}</li>
        {/each}
      </ul>
    </div>

    <div class="space-y-1">
      <p class="text-[10px] font-medium text-neutral-300">
        {locale.t("generation.video.h3_guide.template_title")}
      </p>
      <pre class="max-h-56 overflow-auto rounded border border-neutral-800 bg-neutral-950/60 p-2 font-mono text-[10px] leading-relaxed text-neutral-300">{h3ManualTemplate(ctx)}</pre>
      <button
        class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
        onclick={copyTemplate}
      >
        {copied
          ? locale.t("generation.video.h3_guide.copied")
          : locale.t("generation.video.h3_guide.copy_template")}
      </button>
    </div>
  {/if}
</div>
