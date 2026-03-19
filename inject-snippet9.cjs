const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');

let lastDivIndex = code.lastIndexOf('  </div>\n</div>');
if (lastDivIndex === -1) lastDivIndex = code.lastIndexOf('  </div>\r\n</div>');

if (lastDivIndex !== -1) {
    let before = code.substring(0, lastDivIndex);
    let after = code.substring(lastDivIndex);
    const injectStr = `    {@render dropZone("right", 1)}\n    {#if isDimensionsAt("right", 1)}{@render dimensionsPanel()}{/if}\n`;
    code = before + injectStr + after;
    code = code.replace("let maxRight = 4;", "let maxRight = 1;");
    fs.writeFileSync('src/lib/components/generation/GenerationPage.svelte', code);
    console.log("Injected right 1");
} else {
    console.log("Could not find bottom of right panel");
}
