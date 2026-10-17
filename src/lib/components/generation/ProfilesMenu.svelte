<script lang="ts">
  /**
   * Profiles: tabs saved under a name. Saving captures the active tab the way
   * a tab parks itself, so app-wide preferences stay out; picking a profile
   * opens it as a new tab, or loads it over the current one.
   */
  import { portal } from "../../utils/portal.js";
  import { profiles, type Profile } from "../../stores/profiles.svelte.js";
  import { generationTabs } from "../../stores/generationTabs.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  let open = $state(false);
  let nameInput = $state("");
  let anchor = $state<HTMLButtonElement | null>(null);
  let inputRef = $state<HTMLInputElement | null>(null);
  let position = $state({ top: 0, right: 0 });

  function show() {
    if (anchor) {
      const rect = anchor.getBoundingClientRect();
      position = { top: rect.bottom + 6, right: Math.max(8, window.innerWidth - rect.right) };
    }
    nameInput = generationTabs.activeTab?.name ?? "";
    open = true;
    queueMicrotask(() => {
      inputRef?.focus();
      inputRef?.select();
    });
  }

  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") open = false;
    };
    const onResize = () => (open = false);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", onResize);
    return () => {
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onResize);
    };
  });

  function save() {
    const name = nameInput.trim();
    if (!name) return;
    const { profile, updated } = profiles.save(name, generationTabs.captureActive());
    generationTabs.nameActive(profile.name);
    gallery.showToast(
      locale.t(updated ? "generation.profiles.updated" : "generation.profiles.saved", { name: profile.name }),
      "success",
    );
    open = false;
  }

  function openInNewTab(profile: Profile) {
    generationTabs.openSnapshot(profile.settings, profile.name);
    open = false;
  }

  function loadHere(profile: Profile) {
    generationTabs.loadIntoActive(profile.settings, profile.name);
    open = false;
  }

  function remove(profile: Profile) {
    if (!confirm(locale.t("generation.profiles.confirm_delete", { name: profile.name }))) return;
    profiles.remove(profile.id);
  }
</script>

<button
  bind:this={anchor}
  type="button"
  class="flex h-7 shrink-0 items-center gap-1.5 rounded-lg border px-2.5 text-xs transition-colors {open
    ? 'border-indigo-500/60 bg-neutral-800 text-neutral-100'
    : 'border-neutral-800 bg-neutral-900 text-neutral-300 hover:border-neutral-700 hover:text-neutral-100'}"
  title={locale.t("generation.profiles.hint")}
  aria-haspopup="dialog"
  aria-expanded={open}
  onclick={() => (open ? (open = false) : show())}
>
  <svg class="size-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z" />
  </svg>
  <span>{locale.t("generation.profiles.button")}</span>
  {#if profiles.profiles.length}
    <span class="tabular-nums text-neutral-500">{profiles.profiles.length}</span>
  {/if}
</button>

{#if open}
  <div use:portal class="fixed inset-0 z-200" role="presentation">
    <button
      type="button"
      class="absolute inset-0 h-full w-full cursor-default"
      tabindex="-1"
      aria-label={locale.t("common.close")}
      onclick={() => (open = false)}
    ></button>
    <div
      class="absolute flex max-h-[70vh] w-80 flex-col rounded-xl border border-neutral-700 bg-neutral-900 p-3 shadow-2xl shadow-black/40"
      style="top: {position.top}px; right: {position.right}px"
      role="dialog"
      aria-label={locale.t("generation.profiles.button")}
    >
      <p class="mb-2 text-[11px] leading-snug text-neutral-400">{locale.t("generation.profiles.hint")}</p>
      <div class="flex gap-1">
        <input
          bind:this={inputRef}
          bind:value={nameInput}
          onkeydown={(e) => {
            if (e.key === "Enter") save();
          }}
          placeholder={locale.t("generation.profiles.name_placeholder")}
          class="min-w-0 flex-1 rounded-md border border-neutral-700 bg-neutral-950 px-2 py-1.5 text-xs text-neutral-200 focus:border-neutral-500 focus:outline-none"
        />
        <button
          type="button"
          onclick={save}
          disabled={!nameInput.trim()}
          class="rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-indigo-500 disabled:opacity-40"
        >
          {locale.t("generation.profiles.save")}
        </button>
      </div>

      <div class="mt-3 min-h-0 flex-1 overflow-y-auto">
        {#if profiles.profiles.length === 0}
          <p class="py-2 text-center text-[11px] text-neutral-500">{locale.t("generation.profiles.empty")}</p>
        {:else}
          <ul class="space-y-1">
            {#each profiles.profiles as profile (profile.id)}
              <li class="group flex items-center gap-0.5 rounded-md border border-neutral-800 bg-neutral-950/60">
                <button
                  type="button"
                  class="min-w-0 flex-1 truncate px-2 py-1.5 text-left text-xs text-neutral-200 hover:text-white"
                  title={locale.t("generation.profiles.open_hint")}
                  onclick={() => openInNewTab(profile)}
                >
                  {profile.name}
                </button>
                <button
                  type="button"
                  class="flex size-6 shrink-0 items-center justify-center rounded text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-neutral-100"
                  title={locale.t("generation.profiles.load_here")}
                  aria-label={locale.t("generation.profiles.load_here")}
                  onclick={() => loadHere(profile)}
                >
                  <svg class="size-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M12 4v11m0 0l-4-4m4 4l4-4M5 20h14" />
                  </svg>
                </button>
                <button
                  type="button"
                  class="mr-0.5 flex size-6 shrink-0 items-center justify-center rounded text-neutral-500 transition-colors hover:bg-neutral-800 hover:text-red-400"
                  title={locale.t("generation.profiles.delete")}
                  aria-label={locale.t("generation.profiles.delete")}
                  onclick={() => remove(profile)}
                >
                  <svg class="size-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M3 6h18" /><path d="M8 6V4h8v2" /><path d="M19 6l-1 14H6L5 6" />
                  </svg>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  </div>
{/if}
