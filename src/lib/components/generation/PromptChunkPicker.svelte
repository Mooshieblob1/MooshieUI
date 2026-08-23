<script lang="ts">
  /**
   * Compact prompt-chunk picker that lives in the prompt box toolbar.
   *
   * Chunks are a prompt feature, so activating one belongs next to the prompt
   * rather than three panels away in the Styles manager. The manager keeps the
   * full editor (create, edit, import, export); this is the short path:
   * activate, change mode, deactivate, copy the inline token.
   *
   * The list is a centred modal rather than a dropdown anchored to the button.
   * The toolbar sits in a narrow column whose width follows the panel layout,
   * so an anchored panel wide enough to preview chunk content ran off the left
   * edge and was clipped at the widths people actually use.
   */
  import { promptPresets, inlineChunkToken, type PromptPreset } from "../../stores/promptPresets.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import PresetActivationModal from "./PresetActivationModal.svelte";

  let open = $state(false);
  let activatingPresetId = $state<string | null>(null);

  const activeCount = $derived(promptPresets.activeEntries.length);

  // Escape closes. Only wired while the modal is open so the listener is not
  // carried by every prompt render.
  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") open = false;
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
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

<button
  type="button"
  onclick={() => (open = true)}
  class="rounded-lg border px-2 py-0.5 text-[10px] transition-colors {activeCount > 0
    ? 'border-indigo-500/50 bg-indigo-500/10 text-indigo-200 hover:border-indigo-400'
    : 'border-neutral-700 bg-neutral-900 text-neutral-300 hover:border-indigo-500 hover:text-indigo-200'}"
  title={locale.t("generation.prompts.chunks_title")}
  aria-expanded={open}
  aria-haspopup="dialog"
>
  {locale.t("styles.manager.tab_presets")}{activeCount > 0 ? ` (${activeCount})` : ""}
</button>

{#if open}
  <!-- Below PresetActivationModal's z-210, so picking a mode stacks on top. -->
  <div
    class="fixed inset-0 z-200 flex items-center justify-center bg-black/80 p-4 backdrop-blur-sm"
    role="dialog"
    aria-modal="true"
    aria-label={locale.t("generation.prompts.chunks_title")}
  >
    <button
      type="button"
      class="absolute inset-0 h-full w-full cursor-default"
      aria-label={locale.t("common.close")}
      onclick={() => (open = false)}
    ></button>

    <div class="relative z-10 flex max-h-[80vh] w-full max-w-md flex-col rounded-xl border border-neutral-700 bg-neutral-900 p-4 shadow-2xl">
      <div class="mb-3 flex items-start justify-between gap-3">
        <h3 class="text-sm font-semibold text-neutral-100">
          {locale.t("generation.prompts.chunks_title")}
        </h3>
        <button
          type="button"
          class="text-lg leading-none text-neutral-500 hover:text-neutral-200"
          onclick={() => (open = false)}
          aria-label={locale.t("common.close")}
        >✕</button>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto">
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
      </div>

      <p class="mt-3 shrink-0 border-t border-neutral-800 pt-2 text-[10px] text-neutral-500">
        {locale.t("generation.prompts.chunks_manage_hint")}
      </p>
    </div>
  </div>
{/if}

{#if activatingPresetId}
  <PresetActivationModal
    presetId={activatingPresetId}
    onclose={() => (activatingPresetId = null)}
  />
{/if}
