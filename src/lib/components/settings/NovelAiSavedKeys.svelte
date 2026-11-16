<script lang="ts">
  // The owner's saved NovelAI keys: a named list to switch between accounts.
  // The keys never come back from the backend, only a name and a hint.
  import { locale } from "../../stores/locale.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";

  let label = $state("");
  let keyInput = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    void novelai.loadSavedKeys();
  });

  async function run(task: () => Promise<void>) {
    busy = true;
    error = null;
    try {
      await task();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function add() {
    void run(async () => {
      await novelai.saveNamedKey(label, keyInput);
      label = "";
      keyInput = "";
    });
  }
</script>

{#if novelai.savedKeysAvailable}
  <div class="border-t border-neutral-800 pt-3 space-y-2">
    <div>
      <span class="text-xs text-neutral-400 block">{locale.t("settings.novelai.saved_keys")}</span>
      <p class="text-[10px] text-neutral-500 mt-0.5">{locale.t("settings.novelai.saved_keys_desc")}</p>
    </div>

    {#if novelai.savedKeys.length > 0}
      <ul class="space-y-1">
        {#each novelai.savedKeys as saved (saved.label)}
          <li class="flex items-center gap-2 rounded-lg bg-neutral-800/60 px-2.5 py-1.5 text-xs">
            <span class="min-w-0 flex-1 truncate text-neutral-200">{saved.label}</span>
            <span class="shrink-0 font-mono text-[10px] text-neutral-500">{saved.hint}</span>
            {#if saved.active}
              <span class="shrink-0 rounded px-1.5 py-0.5 text-[10px] bg-teal-900/60 text-teal-300">{locale.t("settings.novelai.saved_key_in_use")}</span>
            {:else}
              <button
                class="shrink-0 rounded px-2 py-0.5 text-[10px] bg-indigo-600 hover:bg-indigo-500 text-white disabled:opacity-40 transition-colors"
                disabled={busy}
                onclick={() => run(() => novelai.useNamedKey(saved.label))}
              >{locale.t("settings.novelai.saved_key_use")}</button>
            {/if}
            <button
              class="shrink-0 rounded p-0.5 text-neutral-500 hover:text-red-400 disabled:opacity-40 transition-colors"
              disabled={busy}
              title={locale.t("settings.novelai.saved_key_remove")}
              aria-label={locale.t("settings.novelai.saved_key_remove")}
              onclick={() => run(() => novelai.deleteNamedKey(saved.label))}
            >&times;</button>
          </li>
        {/each}
      </ul>
    {/if}

    <div class="flex flex-wrap gap-2">
      <input
        type="text"
        autocomplete="off"
        spellcheck="false"
        maxlength="40"
        bind:value={label}
        placeholder={locale.t("settings.novelai.saved_key_name")}
        aria-label={locale.t("settings.novelai.saved_key_name")}
        class="w-32 bg-neutral-800 border border-neutral-700 rounded-lg px-2.5 py-1.5 text-xs text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
      />
      <input
        type="password"
        autocomplete="off"
        spellcheck="false"
        bind:value={keyInput}
        placeholder={locale.t("settings.novelai.api_key_placeholder")}
        aria-label={locale.t("settings.novelai.api_key")}
        class="min-w-0 flex-1 bg-neutral-800 border border-neutral-700 rounded-lg px-2.5 py-1.5 text-xs font-mono text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors"
      />
      <button
        class="shrink-0 px-3 py-1.5 rounded-lg text-xs bg-neutral-800 border border-neutral-700 hover:bg-neutral-700 disabled:opacity-40 disabled:cursor-not-allowed text-neutral-200 transition-colors"
        disabled={busy || !label.trim() || !keyInput.trim()}
        onclick={add}
      >{locale.t("settings.novelai.saved_key_add")}</button>
    </div>
    {#if error}
      <p class="text-[10px] text-red-400">{error}</p>
    {/if}
  </div>
{/if}
