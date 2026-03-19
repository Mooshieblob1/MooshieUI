const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');

const r = (find, replace) => {
    if(code.includes(find)) {
        code = code.replace(find, replace);
        console.log("Replaced!");
    } else {
        console.log("Could not find:", find.substring(0, 50));
    }
}

r(`<!-- Right panel: Model & Sampler Settings -->
  <div
    class="overflow-y-auto p-4 space-y-4 shrink-0"
    style="width: {rightWidth}px"
  >`, `<!-- Right panel: Model & Sampler Settings -->
  <div
    class="overflow-y-auto p-4 space-y-4 shrink-0"
    style="width: {rightWidth}px"
  >
    {@render dropZone("right", 0)}
    {#if isDimensionsAt("right", 0)}{@render dimensionsPanel()}{/if}`);

r(`        <div class="px-3 pb-3 pt-1">
          <ModelSelector />
        </div>
      {/if}
    </div>`, `        <div class="px-3 pb-3 pt-1">
          <ModelSelector />
        </div>
      {/if}
    </div>
    {@render dropZone("right", 1)}
    {#if isDimensionsAt("right", 1)}{@render dimensionsPanel()}{/if}`);

r(`        <div class="px-3 pb-3 pt-1">
          <SamplerSettings />
        </div>
      {/if}
    </div>`, `        <div class="px-3 pb-3 pt-1">
          <SamplerSettings />
        </div>
      {/if}
    </div>
    {@render dropZone("right", 2)}
    {#if isDimensionsAt("right", 2)}{@render dimensionsPanel()}{/if}`);

r(`        <div class="px-3 pb-3 pt-1">
          <UpscaleSettings />
        </div>
      {/if}
    </div>`, `        <div class="px-3 pb-3 pt-1">
          <UpscaleSettings />
        </div>
      {/if}
    </div>
    {@render dropZone("right", 3)}
    {#if isDimensionsAt("right", 3)}{@render dimensionsPanel()}{/if}`);

fs.writeFileSync('src/lib/components/generation/GenerationPage.svelte', code);
