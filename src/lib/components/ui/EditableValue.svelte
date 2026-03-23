<script lang="ts">
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

  function startEdit() {
    editValue = decimals > 0 ? value.toFixed(decimals) : String(value);
    editing = true;
    queueMicrotask(() => {
      inputEl?.select();
    });
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

{#if editing}
  <input
    bind:this={inputEl}
    type="text"
    inputmode="decimal"
    class="text-neutral-300 bg-transparent border-none outline-none text-right w-[3.5ch] text-xs p-0 m-0 tabular-nums"
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
