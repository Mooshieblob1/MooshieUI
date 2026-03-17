<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";

  interface Props {
    suggestedAspect?: { w: number; h: number } | null;
  }
  let { suggestedAspect = null }: Props = $props();

  let aspectW = $state(1);
  let aspectH = $state(1);
  let sideLength = $state(1024);

  // When an input image is loaded, adopt its aspect ratio
  let lastAppliedKey = "";
  $effect(() => {
    if (suggestedAspect) {
      const key = `${suggestedAspect.w}:${suggestedAspect.h}`;
      if (key !== lastAppliedKey) {
        lastAppliedKey = key;
        aspectW = suggestedAspect.w;
        aspectH = suggestedAspect.h;
        recalc();
      }
    }
  });

  const presets = [
    { label: "1:1", w: 1, h: 1 },
    { label: "4:3", w: 4, h: 3 },
    { label: "3:2", w: 3, h: 2 },
    { label: "16:9", w: 16, h: 9 },
    { label: "21:9", w: 21, h: 9 },
    { label: "3:4", w: 3, h: 4 },
    { label: "2:3", w: 2, h: 3 },
    { label: "9:16", w: 9, h: 16 },
  ];

  function recalc() {
    const aw = Math.max(1, aspectW);
    const ah = Math.max(1, aspectH);
    const side = Math.max(64, sideLength);
    // Target area = side², distributed across the aspect ratio
    const area = side * side;
    const w = Math.round(Math.sqrt(area * (aw / ah)) / 8) * 8;
    const h = Math.round(Math.sqrt(area * (ah / aw)) / 8) * 8;
    generation.width = w;
    generation.height = h;
  }

  function applyPreset(w: number, h: number) {
    aspectW = w;
    aspectH = h;
    recalc();
  }

  function swapAspect() {
    const tmp = aspectW;
    aspectW = aspectH;
    aspectH = tmp;
    recalc();
  }

  const activePreset = $derived(
    presets.find((p) => p.w === aspectW && p.h === aspectH)?.label ?? ""
  );
</script>

<div class="space-y-3">
  <!-- Aspect Ratio -->
  <div>
    <label class="block text-xs text-neutral-400 mb-1.5">Aspect Ratio</label>
    <div class="flex items-center gap-1 flex-wrap mb-2">
      {#each presets as preset}
        <button
          onclick={() => applyPreset(preset.w, preset.h)}
          class="text-xs px-2 py-0.5 rounded transition-colors {activePreset === preset.label
            ? 'bg-indigo-600 text-white'
            : 'bg-neutral-800 border border-neutral-700 text-neutral-400 hover:bg-neutral-700'}"
        >
          {preset.label}
        </button>
      {/each}
    </div>
    <div class="flex items-center gap-1.5">
      <div class="flex-1">
        <span class="block text-[10px] text-neutral-500 mb-0.5">W</span>
        <input
          type="number"
          bind:value={aspectW}
          oninput={recalc}
          min="1"
          max="100"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-sm text-neutral-100 text-center focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>
      <span class="text-neutral-500 text-sm mt-4">:</span>
      <div class="flex-1">
        <span class="block text-[10px] text-neutral-500 mb-0.5">H</span>
        <input
          type="number"
          bind:value={aspectH}
          oninput={recalc}
          min="1"
          max="100"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-sm text-neutral-100 text-center focus:outline-none focus:border-indigo-500 transition-colors"
        />
      </div>
      <button
        onclick={swapAspect}
        class="text-neutral-400 hover:text-neutral-200 transition-colors shrink-0 mt-4"
        title="Swap W/H"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M7 16V4m0 0L3 8m4-4l4 4M17 8v12m0 0l4-4m-4 4l-4-4"/>
        </svg>
      </button>
    </div>
  </div>

  <!-- Side Length -->
  <div>
    <label class="block text-xs text-neutral-400 mb-1.5">Resolution<InfoTip text="The total pixel area of your image, expressed as an equivalent square side length. 1024 = ~1 megapixel. Higher resolution = more detail but slower generation and more VRAM usage." /></label>
    <input
      type="number"
      bind:value={sideLength}
      oninput={recalc}
      min="64"
      max="2048"
      step="8"
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-1.5 text-sm text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors"
    />
  </div>

  <!-- Resulting dimensions -->
  <div class="flex items-center justify-between text-xs text-neutral-400">
    <span>Result</span>
    <span class="text-neutral-200">{generation.width} &times; {generation.height}</span>
  </div>
</div>
