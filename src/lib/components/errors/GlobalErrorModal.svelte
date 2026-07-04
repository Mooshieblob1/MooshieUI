<script lang="ts">
  import { errorModal } from "../../stores/errorModal.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import ErrorCard from "./ErrorCard.svelte";
</script>

{#if errorModal.current}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
    onclick={(e) => { if (e.target === e.currentTarget) errorModal.close(); }}
    onkeydown={(e) => { if (e.key === "Escape") errorModal.close(); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="w-full max-w-lg space-y-3">
      <ErrorCard error={errorModal.current} />
      <div class="flex justify-end">
        <button
          onclick={() => errorModal.close()}
          class="rounded-lg bg-neutral-800 px-4 py-2 text-sm text-neutral-300 hover:bg-neutral-700"
        >
          {locale.t("common.close")}
        </button>
      </div>
    </div>
  </div>
{/if}
