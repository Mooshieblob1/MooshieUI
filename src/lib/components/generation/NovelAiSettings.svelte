<script lang="ts">
  /**
   * The NovelAI panel: everything NovelAI owns that ComfyUI has no equivalent
   * for, plus the characters and reference sub-panels.
   *
   * Sampler, steps and guidance are deliberately absent: those are top-level
   * generation params shared with ComfyUI, and they stay in the sampler panel
   * so switching backends does not move them.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import NovelAiCharacters from "./NovelAiCharacters.svelte";
  import NovelAiReferences from "./NovelAiReferences.svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  const UC_PRESETS = [0, 1, 2, 3];

  const nai = $derived(generation.novelaiSettings);
  const isImageMode = $derived(generation.mode === "img2img" || generation.mode === "inpainting");
  const postProcessArmed = $derived(generation.upscaleEnabled || generation.facefixEnabled);

  /**
   * The local pass loads the image through ComfyUI's `LoadImage`, whose IMAGE
   * output is RGB, so it would flatten the alpha the user paid V5 for. The
   * backend skips the pass rather than destroy the transparency; this says so
   * before the Anlas is spent.
   */
  const transparencyActive = $derived(
    nai.transparent_background && generation.supportsNovelAiTransparency,
  );

  /**
   * The local model picker spans two folders, so the option value carries the
   * folder with it. A colon is safe as the separator: no filesystem this runs
   * on allows one in a file name, and only the first is split on so a
   * subfolder path survives intact.
   */
  const localModelValue = $derived(
    nai.local_checkpoint
      ? `${nai.local_model_category ?? "checkpoints"}:${nai.local_checkpoint}`
      : "",
  );

  /**
   * A split-file model is only loadable once its text encoder and VAE have both
   * been resolved. Selection fills them from the ModelSpec recommendation, and
   * that recommendation is deliberately omitted rather than substituted when
   * nothing installed is compatible, so an empty slot here means the companion
   * file is missing from disk.
   */
  const splitCompanionsMissing = $derived(
    !!nai.local_checkpoint &&
      nai.local_use_split_model &&
      (!nai.local_clip_model?.trim() || !nai.local_vae?.trim()),
  );

  function pickLocalModel(value: string) {
    if (!value) {
      generation.setNovelAiLocalCheckpoint(null);
      return;
    }
    const split = value.indexOf(":");
    generation.setNovelAiLocalCheckpoint(
      value.slice(split + 1),
      value.slice(0, split),
      models.textEncoders,
      models.vaes,
    );
  }
</script>

