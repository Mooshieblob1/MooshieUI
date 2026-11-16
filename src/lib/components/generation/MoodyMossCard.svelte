<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import { civitaiModelPageUrl } from "../../utils/civitaiUrl.js";
  import { downloadModel } from "../../utils/api.js";
  import {
    MOODY_MOSS_HD_SCALE,
    MOODY_MOSS_MODEL_ID,
    MOODY_MOSS_WORKFLOW_ID,
  } from "../../utils/samplingRecommendation.js";
  import {
    MOODY_MOSS_LONG_EDGES,
    downloadSizeGb,
    missingMoodyMossFiles,
    moodyMossLongEdgeSize,
    moodyMossRequiredFiles,
    type MoodyMossSettings,
  } from "../../utils/moodyMoss.js";

  let { onApply }: { onApply: () => void } = $props();

  let open = $state(true);
  let downloading = $state(false);
  let downloadError = $state<string | null>(null);

  const s = $derived(generation.moodyMoss);
  const txt2img = $derived(generation.mode === "txt2img");

  function set<K extends keyof MoodyMossSettings>(key: K, value: MoodyMossSettings[K], save = true) {
    generation.moodyMoss = { ...generation.moodyMoss, [key]: value };
    if (save) generation.saveSettings();
  }

  /**
   * Size of the Moody Moss second pass. `LatentUpscaleBy` scales the Flux 2
   * latent (1/16 of the image) and rounds, so the image grows in 16 px steps.
   */
  function hdSize(px: number): number {
    return Math.round(Math.floor(px / 16) * MOODY_MOSS_HD_SCALE) * 16;
  }

  const output = $derived.by(() => {
    const hd = generation.moodyMossHdActive;
    let width = hd ? hdSize(generation.width) : generation.width;
    let height = hd ? hdSize(generation.height) : generation.height;
    // SeedVR2 and the plain resize scale by a fixed factor; a model upscale
    // depends on the model, so its size is not guessed here.
    if (
      generation.upscaleEnabled &&
      (generation.upscaleMethod === "seedvr2" || generation.upscaleMethod === "algorithmic")
    ) {
      width = Math.round(width * generation.upscaleScale);
      height = Math.round(height * generation.upscaleScale);
    }
    if (txt2img && s.seedvr2) ({ width, height } = moodyMossLongEdgeSize(width, height, s.longEdge));
    return { width, height };
  });

  const missing = $derived(missingMoodyMossFiles(moodyMossRequiredFiles(s), models));

  async function downloadMissing() {
    downloading = true;
    downloadError = null;
    try {
      for (const file of missing) {
        await downloadModel(file.url, file.category, file.filename, undefined, file.sha256);
      }
      await models.refresh();
    } catch (e) {
      downloadError = String(e);
    } finally {
      downloading = false;
    }
  }

  const loraOptions = $derived(models.loras);
</script>

