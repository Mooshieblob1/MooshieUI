<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { models } from "../../stores/models.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import EditableValue from "../ui/EditableValue.svelte";

  let randomSeed = $derived(generation.seed === -1);
  const activeModelName = $derived((generation.diffusionModel || generation.checkpoint || "").toLowerCase());
  const hasAnimaRecommendation = $derived(generation.isAnima || activeModelName.includes("anima"));
  const hasSihRecommendation = $derived(activeModelName.includes("sih") || activeModelName.includes("σih"));

  function recommendedStepRange() {
    const sampler = generation.samplerName.toLowerCase();
    if (sampler.includes("euler")) return { min: 18, max: 28 };
    if (sampler.includes("dpmpp")) return { min: 24, max: 36 };
    return { min: 20, max: 30 };
  }

  function recommendedCfgRange() {
    if (isCfgPpSampler(generation.samplerName)) return { min: 1.0, max: 2.2, target: 1.4 };
    return { min: 4.0, max: 8.0, target: 6.0 };
  }

  const stepsOutOfRange = $derived(
    generation.steps < recommendedStepRange().min || generation.steps > recommendedStepRange().max
  );

  const cfgOutOfRange = $derived(
    generation.cfg < recommendedCfgRange().min || generation.cfg > recommendedCfgRange().max
  );

  const metadataUpgradedToBoth = $derived(
    generation.outputBitDepth === "16bit" && generation.metadataMode === "stealth"
  );

  const effectiveMetadataMode = $derived(
    metadataUpgradedToBoth ? "both" : generation.metadataMode
  );

  /** CFG++ samplers use an alternative guidance method that works best at low CFG (~1-2). */
  function isCfgPpSampler(name: string): boolean {
    return name.includes("cfg_pp");
  }

  function onSamplerChange() {
    if (isCfgPpSampler(generation.samplerName) && generation.cfg > 5) {
      generation.cfg = 1.4;
    }
  }

  function applyRecommendedSamplerTuning() {
    const stepRange = recommendedStepRange();
    const cfgRange = recommendedCfgRange();
    generation.steps = Math.round((stepRange.min + stepRange.max) / 2);
    generation.cfg = cfgRange.target;
  }

  function applyAnimaRecommendation() {
    generation.steps = 30;
    generation.cfg = 4.0;
    generation.samplerName = "er_sde";
    generation.scheduler = "sgm_uniform";
    // Face fix and upscale steps are 1/3 of main steps
    generation.facefixSteps = Math.ceil(30 / 3);
    generation.upscaleSteps = Math.ceil(30 / 3);
  }

  function applySihRecommendation() {
    generation.steps = 20;
    generation.cfg = 1.4;
    generation.samplerName = "euler_cfg_pp";
    generation.scheduler = "sgm_uniform";
    // Face fix and upscale steps are 1/3 of main steps
    generation.facefixSteps = Math.ceil(20 / 3);
    generation.upscaleSteps = Math.ceil(20 / 3);
  }
</script>

