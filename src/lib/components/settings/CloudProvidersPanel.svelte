<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { keyEnv, toStoredKey } from "../../stores/keyEnv.svelte.js";
  import { cloudKeyStatus, setCloudApiKey } from "../../utils/api.js";
  import type { CloudKeyStatus, CloudProviderId } from "../../types/index.js";
  import KeyEnvToggle from "./KeyEnvToggle.svelte";

  /**
   * Key entry for the cloud voice and video providers.
   *
   * Same model as the NovelAI key: each key is written by a dedicated command
   * and never read back, so nothing here touches SettingsPage's cached config.
   * A named account sees and stores only its own keys.
   */

  interface Props {
    /** The owner (desktop, localhost or admin) rather than a named account. */
    isOwner: boolean;
  }

  let { isOwner }: Props = $props();

  // Brand names and API hosts are not translated.
  const providers: { id: CloudProviderId; name: string; host: string; purposeKey: string }[] = [
    { id: "elevenlabs", name: "ElevenLabs", host: "api.elevenlabs.io", purposeKey: "settings.cloud.purpose_voice" },
    { id: "fal", name: "fal.ai", host: "fal.run", purposeKey: "settings.cloud.purpose_video" },
    { id: "segmind", name: "Segmind", host: "api.segmind.com", purposeKey: "settings.cloud.purpose_video_alt" },
  ];

  let status = $state<CloudKeyStatus>({ elevenlabs: false, fal: false, segmind: false });
  let inputs = $state<Record<CloudProviderId, string>>({ elevenlabs: "", fal: "", segmind: "" });
  /** Per field: the input holds an environment variable name, not a key. */
  let envModes = $state<Record<CloudProviderId, boolean>>({ elevenlabs: false, fal: false, segmind: false });
  let saving = $state<CloudProviderId | null>(null);
  let errors = $state<Partial<Record<CloudProviderId, string>>>({});
  let loadError = $state<string | null>(null);

  async function refresh() {
    try {
      status = await cloudKeyStatus();
      loadError = null;
    } catch (e) {
      loadError = e instanceof Error ? e.message : String(e);
    }
  }

  $effect(() => {
    void refresh();
  });

  async function save(id: CloudProviderId, value: string) {
    saving = id;
    errors = { ...errors, [id]: undefined };
    try {
      await setCloudApiKey(id, value.trim());
      // Never keep the secret in component state once it is stored.
      inputs = { ...inputs, [id]: "" };
      envModes = { ...envModes, [id]: false };
      // Re-read rather than trust the save's answer: a saved variable name
      // only counts as a key while that variable is actually set.
      await Promise.all([refresh(), keyEnv.refresh()]);
    } catch (e) {
      errors = { ...errors, [id]: e instanceof Error ? e.message : String(e) };
    } finally {
      saving = null;
    }
  }
</script>

<section class="bg-neutral-900 rounded-xl border border-neutral-800 overflow-hidden mb-4">
  <div class="w-full flex items-center justify-between p-5 text-sm font-medium text-neutral-200">
    <span class="flex items-center gap-2">
      <svg xmlns="http://www.w3.org/2000/svg" class="w-4 h-4 text-teal-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polygon points="23 7 16 12 23 17 23 7"/><rect x="1" y="5" width="15" height="14" rx="2" ry="2"/></svg>
      {locale.t('settings.cloud.title')}
    </span>
  </div>

  <div class="px-5 pb-5 space-y-4">
    <p class="text-[10px] text-neutral-500">{locale.t(isOwner ? 'settings.cloud.desc' : 'settings.cloud.account_desc')}</p>
    <p class="text-[10px] text-amber-400">{locale.t('settings.cloud.not_available_yet')}</p>
    {#if loadError}
      <p class="text-[10px] text-red-400">{loadError}</p>
    {/if}

    {#each providers as provider (provider.id)}
      {@const inputId = `cloud-key-${provider.id}`}
      <div class="border-t border-neutral-800 pt-3 first:border-t-0 first:pt-0">
        <label class="text-xs text-neutral-300 block" for={inputId}>{provider.name}</label>
        <p class="text-[10px] text-neutral-500 mb-1">{locale.t(provider.purposeKey)}</p>
        <div class="flex gap-2">
          <input
            id={inputId}
            type={envModes[provider.id] ? "text" : "password"}
            autocomplete="off"
            spellcheck="false"
            bind:value={inputs[provider.id]}
            placeholder={envModes[provider.id]
              ? locale.t('settings.key_env.placeholder')
              : status[provider.id]
                ? locale.t('settings.cloud.key_set')
                : locale.t('settings.cloud.key_placeholder', { provider: provider.name })}
            class="flex-1 min-w-0 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 focus:outline-none focus:border-indigo-500 transition-colors font-mono"
          />
          <button
            class="shrink-0 px-3 py-2 rounded-lg text-sm bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 disabled:cursor-not-allowed text-white transition-colors"
            disabled={saving !== null || inputs[provider.id].trim() === ''}
            onclick={() => { void save(provider.id, toStoredKey(inputs[provider.id], envModes[provider.id])); }}
          >
            {locale.t('settings.cloud.save_key')}
          </button>
          {#if status[provider.id] || keyEnv.ref(provider.id)}
            <button
              class="shrink-0 px-3 py-2 rounded-lg text-sm bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 disabled:cursor-not-allowed text-neutral-300 transition-colors"
              disabled={saving !== null}
              onclick={() => { void save(provider.id, ''); }}
            >
              {locale.t('settings.cloud.clear_key')}
            </button>
          {/if}
        </div>
        {#if errors[provider.id]}
          <p class="text-[10px] text-red-400 mt-1">{errors[provider.id]}</p>
        {:else if status[provider.id] && !keyEnv.ref(provider.id)}
          <p class="text-[10px] text-emerald-400 mt-1">{locale.t('settings.cloud.key_set_hint')}</p>
        {/if}
        {#if isOwner}
          <KeyEnvToggle field={provider.id} bind:envMode={envModes[provider.id]} />
        {/if}
        <p class="text-[10px] text-neutral-500 mt-1">
          {locale.t('settings.cloud.destination', { provider: provider.name, destination: provider.host })}
        </p>
      </div>
    {/each}
  </div>
</section>
