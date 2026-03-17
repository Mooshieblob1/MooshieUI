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

  // Panel widths (px)
  let leftWidth = $state(360);
  let rightWidth = $state(300);

  const LEFT_MIN = 280;
  const LEFT_MAX = 520;
  const RIGHT_MIN = 250;
  const RIGHT_MAX = 450;

  let dragging = $state<"left" | "right" | null>(null);
  let dragStartX = 0;
  let dragStartWidth = 0;

  function onDividerDown(side: "left" | "right", e: MouseEvent) {
    dragging = side;
    dragStartX = e.clientX;
    dragStartWidth = side === "left" ? leftWidth : rightWidth;
    e.preventDefault();
  }

  function onPointerMove(e: MouseEvent) {
    if (!dragging) return;
    const delta = e.clientX - dragStartX;
    if (dragging === "left") {
      leftWidth = Math.min(LEFT_MAX, Math.max(LEFT_MIN, dragStartWidth + delta));
    } else {
      rightWidth = Math.min(RIGHT_MAX, Math.max(RIGHT_MIN, dragStartWidth - delta));
    }
  }

  function onPointerUp() {
    dragging = null;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="flex h-full select-none"
  onmousemove={onPointerMove}
  onmouseup={onPointerUp}
  onmouseleave={onPointerUp}
>
  <!-- Left panel: Image Settings -->
  <div
    class="overflow-y-auto p-4 flex flex-col gap-4 shrink-0"
    style="width: {leftWidth}px"
  >
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
      <DimensionControls />
    </div>

    {#if generation.mode !== "txt2img"}
      <div class="border-t border-neutral-800 pt-4">
        <div class="bg-neutral-800 border border-neutral-700 rounded-lg p-4 text-center text-sm text-neutral-500">
          Image upload — coming soon
        </div>
      </div>
    {/if}

    <div class="mt-auto pt-4">
      <GenerateButton />
    </div>
  </div>

  <!-- Left divider -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="w-1 shrink-0 cursor-col-resize hover:bg-indigo-500/40 transition-colors {dragging === 'left' ? 'bg-indigo-500/60' : 'bg-neutral-800'}"
    onmousedown={(e) => onDividerDown("left", e)}
  ></div>

  <!-- Center panel: Preview & Output -->
  <div class="flex-1 min-w-0 p-6 flex flex-col gap-4 overflow-y-auto">
    <ProgressBar />
    <PreviewImage />
  </div>

  <!-- Right divider -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="w-1 shrink-0 cursor-col-resize hover:bg-indigo-500/40 transition-colors {dragging === 'right' ? 'bg-indigo-500/60' : 'bg-neutral-800'}"
    onmousedown={(e) => onDividerDown("right", e)}
  ></div>

  <!-- Right panel: Model & Sampler Settings -->
  <div
    class="overflow-y-auto p-4 space-y-4 shrink-0"
    style="width: {rightWidth}px"
  >
    <ModelSelector />

    <div class="border-t border-neutral-800 pt-4">
      <SamplerSettings />
    </div>

    <div class="border-t border-neutral-800 pt-4">
      <UpscaleSettings />
    </div>
  </div>
</div>
