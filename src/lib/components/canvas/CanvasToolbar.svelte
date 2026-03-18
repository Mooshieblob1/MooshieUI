<script lang="ts">
  import { canvas, type ToolType } from "../../stores/canvas.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { canvasHistory } from "../../stores/canvasHistory.svelte.js";
  import BrushSettings from "./controls/BrushSettings.svelte";
  import ColorPicker from "./controls/ColorPicker.svelte";

  const tools: { id: ToolType; label: string; hotkey: string; icon: string }[] = [
    {
      id: "brush",
      label: "Brush",
      hotkey: "B",
      icon: `<path d="M12 19l7-7 3 3-7 7-3-3z"/><path d="M18 13l-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/><path d="M2 2l7.586 7.586"/><circle cx="11" cy="11" r="2"/>`,
    },
    {
      id: "eraser",
      label: "Eraser",
      hotkey: "E",
      icon: `<path d="M20 20H7L3 16c-.8-.8-.8-2 0-2.8L13.8 2.4c.8-.8 2-.8 2.8 0L21 6.8c.8.8.8 2 0 2.8L12 18"/>`,
    },
    {
      id: "rectFill",
      label: "Rectangle",
      hotkey: "U",
      icon: `<rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>`,
    },
    {
      id: "eyedropper",
      label: "Eyedropper",
      hotkey: "I",
      icon: `<path d="M2 22l1-1h3l9-9"/><path d="M3 21v-3l9-9"/><path d="M14.5 5.5l4-4a1.4 1.4 0 0 1 2 2l-4 4"/>`,
    },
    {
      id: "move",
      label: "Move",
      hotkey: "V",
      icon: `<polyline points="5 9 2 12 5 15"/><polyline points="9 5 12 2 15 5"/><polyline points="15 19 12 22 9 19"/><polyline points="19 9 22 12 19 15"/><line x1="2" y1="12" x2="22" y2="12"/><line x1="12" y1="2" x2="12" y2="22"/>`,
    },
    {
      id: "view",
      label: "Pan",
      hotkey: "H",
      icon: `<path d="M18 11V6a2 2 0 0 0-2-2 2 2 0 0 0-2 2"/><path d="M14 10V4a2 2 0 0 0-2-2 2 2 0 0 0-2 2v2"/><path d="M10 10.5V6a2 2 0 0 0-2-2 2 2 0 0 0-2 2v8"/><path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.9-5.7-2.4L3.4 16a2 2 0 0 1 3.2-2.4L8 15"/>`,
    },
  ];

  function handleToolClick(id: ToolType) {
    canvas.setTool(id);
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (!canvas.isPointerOverStage) return;

    // Don't trigger if typing in an input
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement) return;

    // Undo/Redo
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") {
      e.preventDefault();
      if (e.shiftKey) {
        canvasHistory.redo();
      } else {
        canvasHistory.undo();
      }
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y") {
      e.preventDefault();
      canvasHistory.redo();
      return;
    }

    // Delete key — clear active layer
    if (e.key === "Delete") {
      const layer = canvas.activeLayer;
      if (layer && !layer.locked && canvas.activeLayerId) {
        canvasHistory.snapshot(layer.id);
        canvas.clearLayer(layer.id);
      }
      return;
    }

    switch (e.key.toLowerCase()) {
      case "b": canvas.setTool("brush"); break;
      case "e": canvas.setTool("eraser"); break;
      case "u": canvas.setTool("rectFill"); break;
      case "i": canvas.setTool("eyedropper"); break;
      case "v": canvas.setTool("move"); break;
      case "h": canvas.setTool("view"); break;
      case "x": canvas.swapColors(); break;
      case "d": canvas.resetColors(); break;
      case "[": canvas.adjustBrushSize(-5); break;
      case "]": canvas.adjustBrushSize(5); break;
    }
  }
</script>

<svelte:window onkeydown={handleKeyDown} />

<div class="flex items-center gap-1 px-3 py-1.5 bg-neutral-900 border-b border-neutral-800">
  {#if generation.mode === "inpainting"}
    <div class="flex items-center gap-1 mr-2">
      <button
        onclick={() => canvas.setInpaintDrawMode("mask")}
        class="px-2 py-1 text-[10px] rounded border transition-colors {canvas.inpaintDrawMode === 'mask'
          ? 'border-indigo-500 text-indigo-300 bg-indigo-500/10'
          : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200'}"
        title="Inpaint Mask Mode"
      >
        Inpaint Mask
      </button>
      <button
        onclick={() => canvas.setInpaintDrawMode("regular")}
        class="px-2 py-1 text-[10px] rounded border transition-colors {canvas.inpaintDrawMode === 'regular'
          ? 'border-indigo-500 text-indigo-300 bg-indigo-500/10'
          : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200'}"
        title="Regular Inpaint Mode"
      >
        Regular Inpaint
      </button>
    </div>

    <div class="w-px h-6 bg-neutral-700 mr-2"></div>
  {/if}

  <!-- Tool buttons -->
  <div class="flex items-center gap-0.5">
    {#each tools as tool}
      <button
        onclick={() => handleToolClick(tool.id)}
        class="relative w-8 h-8 flex items-center justify-center rounded-md transition-colors {canvas.activeTool === tool.id
          ? 'bg-indigo-600 text-white'
          : 'text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800'}"
        title="{tool.label} ({tool.hotkey})"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          {@html tool.icon}
        </svg>
      </button>
    {/each}
  </div>

  <div class="w-px h-6 bg-neutral-700 mx-2"></div>

  <BrushSettings />

  <div class="w-px h-6 bg-neutral-700 mx-2"></div>

  <ColorPicker />

  <div class="w-px h-6 bg-neutral-700 mx-2"></div>

  <div class="flex items-center gap-1">
    <button
      onclick={() => canvasHistory.undo()}
      disabled={!canvasHistory.canUndo}
      class="px-2 py-1 text-[10px] rounded border transition-colors {canvasHistory.canUndo
        ? 'border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300'
        : 'border-neutral-800 text-neutral-600 cursor-not-allowed'}"
      title="Undo (Ctrl+Z)"
    >
      Undo
    </button>
    <button
      onclick={() => canvasHistory.redo()}
      disabled={!canvasHistory.canRedo}
      class="px-2 py-1 text-[10px] rounded border transition-colors {canvasHistory.canRedo
        ? 'border-neutral-700 text-neutral-300 hover:border-indigo-500 hover:text-indigo-300'
        : 'border-neutral-800 text-neutral-600 cursor-not-allowed'}"
      title="Redo (Ctrl+Shift+Z / Ctrl+Y)"
    >
      Redo
    </button>
  </div>

  <div class="w-px h-6 bg-neutral-700 mx-2"></div>

  <!-- Zoom controls -->
  <div class="flex items-center gap-1">
    <button
      onclick={() => canvas.zoomOut()}
      class="w-6 h-6 flex items-center justify-center rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 text-sm"
      title="Zoom out"
    >
      -
    </button>
    <span class="text-xs text-neutral-300 tabular-nums w-10 text-center">{canvas.zoomPercent}%</span>
    <button
      onclick={() => canvas.zoomIn()}
      class="w-6 h-6 flex items-center justify-center rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 text-sm"
      title="Zoom in"
    >
      +
    </button>
    <button
      onclick={() => canvas.resetZoom()}
      class="text-[10px] px-1.5 py-0.5 rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800"
      title="Reset zoom"
    >
      1:1
    </button>
  </div>
</div>
