<script lang="ts">
  /**
   * Prompt field for the Comic page builder, wrapping the shared PromptTextarea
   * with tag autocomplete, spellcheck, highlighting, and weight controls.
   *
   * Mirrors NovelAiCharacterPrompt's local + lastSynced debounce pattern so
   * external store updates (undo, drafts, template switches) sync cleanly
   * without fighting in-flight typing.
   */
  import { onDestroy } from "svelte";
  import PromptTextarea from "../generation/PromptTextarea.svelte";

  interface Props {
    id?: string;
    value: string;
    onchange: (next: string) => void;
    placeholder?: string;
    rows?: number;
    minHeight?: string;
    storageKey?: string;
    tagAssist?: boolean;
    suggestLoraKeywords?: boolean;
    ariaLabel?: string;
    containerClass?: string;
    compact?: boolean;
  }

  let {
    id,
    value,
    onchange,
    placeholder = "",
    rows = 2,
    minHeight = "min-h-12",
    storageKey,
    tagAssist = true,
    suggestLoraKeywords = true,
    ariaLabel,
    containerClass = "",
    compact = true,
  }: Props = $props();

  // svelte-ignore state_referenced_locally
  let local = $state(value);
  // svelte-ignore state_referenced_locally
  let lastSynced = value;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    if (value !== lastSynced) {
      clearTimeout(debounceTimer);
      debounceTimer = undefined;
      lastSynced = value;
      local = value;
    }
  });

  $effect(() => {
    const next = local;
    if (next === lastSynced) return;
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debounceTimer = undefined;
      lastSynced = next;
      onchange(next);
    }, 200);
  });

  onDestroy(() => {
    if (debounceTimer !== undefined) {
      clearTimeout(debounceTimer);
      debounceTimer = undefined;
      if (local !== lastSynced) {
        lastSynced = local;
        onchange(local);
      }
    }
  });
</script>

{#if containerClass}
  <div class={containerClass}>
    <PromptTextarea
      {id}
      bind:value={local}
      {placeholder}
      {rows}
      {minHeight}
      {storageKey}
      {tagAssist}
      {suggestLoraKeywords}
      {ariaLabel}
      {compact}
    />
  </div>
{:else}
  <PromptTextarea
    {id}
    bind:value={local}
    {placeholder}
    {rows}
    {minHeight}
    {storageKey}
    {tagAssist}
    {suggestLoraKeywords}
    {ariaLabel}
    {compact}
  />
{/if}
