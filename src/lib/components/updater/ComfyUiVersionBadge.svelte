<script lang="ts">
  import { comfyuiUpdate } from "../../stores/comfyuiUpdate.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  const outdated = $derived(comfyuiUpdate.updateAvailable);
  const installedLabel = $derived(
    comfyuiUpdate.installed ?? locale.t("settings.performance.comfyui_unknown"),
  );
</script>

{#if comfyuiUpdate.installed}
  <div class="relative mb-1">
    {#if comfyuiUpdate.showBubble}
      <!--
        Anchored left rather than centred: the sidebar is only 56px wide, so a
        centred bubble would hang off the left edge of the window. Nothing in
        the sidebar column clips overflow, so z-50 is enough to float it over
        the main panel.
      -->
      <div
        class="absolute bottom-full left-0 z-50 mb-2.5 w-56 rounded-lg border border-amber-700/60 bg-neutral-900 p-3 text-left shadow-xl shadow-black/40"
        role="dialog"
        aria-label={locale.t("comfyui_update.title")}
      >
        <div class="flex items-start gap-2">
          <p class="flex-1 text-[11px] font-semibold text-amber-300">
            {locale.t("comfyui_update.title")}
          </p>
          <button
            class="-mr-1 -mt-1 shrink-0 rounded p-1 text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-neutral-300 disabled:opacity-50"
            onclick={() => comfyuiUpdate.dismissBubble()}
            disabled={comfyuiUpdate.updating}
            title={locale.t("comfyui_update.dismiss")}
            aria-label={locale.t("comfyui_update.dismiss")}
          >
            <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <p class="mt-1 font-mono text-[11px] text-neutral-300">
          {installedLabel} <span class="text-neutral-500">&rarr;</span>
          <span class="text-amber-300">{comfyuiUpdate.target}</span>
        </p>

        {#if comfyuiUpdate.updating}
          <p class="mt-2 flex items-start gap-1.5 text-[10px] text-indigo-300">
            <span
              class="mt-0.5 inline-block h-3 w-3 shrink-0 animate-spin rounded-full border border-indigo-400 border-t-transparent"
            ></span>
            <span class="min-w-0 break-words">
              {comfyuiUpdate.progress ?? locale.t("comfyui_update.updating")}
            </span>
          </p>
        {:else}
          {#if comfyuiUpdate.error}
            <p class="mt-2 break-words text-[10px] text-red-400">{comfyuiUpdate.error}</p>
          {:else}
            <p class="mt-2 text-[10px] leading-snug text-neutral-400">
              {locale.t("comfyui_update.description")}
            </p>
          {/if}
          <div class="mt-2.5 flex items-center gap-2">
            <button
              class="rounded-md bg-amber-600 px-2.5 py-1 text-[11px] font-medium text-white transition-colors hover:bg-amber-500"
              onclick={() => comfyuiUpdate.update()}
            >
              {comfyuiUpdate.error
                ? locale.t("comfyui_update.retry")
                : locale.t("comfyui_update.action")}
            </button>
            <button
              class="text-[11px] text-neutral-400 transition-colors hover:text-neutral-200"
              onclick={() => comfyuiUpdate.dismissBubble()}
            >
              {locale.t("comfyui_update.dismiss")}
            </button>
          </div>
        {/if}

        <!-- Tail, centred on the badge below (sidebar content box is ~44px wide). -->
        <div
          class="absolute left-[22px] top-full -mt-[7px] h-3 w-3 -translate-x-1/2 rotate-45 border-b border-r border-amber-700/60 bg-neutral-900"
        ></div>
      </div>
    {/if}

    {#if outdated}
      <button
        class="flex w-full items-center justify-center gap-1 rounded text-[10px] text-amber-500 transition-colors hover:text-amber-400"
        onclick={() => comfyuiUpdate.toggleBubble()}
        title={locale.t("comfyui_update.badge_title", {
          installed: installedLabel,
          target: comfyuiUpdate.target,
        })}
      >
        {@render versionMark()}
        v{comfyuiUpdate.installed}
        {#if comfyuiUpdate.updating}
          <span
            class="inline-block h-2 w-2 shrink-0 animate-spin rounded-full border border-amber-500 border-t-transparent"
          ></span>
        {:else}
          <span class="inline-block h-1.5 w-1.5 shrink-0 rounded-full bg-amber-500" aria-hidden="true"></span>
        {/if}
      </button>
    {:else}
      <span
        class="flex select-none items-center justify-center gap-1 text-center text-[10px] text-neutral-500"
        title={locale.t("settings.performance.comfyui_up_to_date")}
      >
        {@render versionMark()}
        v{comfyuiUpdate.installed}
      </span>
    {/if}
  </div>
{/if}

{#snippet versionMark()}
  <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" class="h-2.5 w-2.5 shrink-0" aria-hidden="true">
    <circle cx="8" cy="8" r="7.5" fill="currentColor" fill-opacity="0.15" stroke="currentColor" stroke-width="1" />
    <text x="8" y="11.2" text-anchor="middle" font-size="9" font-weight="700" fill="currentColor">C</text>
  </svg>
{/snippet}
