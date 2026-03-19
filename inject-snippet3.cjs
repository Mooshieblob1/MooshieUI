const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');

const insertions = [
  // LEFT 0: Before mode tabs
  {
    find: `    <!-- Mode tabs -->\n    <div class="flex gap-1 bg-neutral-900 rounded-lg p-1">`,
    replace: `    {@render dropZone("left", 0)}\n    {#if isDimensionsAt("left", 0)}\n      {@render dimensionsPanel()}\n    {/if}\n\n    <!-- Mode tabs -->\n    <div class="flex gap-1 bg-neutral-900 rounded-lg p-1">`
  },
  // LEFT 1: After Prompts
  {
    find: `        <div class="px-3 pb-3 pt-1">\n          <PromptInputs />\n        </div>\n      {/if}\n    </div>`,
    replace: `        <div class="px-3 pb-3 pt-1">\n          <PromptInputs />\n        </div>\n      {/if}\n    </div>\n\n    {@render dropZone("left", 1)}\n    {#if isDimensionsAt("left", 1)}\n      {@render dimensionsPanel()}\n    {/if}`
  },
  // LEFT 2: After Image Inputs
  {
    find: `              </button>\n            {/if}\n          </div>\n        {/if}\n      </div>\n    {/if}`,
    replace: `              </button>\n            {/if}\n          </div>\n        {/if}\n      </div>\n    {/if}\n\n    {@render dropZone("left", 2)}\n    {#if isDimensionsAt("left", 2)}\n      {@render dimensionsPanel()}\n    {/if}`
  },
  // LEFT 3: Just above Generate (maybe max is 2 or 3?)
  {
    find: `    <div class="sticky bottom-0 mt-auto border-t border-neutral-800 bg-neutral-950/95 backdrop-blur-sm rounded-t-lg px-3 pt-3 pb-4">\n      <h3 class="text-xs text-neutral-400 mb-2 font-medium">Generate</h3>`,
    replace: `    {@render dropZone("left", 3)}\n    {#if isDimensionsAt("left", 3)}\n      {@render dimensionsPanel()}\n    {/if}\n\n    <div class="sticky bottom-0 mt-auto border-t border-neutral-800 bg-neutral-950/95 backdrop-blur-sm rounded-t-lg px-3 pt-3 pb-4">\n      <h3 class="text-xs text-neutral-400 mb-2 font-medium">Generate</h3>`
  },
  // RIGHT 0: Before Model Settings
  {
    find: `  <!-- Right panel: Generation Settings -->\n  <div\n    class="overflow-y-auto p-4 space-y-4 shrink-0"\n    style="width: {rightWidth}px"\n  >`,
    replace: `  <!-- Right panel: Generation Settings -->\n  <div\n    class="overflow-y-auto p-4 space-y-4 shrink-0"\n    style="width: {rightWidth}px"\n  >\n    {@render dropZone("right", 0)}\n    {#if isDimensionsAt("right", 0)}\n      {@render dimensionsPanel()}\n    {/if}`
  },
  // RIGHT 1: After Model Settings
  {
    find: `      {#if modelSectionOpen}\n        <div class="px-3 pb-3 pt-1">\n          <ModelSelector />\n        </div>\n      {/if}\n    </div>`,
    replace: `      {#if modelSectionOpen}\n        <div class="px-3 pb-3 pt-1">\n          <ModelSelector />\n        </div>\n      {/if}\n    </div>\n\n    {@render dropZone("right", 1)}\n    {#if isDimensionsAt("right", 1)}\n      {@render dimensionsPanel()}\n    {/if}`
  },
  // RIGHT 2: After Sampler Settings
  {
    find: `      {#if controlsSectionOpen}\n        <div class="px-3 pb-3 pt-1">\n          <SamplerSettings />\n        </div>\n      {/if}\n    </div>`,
    replace: `      {#if controlsSectionOpen}\n        <div class="px-3 pb-3 pt-1">\n          <SamplerSettings />\n        </div>\n      {/if}\n    </div>\n\n    {@render dropZone("right", 2)}\n    {#if isDimensionsAt("right", 2)}\n      {@render dimensionsPanel()}\n    {/if}`
  },
  // RIGHT 3: After Upscaling
  {
    find: `      {#if postSectionOpen}\n        <div class="px-3 pb-3 pt-1">\n          <UpscaleSettings />\n        </div>\n      {/if}\n    </div>`,
    replace: `      {#if postSectionOpen}\n        <div class="px-3 pb-3 pt-1">\n          <UpscaleSettings />\n        </div>\n      {/if}\n    </div>\n\n    {@render dropZone("right", 3)}\n    {#if isDimensionsAt("right", 3)}\n      {@render dimensionsPanel()}\n    {/if}`
  }
];

let changedCount = 0;
for (const ins of insertions) {
  if (code.includes(ins.find)) {
    code = code.replace(ins.find, ins.replace);
    changedCount++;
  } else {
    console.log("Could not find:", ins.find.slice(0, 50) + "...");
  }
}

console.log('Injected snippets at ' + changedCount + ' locations.');
fs.writeFileSync('src/lib/components/generation/GenerationPage.svelte', code);