{#snippet toggle(on: boolean, label: string, onclick: () => void)}
  <button
    class="relative w-10 h-5 rounded-full transition-colors shrink-0 {on ? 'bg-indigo-600' : 'bg-neutral-700'}"
    {onclick}
    role="switch"
    aria-checked={on}
    aria-label={label}
  >
    <span class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {on ? 'translate-x-5' : ''}"></span>
  </button>
{/snippet}

{#snippet row(label: string, tip: string, on: boolean, onclick: () => void, wrongMode: string | null)}
  <div class="flex items-center justify-between gap-2">
    <span class="text-[11px] text-neutral-300">
      {label}<InfoTip text={tip} />
      {#if on && wrongMode}
        <span class="ml-1 text-[10px] text-amber-300/90">{wrongMode}</span>
      {/if}
    </span>
    {@render toggle(on, label, onclick)}
  </div>
{/snippet}

{#snippet slider(label: string, value: number, min: number, max: number, step: number, onchange: (v: number) => void)}
  <div use:scrollCapture>
    <div class="flex items-center justify-between text-[11px] text-neutral-400 mb-0.5">
      <span>{label}</span>
      <EditableValue {value} {min} {max} {step} decimals={2} {onchange} />
    </div>
    <input
      type="range"
      {min}
      {max}
      {step}
      {value}
      oninput={(e) => onchange(Number((e.currentTarget as HTMLInputElement).value))}
      class="w-full accent-indigo-500"
    />
  </div>
{/snippet}

{#snippet detailer(prefix: "face" | "area")}
  {@const promptKey = prefix === "face" ? "facePrompt" : "areaPrompt"}
  {@const loraKey = prefix === "face" ? "faceLora" : "areaLora"}
  {@const strengthKey = prefix === "face" ? "faceLoraStrength" : "areaLoraStrength"}
  {@const denoiseKey = prefix === "face" ? "faceDenoise" : "areaDenoise"}
  <div class="pl-2 border-l border-indigo-700/40 space-y-1.5">
    <input
      type="text"
      value={s[promptKey]}
      oninput={(e) => set(promptKey, (e.currentTarget as HTMLInputElement).value, false)}
      onchange={() => generation.saveSettings()}
      placeholder={locale.t("generation.sampler.moody_moss_prompt_placeholder")}
      class="w-full bg-neutral-800 border border-neutral-700 rounded px-2 py-1 text-[11px] text-neutral-200 placeholder-neutral-500 focus:outline-none focus:border-indigo-500"
    />
    <div class="flex items-center gap-1.5">
      <select
        value={s[loraKey]}
        onchange={(e) => set(loraKey, (e.currentTarget as HTMLSelectElement).value)}
        aria-label={locale.t("generation.sampler.moody_moss_lora")}
        class="min-w-0 flex-1 bg-neutral-800 border border-neutral-700 rounded px-1.5 py-1 text-[11px] text-neutral-200 focus:outline-none focus:border-indigo-500"
      >
        <option value="">{locale.t("generation.sampler.moody_moss_no_lora")}</option>
        {#each loraOptions as lora (lora)}
          <option value={lora}>{lora}</option>
        {/each}
        {#if s[loraKey] && !loraOptions.includes(s[loraKey])}
          <option value={s[loraKey]}>{s[loraKey]}</option>
        {/if}
      </select>
      {#if s[loraKey]}
        <EditableValue value={s[strengthKey]} min={-2} max={2} step={0.05} decimals={2} onchange={(v) => set(strengthKey, v)} />
      {/if}
    </div>
    {@render slider(locale.t("generation.sampler.moody_moss_denoise"), s[denoiseKey], 0.05, 1, 0.01, (v) => set(denoiseKey, v))}
  </div>
{/snippet}

<div class="rounded-lg border border-indigo-700/50 bg-indigo-900/15 overflow-hidden">
  <button class="w-full flex items-center justify-between px-2.5 py-2 text-left" onclick={() => (open = !open)}>
    <p class="text-xs text-indigo-300 font-medium">{locale.t("generation.sampler.moody_moss_recommended")}</p>
    <svg class="w-3 h-3 text-indigo-400 shrink-0 transition-transform {open ? '' : '-rotate-90'}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
  </button>
  {#if open}
    <div class="flex items-start justify-between gap-2 px-2.5 pb-2.5">
      <p class="text-[11px] text-neutral-300">{locale.t("generation.sampler.moody_moss_hint")}</p>
      <button
        class="shrink-0 px-2 py-1 text-[10px] rounded border border-indigo-500/70 text-indigo-200 hover:border-indigo-400 hover:text-indigo-100 transition-colors"
        onclick={onApply}
      >
        {locale.t("common.apply")}
      </button>
    </div>
    <div class="px-2.5 pb-2.5 space-y-2">
      {@render row(
        locale.t("generation.sampler.moody_moss_hd_toggle"),
        locale.t("generation.sampler.moody_moss_hd_tip"),
        generation.moodyMossHd,
        () => {
          generation.moodyMossHd = !generation.moodyMossHd;
          generation.saveSettings();
        },
        txt2img ? null : locale.t("generation.sampler.moody_moss_txt2img_only"),
      )}

      {@render row(
        locale.t("generation.sampler.moody_moss_face"),
        locale.t("generation.sampler.moody_moss_face_tip"),
        s.faceDetail,
        () => set("faceDetail", !s.faceDetail),
        txt2img ? null : locale.t("generation.sampler.moody_moss_txt2img_only"),
      )}
      {#if s.faceDetail}
        {@render detailer("face")}
      {/if}

      {@render row(
        locale.t("generation.sampler.moody_moss_area"),
        locale.t("generation.sampler.moody_moss_area_tip"),
        s.areaDetail,
        () => set("areaDetail", !s.areaDetail),
        generation.mode === "inpainting" ? null : locale.t("generation.sampler.moody_moss_inpainting_only"),
      )}
      {#if s.areaDetail}
        {@render detailer("area")}
      {/if}

      {@render row(
        locale.t("generation.sampler.moody_moss_skin"),
        locale.t("generation.sampler.moody_moss_skin_tip"),
        s.skinContrast,
        () => set("skinContrast", !s.skinContrast),
        txt2img ? null : locale.t("generation.sampler.moody_moss_txt2img_only"),
      )}
      {#if s.skinContrast}
        <div class="pl-2 border-l border-indigo-700/40">
          {@render slider(locale.t("generation.sampler.moody_moss_strength"), s.skinContrastStrength, 0, 1, 0.01, (v) => set("skinContrastStrength", v))}
        </div>
      {/if}

      {@render row(
        locale.t("generation.sampler.moody_moss_seedvr2_final"),
        locale.t("generation.sampler.moody_moss_seedvr2_final_tip"),
        s.seedvr2,
        () => set("seedvr2", !s.seedvr2),
        txt2img ? null : locale.t("generation.sampler.moody_moss_txt2img_only"),
      )}
      {#if s.seedvr2}
        <div class="pl-2 border-l border-indigo-700/40 space-y-1.5">
          <div class="flex items-center justify-between gap-2 text-[11px] text-neutral-400">
            <span>{locale.t("generation.sampler.moody_moss_model")}</span>
            <div class="flex rounded border border-neutral-700 overflow-hidden">
              {#each [false, true] as large (large)}
                <button
                  class="px-2 py-0.5 text-[10px] transition-colors {s.seedvr2Large === large
                    ? 'bg-indigo-600 text-white'
                    : 'text-neutral-400 hover:text-neutral-200'}"
                  onclick={() => set("seedvr2Large", large)}
                >{large ? "7B" : "3B"}</button>
              {/each}
            </div>
          </div>
          <div class="flex items-center justify-between gap-2 text-[11px] text-neutral-400">
            <span>{locale.t("generation.sampler.moody_moss_long_edge")}</span>
            <div class="flex rounded border border-neutral-700 overflow-hidden">
              {#each MOODY_MOSS_LONG_EDGES as edge (edge)}
                <button
                  class="px-2 py-0.5 text-[10px] tabular-nums transition-colors {s.longEdge === edge
                    ? 'bg-indigo-600 text-white'
                    : 'text-neutral-400 hover:text-neutral-200'}"
                  onclick={() => set("longEdge", edge)}
                >{edge}</button>
              {/each}
            </div>
          </div>
          {@render slider(locale.t("generation.sampler.moody_moss_mix"), s.seedvr2Mix, 0, 1, 0.01, (v) => set("seedvr2Mix", v))}
        </div>
      {/if}

      {#if missing.length > 0}
        <button
          class="px-2 py-1 text-[10px] rounded border border-neutral-600 text-neutral-300 hover:border-indigo-400 hover:text-indigo-100 transition-colors disabled:opacity-50"
          onclick={downloadMissing}
          disabled={downloading}
        >
          {locale.t("generation.sampler.moody_moss_download", { size: downloadSizeGb(missing) })}
        </button>
      {/if}
      {#if downloadError}
        <p class="text-[10px] text-red-400 break-words">{downloadError}</p>
      {/if}

      <p class="text-[11px] text-neutral-400 tabular-nums">
        {locale.t("generation.sampler.moody_moss_output", { width: output.width, height: output.height })}
      </p>
      <p class="text-[10px] text-neutral-500">
        <a
          href={civitaiModelPageUrl(MOODY_MOSS_MODEL_ID, true)}
          target="_blank"
          rel="noopener noreferrer"
          class="hover:text-indigo-300 underline decoration-dotted"
        >{locale.t("generation.sampler.moody_moss_model_link")}</a>
        ·
        <a
          href={civitaiModelPageUrl(MOODY_MOSS_WORKFLOW_ID, true)}
          target="_blank"
          rel="noopener noreferrer"
          class="hover:text-indigo-300 underline decoration-dotted"
        >{locale.t("generation.sampler.moody_moss_workflow_link")}</a>
      </p>
    </div>
  {/if}
</div>
