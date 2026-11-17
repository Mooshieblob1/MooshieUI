<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { h3UserLoraChoices } from "../../utils/h3Models.js";
  import { scrollCapture } from "../../utils/scrollCapture.js";
  import EditableValue from "../ui/EditableValue.svelte";
  import InfoTip from "../ui/InfoTip.svelte";

  // Same ranges as the image LoRA sliders: typed values may go past the
  // slider, which stretches to fit them.
  const STRENGTH_MIN = -10;
  const STRENGTH_MAX = 10;
  const SLIDER_MIN = 0;
  const SLIDER_MAX = 2;
  const SLIDER_STEP = 0.05;

  /** Detected H3 LoRAs, without the adapters the sampling mode loads itself. */
  const choices = $derived(
    h3UserLoraChoices(models.h3VideoLoras, generation.videoTurboEnabled ? generation.videoTurboLora : null),
  );
  const activeCount = $derived(generation.videoLoras.filter((l) => l.enabled && l.name).length);

  function displayName(name: string): string {
    return (name.split(/[\\/]/).pop() ?? name).replace(/\.safetensors$/i, "");
  }

  function add() {
    // Start on the first H3 LoRA not already in the stack, so one click is enough.
    const used = new Set(generation.videoLoras.map((l) => l.name));
    generation.addVideoLora(choices.find((name) => !used.has(name)) ?? "");
    generation.saveSettings();
  }

  function update(index: number, patch: { name?: string; strength?: number; enabled?: boolean }) {
    generation.updateVideoLora(index, patch);
    generation.saveSettings();
  }

  function setStrength(index: number, value: number) {
    if (!Number.isFinite(value)) return;
    update(index, { strength: Math.round(value * 100) / 100 });
  }

  function remove(index: number) {
    generation.removeVideoLora(index);
    generation.saveSettings();
  }
</script>

<div class="space-y-2">
  <div class="flex items-center justify-between">
    <div class="flex items-center gap-2">
      <span class="text-xs text-neutral-400">
        {locale.t("generation.video.loras")}<InfoTip text={locale.t("generation.video.loras_tip")} />
      </span>
      {#if activeCount > 0}
        <span class="text-[10px] px-1.5 py-0.5 rounded-full bg-indigo-600/20 text-indigo-400 tabular-nums">{activeCount}</span>
      {/if}
    </div>
    <button
      type="button"
      onclick={add}
      class="min-h-8 px-1 text-xs text-indigo-400 hover:text-indigo-300 transition-colors"
    >
      {locale.t("generation.video.loras_add")}
    </button>
  </div>

  {#if choices.length === 0 && generation.videoLoras.length === 0}
    <p class="text-[11px] text-neutral-500">{locale.t("generation.video.loras_none")}</p>
  {/if}

  {#each generation.videoLoras as lora, i (i)}
    {@const listed = lora.name === "" || choices.includes(lora.name)}
    {@const missing = lora.name !== "" && models.loras.length > 0 && !models.loras.includes(lora.name)}
    <div
      class="rounded-lg border p-2.5 space-y-2 transition-opacity {lora.enabled
        ? 'bg-neutral-800 border-neutral-700'
        : 'bg-neutral-800/50 border-neutral-700/50 opacity-50'}"
    >
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="relative w-8 h-4 rounded-full transition-colors shrink-0 {lora.enabled ? 'bg-indigo-600' : 'bg-neutral-700'}"
          onclick={() => update(i, { enabled: !lora.enabled })}
          role="switch"
          aria-checked={lora.enabled}
          title={lora.enabled ? locale.t("generation.lora.disable") : locale.t("generation.lora.enable")}
          aria-label={lora.enabled ? locale.t("generation.lora.disable") : locale.t("generation.lora.enable")}
        >
          <span class="absolute top-0.5 left-0.5 w-3 h-3 rounded-full bg-white transition-transform {lora.enabled ? 'translate-x-4' : ''}"></span>
        </button>
        <select
          value={lora.name}
          onchange={(e) => update(i, { name: (e.currentTarget as HTMLSelectElement).value })}
          aria-label={locale.t("generation.video.loras_select")}
          class="flex-1 min-w-0 bg-neutral-900 border border-neutral-700 rounded px-2 py-1 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
        >
          {#if lora.name === ""}
            <option value="" disabled>{locale.t("generation.video.loras_select")}</option>
          {/if}
          {#if !listed}
            <option value={lora.name}>
              {missing ? locale.t("generation.video.loras_not_found", { name: displayName(lora.name) }) : displayName(lora.name)}
            </option>
          {/if}
          {#each choices as name (name)}
            <option value={name}>{displayName(name)}</option>
          {/each}
        </select>
        <button
          type="button"
          onclick={() => remove(i)}
          class="text-neutral-500 hover:text-red-400 transition-colors text-sm leading-none shrink-0 px-1"
          title={locale.t("common.remove")}
          aria-label={locale.t("common.remove")}
        >
          &times;
        </button>
      </div>
      {#if lora.name}
        <div use:scrollCapture>
          <div class="flex items-center justify-between text-xs mb-0.5">
            <span class="text-neutral-500">{locale.t("generation.model.lora_strength")}</span>
            <EditableValue
              value={lora.strength_model}
              min={STRENGTH_MIN}
              max={STRENGTH_MAX}
              step={0.01}
              decimals={2}
              onchange={(v) => setStrength(i, v)}
            />
          </div>
          <input
            type="range"
            value={lora.strength_model}
            oninput={(e) => setStrength(i, Number((e.currentTarget as HTMLInputElement).value))}
            ondblclick={() => setStrength(i, 1)}
            min={Math.min(SLIDER_MIN, Math.floor(lora.strength_model))}
            max={Math.max(SLIDER_MAX, Math.ceil(lora.strength_model))}
            step={SLIDER_STEP}
            title={locale.t("generation.model.lora_strength_reset")}
            aria-label={locale.t("generation.model.lora_strength")}
            class="w-full accent-indigo-500"
          />
        </div>
      {/if}
    </div>
  {/each}
</div>
