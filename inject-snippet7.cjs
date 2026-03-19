const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');

const insertions = [
  {
    after: `<ModelSelector />\n        </div>\n      {/if}\n    </div>`,
    text: `\n\n    {@render dropZone("right", 1)}\n    {#if isDimensionsAt("right", 1)}\n      {@render dimensionsPanel()}\n    {/if}`
  },
  {
    after: `<SamplerSettings />\n        </div>\n      {/if}\n    </div>`,
    text: `\n\n    {@render dropZone("right", 2)}\n    {#if isDimensionsAt("right", 2)}\n      {@render dimensionsPanel()}\n    {/if}`
  },
  {
    after: `<UpscaleSettings />\n        </div>\n      {/if}\n    </div>`,
    text: `\n\n    {@render dropZone("right", 3)}\n    {#if isDimensionsAt("right", 3)}\n      {@render dimensionsPanel()}\n    {/if}`
  }
];

insertions.forEach(ins => {
  const normCode = code.replace(/\r\n/g, '\n');
  const normAfter = ins.after.replace(/\n+^\s*/gm, '').replace(/\s+/g, '');
  
  // Find substring by stripping whitespace
  // Wait, the easier way is just simple string replace for small chunks.
});
