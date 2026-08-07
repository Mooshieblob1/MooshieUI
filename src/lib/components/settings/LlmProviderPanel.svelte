<script lang="ts">
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { isBrowserMode } from "../../utils/ipc.js";
  import type { LlmProviderState } from "../../types/index.js";

  /**
   * Provider picker and auth row for the external LLM path.
   *
   * Every field here except the enable toggle is written by a dedicated Rust
   * command, never by a full-config save: the API key must not travel back
   * through the frontend, and the OAuth key never reaches it at all. That
   * splits ownership with SettingsPage, which keeps a config snapshot and
   * replaces the whole config on any unrelated autosave, so `onstate` mirrors
   * the authoritative snapshot back into it after each mutation.
   */

  interface Props {
    /** `config.llm_external_enabled`. The one field the provider commands do
     *  not own, because a keyless self-hosted endpoint has no key to imply it. */
    enabled: boolean;
    /** SettingsPage's autosave, used only by the enable toggle. */
    autoSave: () => Promise<void>;
    /** Re-sync SettingsPage's cached config from the authoritative snapshot. */
    onstate: (state: LlmProviderState) => void;
  }

  let { enabled = $bindable(), autoSave, onstate }: Props = $props();

  /** Ids the Rust registry knows. Base URL, default model and wire format stay
   *  server-side; the frontend only needs the id and a label. */
  const PROVIDER_IDS = [
    "anthropic",
    "openai",
    "xai",
    "openrouter",
    "custom",
  ] as const;

  let error = $state<string | null>(null);
  let loaded = false;

  const provider = $derived(promptAssistant.provider);
  const providerId = $derived(provider?.provider ?? "custom");
  const isCustom = $derived(providerId === "custom");
  /** Sign-in binds a loopback listener on the machine running Rust, which in
   *  browser mode is the server rather than the user's machine. */
  const canOauth = $derived(!!provider?.oauth && !isBrowserMode);

  $effect(() => {
    if (loaded || promptAssistant.provider) return;
    loaded = true;
    promptAssistant.loadProvider();
  });

  function providerLabel(id: string): string {
    return locale.t(`settings.prompt_assistant.provider_${id}`);
  }

  /** Run a provider mutation, surface its failure, then re-sync SettingsPage. */
  async function run(fn: () => Promise<void>): Promise<void> {
    error = null;
    try {
      await fn();
      if (promptAssistant.provider) onstate(promptAssistant.provider);
      await promptAssistant.refreshStatus();
    } catch (e) {
      error = String(e);
    }
  }

  async function onToggleEnabled(next: boolean) {
    enabled = next;
    error = null;
    try {
      await autoSave();
      await promptAssistant.loadProvider();
      await promptAssistant.refreshStatus();
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="border-t border-neutral-800 pt-3 mt-1 space-y-3">
  <label class="flex items-center justify-between gap-3 cursor-pointer">
    <span>
      <span class="text-xs text-neutral-300 block">{locale.t('settings.prompt_assistant.external_title')}</span>
      <span class="text-[10px] text-neutral-500">{locale.t('settings.prompt_assistant.external_desc')}</span>
    </span>
    <input
      type="checkbox"
      checked={enabled}
      onchange={(e) => onToggleEnabled(e.currentTarget.checked)}
      class="accent-indigo-500 w-4 h-4 shrink-0"
    />
  </label>

  {#if enabled}
    <div>
      <label for="llm-provider" class="text-xs text-neutral-400 block mb-1">{locale.t('settings.prompt_assistant.provider')}</label>
      <select
        id="llm-provider"
        value={providerId}
        disabled={promptAssistant.providerBusy}
        onchange={(e) => run(() => promptAssistant.selectProvider(e.currentTarget.value))}
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
      >
        {#each PROVIDER_IDS as id (id)}
          <option value={id}>{providerLabel(id)}</option>
        {/each}
      </select>
    </div>

    {#if isCustom}
      <div>
        <label for="llm-base-url" class="text-xs text-neutral-400 block mb-1">{locale.t('settings.prompt_assistant.external_url')}</label>
        <input
          id="llm-base-url"
          type="text"
          value={provider?.base_url ?? ""}
          disabled={promptAssistant.providerBusy}
          onchange={(e) => run(() => promptAssistant.saveBaseUrl(e.currentTarget.value))}
          placeholder="http://localhost:1234/v1"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
        />
      </div>
    {/if}

    <div class="space-y-1.5">
      <div class="flex items-center justify-between gap-2">
        <label for="llm-api-key" class="text-xs text-neutral-400">{locale.t('settings.prompt_assistant.external_key')}</label>
        {#if provider?.api_key_configured}
          <span class="flex items-center gap-1.5 shrink-0">
            <span class="text-[10px] text-green-400">{locale.t('settings.prompt_assistant.key_configured')}</span>
            <button
              class="rounded border border-neutral-700 px-1.5 py-0.5 text-[10px] text-neutral-400 hover:bg-neutral-800 disabled:opacity-40"
              disabled={promptAssistant.providerBusy}
              onclick={() => run(() => promptAssistant.saveApiKey(""))}
            >
              {locale.t('settings.prompt_assistant.clear_key')}
            </button>
          </span>
        {/if}
      </div>
      <input
        id="llm-api-key"
        type="password"
        value=""
        disabled={promptAssistant.providerBusy}
        onchange={(e) => {
          const key = e.currentTarget.value;
          e.currentTarget.value = "";
          run(() => promptAssistant.saveApiKey(key));
        }}
        placeholder={provider?.api_key_configured
          ? locale.t('settings.prompt_assistant.key_replace_placeholder')
          : "sk-..."}
        autocomplete="off"
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
      />
      <p class="text-[10px] text-neutral-500">
        {isCustom
          ? locale.t('settings.prompt_assistant.external_key_hint')
          : locale.t('settings.prompt_assistant.key_stored_hint')}
      </p>

      {#if provider?.oauth}
        {#if canOauth}
          <button
            class="rounded-lg border border-indigo-500/60 px-2 py-1 text-[11px] text-indigo-300 hover:bg-indigo-500/10 disabled:opacity-40"
            disabled={promptAssistant.providerBusy}
            onclick={() => run(() => promptAssistant.connectOauth(providerId))}
          >
            {promptAssistant.providerBusy
              ? locale.t('settings.prompt_assistant.oauth_waiting')
              : locale.t('settings.prompt_assistant.oauth_connect', { provider: providerLabel(providerId) })}
          </button>
        {:else}
          <p class="text-[10px] text-neutral-500">{locale.t('settings.prompt_assistant.oauth_desktop_only')}</p>
        {/if}
      {/if}
    </div>

    <div>
      <div class="flex items-center justify-between gap-2 mb-1">
        <label for="llm-model" class="text-xs text-neutral-400">{locale.t('settings.prompt_assistant.external_model')}</label>
        <button
          class="rounded border border-neutral-700 px-1.5 py-0.5 text-[10px] text-neutral-400 hover:bg-neutral-800 disabled:opacity-40"
          disabled={promptAssistant.providerBusy}
          onclick={() => run(() => promptAssistant.refreshExternalModels())}
        >
          {locale.t('settings.prompt_assistant.load_models')}
        </button>
      </div>
      <input
        id="llm-model"
        type="text"
        list="llm-model-options"
        value={provider?.model ?? ""}
        disabled={promptAssistant.providerBusy}
        onchange={(e) => run(() => promptAssistant.saveModel(e.currentTarget.value))}
        placeholder="gpt-4o-mini"
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 focus:outline-none focus:border-indigo-500 transition-colors disabled:opacity-50"
      />
      <!-- Free text with suggestions: plenty of self-hosted endpoints serve
           chat completions without implementing `/models`. -->
      <datalist id="llm-model-options">
        {#each promptAssistant.externalModels as model (model)}
          <option value={model}></option>
        {/each}
      </datalist>
      <p class="text-[10px] text-neutral-500 mt-1">{locale.t('settings.prompt_assistant.model_hint')}</p>
    </div>

    {#if error}
      <p class="text-[10px] text-red-400 break-words">{error}</p>
    {/if}
  {/if}
</div>
