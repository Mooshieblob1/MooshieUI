<script lang="ts">
  import { promptPresets, type PromptPreset, type PresetMode } from "../../stores/promptPresets.svelte.js";

  interface Props {
    presetId: string;
    onclose: () => void;
    /** Optional callback once a mode is picked and the preset becomes active. */
    onactivated?: (mode: PresetMode) => void;
  }

  let { presetId, onclose, onactivated }: Props = $props();

  const preset = $derived(promptPresets.getById(presetId));
  const choiceCount = $derived(promptPresets.countChoices(presetId));
  const currentMode = $derived(promptPresets.activeMode(presetId));

  function pick(mode: PresetMode) {
    if (!preset) return;
    promptPresets.activate(preset.id, mode);
    onactivated?.(mode);
    onclose();
  }
</script>

<div
  class="fixed inset-0 z-210 flex items-center justify-center bg-black/80 backdrop-blur-sm"
  role="dialog"
  aria-modal="true"
  aria-label="Activate preset"
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label="Cancel"
    onclick={onclose}
  ></button>

  {#if preset}
    <div class="relative z-10 w-full max-w-md rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
      <div class="mb-3 flex items-start justify-between gap-3">
        <div>
          <h3 class="text-sm font-semibold text-neutral-100">Activate "{preset.name}"</h3>
          <p class="mt-1 text-[11px] text-neutral-500">
            Choose how this preset contributes to the prompt. The content is injected at generation time and will NOT appear in your prompt textbox.
          </p>
        </div>
        <button
          type="button"
          class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
          onclick={onclose}
          aria-label="Cancel"
        >✕</button>
      </div>

      <div class="mb-3 rounded border border-neutral-800 bg-neutral-950/60 p-2">
        <p class="text-[10px] uppercase tracking-wide text-neutral-500">Content preview</p>
        <p class="mt-1 max-h-20 overflow-y-auto whitespace-pre-wrap wrap-break-word font-mono text-[11px] text-neutral-200">
          {preset.content || "(empty)"}
        </p>
      </div>

      <div class="grid grid-cols-1 gap-2">
        <button
          type="button"
          class="flex items-start gap-3 rounded-lg border {currentMode === 'prepend' ? 'border-indigo-500 bg-indigo-500/10' : 'border-neutral-700 bg-neutral-800/60'} p-3 text-left hover:border-indigo-500"
          onclick={() => pick("prepend")}
        >
          <span class="text-lg leading-none text-indigo-300">↑</span>
          <span class="flex-1">
            <span class="block text-sm font-medium text-neutral-100">Prepend</span>
            <span class="block text-[11px] text-neutral-500">Insert at the start of the positive prompt.</span>
          </span>
        </button>

        <button
          type="button"
          class="flex items-start gap-3 rounded-lg border {currentMode === 'append' ? 'border-indigo-500 bg-indigo-500/10' : 'border-neutral-700 bg-neutral-800/60'} p-3 text-left hover:border-indigo-500"
          onclick={() => pick("append")}
        >
          <span class="text-lg leading-none text-indigo-300">↓</span>
          <span class="flex-1">
            <span class="block text-sm font-medium text-neutral-100">Append</span>
            <span class="block text-[11px] text-neutral-500">Insert at the end of the positive prompt.</span>
          </span>
        </button>

        <button
          type="button"
          class="flex items-start gap-3 rounded-lg border {currentMode === 'wildcard' ? 'border-indigo-500 bg-indigo-500/10' : 'border-neutral-700 bg-neutral-800/60'} p-3 text-left hover:border-indigo-500 {choiceCount < 2 ? 'opacity-60' : ''}"
          onclick={() => pick("wildcard")}
          disabled={choiceCount < 1}
        >
          <span class="text-lg leading-none text-indigo-300">🎲</span>
          <span class="flex-1">
            <span class="block text-sm font-medium text-neutral-100">Wildcard (random)</span>
            <span class="block text-[11px] text-neutral-500">
              {#if choiceCount === 0}
                Add comma- or newline-separated options first.
              {:else if choiceCount === 1}
                Only one option — will pick that every time.
              {:else}
                Pick one of {choiceCount} options at random per generation.
              {/if}
            </span>
          </span>
        </button>
      </div>

      <div class="mt-4 flex justify-end">
        <button
          type="button"
          class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-300 hover:text-neutral-100"
          onclick={onclose}
        >Cancel</button>
      </div>
    </div>
  {:else}
    <div class="relative z-10 rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-sm text-neutral-300">
      Preset not found.
      <button class="ml-3 text-indigo-400 hover:text-indigo-300" onclick={onclose}>Close</button>
    </div>
  {/if}
</div>
