<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import {
    animaCameraData as data, animaCameraLogic as logic, normalizeAnimaCamera,
    type CameraAxis, type CameraGroup, type ExtraSelection,
  } from "../../utils/animaCamera.js";
  import type { createScene } from "./animaCameraScene.js";
  import InfoTip from "../ui/InfoTip.svelte";

  const id = $props.id();
  const prefix = "generation.anima_camera";
  const axes = [
    { key: "horizontal_angle", min: 0, max: 360, step: 1 },
    { key: "vertical_angle", min: -30, max: 60, step: 1 },
    { key: "zoom", min: 0, max: 10, step: 0.1 },
  ] as const;
  let expanded = $state(false);
  let extrasExpanded = $state(false);
  let viewport = $state<HTMLDivElement>();
  let scene = $state.raw<ReturnType<typeof createScene> | null>(null);
  let unavailable = $state(false);

  const t = (key: string) => locale.t(`${prefix}.${key}`);
  const inputClass = "min-w-0 rounded-md border border-neutral-700 bg-neutral-900 px-2 py-1.5 text-xs text-neutral-300 focus:border-indigo-500 focus:outline-none";

  function setAxis(key: CameraAxis, value: number) {
    generation.animaCamera = normalizeAnimaCamera({ ...generation.animaCamera, [key]: value });
  }

  function setExtra(key: CameraGroup, patch: Partial<ExtraSelection>) {
    const current = generation.animaCamera;
    generation.animaCamera = normalizeAnimaCamera({
      ...current, extras: { ...current.extras, [key]: { ...current.extras[key], ...patch } },
    });
    generation.saveSettings();
  }

  $effect(() => {
    if (!expanded || !viewport) return;
    const host = viewport;
    let disposed = false;
    let mounted: ReturnType<typeof createScene> | null = null;
    unavailable = false;
    import("./animaCameraScene.js").then(({ createScene }) => {
      if (disposed) return;
      mounted = createScene(host, () => generation.animaCamera, setAxis,
        () => generation.saveSettings(),
        () => {
          unavailable = true;
          mounted?.dispose();
          mounted = null;
          scene = null;
        });
      scene = mounted;
    }).catch((error) => {
      if (disposed) return;
      console.warn("Anima camera guide unavailable:", error);
      unavailable = true;
    });
    return () => {
      disposed = true;
      mounted?.dispose();
      scene = null;
    };
  });

  $effect(() => { scene?.update(generation.animaCamera); });
</script>