<div class="space-y-4">
  <NovelAiCharacters />

  <div class="border-t border-neutral-800 pt-3">
    <NovelAiReferences />
  </div>

  <div class="border-t border-neutral-800 pt-3 space-y-2">
    <span class="text-xs text-neutral-400">
      {locale.t("generation.novelai.advanced.title")}
    </span>

    <label class="flex items-center gap-2 text-xs text-neutral-300">
      <input
        type="checkbox"
        class="accent-indigo-500"
        checked={nai.quality_toggle}
        onchange={(e) => generation.updateNovelAiSettings({ quality_toggle: e.currentTarget.checked })}
      />
      {locale.t("generation.novelai.advanced.quality_toggle")}
      <InfoTip text={locale.t("generation.novelai.advanced.quality_toggle_desc")} />
    </label>

    <label class="block text-[11px] text-neutral-500">
      {locale.t("generation.novelai.advanced.uc_preset")}
      <select
        class="mt-1 w-full px-2 py-1 text-xs rounded-md bg-neutral-950 border border-neutral-800 text-neutral-200 focus:outline-none focus:border-indigo-600"
        value={String(nai.uc_preset)}
        onchange={(e) => generation.updateNovelAiSettings({ uc_preset: Number(e.currentTarget.value) })}
      >
        {#each UC_PRESETS as preset (preset)}
          <option value={String(preset)}>
            {locale.t(`generation.novelai.advanced.uc_preset_${preset}`)}
          </option>
        {/each}
      </select>
    </label>

    <label class="flex items-center gap-2 text-xs text-neutral-300">
      <input
        type="checkbox"
        class="accent-indigo-500"
        checked={nai.legacy_uc}
        onchange={(e) => generation.updateNovelAiSettings({ legacy_uc: e.currentTarget.checked })}
      />
      {locale.t("generation.novelai.advanced.legacy_uc")}
      <InfoTip text={locale.t("generation.novelai.advanced.legacy_uc_desc")} />
    </label>

    <label class="flex items-center gap-2 text-xs text-neutral-300">
      <input
        type="checkbox"
        class="accent-indigo-500"
        checked={nai.variety_plus}
        onchange={(e) => generation.updateNovelAiSettings({ variety_plus: e.currentTarget.checked })}
      />
      {locale.t("generation.novelai.advanced.variety_plus")}
      <InfoTip text={locale.t("generation.novelai.advanced.variety_plus_desc")} />
    </label>

    {#if generation.supportsNovelAiTransparency}
      <label class="flex items-center gap-2 text-xs text-neutral-300">
        <input
          type="checkbox"
          class="accent-indigo-500"
          checked={nai.transparent_background}
          onchange={(e) =>
            generation.updateNovelAiSettings({ transparent_background: e.currentTarget.checked })}
        />
        {locale.t("generation.novelai.advanced.transparent_background")}
        <InfoTip text={locale.t("generation.novelai.advanced.transparent_background_desc")} />
      </label>
    {/if}

    <label class="flex items-center gap-2 text-xs text-neutral-300">
      <input
        type="checkbox"
        class="accent-indigo-500"
        checked={nai.dynamic_thresholding}
        onchange={(e) =>
          generation.updateNovelAiSettings({ dynamic_thresholding: e.currentTarget.checked })}
      />
      {locale.t("generation.novelai.advanced.dynamic_thresholding")}
      <InfoTip text={locale.t("generation.novelai.advanced.dynamic_thresholding_desc")} />
    </label>

    <label class="block text-[11px] text-neutral-500">
      {locale.t("generation.novelai.advanced.cfg_rescale")}
      {nai.cfg_rescale.toFixed(2)}
      <input
        type="range"
        min="0"
        max="1"
        step="0.02"
        class="w-full accent-indigo-500"
        value={nai.cfg_rescale}
        oninput={(e) =>
          generation.updateNovelAiSettings({ cfg_rescale: Number(e.currentTarget.value) })}
      />
    </label>

    <label class="block text-[11px] text-neutral-500">
      {locale.t("generation.novelai.advanced.uncond_scale")}
      {nai.uncond_scale.toFixed(2)}
      <input
        type="range"
        min="0"
        max="1.5"
        step="0.05"
        class="w-full accent-indigo-500"
        value={nai.uncond_scale}
        oninput={(e) =>
          generation.updateNovelAiSettings({ uncond_scale: Number(e.currentTarget.value) })}
      />
    </label>
    {#if nai.uncond_scale !== 1}
      <p class="text-[11px] text-amber-400/90">
        {locale.t("generation.novelai.advanced.uncond_cost")}
      </p>
    {/if}
  </div>

  {#if isImageMode}
    <div class="border-t border-neutral-800 pt-3 space-y-2">
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
          oninput={(e) =>
            generation.updateNovelAiSettings({ strength: Number(e.currentTarget.value) })}
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
  {/if}

  <div class="border-t border-neutral-800 pt-3 space-y-2">
    <label class="flex items-center gap-2 text-xs text-neutral-300">
      <input
        type="checkbox"
        class="accent-indigo-500"
        checked={nai.local_post_process}
        onchange={(e) =>
          generation.updateNovelAiSettings({ local_post_process: e.currentTarget.checked })}
      />
      {locale.t("generation.novelai.local.title")}
      <InfoTip text={locale.t("generation.novelai.local.desc")} />
    </label>

    {#if nai.local_post_process}
      <label class="block text-[11px] text-neutral-500">
        {locale.t("generation.novelai.local.checkpoint")}
        <select
          class="mt-1 w-full px-2 py-1 text-xs rounded-md bg-neutral-950 border border-neutral-800 text-neutral-200 focus:outline-none focus:border-indigo-600"
          value={localModelValue}
          onchange={(e) => pickLocalModel(e.currentTarget.value)}
        >
          <option value="">{locale.t("generation.novelai.local.checkpoint_none")}</option>
          {#if models.checkpoints.length > 0}
            <optgroup label={locale.t("generation.novelai.local.group_checkpoints")}>
              {#each models.checkpoints as name (name)}
                <option value={`checkpoints:${name}`}>{name}</option>
              {/each}
            </optgroup>
          {/if}
          {#if models.diffusionModels.length > 0}
            <optgroup label={locale.t("generation.novelai.local.group_diffusion")}>
              {#each models.diffusionModels as name (name)}
                <option value={`diffusion_models:${name}`}>{name}</option>
              {/each}
            </optgroup>
          {/if}
        </select>
      </label>

      {#if nai.local_checkpoint && nai.local_use_split_model}
        {#if splitCompanionsMissing}
          <p class="text-[11px] text-amber-400/90">
            {locale.t("generation.novelai.local.split_missing")}
          </p>
        {:else}
          <p class="text-[11px] text-neutral-500">
            {locale.t("generation.novelai.local.auto_split", {
              clip: nai.local_clip_model ?? "",
              vae: nai.local_vae ?? "",
            })}
          </p>
        {/if}
      {/if}

      {#if nai.local_checkpoint && nai.local_sampler}
        <p class="text-[11px] text-neutral-500">
          {locale.t("generation.novelai.local.sampling", {
            sampler: nai.local_sampler,
            scheduler: nai.local_scheduler ?? "",
            cfg: (nai.local_cfg ?? 0).toFixed(1),
          })}
        </p>
      {/if}

      {#if !nai.local_checkpoint}
        <p class="text-[11px] text-amber-400/90">
          {locale.t("generation.novelai.local.checkpoint_required")}
        </p>
      {:else if !postProcessArmed}
        <p class="text-[11px] text-amber-400/90">
          {locale.t("generation.novelai.local.nothing_to_do")}
        </p>
      {:else if transparencyActive}
        <p class="text-[11px] text-amber-400/90">
          {locale.t("generation.novelai.local.transparency_conflict")}
        </p>
      {/if}
    {/if}
  </div>
</div>
