<script lang="ts">
  import MobileBottomSheet from "./MobileBottomSheet.svelte";

  export interface ActionSheetItem {
    label: string;
    icon?: import("svelte").Snippet;
    onSelect: () => void;
    destructive?: boolean;
    disabled?: boolean;
  }

  interface Props {
    open: boolean;
    title?: string;
    items: ActionSheetItem[];
    onClose?: () => void;
  }

  let { open, title, items, onClose }: Props = $props();

  function pick(item: ActionSheetItem) {
    if (item.disabled) return;
    onClose?.();
    // Defer so the close animation runs first.
    setTimeout(() => item.onSelect(), 0);
  }
</script>

<MobileBottomSheet
  {open}
  snap="peek"
  snaps={["peek"]}
  {title}
  auto
  {onClose}
>
  <ul class="flex flex-col gap-1 py-1">
    {#each items as item}
      <li>
        <button
          type="button"
          class="w-full touch-target flex items-center gap-3 px-3 py-3 rounded-lg text-left transition-colors
            {item.disabled ? 'opacity-40 cursor-not-allowed' : 'hover:bg-neutral-800 active:bg-neutral-700'}
            {item.destructive ? 'text-red-400' : 'text-neutral-200'}"
          disabled={item.disabled}
          onclick={() => pick(item)}
        >
          {#if item.icon}
            <span class="w-5 h-5 flex items-center justify-center shrink-0">
              {@render item.icon()}
            </span>
          {/if}
          <span class="text-sm font-medium">{item.label}</span>
        </button>
      </li>
    {/each}
  </ul>
</MobileBottomSheet>
