const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');

const normalize = (str) => str.replace(/\r\n/g, '\n').replace(/\s+/g, ' ');

const replaceFuzzy = (code, search, replace) => {
    const lines = code.split('\n');
    let searchLines = search.split('\n').map(s => s.trim()).filter(Boolean);
    
    for (let i = 0; i < lines.length; i++) {
        let match = true;
        for (let j = 0; j < searchLines.length; j++) {
            if (i + j >= lines.length || !lines[i + j].includes(searchLines[j])) {
                match = false;
                break;
            }
        }
        if (match) {
            const spaces = lines[i].match(/^\s*/)[0];
            const toInsert = replace.split('\n').map((line, idx) => {
              if (idx === 0) return line;
              return line;
            }).join('\n');
            // We just construct the replacement string and replace the block
            const blockLength = searchLines.length;
            const originalBlock = lines.slice(i, i + blockLength).join('\n');
            const newBlock = originalBlock + '\n\n' + replace; // for simple after-insertion
            
            // Wait, for BEFORE insertion (mode tabs, right panel), we need to insert BEFORE.
            return { index: i, length: blockLength, originalBlock };
        }
    }
    return null;
}

// I'll just use regex logic that tolerates carriage returns
function patch(regex, repl) {
  if (regex.test(code)) {
    code = code.replace(regex, repl);
    console.log("Patched!");
  } else {
    console.log("Failed to patch regex:", regex);
  }
}

patch(/(\s*<!-- Mode tabs -->\s*<div class="flex gap-1 bg-neutral-900 rounded-lg p-1">)/,
  `\n    {@render dropZone("left", 0)}\n    {#if isDimensionsAt("left", 0)}\n      {@render dimensionsPanel()}\n    {/if}$1`);

patch(/(<div class="px-3 pb-3 pt-1">\s*<PromptInputs \/>\s*<\/div>\s*\{\/if\}\s*<\/div>)/,
  `$1\n\n    {@render dropZone("left", 1)}\n    {#if isDimensionsAt("left", 1)}\n      {@render dimensionsPanel()}\n    {/if}`);

patch(/(<\/button>\s*\{\/if\}\s*<\/div>\s*\{\/if\}\s*<\/div>\s*\{\/if\})/,
  `$1\n\n    {@render dropZone("left", 2)}\n    {#if isDimensionsAt("left", 2)}\n      {@render dimensionsPanel()}\n    {/if}`);

patch(/(\s*<div class="sticky bottom-0 mt-auto border-t border-neutral-800 bg-neutral-950\/95)/,
  `\n\n    {@render dropZone("left", 3)}\n    {#if isDimensionsAt("left", 3)}\n      {@render dimensionsPanel()}\n    {/if}$1`);


patch(/(\s*<!-- Right panel: Generation Settings -->\s*<div\s*class="overflow-y-auto p-4 space-y-4 shrink-0"\s*style="width: \{rightWidth\}px"\s*>)/,
  `$1\n    {@render dropZone("right", 0)}\n    {#if isDimensionsAt("right", 0)}\n      {@render dimensionsPanel()}\n    {/if}`);

patch(/(<div class="px-3 pb-3 pt-1">\s*<ModelSelector \/>\s*<\/div>\s*\{\/if\}\s*<\/div>)/,
  `$1\n\n    {@render dropZone("right", 1)}\n    {#if isDimensionsAt("right", 1)}\n      {@render dimensionsPanel()}\n    {/if}`);

patch(/(<div class="px-3 pb-3 pt-1">\s*<SamplerSettings \/>\s*<\/div>\s*\{\/if\}\s*<\/div>)/,
  `$1\n\n    {@render dropZone("right", 2)}\n    {#if isDimensionsAt("right", 2)}\n      {@render dimensionsPanel()}\n    {/if}`);

patch(/(<div class="px-3 pb-3 pt-1">\s*<UpscaleSettings \/>\s*<\/div>\s*\{\/if\}\s*<\/div>)/,
  `$1\n\n    {@render dropZone("right", 3)}\n    {#if isDimensionsAt("right", 3)}\n      {@render dimensionsPanel()}\n    {/if}`);

code = code.replace(/\{#if isDimensionsAt\("left", 3\)\}\s*\{\@render dimensionsPanel\(\)\}\s*\{\/if\}\s*\{\@render dropZone\("left", 3\)\}/, ""); // clean up duplicate if any
fs.writeFileSync('src/lib/components/generation/GenerationPage.svelte', code);
