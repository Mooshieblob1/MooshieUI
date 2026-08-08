<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { h3Guide } from "../../stores/h3Guide.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { buildH3Context } from "../../utils/h3Prompt.js";

  /**
   * Entry point for the H3 prompting guide. The guide itself used to expand in
   * place under the prompt box, which turned the panel into a column of prose
   * nobody read; it now renders over the preview column so the prompt stays
   * reachable while it is open, and this is just the door to it.
   *
   * The button still shows the live task type, because which of the two H3
   * formats applies is the one thing worth knowing without opening anything.
   */

  const taskType = $derived(
    buildH3Context({
      variant: generation.videoVariant,
      frames: generation.videoFrameLength,
      hasFirstFrame: !!generation.videoFirstFrame,
      hasLastFrame: !!generation.videoEffectiveLastFrame,
      referenceImageCount: generation.videoRefImageFilenames.length,
    }).taskType,
  );
</script>

<button
  class="flex w-full items-center gap-2 rounded-lg border px-2.5 py-2 text-left text-xs transition-colors {h3Guide.open
    ? 'border-[var(--theme-accent-600)] bg-neutral-800/60 text-neutral-100'
    : 'border-neutral-800 bg-neutral-900/50 text-neutral-300 hover:border-neutral-700 hover:bg-neutral-800/50'}"
  onclick={() => h3Guide.toggle()}
  aria-expanded={h3Guide.open}
>
  <svg
    class="h-3.5 w-3.5 shrink-0 text-neutral-500"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    viewBox="0 0 24 24"
    aria-hidden="true"
  >
    <path
      stroke-linecap="round"
      stroke-linejoin="round"
      d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253"
    />
  </svg>
  <span class="min-w-0 flex-1 truncate">{locale.t("generation.video.h3_guide.title")}</span>
  <span class="shrink-0 rounded bg-neutral-800 px-1.5 py-0.5 font-mono text-[10px] text-indigo-300">
    {taskType.toUpperCase()}
  </span>
  <span class="shrink-0 text-[10px] text-neutral-500">
    {locale.t(h3Guide.open ? "common.close" : "generation.video.h3_guide.open")}
  </span>
</button>
