const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');

code = code.replace(/<!-- Right panel: Model & Sampler Settings -->[\n\r\s]*<div[\n\r\s]*class="overflow-y-auto p-4 space-y-4 shrink-0"[\n\r\s]*style="width: \{rightWidth\}px"[\n\r\s]*>/, 
`$&
    {@render dropZone("right", 0)}
    {#if isDimensionsAt("right", 0)}{@render dimensionsPanel()}{/if}`);

code = code.replace(/<div class="px-3 pb-3 pt-1">[\n\r\s]*<ModelSelector \/>[\n\r\s]*<\/div>[\n\r\s]*\{\/if\}[\n\r\s]*<\/div>/g,
`$&
    {@render dropZone("right", 1)}
    {#if isDimensionsAt("right", 1)}{@render dimensionsPanel()}{/if}`);

code = code.replace(/<div class="px-3 pb-3 pt-1">[\n\r\s]*<SamplerSettings \/>[\n\r\s]*<\/div>[\n\r\s]*\{\/if\}[\n\r\s]*<\/div>/g,
`$&
    {@render dropZone("right", 2)}
    {#if isDimensionsAt("right", 2)}{@render dimensionsPanel()}{/if}`);

code = code.replace(/<div class="px-3 pb-3 pt-1">[\n\r\s]*<UpscaleSettings \/>[\n\r\s]*<\/div>[\n\r\s]*\{\/if\}[\n\r\s]*<\/div>/g,
`$&
    {@render dropZone("right", 3)}
    {#if isDimensionsAt("right", 3)}{@render dimensionsPanel()}{/if}`);

fs.writeFileSync('src/lib/components/generation/GenerationPage.svelte', code);
console.log('Patched right side!');