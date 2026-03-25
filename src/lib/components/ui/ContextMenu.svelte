<script lang="ts">
  import type { Snippet } from "svelte";

  export interface ContextMenuItem {
    label: string;
    icon?: Snippet;
    action: () => void;
    separator?: boolean;
    destructive?: boolean;
  }

  interface Props {
    items: ContextMenuItem[];
    x: number;
    y: number;
    visible: boolean;
    onclose: () => void;
  }

  let { items, x, y, visible, onclose }: Props = $props();

  let menuEl: HTMLDivElement | undefined = $state();

  // Clamp position to viewport
  const clampedX = $derived.by(() => {
    if (!menuEl) return x;
    const w = menuEl.offsetWidth;
    return Math.min(x, window.innerWidth - w - 8);
  });
  const clampedY = $derived.by(() => {
    if (!menuEl) return y;
    const h = menuEl.offsetHeight;
    return Math.min(y, window.innerHeight - h - 8);
  });

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }

  function handleClickOutside(e: MouseEvent) {
    if (menuEl && !menuEl.contains(e.target as Node)) {
      onclose();
    }
  }

  $effect(() => {
    if (visible) {
      document.addEventListener("click", handleClickOutside, true);
      document.addEventListener("keydown", handleKeydown);
      window.addEventListener("scroll", onclose, true);
      window.addEventListener("blur", onclose);
      return () => {
        document.removeEventListener("click", handleClickOutside, true);
        document.removeEventListener("keydown", handleKeydown);
        window.removeEventListener("scroll", onclose, true);
        window.removeEventListener("blur", onclose);
      };
    }
  });
</script>

{#if visible}
  <div
    bind:this={menuEl}
    class="fixed z-[100] min-w-[180px] bg-neutral-800 border border-neutral-700 rounded-lg shadow-xl py-1 select-none"
    style="left: {clampedX}px; top: {clampedY}px;"
    role="menu"
  >
    {#each items as item}
      {#if item.separator}
        <div class="h-px bg-neutral-700 my-1"></div>
      {/if}
      <button
        class="w-full px-3 py-1.5 text-left text-sm flex items-center gap-2 transition-colors {item.destructive ? 'text-red-400 hover:bg-red-500/10' : 'text-neutral-200 hover:bg-neutral-700'}"
        onclick={() => { item.action(); onclose(); }}
        role="menuitem"
      >
        {#if item.icon}
          <span class="w-4 h-4 flex items-center justify-center shrink-0">
            {@render item.icon()}
          </span>
        {/if}
        {item.label}
      </button>
    {/each}
  </div>
{/if}
