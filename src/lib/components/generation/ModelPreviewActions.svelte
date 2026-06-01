<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { dispatchModelPreviewAction } from "../../utils/modelPreviewImage.js";

  interface Props {
    imageUrl: string;
    modelLabel: string;
  }

  let { imageUrl, modelLabel }: Props = $props();

  const showStyleRef = $derived(generation.isAnima && generation.mode === "txt2img");

  function stop(e: MouseEvent) {
    e.stopPropagation();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions a11y_click_events_have_key_events -->
<div
  class="absolute right-1 top-1 z-10 flex flex-col gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity"
  onclick={stop}
>
  <button
    type="button"
    class="rounded bg-black/75 px-1.5 py-0.5 text-[9px] text-neutral-200 hover:bg-black/90 whitespace-nowrap"
    title={locale.t('model.preview_interrogate_tip')}
    onclick={() => dispatchModelPreviewAction({ type: "interrogate", imageUrl })}
  >
    {locale.t('model.preview_interrogate')}
  </button>
  <button
    type="button"
    class="rounded bg-black/75 px-1.5 py-0.5 text-[9px] text-neutral-200 hover:bg-black/90 whitespace-nowrap"
    title={locale.t('model.preview_img2img_tip')}
    onclick={() => dispatchModelPreviewAction({ type: "img2img", imageUrl })}
  >
    {locale.t('model.preview_img2img')}
  </button>
  {#if showStyleRef}
    <button
      type="button"
      class="rounded bg-black/75 px-1.5 py-0.5 text-[9px] text-neutral-200 hover:bg-black/90 whitespace-nowrap"
      title={locale.t('model.preview_style_ref_tip')}
      onclick={() => dispatchModelPreviewAction({ type: "style_reference", imageUrl })}
    >
      {locale.t('model.preview_style_ref')}
    </button>
  {/if}
</div>
