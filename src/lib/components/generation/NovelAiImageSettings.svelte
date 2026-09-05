<script lang="ts">
  /**
   * NovelAI's img2img controls: strength and noise, plus the inpainting
   * paste-back switch. Rendered directly under the image upload, in the slot
   * the local denoise slider occupies for ComfyUI, so the controls live next
   * to the image they act on rather than in the NovelAI panel.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";

  const nai = $derived(generation.novelaiSettings);
</script>

<div class="space-y-2">
  <span class="text-xs text-neutral-400">{locale.t("generation.novelai.image.title")}</span>

  <label class="block text-[11px] text-neutral-500">
    {locale.t("generation.novelai.image.strength")}
    {nai.strength.toFixed(2)}
    <input
      type="range"
      min="0.01"
      max="0.99"
      step="0.01"
      class="w-full accent-indigo-500"
      value={nai.strength}
      oninput={(e) => generation.updateNovelAiSettings({ strength: Number(e.currentTarget.value) })}
    />
  </label>

  <label class="block text-[11px] text-neutral-500">
    {locale.t("generation.novelai.image.noise")}
    {nai.noise.toFixed(2)}
    <input
      type="range"
      min="0"
      max="1"
      step="0.01"
      class="w-full accent-indigo-500"
      value={nai.noise}
      oninput={(e) => generation.updateNovelAiSettings({ noise: Number(e.currentTarget.value) })}
    />
  </label>

  {#if generation.mode === "inpainting"}
    <label class="flex items-center gap-2 text-xs text-neutral-300">
      <input
        type="checkbox"
        class="accent-indigo-500"
        checked={nai.add_original_image}
        onchange={(e) =>
          generation.updateNovelAiSettings({ add_original_image: e.currentTarget.checked })}
      />
      {locale.t("generation.novelai.image.add_original")}
      <InfoTip text={locale.t("generation.novelai.image.add_original_desc")} />
    </label>
  {/if}
</div>
