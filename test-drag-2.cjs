const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');

const snippetSearchRegex = /\{#snippet dimensionsPanel\(\)\}\s*<div\s*class="rounded-lg border border-neutral-800 bg-neutral-900\/40 \{draggingDimensions \? 'opacity-50' : 'opacity-100'\}"\s*draggable="true"\s*ondragstart=\{onDimensionsDragStart\}\s*ondragend=\{onDimensionsDragEnd\}\s*>\s*<div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800\/50">\s*<div class="flex items-center px-3 cursor-grab active:cursor-grabbing text-neutral-600 hover:text-neutral-400" title="Drag to move">/;

code = code.replace(snippetSearchRegex, `{#snippet dimensionsPanel()}
  <div 
    bind:this={dimensionsPanelRef}
    class="rounded-lg border border-neutral-800 bg-neutral-900/40 {draggingDimensions ? 'opacity-50' : 'opacity-100'}"
  >
    <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div 
        draggable="true"
        ondragstart={onDimensionsDragStart}
        ondragend={onDimensionsDragEnd}
        class="flex items-center px-3 cursor-grab active:cursor-grabbing text-neutral-600 hover:text-medium hover:text-neutral-400"
        title="Drag to move"
      >`);

// Also add dimensionsPanelRef state
code = code.replace("let draggingDimensions = $state(false);", "let draggingDimensions = $state(false);\n  let dimensionsPanelRef = $state<HTMLElement | null>(null);");

// And update the dragStart function:
const dragStartSearch = `  function onDimensionsDragStart(e: DragEvent) {
    draggingDimensions = true;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      // Required for Firefox
      e.dataTransfer.setData('text/plain', 'dimensions');
      // Set drag image to a transparent pixel if needed, but let's just let it use the element itself!
    }
  }`;

const dragStartReplace = `  function onDimensionsDragStart(e: DragEvent) {
    draggingDimensions = true;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', 'dimensions');
      if (dimensionsPanelRef) {
        // Set the drag image to the parent container so we drag the whole card visually
        e.dataTransfer.setDragImage(dimensionsPanelRef, 20, 20);
      }
    }
  }`;

// handle different TS annotations or lacks thereof:
const looseRegex = /function onDimensionsDragStart\([^\)]*\)\s*\{\s*draggingDimensions\s*=\s*true;\s*if\s*\(e\.dataTransfer\)\s*\{[^}]*?[^}]*?[^}]*?\}\s*\}/;
code = code.replace(looseRegex, dragStartReplace);

fs.writeFileSync('src/lib/components/generation/GenerationPage.svelte', code);
console.log('Regex worked?', code.includes('bind:this={dimensionsPanelRef}'));
