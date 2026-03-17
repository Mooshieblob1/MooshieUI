---
applyTo: "src/lib/components/**"
---

# Svelte Component Conventions

## Styling

- **Tailwind CSS utility classes only.** Do NOT add `<style>` blocks to components.
- Dark theme only: use `neutral-950`/`900`/`800`/`700` for backgrounds/borders, `neutral-400`/`300`/`50` for text.
- Accent color: `bg-indigo-600`, `hover:bg-indigo-500` for primary actions.
- Dynamic classes via ternary in the `class` attribute:
  ```svelte
  class="... {isActive ? 'bg-indigo-600' : 'bg-neutral-800'}"
  ```
- Dynamic inline styles when needed (e.g., progress bars):
  ```svelte
  style="width: {progress.percentage}%"
  ```
- Hover effects use Tailwind `group`/`group-hover:` pattern for reveal-on-hover UI.

## Store Consumption

- Import store singletons directly — no props drilling:
  ```typescript
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  ```
- Read state directly in markup: `{progress.currentStep}`, `{generation.checkpoint}`.
- Two-way bind to store fields: `bind:value={generation.positivePrompt}`.
- Call store methods directly: `generation.saveSettings()`, `progress.startGeneration(id)`.
- Use `$derived` for component-local computed values:
  ```typescript
  const canGenerate = $derived(!!generation.checkpoint);
  ```
- Use `$state` for component-local state only (not for shared state — that goes in stores).

## Event Handling

- Use Svelte 5 event attributes directly on elements: `onclick`, `oninput`, `onchange`.
- Do NOT use legacy `on:click` directive syntax.
- Inline handlers for simple mutations:
  ```svelte
  <button onclick={() => (currentPage = "gallery")}>Gallery</button>
  ```
- Named functions for async or multi-step logic:
  ```typescript
  async function handleGenerate() {
    const params = generation.toParams();
    const promptId = await generate(params);
    progress.startGeneration(promptId);
    generation.saveSettings();
  }
  ```

## Tauri Integration

- Components do NOT call `invoke()` directly. Use wrappers from `../../utils/api.js`.
- Tauri event listeners (`listen()`) belong in `App.svelte`, which updates stores. Components read stores, not events.

## Component Structure

- File naming: `PascalCase.svelte`.
- Organize by feature domain: `generation/`, `progress/`, `gallery/`, `setup/`.
- No props passing between components — read shared state from stores.
- `$state` and `$derived` in the `<script>` block for local reactive state.
- Template uses `{#if}`, `{#each}`, `{:else}` blocks for conditional/list rendering.

## Patterns Reference

```svelte
<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { someApiCall } from "../../utils/api.js";

  let localRef: HTMLElement;
  let localState = $state(false);
  const computed = $derived(generation.steps > 10);

  async function handleAction() {
    await someApiCall(generation.toParams());
    generation.saveSettings();
  }
</script>

{#if progress.isGenerating}
  <div class="flex items-center gap-2 text-neutral-400">
    Step {progress.currentStep} / {progress.totalSteps}
  </div>
{:else}
  <button onclick={handleAction} class="bg-indigo-600 hover:bg-indigo-500 rounded-xl px-4 py-2">
    Go
  </button>
{/if}
```
