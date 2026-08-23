<script lang="ts">
  /**
   * NovelAI prompt budget bar, drawn under each prompt box in NovelAI mode.
   *
   * NovelAI caps a prompt at 1471 tokens, a single hard ceiling rather than
   * CLIP's 75-token chunk boundary, so it reads better as a fill bar than as the
   * "n/75" chunk counter the ComfyUI path shows above the box.
   *
   * With extra boxes in play the reading is the side total, not this box's own
   * count: the boxes are concatenated into one prompt before the request goes
   * out, so a per-box number would understate what is actually being sent. The
   * same total is therefore repeated under every box on that side.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { joinPromptBoxes } from "../../utils/promptSanitize.js";
  import { estimatePromptTokens, NOVELAI_TOKEN_LIMIT } from "../../utils/promptTokens.js";

  interface Props {
    side: "positive" | "negative";
  }

  let { side }: Props = $props();

  const extras = $derived(
    side === "positive" ? generation.extraPositiveBoxes : generation.extraNegativeBoxes,
  );
  const shared = $derived(extras.length > 0);
  const tokens = $derived(
    estimatePromptTokens(
      joinPromptBoxes([
        side === "positive" ? generation.positivePrompt : generation.negativePrompt,
        ...extras.map((b) => b.content),
      ]),
    ),
  );
  const ratio = $derived(Math.min(1, tokens / NOVELAI_TOKEN_LIMIT));
  const over = $derived(tokens > NOVELAI_TOKEN_LIMIT);
  const near = $derived(!over && ratio >= 0.9);
</script>

<div
  class="mt-1 flex items-center gap-2"
  title={shared
    ? locale.t("generation.novelai.tokens_total_tip", { limit: String(NOVELAI_TOKEN_LIMIT) })
    : locale.t("generation.novelai.tokens_tip", { limit: String(NOVELAI_TOKEN_LIMIT) })}
>
  <div class="h-1 flex-1 overflow-hidden rounded-full bg-neutral-800">
    <div
      class="h-full rounded-full transition-all {over
        ? 'bg-red-500'
        : near
          ? 'bg-amber-400'
          : 'bg-indigo-500'}"
      style="width: {ratio * 100}%"
    ></div>
  </div>
  <span
    class="shrink-0 tabular-nums text-[10px] {over
      ? 'text-red-400'
      : near
        ? 'text-amber-400'
        : 'text-neutral-500'}"
  >
    {tokens}/{NOVELAI_TOKEN_LIMIT}{shared ? ` ${locale.t("generation.novelai.tokens_total_suffix")}` : ""}
  </span>
</div>
