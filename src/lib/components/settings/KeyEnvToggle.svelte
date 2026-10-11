<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { keyEnv } from "../../stores/keyEnv.svelte.js";
  import type { KeyEnvField } from "../../types/index.js";

  /**
   * The "use an environment variable" switch and status line under an owner
   * API key field. The field itself stays in its own component: this only
   * flips `envMode`, which the field uses to pick its input type, placeholder
   * and `toStoredKey()` wrapping. Renders nothing for a named account.
   */

  interface Props {
    field: KeyEnvField;
    envMode: boolean;
  }

  let { field, envMode = $bindable() }: Props = $props();

  const ref = $derived(keyEnv.ref(field));

  $effect(() => {
    keyEnv.ensureLoaded();
  });
</script>

{#if keyEnv.available}
  {#if ref}
    <p class="text-[10px] mt-1 {ref.set ? 'text-emerald-400' : 'text-amber-400'}">
      {ref.set
        ? locale.t("settings.key_env.reading", { name: ref.name })
        : locale.t("settings.key_env.not_set", { name: ref.name })}
    </p>
  {/if}
  <button
    type="button"
    class="text-[10px] text-indigo-400 hover:text-indigo-300 underline-offset-2 hover:underline mt-1 cursor-pointer"
    onclick={() => { envMode = !envMode; }}
  >
    {envMode ? locale.t("settings.key_env.use_key") : locale.t("settings.key_env.use_env")}
  </button>
  {#if envMode}
    <p class="text-[10px] text-neutral-500 mt-1">{locale.t("settings.key_env.hint")}</p>
  {/if}
{/if}
