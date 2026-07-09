<script lang="ts">
  import ExtraPromptBoxItem from "./ExtraPromptBoxItem.svelte";
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    side: "positive" | "negative";
  }

  let { side }: Props = $props();

  let boxes = $derived(
    side === "positive" ? generation.extraPositiveBoxes : generation.extraNegativeBoxes,
  );

  function addBox() {
    if (side === "positive") generation.addPositiveBox();
    else generation.addNegativeBox();
  }
</script>

{#each boxes as box, i (box.id)}
  <ExtraPromptBoxItem
    id={box.id}
    name={box.name}
    content={box.content}
    index={i + 1}
    {side}
  />
{/each}

<button
  type="button"
  onclick={addBox}
  class="mt-2 flex w-full items-center justify-center gap-1.5 rounded-lg border border-dashed border-neutral-700 py-1.5 text-xs text-neutral-400 transition-colors hover:border-indigo-500/60 hover:text-indigo-200"
>
  <span class="text-sm leading-none">+</span>
  {locale.t("generation.prompts.extra_box_add")}
</button>
