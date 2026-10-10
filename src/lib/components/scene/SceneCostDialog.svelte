<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { animeScene } from "../../stores/animeScene.svelte.js";

  /**
   * Itemized cost confirmation (research doc 6.6). Every paid scene step goes
   * through this; nothing spends until the user presses Confirm.
   */
  const pending = $derived(animeScene.pendingConfirm);

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") pending?.resolve(false);
  }
</script>

{#if pending}
  <div
    class="fixed inset-0 z-80 flex items-center justify-center bg-black/60 p-4"
    role="dialog"
    aria-modal="true"
    aria-labelledby="scene-cost-title"
    tabindex="-1"
    {onkeydown}
  >
    <div class="w-full max-w-md rounded-xl border border-neutral-700 bg-neutral-900 p-5 space-y-4">
      <h2 id="scene-cost-title" class="text-sm font-medium text-neutral-100">{locale.t(pending.titleKey)}</h2>
      <ul class="space-y-1.5">
        {#each pending.items as item, i (i)}
          <li class="text-xs text-neutral-300">{locale.t(item.labelKey, item.params)}</li>
        {/each}
      </ul>
      {#each pending.noteKeys as key (key)}
        <p class="text-[10px] text-neutral-500">{locale.t(key)}</p>
      {/each}
      <div class="flex justify-end gap-2">
        <button
          class="px-3 py-2 rounded-lg text-sm bg-neutral-800 hover:bg-neutral-700 text-neutral-300 transition-colors"
          onclick={() => pending.resolve(false)}
        >
          {locale.t("scene.cost.cancel")}
        </button>
        <!-- svelte-ignore a11y_autofocus -->
        <button
          class="px-3 py-2 rounded-lg text-sm bg-indigo-600 hover:bg-indigo-500 text-white transition-colors"
          onclick={() => pending.resolve(true)}
          autofocus
        >
          {locale.t("scene.cost.confirm")}
        </button>
      </div>
    </div>
  </div>
{/if}
