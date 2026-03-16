<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import PromptInputs from "./PromptInputs.svelte";
  import ModelSelector from "./ModelSelector.svelte";
  import SamplerSettings from "./SamplerSettings.svelte";
  import DimensionControls from "./DimensionControls.svelte";
  import GenerateButton from "./GenerateButton.svelte";
  import UpscaleSettings from "./UpscaleSettings.svelte";
  import ProgressBar from "../progress/ProgressBar.svelte";
  import PreviewImage from "../progress/PreviewImage.svelte";

  const modes = [
    { id: "txt2img" as const, label: "Text to Image" },
    { id: "img2img" as const, label: "Image to Image" },
    { id: "inpainting" as const, label: "Inpainting" },
  ];
</script>

<div class="flex h-full">
  <!-- Left panel: controls -->
  <div class="w-[380px] min-w-[340px] border-r border-neutral-800 overflow-y-auto p-4 space-y-4">
    <!-- Mode tabs -->
    <div class="flex gap-1 bg-neutral-900 rounded-lg p-1">
      {#each modes as mode}
        <button
          onclick={() => (generation.mode = mode.id)}
          class="flex-1 text-xs py-1.5 rounded-md transition-colors {generation.mode ===
          mode.id
            ? 'bg-neutral-700 text-white'
            : 'text-neutral-400 hover:text-neutral-200'}"
        >
          {mode.label}
        </button>
      {/each}
    </div>

    <PromptInputs />

    <div class="border-t border-neutral-800 pt-4">
      <ModelSelector />
    </div>

    <div class="border-t border-neutral-800 pt-4">
      <SamplerSettings />
    </div>

    <div class="border-t border-neutral-800 pt-4">
      <DimensionControls />
    </div>

    <div class="border-t border-neutral-800 pt-4">
      <UpscaleSettings />
    </div>

    {#if generation.mode !== "txt2img"}
      <div class="border-t border-neutral-800 pt-4">
        <div class="bg-neutral-800 border border-neutral-700 rounded-lg p-4 text-center text-sm text-neutral-500">
          Image upload — coming soon
        </div>
      </div>
    {/if}

    <div class="pt-2">
      <GenerateButton />
    </div>
  </div>

  <!-- Right panel: preview & output -->
  <div class="flex-1 p-6 flex flex-col gap-4 overflow-y-auto">
    <ProgressBar />
    <PreviewImage />
  </div>
</div>
