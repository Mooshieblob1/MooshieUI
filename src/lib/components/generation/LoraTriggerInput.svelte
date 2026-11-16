<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { loraTriggers } from "../../stores/loraTriggers.svelte.js";

  /**
   * The user's own trigger text for one LoRA file. Saved on blur or Enter;
   * when the LoRA is on, the prompt swaps the old text for the new one.
   */
  let { name }: { name: string } = $props();

  let draft = $state("");
  let editing = $state(false);
  const shown = $derived(editing ? draft : loraTriggers.get(name));

  function commit() {
    if (!editing) return;
    editing = false;
    const before = loraTriggers.get(name);
    const after = loraTriggers.set(name, draft);
    if (before !== after) {
      generation.replaceLoraTrigger(name, before, after);
      generation.saveSettings();
    }
  }
</script>

<!-- Clicks stay here so a LoRA card under it does not toggle. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<label
  class="flex items-center gap-1.5 rounded border border-neutral-700 bg-neutral-900/60 px-1.5 py-0.5 focus-within:border-indigo-500"
  title={locale.t("lora.custom_trigger_tip")}
  onclick={(e) => e.stopPropagation()}
>
  <svg xmlns="http://www.w3.org/2000/svg" class="w-3 h-3 shrink-0 text-amber-400/80" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z"/><line x1="7" y1="7" x2="7.01" y2="7"/></svg>
  <input
    type="text"
    value={shown}
    placeholder={locale.t("lora.custom_trigger_placeholder")}
    aria-label={locale.t("lora.custom_trigger_placeholder")}
    spellcheck={false}
    class="min-w-0 flex-1 bg-transparent text-[11px] text-amber-200 placeholder-neutral-500 focus:outline-none"
    onfocus={() => {
      draft = loraTriggers.get(name);
      editing = true;
    }}
    oninput={(e) => (draft = (e.currentTarget as HTMLInputElement).value)}
    onblur={commit}
    onkeydown={(e) => {
      e.stopPropagation();
      if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
      else if (e.key === "Escape") {
        editing = false;
        (e.currentTarget as HTMLInputElement).blur();
      }
    }}
  />
</label>
