<script lang="ts">
  import { tick } from "svelte";

  interface Props {
    value: number;
    min: number;
    max: number;
    step: number;
    decimals?: number;
    suffix?: string;
    onchange: (value: number) => void;
  }

  let { value, min, max, step, decimals = 0, suffix = "", onchange }: Props = $props();

  let editing = $state(false);
  let inputEl = $state<HTMLInputElement | null>(null);
  let editValue = $state("");

  async function startEdit() {
    editValue = decimals > 0 ? value.toFixed(decimals) : String(value);
    editing = true;
    await tick();
    inputEl?.focus();
    inputEl?.select();
  }

  function commit() {
    editing = false;
    const parsed = parseFloat(editValue);
    if (isNaN(parsed)) return;
    const clamped = Math.min(max, Math.max(min, parsed));
    const snapped = Math.round(clamped / step) * step;
    const fixed = decimals > 0 ? parseFloat(snapped.toFixed(decimals)) : Math.round(snapped);
    onchange(fixed);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      commit();
    } else if (e.key === "Escape") {
      editing = false;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span onclick={(e) => e.preventDefault()}>
{#if editing}
  <input
    bind:this={inputEl}
    type="text"
    inputmode="decimal"
    class="text-neutral-100 bg-neutral-800 rounded border border-indigo-500/70 outline-none text-right w-[4ch] text-xs px-0.5 py-0 m-0 tabular-nums"
    style="font: inherit; line-height: inherit;"
    bind:value={editValue}
    onblur={commit}
    onkeydown={onKeydown}
  />
{:else}
  <button
    class="text-neutral-300 cursor-text tabular-nums hover:text-indigo-300 transition-colors"
    onclick={startEdit}
    title="Click to type a value"
  >
    {decimals > 0 ? value.toFixed(decimals) : value}{suffix}
  </button>
{/if}
</span>
