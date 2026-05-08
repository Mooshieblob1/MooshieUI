<script lang="ts">
  import MobileTopBar from "./MobileTopBar.svelte";
  import MobileBottomSheet from "./MobileBottomSheet.svelte";
  import MobilePromptHistory from "./MobilePromptHistory.svelte";
  import MobileExtrasPanel from "./MobileExtrasPanel.svelte";
  import PromptInputs from "../generation/PromptInputs.svelte";
  import ModelSelector from "../generation/ModelSelector.svelte";
  import SamplerSettings from "../generation/SamplerSettings.svelte";
  import DimensionControls from "../generation/DimensionControls.svelte";
  import LoraGallery from "../generation/LoraGallery.svelte";
  import GenerateButton from "../generation/GenerateButton.svelte";
  import ProgressBar from "../progress/ProgressBar.svelte";
  import PreviewImage from "../progress/PreviewImage.svelte";
  import { locale } from "../../stores/locale.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";

  let sheetOpen = $state(true);
  let sheetSnap = $state<"peek" | "half" | "full">("half");

  const modes: { id: "txt2img" | "img2img" | "inpainting"; label: string }[] = [
    { id: "txt2img", label: "Text" },
    { id: "img2img", label: "Image" },
    { id: "inpainting", label: "Inpaint" },
  ];

  type Section = "prompts" | "model" | "sampler" | "dimensions" | "loras";
  let openSections = $state<Record<Section, boolean>>({
    prompts: true,
    model: false,
    sampler: false,
    dimensions: false,
    loras: false,
  });

  function toggle(s: Section) {
    openSections[s] = !openSections[s];
    if (openSections[s] && sheetSnap === "peek") sheetSnap = "half";
  }

  const sections: { id: Section; titleKey: string; titleFallback: string }[] = [
    { id: "prompts", titleKey: "generation.prompts.title", titleFallback: "Prompts" },
    { id: "model", titleKey: "generation.model.title", titleFallback: "Model" },
    { id: "sampler", titleKey: "generation.sampler.title", titleFallback: "Sampler" },
    { id: "dimensions", titleKey: "generation.dimensions.title", titleFallback: "Dimensions" },
    { id: "loras", titleKey: "generation.loras.title", titleFallback: "LoRAs" },
  ];

  function tt(key: string, fb: string) {
    const v = locale.t(key);
    return v === key ? fb : v;
  }
</script>

<div class="h-full flex flex-col bg-neutral-950 relative overflow-hidden">
  <MobileTopBar title="">
    {#snippet rightAction()}
      <div
        class="flex items-center rounded-full bg-neutral-800/80 border border-neutral-700 p-0.5 mr-1"
        role="tablist"
        aria-label={tt("generation.mode.title", "Generation mode")}
      >
        {#each modes as m}
          {@const active = generation.mode === m.id}
          <button
            type="button"
            role="tab"
            aria-selected={active}
            onclick={() => (generation.mode = m.id)}
            class="px-3 h-7 text-[11px] font-semibold rounded-full transition-colors whitespace-nowrap {active
              ? 'bg-indigo-600 text-white shadow'
              : 'text-neutral-300 hover:text-neutral-100'}"
          >
            {tt(`generation.mode.${m.id}`, m.label)}
          </button>
        {/each}
      </div>
    {/snippet}
  </MobileTopBar>

  <!-- Preview area + side icons -->
  <div class="flex-1 min-h-0 flex overflow-hidden">
    <div class="flex-1 min-w-0 flex flex-col overflow-hidden">
      <div class="px-3 pt-3 shrink-0">
        <ProgressBar />
      </div>
      <div class="shrink-0 px-3 py-3 {sheetOpen ? 'flex-1 min-h-0 overflow-y-auto no-scroll-chain' : ''}">
        <PreviewImage />
      </div>
      {#if !sheetOpen}
        <div class="flex-1 min-h-0 border-t border-neutral-800/60">
          <MobilePromptHistory />
        </div>
      {/if}
    </div>
    <MobileExtrasPanel />
  </div>

  <!-- Bottom dock: chevron toggle + (when collapsed) generate button -->
  {#if !sheetOpen}
    <div
      class="shrink-0 border-t border-neutral-800 bg-neutral-900/95 backdrop-blur-sm"
      style="padding-bottom: env(safe-area-inset-bottom, 0px);"
    >
      <button
        type="button"
        class="w-full flex items-center justify-center py-1.5 text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800/60 transition-colors"
        onclick={() => (sheetOpen = true)}
        aria-label={tt("generation.show_params", "Show parameters")}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <polyline points="18 15 12 9 6 15"/>
        </svg>
      </button>
      <div class="px-3 pb-2">
        <GenerateButton />
        {#if progress.isGenerating && progress.totalSteps > 0}
          <p class="text-[11px] text-neutral-500 mt-1.5 text-center">
            {progress.phaseLabel ?? ""} · {Math.round(progress.percentage)}%
          </p>
        {/if}
      </div>
    </div>
  {/if}

  <MobileBottomSheet
    open={sheetOpen}
    bind:snap={sheetSnap}
    snaps={["peek", "half", "full"]}
    onClose={() => (sheetOpen = false)}
    title={tt("generation.params", "Parameters")}
  >
    <div class="space-y-2">
      {#each sections as section}
        <div class="rounded-xl border border-neutral-800 bg-neutral-900/60 overflow-hidden">
          <button
            type="button"
            class="w-full touch-target flex items-center justify-between px-3 py-2.5 text-left text-sm font-medium text-neutral-100 hover:bg-neutral-800/60 transition-colors"
            onclick={() => toggle(section.id)}
            aria-expanded={openSections[section.id]}
          >
            <span>{tt(section.titleKey, section.titleFallback)}</span>
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 transition-transform {openSections[section.id] ? 'rotate-180' : ''}"
              viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="6 9 12 15 18 9"/>
            </svg>
          </button>
          {#if openSections[section.id]}
            <div class="px-3 pb-3 pt-1">
              {#if section.id === "prompts"}
                <PromptInputs />
              {:else if section.id === "model"}
                <ModelSelector />
              {:else if section.id === "sampler"}
                <SamplerSettings />
              {:else if section.id === "dimensions"}
                <DimensionControls />
              {:else if section.id === "loras"}
                <LoraGallery />
              {/if}
            </div>
          {/if}
        </div>
      {/each}

      <!-- Sticky generate button area inside sheet -->
      <div class="sticky bottom-0 -mx-4 px-4 pt-3 pb-2 bg-gradient-to-t from-neutral-900 via-neutral-900 to-transparent">
        <GenerateButton />
        {#if progress.isGenerating && progress.totalSteps > 0}
          <p class="text-[11px] text-neutral-500 mt-1.5 text-center">
            {progress.phaseLabel ?? ""} · {Math.round(progress.percentage)}%
          </p>
        {/if}
      </div>
    </div>
  </MobileBottomSheet>
</div>
