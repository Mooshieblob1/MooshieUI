<script lang="ts">
  /**
   * Compact prompt-chunk picker that lives in the prompt box toolbar.
   *
   * Chunks are a prompt feature, so activating one belongs next to the prompt
   * rather than three panels away in the Styles manager. The manager keeps the
   * full editor (create, edit, import, export); this is the short path:
   * activate, change mode, deactivate, copy the inline token.
   */
  import { promptPresets, inlineChunkToken, type PromptPreset } from "../../stores/promptPresets.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import PresetActivationModal from "./PresetActivationModal.svelte";

  let open = $state(false);
  let activatingPresetId = $state<string | null>(null);
  let root: HTMLDivElement | undefined = $state();

  const activeCount = $derived(promptPresets.activeEntries.length);

  // Close on an outside click or Escape. Only wired while the panel is open so
  // the listeners are not carried by every prompt render.
  $effect(() => {
    if (!open) return;
    const onPointer = (e: MouseEvent) => {
      if (root && !root.contains(e.target as Node)) open = false;
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") open = false;
    };
    document.addEventListener("mousedown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  });

  function modeIcon(mode: string): string {
    if (mode === "prepend") return "↑";
    if (mode === "append") return "↓";
    if (mode === "wildcard_ordered") return "1→";
    return "🎲";
  }

  function onChunkClick(preset: PromptPreset) {
    if (promptPresets.isActive(preset.id)) {
      promptPresets.deactivate(preset.id);
      return;
    }
    activatingPresetId = preset.id;
  }

  async function copyToken(preset: PromptPreset) {
    const token = inlineChunkToken(preset.name);
    try {
      await navigator.clipboard?.writeText(token);
      gallery.showToast(locale.t("generation.prompts.chunk_token_copied", { token }), "success");
    } catch {
      // Clipboard is blocked in some browser contexts; the token is still shown
      // on the button, so this is not worth an error toast.
    }
  }
</script>

<div class="relative" bind:this={root}>
  <button
    type="button"
    onclick={() => (open = !open)}
    class="rounded-lg border px-2 py-0.5 text-[10px] transition-colors {activeCount > 0
      ? 'border-indigo-500/50 bg-indigo-500/10 text-indigo-200 hover:border-indigo-400'
      : 'border-neutral-700 bg-neutral-900 text-neutral-300 hover:border-indigo-500 hover:text-indigo-200'}"
    title={locale.t("generation.prompts.chunks_title")}
    aria-expanded={open}
    aria-haspopup="true"
  >
    {locale.t("styles.manager.tab_presets")}{activeCount > 0 ? ` (${activeCount})` : ""}
  </button>

  {#if open}
    <div
      class="absolute right-0 z-50 mt-1 max-h-72 w-72 overflow-y-auto rounded-lg border border-neutral-700 bg-neutral-900 p-2 shadow-xl"
      role="menu"
      tabindex="-1"
    >
      {#if promptPresets.presets.length === 0}
        <p class="p-2 text-center text-[11px] text-neutral-500">
          {locale.t("styles.manager.empty_presets")}
        </p>
      {:else}
        <ul class="space-y-1">
          {#each promptPresets.presets as preset (preset.id)}
            {@const active = promptPresets.isActive(preset.id)}
            {@const mode = promptPresets.activeMode(preset.id)}
            <li class="flex items-center gap-1 rounded border {active
              ? 'border-indigo-500/50 bg-indigo-500/5'
              : 'border-neutral-800 bg-neutral-950/60'} px-1.5 py-1">
              <button
                type="button"
                class="min-w-0 flex-1 text-left"
                onclick={() => onChunkClick(preset)}
                title={active
                  ? locale.t("styles.manager.deactivate")
                  : locale.t("styles.manager.activate_ellipsis")}
              >
                <span class="flex items-center gap-1.5">
                  <span class="truncate text-[11px] {active ? 'text-indigo-200' : 'text-neutral-200'}">{preset.name}</span>
                  {#if active && mode}
                    <span class="shrink-0 font-mono text-[9px] text-indigo-300/80">{modeIcon(mode)}</span>
                  {/if}
                </span>
                <span class="block truncate font-mono text-[10px] text-neutral-500">
                  {preset.content || locale.t("common.empty")}
                </span>
              </button>
              {#if active && mode}
                <button
                  type="button"
                  class="shrink-0 rounded border border-neutral-700 bg-neutral-800 px-1 py-0.5 text-[10px] text-neutral-400 hover:text-indigo-200"
                  onclick={() => (activatingPresetId = preset.id)}
                  title={locale.t("styles.manager.change_mode")}
                  aria-label={locale.t("styles.manager.change_mode")}
                >↻</button>
              {/if}
              <button
                type="button"
                class="shrink-0 rounded border border-neutral-800 bg-neutral-900 px-1 py-0.5 font-mono text-[10px] text-neutral-400 hover:border-indigo-500/40 hover:text-indigo-200"
                onclick={() => copyToken(preset)}
                title={locale.t("styles.manager.inline_token_title")}
                aria-label={locale.t("styles.manager.inline_token_title")}
              >@</button>
            </li>
          {/each}
        </ul>
      {/if}
      <p class="mt-2 border-t border-neutral-800 pt-1.5 text-[10px] text-neutral-500">
        {locale.t("generation.prompts.chunks_manage_hint")}
      </p>
    </div>
  {/if}
</div>

{#if activatingPresetId}
  <PresetActivationModal
    presetId={activatingPresetId}
    onclose={() => (activatingPresetId = null)}
  />
{/if}
