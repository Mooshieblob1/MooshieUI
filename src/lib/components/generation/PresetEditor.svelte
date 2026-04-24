<script lang="ts">
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import PromptTextarea from "./PromptTextarea.svelte";

  interface Props {
    presetId: string;
    onclose: () => void;
  }

  let { presetId, onclose }: Props = $props();

  const preset = $derived(promptPresets.getById(presetId));
  const choiceCount = $derived(promptPresets.countChoices(presetId));

  function setName(v: string) {
    if (!preset) return;
    promptPresets.update(preset.id, { name: v });
  }

  // Local mirror bound to PromptTextarea; writes back to the store on change.
  // The component is remounted per-preset (parent gates with {#if editingPresetId}),
  // so a one-shot initializer + forward effect is sufficient.
  let contentLocal = $state("");
  let contentInitialized = $state(false);

  $effect(() => {
    if (!contentInitialized && preset) {
      contentLocal = preset.content;
      contentInitialized = true;
    }
  });

  $effect(() => {
    if (contentInitialized && preset && contentLocal !== preset.content) {
      promptPresets.update(preset.id, { content: contentLocal });
    }
  });
</script>

<div
  class="fixed inset-0 z-205 flex items-center justify-center bg-black/80 backdrop-blur-sm"
  role="dialog"
  aria-modal="true"
  aria-label="Edit Prompt Preset"
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label="Close"
    onclick={onclose}
  ></button>

  {#if preset}
    <div class="relative z-10 w-full max-w-2xl max-h-[92vh] overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
      <div class="mb-4 flex items-start justify-between gap-3">
        <div>
          <h2 class="text-sm font-semibold text-neutral-100">Edit Prompt Preset</h2>
          <p class="text-[11px] text-neutral-500">
            Store any prompt fragment as a reusable variable. Separate entries by comma or newline — used for the wildcard mode to pick one at random.
          </p>
        </div>
        <button
          type="button"
          class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
          onclick={onclose}
          aria-label="Close"
        >✕</button>
      </div>

      <div class="space-y-4">
        <div>
          <label for="pst-name" class="mb-1 block text-[10px] uppercase tracking-wide text-neutral-500">Name</label>
          <input
            id="pst-name"
            type="text"
            value={preset.name}
            oninput={(e) => setName((e.currentTarget as HTMLInputElement).value)}
            placeholder="e.g. Cool hair colors"
            class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
          />
        </div>

        <div>
          <div class="mb-1 flex items-center justify-between">
            <label for="pst-content" class="text-[10px] uppercase tracking-wide text-neutral-500">Content</label>
            <span class="text-[10px] text-neutral-500">{choiceCount} wildcard option{choiceCount === 1 ? "" : "s"}</span>
          </div>
          <PromptTextarea
            bind:value={contentLocal}
            placeholder="blue hair, red hair, green hair — OR — masterpiece, best quality, intricate details"
            rows={8}
            minHeight="min-h-40"
          />
          <p class="mt-1 text-[10px] text-neutral-500">
            <span class="text-indigo-300">Prepend / Append:</span> whole content is injected as-is.
            <br />
            <span class="text-indigo-300">Wildcard:</span> one comma/newline-separated entry is picked at random per generation.
            <br />
            <span class="text-neutral-400">Tip:</span> autocomplete suggestions match the active model's tag list.
          </p>
        </div>
      </div>

      <div class="mt-4 flex justify-end">
        <button
          type="button"
          class="rounded bg-indigo-600 px-4 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
          onclick={onclose}
        >Done</button>
      </div>
    </div>
  {:else}
    <div class="relative z-10 rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-sm text-neutral-300">
      Preset not found.
      <button class="ml-3 text-indigo-400 hover:text-indigo-300" onclick={onclose}>Close</button>
    </div>
  {/if}
</div>