<div class="space-y-3">
  {#if hasAnimaRecommendation}
    <div class="rounded-lg border border-indigo-700/50 bg-indigo-900/15 p-2.5">
      <div class="flex items-start justify-between gap-2">
        <div>
          <p class="text-xs text-indigo-300 font-medium">Anima Recommended Settings</p>
          <p class="text-[11px] text-neutral-300 mt-0.5">30 steps, CFG 4, sampler `er_sde` (from Anima model card guidance).</p>
        </div>
        <button
          class="px-2 py-1 text-[10px] rounded border border-indigo-500/70 text-indigo-200 hover:border-indigo-400 hover:text-indigo-100 transition-colors"
          onclick={applyAnimaRecommendation}
        >
          Apply
        </button>
      </div>
    </div>
  {/if}

  {#if hasSihRecommendation}
    <div class="rounded-lg border border-neutral-700 bg-neutral-900/60 p-2.5">
      <div class="flex items-start justify-between gap-2">
        <div>
          <p class="text-xs text-neutral-300 font-medium">SIH Recommended Settings</p>
          <p class="text-[11px] text-neutral-400 mt-0.5">No public SIH model-card settings found; using project defaults: 20 steps, CFG 1.4, `euler_cfg_pp`, `sgm_uniform`.</p>
        </div>
        <button
          class="px-2 py-1 text-[10px] rounded border border-neutral-600 text-neutral-300 hover:border-neutral-500 hover:text-neutral-200 transition-colors"
          onclick={applySihRecommendation}
        >
          Apply
        </button>
      </div>
    </div>
  {/if}

  <!-- Sampler + Scheduler -->
  <div class="grid grid-cols-2 gap-2">
    <div>
      <label class="block text-xs text-neutral-400 mb-1">Sampler<InfoTip text="The algorithm used to progressively remove noise from the image. Different samplers produce different results — 'euler' is fast and reliable, 'dpmpp' variants offer higher quality, 'ancestral' ones add randomness for variety." /></label>
      <select
        bind:value={generation.samplerName}
        onchange={onSamplerChange}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each models.samplers as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
    </div>
    <div>
      <label class="block text-xs text-neutral-400 mb-1">Scheduler<InfoTip text="Controls how noise is distributed across steps. 'normal' is standard, 'karras' front-loads detail work for sharper results, 'sgm_uniform' spaces steps evenly." /></label>
      <select
        bind:value={generation.scheduler}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
      >
        {#each models.schedulers as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
    </div>
  </div>

  <!-- Steps + CFG side-by-side -->
  <div class="grid grid-cols-2 gap-2">
    <div>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>Steps<InfoTip text="How many denoising iterations to run. More steps = finer detail but slower. 20-30 is a good balance for most samplers. Some (like 'euler') converge fast and don't benefit much beyond 25." /></span>
        <EditableValue value={generation.steps} min={1} max={150} step={1} onchange={(v) => generation.steps = v} />
      </label>
      <input
        type="range"
        bind:value={generation.steps}
        min="1"
        max="150"
        step="1"
        class="w-full accent-indigo-500"
      />
    </div>
    <div>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>CFG<InfoTip text="Classifier-Free Guidance — how closely the AI follows your prompt. Higher = more literal but can look artificial. Lower = more creative but may ignore parts of your prompt. CFG++ samplers work best around 1-2." /></span>
        <EditableValue value={generation.cfg} min={0} max={30} step={0.1} decimals={1} onchange={(v) => generation.cfg = v} />
      </label>
      <input
        type="range"
        bind:value={generation.cfg}
        min="0"
        max="30"
        step="0.1"
        class="w-full accent-indigo-500"
      />
    </div>
  </div>
  <!-- Recommendations row -->
  <div class="flex items-center justify-between gap-2 -mt-1">
    <span class="text-[10px] {stepsOutOfRange ? 'text-amber-400' : 'text-neutral-500'} truncate">
      Steps: {recommendedStepRange().min}-{recommendedStepRange().max}
    </span>
    <span class="text-[10px] {cfgOutOfRange ? 'text-amber-400' : 'text-neutral-500'} truncate">
      CFG: {recommendedCfgRange().min.toFixed(1)}-{recommendedCfgRange().max.toFixed(1)}
    </span>
    {#if cfgOutOfRange || stepsOutOfRange}
      <button
        class="text-[10px] px-2 py-0.5 rounded border border-amber-700/70 text-amber-300 hover:border-amber-500 hover:text-amber-200 transition-colors shrink-0"
        onclick={applyRecommendedSamplerTuning}
      >
        Fix
      </button>
    {/if}
  </div>

  <!-- Seed + Batch Size -->
  <div class="grid grid-cols-2 gap-2">
    <div>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>Seed<InfoTip text="A number that determines the 'randomness' of your image. Same seed + same settings = same image. Use 'Random' for variety, or set a specific seed to reproduce or iterate on a result." /></span>
        <button
          class="text-[10px] px-1.5 py-0.5 rounded {randomSeed
            ? 'bg-indigo-600 text-white'
            : 'bg-neutral-700 text-neutral-300'} transition-colors"
          onclick={() => (generation.seed = randomSeed ? (progress.lastCompletedSeed ?? 0) : -1)}
        >
          Rng
        </button>
      </label>
      {#if !randomSeed}
        <input
          type="number"
          bind:value={generation.seed}
          min="0"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
        />
      {:else}
        <div class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-500">
          Random
        </div>
      {/if}
    </div>
    <div>
      <label class="flex items-center justify-between text-xs text-neutral-400 mb-1">
        <span>Batch<InfoTip text="How many images to generate at once. Higher values use more VRAM but let you compare results quickly. Each image uses the same prompt but a different seed." /></span>
        <EditableValue value={generation.batchSize} min={1} max={8} step={1} onchange={(v) => generation.batchSize = v} />
      </label>
      <input
        type="range"
        bind:value={generation.batchSize}
        min="1"
        max="8"
        step="1"
        class="w-full accent-indigo-500"
      />
    </div>
  </div>

  <!-- Bit Depth + Metadata -->
  <div class="grid grid-cols-2 gap-2">
    <div>
      <label class="block text-xs text-neutral-400 mb-1">Bit Depth<InfoTip text="8-bit is standard. 16-bit preserves more precision from the model's float32 output — useful if you plan to post-process in Photoshop/GIMP. Requires OpenCV in the ComfyUI environment." /></label>
      <div class="flex gap-1">
        {#each ["8bit", "16bit"] as depth}
          <button
            class="flex-1 py-1 text-[11px] rounded-lg border transition-colors {generation.outputBitDepth === depth
              ? 'bg-indigo-600/30 border-indigo-500 text-indigo-300'
              : 'bg-neutral-800/50 border-neutral-700 text-neutral-400 hover:border-neutral-600'}"
            onclick={() => generation.outputBitDepth = depth as "8bit" | "16bit"}
          >
            {depth === "8bit" ? "8-bit" : "16-bit"}
          </button>
        {/each}
      </div>
    </div>
    <div>
      <label class="block text-xs text-neutral-400 mb-1">Metadata<InfoTip text="How generation parameters are saved into images. 'Text Chunk' is standard PNG metadata (fast, widely supported). 'Stealth Alpha' hides metadata in the alpha channel and can survive social media re-uploads that strip PNG chunks. 'Both' writes to both locations. For 16-bit PNGs, selecting Stealth Alpha is automatically upgraded to Both for better compatibility." /></label>
      <div class="flex gap-1">
        {#each [["text_chunk", "Txt"], ["stealth", "Stealth"], ["both", "Both"]] as [value, label]}
          <button
            class="flex-1 py-1 text-[11px] rounded-lg border transition-colors {effectiveMetadataMode === value
              ? 'bg-indigo-600/30 border-indigo-500 text-indigo-300'
              : 'bg-neutral-800/50 border-neutral-700 text-neutral-400 hover:border-neutral-600'}"
            onclick={() => generation.metadataMode = value as "text_chunk" | "stealth" | "both"}
          >
            {label}
          </button>
        {/each}
      </div>
    </div>
  </div>
  {#if metadataUpgradedToBoth}
    <p class="text-[10px] text-indigo-300 -mt-1">
      16-bit active — Stealth Alpha upgraded to Both.
    </p>
  {/if}

</div>
