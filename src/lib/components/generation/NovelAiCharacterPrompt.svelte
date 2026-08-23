<script lang="ts">
  /**
   * One character prompt field, wired to the shared prompt editor.
   *
   * These are prompt boxes like any other, so they get the same chunk
   * highlighting, autocomplete and weight editing as the main box. Generation
   * resolves `@[Chunk]` inside them too (see `resolveNovelAiCharacters`), and a
   * field that resolves tokens but does not show them reads as broken.
   *
   * The store is the single source of truth, but `PromptTextarea` binds its
   * value, so this keeps a local editing copy and pushes it back on a debounce
   * (every write triggers a settings save).
   */
  import { onDestroy } from "svelte";
  import PromptTextarea from "./PromptTextarea.svelte";
  import { generation } from "../../stores/generation.svelte.js";

  interface Props {
    index: number;
    field: "prompt" | "negative_prompt";
    value: string;
    placeholder?: string;
    rows?: number;
    minHeight?: string;
  }

  let { index, field, value, placeholder = "", rows = 2, minHeight = "min-h-16" }: Props = $props();

  // svelte-ignore state_referenced_locally
  // Intentional: seed the local editing copy from the prop's initial value.
  // Later store changes are adopted by the $effect below.
  let local = $state(value);
  // Plain (non-reactive) mirror of the last value synced in either direction.
  // The effects read it without tracking it, so they fire only on a real store
  // or input change and never revert an in-flight keystroke.
  // svelte-ignore state_referenced_locally
  let lastSynced = value;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    if (value !== lastSynced) {
      lastSynced = value;
      local = value;
    }
  });

  $effect(() => {
    const next = local;
    if (next === lastSynced) return;
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      lastSynced = next;
      generation.updateNovelAiCharacter(
        index,
        field === "prompt" ? { prompt: next } : { negative_prompt: next },
      );
    }, 400);
  });

  onDestroy(() => clearTimeout(debounceTimer));
</script>

<PromptTextarea
  bind:value={local}
  {placeholder}
  {rows}
  {minHeight}
  storageKey={`mooshieui.promptHeight.novelaiCharacter.${field}.${index}`}
/>
