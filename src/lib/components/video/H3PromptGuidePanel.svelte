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
   * The in-app half of H3 prompting, as a dialog rather than a column of prose:
   * four tabs, each answering one question - what the fields are, what the
   * syntax rules are, what a finished prompt looks like, and what to paste in.
   *
   * Everything mirrors whatever the video panel is set to right now, so the
   * annotated example is the one that would actually be sent (reference mode has
   * six sections and states the style before `[Shot 1]`; every other task type
   * has three fields and states it after).
   *
   * The example text is deliberately English and deliberately not translated:
   * these are literal demonstrations of the format H3 was trained on, and a
   * translated one would teach the wrong thing. Only the explanations around
   * them go through `locale.t`.
   */

  let { onClose }: { onClose: () => void } = $props();

  type TabId = "structure" | "rules" | "example" | "template";

  /** One annotated field: the literal name H3 reads, what belongs in it, and a one-line sample. */
  interface GuideSection {
    field: string;
    descKey: string;
    example: string;
  }

  const SECTION_INTEGRATED: GuideSection = {
    field: "integrated_multimodal_description",
    descKey: "generation.video.h3_guide.section_integrated",
    example: `[Shot 1] Live-action, cinematic. A woman in a grey raincoat waits under a station awning, rain sheeting behind her. The camera pushes in with small amplitude at slow speed.`,
  };
  const SECTION_SOUNDSCAPE: GuideSection = {
    field: "overall_soundscape",
    descKey: "generation.video.h3_guide.section_soundscape",
    example: `Heavy rain drums on the metal awning and splashes off the concrete. A train rumbles closer and its brakes rise into a thin metallic squeal.`,
  };
  const SECTION_MUSIC: GuideSection = {
    field: "non_diegetic_music",
    descKey: "generation.video.h3_guide.section_music",
    example: `A sustained cello holds under the rain, joined by a slow piano figure at about 60 beats per minute. The dynamics stay soft throughout.`,
  };

  const BASE_SECTIONS: GuideSection[] = [SECTION_INTEGRATED, SECTION_SOUNDSCAPE, SECTION_MUSIC];

  const REF_SECTIONS: GuideSection[] = [
    {
      field: "subject_definitions",
      descKey: "generation.video.h3_guide.section_subjects",
      example: `<Subject 1> is the tabby cat in <Picture 1>, with the white chest blaze, torn left ear and amber eyes.`,
    },
    {
      field: "summary",
      descKey: "generation.video.h3_guide.section_summary",
      example: `[reference generation] The target video shows a cat settling into a chair in a sunlit study, using <Subject 1> as the animal.`,
    },
    {
      field: "retention_analysis",
      descKey: "generation.video.h3_guide.section_retention",
      example: `<Subject 1> (appears in [Shot 1]): fully_preserved - coat pattern, chest blaze and eye colour stay identical to the reference.`,
    },
    {
      field: "detailed_description",
      descKey: "generation.video.h3_guide.section_detailed",
      example: `The target video is in a live-action documentary style with warm afternoon daylight. [Shot 1] A low, wide view of a study, the chair centred against a bookshelf.`,
    },
    SECTION_SOUNDSCAPE,
    SECTION_MUSIC,
  ];

  /** The five syntax rules that break a prompt most often when they are missed. */
  const RULES = [
    {
      titleKey: "generation.video.h3_guide.rule_shots_title",
      descKey: "generation.video.h3_guide.rule_shots",
      example: `[Shot 2] At 00:03.500, still live-action cinematic, the camera cuts to the empty platform edge.`,
    },
    {
      titleKey: "generation.video.h3_guide.rule_style_title",
      descKey: "generation.video.h3_guide.rule_style",
      example: `[Shot 2] At 00:02.800, still 2D-animated with flat cel shading, the shot cuts to the rooftop.`,
    },
    {
      titleKey: "generation.video.h3_guide.rule_camera_title",
      descKey: "generation.video.h3_guide.rule_camera",
      example: `The camera pushes in with small amplitude at slow speed.`,
    },
    {
      titleKey: "generation.video.h3_guide.rule_dialogue_title",
      descKey: "generation.video.h3_guide.rule_dialogue",
      example: `The young woman with a quiet, breathy voice (S1) says: <d>[English] I get off at the next station.</d>`,
    },
    {
      titleKey: "generation.video.h3_guide.rule_text_title",
      descKey: "generation.video.h3_guide.rule_text",
      example: `A red neon sign reading "OPEN" glows above the doorway.`,
    },
  ];

  let tab = $state<TabId>("structure");
  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;

  const ctx = $derived(
    buildH3Context({
      variant: generation.videoVariant,
      frames: generation.videoFrameLength,
      hasFirstFrame: !!generation.videoFirstFrame,
      hasLastFrame: !!generation.videoEffectiveLastFrame,
      referenceImageCount: generation.videoRefImageFilenames.length,
    }),
  );
  const isRef = $derived(h3FormatOf(ctx.taskType) === "ref");
  const sections = $derived(isRef ? REF_SECTIONS : BASE_SECTIONS);
  const instruction = $derived(h3InstructionLine(ctx));
  const template = $derived(h3ManualTemplate(ctx));

  /**
   * A complete, sendable prompt rather than a skeleton - split per field so the
   * tab reads as a sequence of short blocks instead of one page-long block.
   * The final timestamp tracks the real duration, because a worked example that
   * runs past the end of the video teaches the one mistake H3 punishes hardest.
   */
  const worked = $derived.by((): { field: string; body: string }[] => {
    const end = ctx.durationSeconds;
    if (isRef) {
      return [
        {
          field: "subject_definitions",
          body: `<Subject 1> is the tabby cat in <Picture 1>, with the white chest blaze, torn left ear and amber eyes.
<Subject 2> is the wooden reading chair in <Picture 2>, with the faded green cushion and carved armrests.`,
        },
        {
          field: "summary",
          body: `[reference generation] The target video shows a cat settling into a chair in a sunlit study, using <Subject 1> as the animal and <Subject 2> as the furniture.`,
        },
        {
          field: "retention_analysis",
          body: `<Subject 1> (appears in [Shot 1], [Shot 2]): fully_preserved - coat pattern, chest blaze, torn ear and eye colour stay identical to the reference.
<Subject 2> (appears in [Shot 1], [Shot 2]): partially_preserved - shape and carving stay, the cushion may catch different light.`,
        },
        {
          field: "detailed_description",
          body: `The target video is in a live-action documentary style with warm afternoon daylight and a muted amber palette.
[Shot 1] A low, wide view of a study, the chair centred against a bookshelf, dust turning in a shaft of window light. <Subject 1> steps into frame from the right, front paws first, and tests the cushion of <Subject 2>. The camera arcs right with small amplitude at slow speed.
[Shot 2] At 00:02.800, still live-action documentary with warm afternoon daylight, the shot cuts to a close-up of the cushion, beginning from <Picture 2>. The cat circles once, folds its legs and settles, tail curling last, holding still through to ${end} seconds.`,
        },
        {
          field: "overall_soundscape",
          body: `Floorboards creak faintly under the chair as the cat shifts its weight, and its claws catch the cushion fabric with a soft tearing rasp. Birdsong carries in through a closed window, muffled and distant. A slow, contented breathing settles in over the last seconds.`,
        },
        {
          field: "non_diegetic_music",
          body: `A muted upright piano plays a slow four-note figure at roughly 70 beats per minute, with a warm double bass entering under it at the cut. Both thin out to a single held piano note as the cat settles.`,
        },
      ];
    }
    return [
      {
        field: "integrated_multimodal_description",
        body: `[Shot 1] Live-action, cinematic. A woman in a grey raincoat stands under a station awning, framed chest-height in a medium close-up, rain sheeting behind her under cool blue streetlight. She turns her head toward the platform and tightens her grip on a paper ticket. The camera pushes in with small amplitude at slow speed. The young woman with a quiet, breathy voice (S1) says: <d>[English] I get off at the next station.</d>
[Shot 2] At 00:03.500, still live-action cinematic under the same cool blue streetlight, the camera cuts to the empty platform edge, the yellow line glossy with rain. Her footsteps enter from the left and stop at the line as the train's headlight grows through the downpour, holding through to ${end} seconds.`,
      },
      {
        field: "overall_soundscape",
        body: `Heavy rain drums on the metal awning and splashes off the concrete below. A train rumbles closer and its brakes rise into a thin metallic squeal. Wet fabric rustles as she shifts her weight, and her breathing stays short and shallow.`,
      },
      {
        field: "non_diegetic_music",
        body: `A single sustained cello holds under the rain, joined at the cut by a slow piano figure at roughly 60 beats per minute. The dynamics stay soft throughout and thin back to the cello alone on the final beat.`,
      },
    ];
  });

  const TABS: { id: TabId; labelKey: string }[] = [
    { id: "structure", labelKey: "generation.video.h3_guide.tab_structure" },
    { id: "rules", labelKey: "generation.video.h3_guide.tab_rules" },
    { id: "example", labelKey: "generation.video.h3_guide.tab_example" },
    { id: "template", labelKey: "generation.video.h3_guide.tab_template" },
  ];

  async function copyTemplate() {
    try {
      await navigator.clipboard.writeText(template);
      copied = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 2000);
    } catch (e) {
      console.error("H3 template copy failed:", e);
      gallery.showToast(locale.t("generation.video.h3_guide.copy_failed"), "error");
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<!-- Deliberately not a modal: it fills the preview column and leaves both side
     panels lit and interactive, so the prompt can be typed while it is open.
     Hence no backdrop, no `aria-modal`, and no focus trap - the close button and
     Escape are the only ways out, and neither steals focus from the prompt box. -->
<div
  class="flex h-full w-full flex-col overflow-hidden rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl"
  role="dialog"
  aria-modal="false"
  aria-label={locale.t("generation.video.h3_guide.title")}
>
  <!-- Header: what this is, plus which of the two formats the current panel settings resolve to -->
  <div class="shrink-0 border-b border-neutral-800 px-5 pt-4 pb-3">
    <div class="flex items-start justify-between gap-3">
      <div class="min-w-0">
        <h2 class="text-base font-semibold text-neutral-100">
          {locale.t("generation.video.h3_guide.title")}
        </h2>
        <div class="mt-1.5 flex flex-wrap items-center gap-1.5">
          <span class="rounded bg-neutral-800 px-1.5 py-0.5 font-mono text-[10px] text-indigo-300">
            {ctx.taskType.toUpperCase()}
          </span>
          <span class="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-300">
            {locale.t(
              isRef
                ? "generation.video.h3_guide.format_ref"
                : "generation.video.h3_guide.format_base",
            )}
          </span>
          <span class="text-[10px] text-neutral-500">
            {locale.t("generation.video.h3_guide.applies", {
              task: ctx.taskType.toUpperCase(),
              frames: String(ctx.frames),
              fps: String(H3_FPS),
              seconds: ctx.durationSeconds,
            })}
          </span>
        </div>
      </div>
      <button
        class="shrink-0 rounded-lg px-2 py-1 text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200"
        onclick={onClose}
        aria-label={locale.t("common.close")}
      >
        ✕
      </button>
    </div>
  </div>

  <!-- Tabs. Four questions, one answer each, so no single panel becomes a wall. -->
  <div class="flex shrink-0 gap-1 border-b border-neutral-800 px-3">
    {#each TABS as t (t.id)}
      <button
        class="border-b-2 px-3 py-2 text-xs transition-colors {tab === t.id
          ? 'border-[var(--theme-accent-500)] text-neutral-100'
          : 'border-transparent text-neutral-400 hover:text-neutral-200'}"
        onclick={() => (tab = t.id)}
      >
        {locale.t(t.labelKey)}
      </button>
    {/each}
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto px-5 py-4">
    {#if tab === "structure"}
      <p class="text-[11px] leading-relaxed text-neutral-400">
        {locale.t("generation.video.h3_guide.intro")}
      </p>

      {#if instruction}
        <div class="mt-3 rounded-lg border border-indigo-900/60 bg-indigo-950/30 p-3">
          <p class="text-[10px] font-medium uppercase tracking-wide text-indigo-300">
            {locale.t("generation.video.h3_guide.instruction_line")}
          </p>
          <p class="mt-1 text-[11px] leading-relaxed text-neutral-400">
            {locale.t("generation.video.h3_guide.section_instruction")}
          </p>
          <pre class="mt-2 overflow-x-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-black/40 p-2 font-mono text-[10px] leading-relaxed text-neutral-300">{instruction}</pre>
        </div>
      {/if}

      <ol class="mt-3 space-y-2">
        {#each sections as section, i (section.field)}
          <li class="rounded-lg border border-neutral-800 bg-neutral-950/60 p-3">
            <div class="flex items-center gap-2">
              <span
                class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-neutral-800 text-[10px] text-neutral-400"
              >
                {i + 1}
              </span>
              <span class="font-mono text-xs text-indigo-300">{section.field}:</span>
            </div>
            <p class="mt-1.5 pl-7 text-[11px] leading-relaxed text-neutral-400">
              {locale.t(section.descKey)}
            </p>
            <div class="mt-2 pl-7">
              <p class="text-[9px] uppercase tracking-wide text-neutral-600">
                {locale.t("generation.video.h3_guide.example")}
              </p>
              <pre class="mt-1 overflow-x-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-black/40 p-2 font-mono text-[10px] leading-relaxed text-neutral-400">{section.example}</pre>
            </div>
          </li>
        {/each}
      </ol>
    {:else if tab === "rules"}
      <div class="space-y-2">
        {#each RULES as rule (rule.titleKey)}
          <div class="rounded-lg border border-neutral-800 bg-neutral-950/60 p-3">
            <p class="text-xs font-medium text-neutral-200">{locale.t(rule.titleKey)}</p>
            <p class="mt-1 text-[11px] leading-relaxed text-neutral-400">
              {locale.t(rule.descKey)}
            </p>
            <pre class="mt-2 overflow-x-auto whitespace-pre-wrap break-words rounded border border-neutral-800 bg-black/40 p-2 font-mono text-[10px] leading-relaxed text-neutral-400">{rule.example}</pre>
          </div>
        {/each}
      </div>
    {:else if tab === "example"}
      <p class="text-[11px] leading-relaxed text-neutral-400">
        {locale.t("generation.video.h3_guide.example_intro")}
      </p>
      {#if instruction}
        <div class="mt-3">
          <p class="font-mono text-[10px] text-indigo-300">
            {locale.t("generation.video.h3_guide.instruction_line")}
          </p>
          <pre class="mt-1 overflow-x-auto whitespace-pre-wrap break-words rounded-lg border border-neutral-800 bg-black/40 p-2.5 font-mono text-[10px] leading-relaxed text-neutral-300">{instruction}</pre>
        </div>
      {/if}
      <div class="mt-3 space-y-3">
        {#each worked as block (block.field)}
          <div>
            <p class="font-mono text-[10px] text-indigo-300">{block.field}:</p>
            <pre class="mt-1 overflow-x-auto whitespace-pre-wrap break-words rounded-lg border border-neutral-800 bg-black/40 p-2.5 font-mono text-[10px] leading-relaxed text-neutral-300">{block.body}</pre>
          </div>
        {/each}
      </div>
    {:else}
      <p class="text-[11px] leading-relaxed text-neutral-400">
        {locale.t("generation.video.h3_guide.template_hint")}
      </p>
      <pre class="mt-3 overflow-x-auto whitespace-pre-wrap break-words rounded-lg border border-neutral-800 bg-black/40 p-3 font-mono text-[10px] leading-relaxed text-neutral-300">{template}</pre>
      <button
        class="mt-3 rounded-lg border border-neutral-600 px-3 py-1 text-[11px] text-neutral-300 hover:bg-neutral-800"
        onclick={copyTemplate}
      >
        {copied
          ? locale.t("generation.video.h3_guide.copied")
          : locale.t("generation.video.h3_guide.copy_template")}
      </button>
    {/if}
  </div>
</div>
