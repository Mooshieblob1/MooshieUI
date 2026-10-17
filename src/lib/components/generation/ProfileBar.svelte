<script lang="ts">
  import { profiles } from "../../stores/profiles.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  let selectedId = $state("");
  let naming = $state(false);
  let nameInput = $state("");
  let inputRef = $state<HTMLInputElement | null>(null);

  const selected = $derived(selectedId ? profiles.getById(selectedId) : null);

  async function onSelect(id: string) {
    selectedId = id;
    if (!id) return;
    const profile = profiles.getById(id);
    if (!profile) return;
    if (await profiles.load(id)) {
      gallery.showToast(locale.t("profiles.loaded", { name: profile.name }), "success");
    }
  }

  function startSave() {
    nameInput = selected?.name ?? "";
    naming = true;
    queueMicrotask(() => {
      inputRef?.focus();
      inputRef?.select();
    });
  }

  function confirmSave() {
    const name = nameInput.trim();
    if (!name) return;
    const overwrite = profiles.findByName(name) !== null;
    const profile = profiles.saveCurrent(name);
    selectedId = profile.id;
    naming = false;
    gallery.showToast(
      locale.t(overwrite ? "profiles.updated" : "profiles.saved", { name: profile.name }),
      "success",
    );
  }

  function remove() {
    if (!selected) return;
    if (!confirm(locale.t("profiles.confirm_delete", { name: selected.name }))) return;
    profiles.remove(selected.id);
    selectedId = "";
  }
</script>

<div class="flex gap-1 items-center mt-1.5">
  {#if naming}
    <input
      bind:this={inputRef}
      bind:value={nameInput}
      onkeydown={(e) => {
        if (e.key === "Enter") confirmSave();
        else if (e.key === "Escape") naming = false;
      }}
      placeholder={locale.t("profiles.name_placeholder")}
      class="flex-1 min-w-0 text-xs bg-neutral-900 border border-neutral-700 rounded-md px-2 py-1.5 text-neutral-200 focus:outline-none focus:border-neutral-500"
    />
    <button
      onclick={confirmSave}
      disabled={!nameInput.trim()}
      class="text-xs px-2.5 py-1.5 rounded-md bg-neutral-700 text-white hover:bg-neutral-600 disabled:opacity-40 transition-colors"
    >
      {locale.t("profiles.save")}
    </button>
    <button
      onclick={() => (naming = false)}
      class="text-xs px-2 py-1.5 rounded-md text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 transition-colors"
    >
      {locale.t("profiles.cancel")}
    </button>
  {:else}
    <select
      value={selectedId}
      onchange={(e) => onSelect((e.currentTarget as HTMLSelectElement).value)}
      title={locale.t("profiles.select_hint")}
      class="flex-1 min-w-0 text-xs bg-neutral-900 border border-neutral-700 rounded-md px-2 py-1.5 text-neutral-200 focus:outline-none focus:border-neutral-500"
    >
      <option value="">
        {profiles.profiles.length ? locale.t("profiles.placeholder") : locale.t("profiles.empty")}
      </option>
      {#each profiles.profiles as profile (profile.id)}
        <option value={profile.id}>{profile.name}</option>
      {/each}
    </select>
    <button
      onclick={startSave}
      title={locale.t("profiles.save_hint")}
      class="text-xs px-2.5 py-1.5 rounded-md bg-neutral-800 text-neutral-200 hover:bg-neutral-700 transition-colors"
    >
      {locale.t("profiles.save")}
    </button>
    {#if selected}
      <button
        onclick={remove}
        title={locale.t("profiles.delete")}
        class="p-1.5 rounded-md text-neutral-500 hover:text-red-400 hover:bg-neutral-800 transition-colors"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/></svg>
      </button>
    {/if}
  {/if}
</div>