<div class="mt-2 rounded-lg border border-neutral-700/60 bg-neutral-800/30">
  <div class="flex items-center gap-2 px-3 py-1.5">
    <button type="button" class="touch-target flex min-w-0 flex-1 items-center gap-2 text-left text-xs text-neutral-300 hover:text-indigo-300"
      aria-expanded={expanded} aria-controls={`${id}-controls`} onclick={() => expanded = !expanded}>
      <span aria-hidden="true">{expanded ? "▾" : "▸"}</span>{t("title")}
    </button>
    <InfoTip text={t("tip")} />
    <label class="touch-target flex items-center gap-1.5 text-xs text-neutral-400">
      <input type="checkbox" class="accent-indigo-500" checked={generation.animaCameraEnabled}
        onchange={(e) => {
          generation.animaCameraEnabled = e.currentTarget.checked;
          if (generation.animaCameraEnabled) expanded = true;
          generation.saveSettings();
        }} />
      {t("enabled")}
    </label>
  </div>
  {#if expanded}
    <div id={`${id}-controls`} class="space-y-3 border-t border-neutral-700/60 p-3">
      <p class="text-[10px] text-neutral-500">{t("guide_tip")}</p>
      <div class="relative h-44 overflow-hidden rounded-md border border-neutral-700 bg-neutral-950" bind:this={viewport}>
        {#if unavailable}
          <p class="absolute inset-0 flex items-center justify-center px-4 text-center text-xs text-neutral-400" role="status">{t("unavailable")}</p>
        {/if}
      </div>
      <div class="grid grid-cols-1 gap-2 sm:grid-cols-3">
        {#each axes as axis}
          <div class="min-w-0 space-y-1">
            <label for={`${id}-${axis.key}-preset`} class="block text-[10px] text-neutral-400">{t(axis.key)}</label>
            <select id={`${id}-${axis.key}-preset`} class={`w-full ${inputClass}`}
              value={logic.closestPreset(axis.key, generation.animaCamera[axis.key]).value}
              onchange={(e) => { setAxis(axis.key, Number(e.currentTarget.value)); generation.saveSettings(); }}>
              {#each data.camera_presets[axis.key] as preset, i}
                <option value={preset.value}>{t(`presets.${axis.key}_${i}`)}</option>
              {/each}
            </select>
            <input id={`${id}-${axis.key}`} type="number" class={`w-full ${inputClass}`}
              aria-label={t(axis.key)} min={axis.min} max={axis.max} step={axis.step}
              value={generation.animaCamera[axis.key]}
              onchange={(e) => { setAxis(axis.key, e.currentTarget.valueAsNumber); generation.saveSettings(); }} />
          </div>
        {/each}
      </div>
      <div class="flex items-center justify-between">
        <label class="touch-target flex items-center gap-2 text-xs text-neutral-300">
          <input type="checkbox" class="accent-indigo-500" checked={generation.animaCamera.roll > 0}
            onchange={(e) => {
              generation.animaCamera = { ...generation.animaCamera, roll: e.currentTarget.checked ? 1 : 0 };
              generation.saveSettings();
            }} />
          {t("dutch")}
        </label>
        <button type="button" class="touch-target text-[11px] text-neutral-400 hover:text-indigo-300"
          onclick={() => { generation.animaCamera = normalizeAnimaCamera(); generation.saveSettings(); }}>{t("reset")}</button>
      </div>
      <button type="button" class="touch-target flex w-full items-center gap-2 text-xs text-neutral-400 hover:text-neutral-200"
        aria-expanded={extrasExpanded} aria-controls={`${id}-extras`} onclick={() => extrasExpanded = !extrasExpanded}>
        <span aria-hidden="true">{extrasExpanded ? "▾" : "▸"}</span>{t("extras")}
      </button>
      {#if extrasExpanded}
        <div id={`${id}-extras`} class="space-y-3 border-l border-neutral-700 pl-3">
          <div class="space-y-1">
            <label for={`${id}-extreme`} class="block text-[10px] text-neutral-400">{t("extreme")}</label>
            <div class="flex gap-2">
              <select id={`${id}-extreme`} class={`flex-1 ${inputClass}`} value={generation.animaCamera.extremePreset}
                onchange={(e) => {
                  generation.animaCamera = { ...generation.animaCamera, extremePreset: e.currentTarget.value };
                  generation.saveSettings();
                }}>
                {#each data.extreme_presets as preset, i}
                  <option value={preset.label}>{i === 0 ? t("none") : t(`presets.extreme_${i}`)}</option>
                {/each}
              </select>
              {#if generation.animaCamera.extremePreset !== "None"}
                <input type="number" min="0.1" max="50" step="0.1" class={`w-20 ${inputClass}`}
                  aria-label={locale.t(`${prefix}.group_weight`, { group: t("extreme") })} value={generation.animaCamera.extremeWeight}
                  onchange={(e) => {
                    generation.animaCamera = normalizeAnimaCamera({ ...generation.animaCamera, extremeWeight: e.currentTarget.valueAsNumber });
                    generation.saveSettings();
                  }} />
              {/if}
            </div>
            {#if generation.animaCamera.extremePreset !== "None"}
              <p class="text-[10px] text-amber-400/80">{t("extreme_tip")}</p>
            {/if}
          </div>
          {#each data.extra_groups as group}
            {@const key = group.key as CameraGroup}
            {@const extra = generation.animaCamera.extras[key]}
            <div class="space-y-1">
              <label for={`${id}-${key}`} class="block text-[10px] text-neutral-400">{t(key)}</label>
              <div class="flex gap-2">
                <select id={`${id}-${key}`} class={`flex-1 ${inputClass}`} value={extra.preset}
                  onchange={(e) => setExtra(key, { preset: e.currentTarget.value })}>
                  <option value="None">{t("none")}</option>
                  <option value="Custom">{t("custom")}</option>
                  {#each group.presets as preset, i}
                    <option value={preset.label}>{t(`presets.${key}_${i}`)}</option>
                  {/each}
                </select>
                {#if extra.preset !== "None"}
                  <input type="number" min="0.1" max="10" step="0.1" class={`w-20 ${inputClass}`}
                    aria-label={locale.t(`${prefix}.group_weight`, { group: t(key) })} value={extra.weight}
                    onchange={(e) => setExtra(key, { weight: e.currentTarget.valueAsNumber })} />
                {/if}
              </div>
              {#if extra.preset === "Custom"}
                <input type="text" class={`w-full ${inputClass}`} aria-label={locale.t(`${prefix}.group_custom`, { group: t(key) })}
                  placeholder={t("custom_placeholder")} value={extra.custom}
                  onchange={(e) => setExtra(key, { custom: e.currentTarget.value })} />
              {/if}
            </div>
          {/each}
        </div>
      {/if}
      <div class="space-y-1">
        <label for={`${id}-preview`} class="block text-[10px] text-neutral-400">{t("preview")}</label>
        <textarea id={`${id}-preview`} readonly rows="3" value={generation.animaCameraPrompt}
          class={`w-full resize-y ${inputClass}`}></textarea>
      </div>
    </div>
  {/if}
</div>
