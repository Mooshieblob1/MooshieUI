<script lang="ts">
  import { music } from "../../stores/music.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { defaultMusicSampling } from "../../utils/musicSettings.js";
  const controls = [
    { key: "temperature", min: 0, max: 5, step: 0.01 }, { key: "top_p", min: 0.01, max: 1, step: 0.01 },
    { key: "top_k", min: 1, max: 32768, step: 1 }, { key: "repetition_penalty", min: 0.01, max: 10, step: 0.01 },
    { key: "max_abc_tokens", min: 1, max: 20000, step: 1 },
  ] as const;
  const input = "min-h-11 w-full rounded-md bg-neutral-900 px-3 py-2 text-sm text-neutral-200 disabled:opacity-40";
</script>

<details>
  <summary class="touch-target cursor-pointer content-center text-xs text-neutral-400">{locale.t("music.sampling")}</summary>
  {#if music.params.sampling}
    <fieldset class="space-y-3 pb-3" disabled={music.busy}>
      <p class="text-xs leading-relaxed text-neutral-500">{locale.t("music.sampling_help")}</p>
      <div class="grid grid-cols-2 gap-3">
        {#each controls as control}<label class="space-y-1 text-xs text-neutral-400"><span>{locale.t(`music.sampling_${control.key}`)}</span><input class={input} type="number" min={control.min} max={control.max} step={control.step} bind:value={music.params.sampling[control.key]} required /></label>{/each}
      </div>
      <label class="touch-target flex items-center gap-2 text-xs text-neutral-400"><input type="checkbox" checked={music.params.sampling.cfg_scale === -1} disabled={!music.capabilities?.extended} onchange={event => { if (music.params.sampling) music.params.sampling.cfg_scale = event.currentTarget.checked ? -1 : 1; }} />{locale.t("music.sampling_auto_cfg")}</label>
      {#if music.params.sampling.cfg_scale !== -1}<label class="block space-y-1 text-xs text-neutral-400"><span>{locale.t("music.sampling_cfg_scale")}</span><input class={input} type="number" min="0" max="20" step="0.01" bind:value={music.params.sampling.cfg_scale} disabled={!music.capabilities?.extended} required /></label>{/if}
      {#if !music.capabilities?.extended}<p class="text-xs text-amber-300">{locale.t("music.sampling_adapter")}</p>{/if}
      <button type="button" class="touch-target px-3 text-xs text-indigo-300" onclick={() => { music.params.sampling = defaultMusicSampling(); music.saveSettings(); }}>{locale.t("music.sampling_reset")}</button>
    </fieldset>
  {/if}
</details>
