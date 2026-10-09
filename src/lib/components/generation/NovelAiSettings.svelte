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
  import NovelAiReferences from "./NovelAiReferences.svelte";
  import InfoTip from "../ui/InfoTip.svelte";
  import {
    NAI_EFFORT_HIGH,
    NAI_EFFORT_MEDIUM,
    NAI_QUALITY_LIGHT,
    NAI_QUALITY_STANDARD,
    NOVELAI_MEDIUM_EFFORT,
    effectiveNovelAiQualityPreset,
    effectiveNovelAiUcPreset,
  } from "../../utils/novelaiModels.js";

  type QualityChoice = "standard" | "light" | "none";

  // The quality label carries an InfoTip button, and a <label> names its first
  // labelable descendant, so the select is tied to it by id instead.
  const uid = $props.id();
  const qualitySelectId = `${uid}-quality`;

  const nai = $derived(generation.novelaiSettings);
  const novelAiModel = $derived(generation.novelAiModel);

  /**
   * Only the presets this model has are listed, and each dropdown shows the one
   * that will actually be sent. The stored pick is left alone, so Light or Furry
   * Focus come back when the user returns to a model that offers them.
   */
  const qualityChoices = $derived<QualityChoice[]>(
    novelAiModel?.lightQuality ? ["standard", "light", "none"] : ["standard", "none"],
  );
  const qualityChoice = $derived<QualityChoice>(
    !nai.quality_toggle
      ? "none"
      : effectiveNovelAiQualityPreset(novelAiModel, nai.quality_preset) === NAI_QUALITY_LIGHT
        ? "light"
        : "standard",
  );
  const ucPresets = $derived(novelAiModel?.ucPresets ?? []);
  const mediumEffort = $derived(generation.novelAiMediumEffort);
  // Medium sends Heavy whatever is stored, so the dropdown shows Heavy and the
  // stored pick waits for High.
  const ucPreset = $derived(
    mediumEffort
      ? NOVELAI_MEDIUM_EFFORT.ucPreset
      : effectiveNovelAiUcPreset(novelAiModel, nai.uc_preset),
  );
  const efforts = [NAI_EFFORT_MEDIUM, NAI_EFFORT_HIGH] as const;
  // Anything but "medium" is High, as on the backend.
  const effortChoice = $derived(nai.effort === NAI_EFFORT_MEDIUM ? NAI_EFFORT_MEDIUM : NAI_EFFORT_HIGH);

  /** One dropdown over two stored fields: the on/off toggle and the stack. */
  function pickQuality(choice: QualityChoice) {
    if (choice === "none") {
      generation.updateNovelAiSettings({ quality_toggle: false });
      return;
    }
    generation.updateNovelAiSettings({
      quality_toggle: true,
      quality_preset: choice === "light" ? NAI_QUALITY_LIGHT : NAI_QUALITY_STANDARD,
    });
  }
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
  <NovelAiReferences />

  <div class="border-t border-neutral-800 pt-3 space-y-2">
    <span class="text-xs text-neutral-400">
      {locale.t("generation.novelai.advanced.title")}
    </span>

    {#if novelAiModel?.mediumEffort}
      <div class="text-[11px] text-neutral-500">
        <span class="flex items-center gap-2">
          {locale.t("generation.novelai.advanced.effort")}
          <InfoTip text={locale.t("generation.novelai.advanced.effort_desc")} />
        </span>
        <div
          class="mt-1 grid grid-cols-2 gap-1 rounded-md bg-neutral-950 border border-neutral-800 p-0.5"
          role="radiogroup"
          aria-label={locale.t("generation.novelai.advanced.effort")}
        >
          {#each efforts as effort (effort)}
            {@const active = effortChoice === effort}
            <button
              type="button"
              role="radio"
              aria-checked={active}
              class="px-2 py-1 text-xs rounded transition-colors {active
                ? 'bg-indigo-600 text-white'
                : 'text-neutral-300 hover:bg-neutral-800'}"
              onclick={() => generation.updateNovelAiSettings({ effort })}
            >
              {locale.t(`generation.novelai.advanced.effort_${effort}`)}
            </button>
          {/each}
        </div>
        {#if mediumEffort}
          <p class="mt-1 text-[11px] text-neutral-500">
            {locale.t("generation.novelai.advanced.effort_medium_note")}
          </p>
        {/if}
      </div>
    {/if}

    <div class="text-[11px] text-neutral-500">
      <span class="flex items-center gap-2">
        <label for={qualitySelectId}>{locale.t("generation.novelai.advanced.quality_toggle")}</label>
        <InfoTip text={locale.t("generation.novelai.advanced.quality_toggle_desc")} />
      </span>
      <select
        id={qualitySelectId}
        class="mt-1 w-full px-2 py-1 text-xs rounded-md bg-neutral-950 border border-neutral-800 text-neutral-200 focus:outline-none focus:border-indigo-600"
        value={qualityChoice}
        onchange={(e) => pickQuality(e.currentTarget.value as QualityChoice)}
      >
        {#each qualityChoices as choice (choice)}
          <option value={choice}>
            {locale.t(`generation.novelai.advanced.quality_preset_${choice}`)}
          </option>
        {/each}
      </select>
    </div>

    <label class="block text-[11px] text-neutral-500">
      {locale.t("generation.novelai.advanced.uc_preset")}
      <select
        class="mt-1 w-full px-2 py-1 text-xs rounded-md bg-neutral-950 border border-neutral-800 text-neutral-200 focus:outline-none focus:border-indigo-600 disabled:opacity-50 disabled:cursor-not-allowed"
        disabled={mediumEffort}
        value={String(ucPreset)}
        onchange={(e) => generation.updateNovelAiSettings({ uc_preset: Number(e.currentTarget.value) })}
      >
        {#each ucPresets as preset (preset)}
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

    <!-- The medium checkpoint has no CFG rescale; NovelAI's client hides it. -->
    {#if !mediumEffort}
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
    {/if}

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
  </div>

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
