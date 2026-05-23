# Mobile Generate Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make mobile Generate automatically inherit desktop Generate features by sharing the same Generate page implementation.

**Architecture:** Replace the bespoke mobile Generate composition with a wrapper around `GenerationPage`, then add mobile-mode behavior to `GenerationPage` via a prop. Keep desktop and mobile layout persistence isolated using mode-scoped storage keys so each surface remains usable.

**Tech Stack:** Svelte 5 runes, existing MooshieUI stores/components, Tailwind classes.

---

### Task 1: Replace mobile Generate surface with shared desktop Generate page

**Files:**
- Modify: `src/lib/components/mobile/MobileGeneratePage.svelte`
- Test: manual browser-mode mobile viewport check

- [x] **Step 1: Replace imports and UI composition with shared page wrapper**

```svelte
<script lang="ts">
  import GenerationPage from "../generation/GenerationPage.svelte";
</script>

<div class="h-full min-h-0 w-full bg-neutral-950">
  <GenerationPage mobileFriendly />
</div>
```

- [x] **Step 2: Verify mobile Generate route compiles**

Run: `npm run check` (or local equivalent)  
Expected: no Svelte compile errors for `MobileGeneratePage`.

### Task 2: Add mobile-friendly mode to shared Generate page

**Files:**
- Modify: `src/lib/components/generation/GenerationPage.svelte`
- Test: manual interaction in browser-mode mobile viewport and desktop app

- [x] **Step 1: Add `mobileFriendly` prop and storage suffix**

```ts
interface Props {
  mobileFriendly?: boolean;
}
let { mobileFriendly = false }: Props = $props();

const storageSuffix = mobileFriendly ? ".mobile" : ".desktop";
const DIMENSIONS_LAYOUT_KEY = `mooshieui.generation.dimensions.layout.v1${storageSuffix}`;
const SECTION_LAYOUT_KEY = `mooshieui.generation.sections.layout.v1${storageSuffix}`;
const COLLAPSE_KEY = `mooshieui.generation.sections.collapsed.v1${storageSuffix}`;
const PANEL_LAYOUT_KEY = `mooshieui.panelLayout.v1${storageSuffix}`;
```

- [x] **Step 2: Initialize mobile-friendly collapsed panel state on mount**

```ts
if (mobileFriendly) {
  leftCollapsed = true;
  rightCollapsed = true;
  bottomCollapsed = false;
}
```

- [ ] **Step 3: Validate behavior in both modes**

Run: `npm run check`  
Expected: compile succeeds; mobile shows all desktop Generate controls through shared page; desktop layout state remains unaffected by mobile usage.
