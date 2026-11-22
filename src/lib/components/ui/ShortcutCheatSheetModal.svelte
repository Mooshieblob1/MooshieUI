<script lang="ts">
  import { portal } from "../../utils/portal.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { shortcuts } from "../../stores/shortcuts.svelte.js";
  import {
    DEFAULT_SHORTCUTS,
    SHORTCUT_SCOPES,
    formatCombo,
    isTypingTarget,
    splitComboParts,
    type ShortcutScope,
  } from "../../utils/shortcuts.js";

  interface Props {
    onCustomize?: () => void;
  }

  let { onCustomize }: Props = $props();

  let searchQuery = $state("");

  const isMac =
    typeof navigator !== "undefined" && /mac/i.test(navigator.platform || navigator.userAgent);

  const scopeLabelKey: Record<ShortcutScope, string> = {
    global: "shortcuts.scope.global",
    generate: "shortcuts.scope.generate",
    gallery: "shortcuts.scope.gallery",
    lightbox: "shortcuts.scope.lightbox",
    canvas: "shortcuts.scope.canvas",
    prompt: "shortcuts.scope.prompt",
    comic: "shortcuts.scope.comic",
  };

  const groupedShortcuts = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    return SHORTCUT_SCOPES.map((scope) => {
      const scopeLabel = locale.t(scopeLabelKey[scope]);
      const items = DEFAULT_SHORTCUTS.filter((def) => {
        if (def.scope !== scope) return false;
        if (!q) return true;
        const label = locale.t(def.labelKey).toLowerCase();
        const combo = formatCombo(shortcuts.effectiveCombo(def.id), isMac).toLowerCase();
        const rawCombo = shortcuts.effectiveCombo(def.id).toLowerCase();
        return (
          label.includes(q) ||
          scopeLabel.toLowerCase().includes(q) ||
          combo.includes(q) ||
          rawCombo.includes(q)
        );
      });
      return { scope, scopeLabel, items };
    }).filter((group) => group.items.length > 0);
  });

  function handleClose(): void {
    searchQuery = "";
    shortcuts.closeCheatSheet();
  }

  function handleWindowKeydown(e: KeyboardEvent): void {
    if (!shortcuts.cheatSheetOpen) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      handleClose();
      return;
    }
    if (!isTypingTarget(e.target) && shortcuts.matches("global.cheat_sheet", e)) {
      e.preventDefault();
      e.stopPropagation();
      handleClose();
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if shortcuts.cheatSheetOpen}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    use:portal
    data-modal-open="true"
    class="fixed inset-0 z-[120] flex items-center justify-center bg-black/70 p-4"
    onclick={(e) => {
      if (e.target === e.currentTarget) handleClose();
    }}
    onkeydown={(e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        handleClose();
      }
    }}
    role="dialog"
    aria-modal="true"
    aria-labelledby="shortcuts-modal-title"
    tabindex="-1"
  >
    <div
      class="flex max-h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl"
    >
      <div class="flex items-center justify-between gap-3 border-b border-neutral-800 px-5 py-4">
        <div class="flex items-center gap-2.5 min-w-0">
          <div
            class="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-indigo-500/15 text-indigo-400"
          >
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <rect x="2" y="4" width="20" height="16" rx="2" />
              <path d="M6 8h.01" />
              <path d="M10 8h.01" />
              <path d="M14 8h.01" />
              <path d="M18 8h.01" />
              <path d="M6 12h.01" />
              <path d="M10 12h.01" />
              <path d="M14 12h.01" />
              <path d="M18 12h.01" />
              <path d="M7 16h10" />
            </svg>
          </div>
          <h2 id="shortcuts-modal-title" class="truncate text-base font-semibold text-neutral-100">
            {locale.t("shortcuts.modal.title")}
          </h2>
        </div>

        <button
          type="button"
          onclick={handleClose}
          class="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200 cursor-pointer"
          aria-label={locale.t("common.close")}
        >
          <svg
            class="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      </div>

      <div class="border-b border-neutral-800 px-5 py-3">
        <div class="relative">
          <svg
            class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-neutral-500"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
          <input
            type="text"
            bind:value={searchQuery}
            placeholder={locale.t("shortcuts.modal.search_placeholder")}
            class="w-full rounded-lg border border-neutral-700 bg-neutral-950/70 py-1.5 pl-9 pr-8 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
          />
          {#if searchQuery}
            <button
              type="button"
              onclick={() => (searchQuery = "")}
              class="absolute right-2.5 top-1/2 -translate-y-1/2 text-neutral-500 hover:text-neutral-200"
              aria-label={locale.t("common.close")}
            >
              <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          {/if}
        </div>
      </div>

      <div class="flex-1 overflow-y-auto px-5 py-4">
        {#if groupedShortcuts.length === 0}
          <div class="py-10 text-center text-sm text-neutral-500">
            {locale.t("shortcuts.modal.empty")}
          </div>
        {:else}
          <div class="grid grid-cols-1 gap-5 md:grid-cols-2">
            {#each groupedShortcuts as group (group.scope)}
              <section class="rounded-lg border border-neutral-800/90 bg-neutral-950/40 p-3.5">
                <h3 class="mb-2.5 text-xs font-semibold uppercase tracking-wider text-indigo-300">
                  {group.scopeLabel}
                </h3>
                <div class="space-y-2">
                  {#each group.items as def (def.id)}
                    {@const combo = shortcuts.effectiveCombo(def.id)}
                    {@const customized = shortcuts.isCustomized(def.id)}
                    {@const parts = splitComboParts(combo, isMac)}
                    <div class="flex items-center justify-between gap-2 text-xs">
                      <div class="flex min-w-0 items-center gap-1.5">
                        <span class="truncate text-neutral-200">{locale.t(def.labelKey)}</span>
                        {#if def.readOnly}
                          <span
                            class="shrink-0 rounded border border-neutral-700/80 bg-neutral-800/80 px-1.5 py-0.5 text-[10px] text-neutral-400"
                            title={locale.t("shortcuts.read_only_tip")}
                          >
                            {locale.t("shortcuts.read_only")}
                          </span>
                        {/if}
                      </div>
                      <div class="flex shrink-0 items-center gap-1">
                        {#each parts as part, idx (idx)}
                          {#if idx > 0}
                            <span class="text-[10px] text-neutral-500">+</span>
                          {/if}
                          <kbd
                            class="inline-flex min-w-6 items-center justify-center rounded border px-1.5 py-0.5 font-mono text-[11px] {customized
                              ? 'border-indigo-500/50 bg-indigo-500/15 text-indigo-200'
                              : 'border-neutral-700 bg-neutral-800 text-neutral-200'}"
                          >
                            {part}
                          </kbd>
                        {/each}
                      </div>
                    </div>
                  {/each}
                </div>
              </section>
            {/each}
          </div>
        {/if}
      </div>

      <div class="flex items-center justify-between gap-3 border-t border-neutral-800 bg-neutral-900 px-5 py-3.5">
        {#if onCustomize}
          <button
            type="button"
            onclick={() => {
              searchQuery = "";
              onCustomize();
            }}
            class="inline-flex items-center gap-1.5 text-xs font-medium text-indigo-400 transition-colors hover:text-indigo-300 cursor-pointer"
          >
            <span>{locale.t("shortcuts.modal.customize")}</span>
          </button>
        {:else}
          <span></span>
        {/if}

        <button
          type="button"
          onclick={handleClose}
          class="rounded-lg bg-indigo-600 px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-indigo-500 cursor-pointer"
        >
          {locale.t("common.close")}
        </button>
      </div>
    </div>
  </div>
{/if}
