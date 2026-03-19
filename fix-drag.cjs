const fs = require('fs');
const path = 'src/lib/components/generation/GenerationPage.svelte';
let code = fs.readFileSync(path, 'utf8');

const regex = /<button[\s\S]*?class="w-full px-3 py-2 flex items-center justify-between text-xs text-neutral-300 hover:text-neutral-100 transition-colors cursor-grab active:cursor-grabbing"[\s\S]*?onclick=\{\(\) => \(dimensionsSectionOpen = !dimensionsSectionOpen\)\}[\s\S]*?title=\{dimensionsSectionOpen \? "Collapse Dimensions" : "Expand Dimensions"\}\s*>\s*<span class="font-medium">Dimensions<\/span>\s*<svg[\s\S]*?<\/svg>\s*<\/button>/g;

const replacementHeader = `<div class="flex items-stretch w-full rounded-t-lg transition-colors hover:bg-neutral-800/50">
          <div draggable="true" ondragstart={(e) => onDimensionsDragStart(e, dimensionsSide, dimensionsIndex)} class="flex items-center px-3 cursor-grab active:cursor-grabbing text-neutral-600 hover:text-neutral-400">
            <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="12" r="1"/><circle cx="9" cy="5" r="1"/><circle cx="9" cy="19" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="5" r="1"/><circle cx="15" cy="19" r="1"/></svg>
          </div>
          <button class="flex-1 flex items-center justify-between py-2 pr-3 text-xs text-neutral-300 hover:text-neutral-100 focus:outline-none" onclick={() => (dimensionsSectionOpen = !dimensionsSectionOpen)} title={dimensionsSectionOpen ? "Collapse Dimensions" : "Expand Dimensions"}>
            <span class="font-medium">Dimensions</span>
            <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5 transition-transform {dimensionsSectionOpen ? '' : '-rotate-90'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
          </button>
        </div>`;

code = code.replace(regex, replacementHeader);
fs.writeFileSync(path, code);
console.log('Drag headers modified.');
