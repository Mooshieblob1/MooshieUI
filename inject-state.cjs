const fs = require('fs');
let code = fs.readFileSync('src/lib/components/generation/GenerationPage.svelte', 'utf8');
code = code.replace('let promptsSectionOpen = $state(true);', `let promptsSectionOpen = $state(true);
  
  // Dimensions Drag Drop State
  let dimensionsSide = $state('left');
  let dimensionsIndex = $state(1);
  let draggingDimensions = $state(false);

  function clampDimensionsPlacement() {
    let maxLeft = generation.mode === 'txt2img' ? 2 : 3;
    let maxRight = 4;
    if (dimensionsSide === 'left' && dimensionsIndex > maxLeft) dimensionsIndex = maxLeft;
    if (dimensionsSide === 'right' && dimensionsIndex > maxRight) dimensionsIndex = maxRight;
  }
  
  $effect(() => {
    generation.mode; // reactive tracking
    clampDimensionsPlacement();
  });

  function onDimensionsDragStart(e) {
    draggingDimensions = true;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      // Required for Firefox
      e.dataTransfer.setData('text/plain', 'dimensions');
      // Set drag image to a transparent pixel if needed, but let's just let it use the element itself!
    }
  }

  function onDimensionsDragEnd(e) {
    draggingDimensions = false;
  }

  function onDimensionsDropTargetOver(e) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
  }

  function onDimensionsDrop(e, side, index) {
    e.preventDefault();
    draggingDimensions = false;
    dimensionsSide = side;
    dimensionsIndex = index;
    clampDimensionsPlacement();
  }

  function isDimensionsAt(side, index) {
    return dimensionsSide === side && dimensionsIndex === index;
  }
`);
console.log('State added.');
fs.writeFileSync('src/lib/components/generation/GenerationPage.svelte', code);
