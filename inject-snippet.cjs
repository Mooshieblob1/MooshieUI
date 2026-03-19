const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');

const originalDimensionsDiv = `<div class="rounded-lg border border-neutral-800 bg-neutral-900/40">
      <button
        class="w-full px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors"
        onclick={() => (dimensionsSectionOpen = !dimensionsSectionOpen)}
        title={dimensionsSectionOpen ? "Collapse Dimensions" : "Expand Dimensions"}
      >
        <span class="font-medium">Dimensions</span>
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {dimensionsSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
      {#if dimensionsSectionOpen}
        <div class="px-3 pb-3 pt-1">
          <DimensionControls suggestedAspect={imageAspect} />
        </div>
      {/if}
    </div>`;

// Check if found exactly once
if (code.split(originalDimensionsDiv).length !== 2) {
  console.log("Original dimensions block not found or multiples found!");
  process.exit(1);
}

// Write the snippet at the very top of HTML
const snippet = `{#snippet dimensionsPanel()}
  <div 
    class="rounded-lg border border-neutral-800 bg-neutral-900/40 {draggingDimensions ? 'opacity-50' : 'opacity-100'}"
    draggable="true"
    ondragstart={onDimensionsDragStart}
    ondragend={onDimensionsDragEnd}
  >
    <div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
      <div class="flex items-center px-3 cursor-grab active:cursor-grabbing text-neutral-600 hover:text-neutral-400" title="Drag to move">
        <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="12" r="1"/><circle cx="9" cy="5" r="1"/><circle cx="9" cy="19" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="5" r="1"/><circle cx="15" cy="19" r="1"/></svg>
      </div>
      <button
        class="flex-1 flex items-center justify-between py-2 pr-3 text-xs text-neutral-300 hover:text-neutral-100 focus:outline-none"
        onclick={() => (dimensionsSectionOpen = !dimensionsSectionOpen)}
        title={dimensionsSectionOpen ? "Collapse Dimensions" : "Expand Dimensions"}
      >
        <span class="font-medium">Dimensions</span>
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {dimensionsSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
      </button>
    </div>
    {#if dimensionsSectionOpen}
      <div class="px-3 pb-3 pt-1">
        <DimensionControls suggestedAspect={imageAspect} />
      </div>
    {/if}
  </div>
{/snippet}

{#snippet dropZone(side, index)}
  {#if draggingDimensions && !isDimensionsAt(side, index)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="h-4 my-2 rounded-lg border-2 border-dashed border-indigo-500/50 bg-indigo-500/10 transition-colors"
      ondragover={onDimensionsDropTargetOver}
      ondrop={(e) => onDimensionsDrop(e, side, index)}
    ></div>
  {/if}
{/snippet}`;

code = code.replace("<!-- svelte-ignore a11y_no_static_element_interactions -->", snippet + "\\n<!-- svelte-ignore a11y_no_static_element_interactions -->");

// Remove original dimensions div
code = code.replace(originalDimensionsDiv, "");

console.log("Snippet injected.");
fs.writeFileSync('src/lib/components/generation/GenerationPage.svelte', code);
