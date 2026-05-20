<script lang="ts">
  import { connection } from "../../stores/connection.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    title: string;
    onBack?: () => void;
    rightAction?: import("svelte").Snippet;
  }
  let { title, onBack, rightAction }: Props = $props();
</script>

<header
  class="shrink-0 flex items-center gap-2 h-12 px-2 border-b border-neutral-800 bg-neutral-950/95 backdrop-blur safe-top"
>
  {#if onBack}
    <button
      type="button"
      onclick={onBack}
      class="touch-target -ml-1 px-2 text-neutral-300 hover:text-neutral-100"
      aria-label={locale.t("common.aria_back")}
    >
      <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none"
        stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="15 18 9 12 15 6"/>
      </svg>
    </button>
  {:else}
    <div class="w-8 flex items-center justify-center">
      <span
        class="w-2 h-2 rounded-full {connection.connected ? 'bg-green-500' : 'bg-red-500'}"
        aria-hidden="true"
      ></span>
    </div>
  {/if}
  <h1 class="flex-1 text-base font-semibold text-neutral-100 truncate">{title}</h1>
  <div class="flex items-center gap-1">
    {@render rightAction?.()}
  </div>
</header>
