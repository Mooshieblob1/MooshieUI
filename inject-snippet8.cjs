const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');

function injectAfter(searchStr, injectStr) {
  let idx = code.indexOf(searchStr);
  if (idx !== -1) {
    let insertAt = idx + searchStr.length;
    // skip past the nearest {/if} and </div>
    let afterIdx = code.indexOf('</div>', insertAt);
    if(afterIdx !== -1) {
        insertAt = afterIdx + 6;
        code = code.substring(0, insertAt) + "\n" + injectStr + code.substring(insertAt);
        console.log("Injected for " + searchStr);
    }
  } else {
    console.log("Not found:", searchStr);
  }
}

// Left 2 (After Image Inputs wrapper div)
// Searching for the end of the generation.mode !== "txt2img" wrapper
// So we find the first occurrence of:
// `{#if generation.mode !== "txt2img"}` and then find the matching `{/if}`? So hard.
// I will just use string splits and look for specific lines.
