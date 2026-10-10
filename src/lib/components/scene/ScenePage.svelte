<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { animeScene } from "../../stores/animeScene.svelte.js";
  import { cloudKeyStatus } from "../../utils/api.js";
  import SceneVoicePanel from "./SceneVoicePanel.svelte";
  import SceneScriptPanel from "./SceneScriptPanel.svelte";
  import SceneCostDialog from "./SceneCostDialog.svelte";

  interface Props {
    onOpenSettings: () => void;
  }

  let { onOpenSettings }: Props = $props();

  let hasKey = $state<boolean | null>(null);
  let confirmReset = $state(false);

  $effect(() => {
    void cloudKeyStatus()
      .then((status) => {
        hasKey = status.elevenlabs;
        if (status.elevenlabs) void animeScene.refreshAccount();
      })
      .catch(() => (hasKey = false));
  });
</script>

<div class="h-full overflow-y-auto">
  <div class="mx-auto max-w-3xl space-y-4 p-4 md:p-6">
    <header class="space-y-1">
      <h1 class="text-base font-semibold text-neutral-100">{locale.t("scene.title")}</h1>
      <p class="text-xs text-neutral-500">{locale.t("scene.subtitle")}</p>
    </header>

    {#if hasKey === false}
      <div class="rounded-xl border border-neutral-800 bg-neutral-900 p-5 space-y-2">
        <p class="text-sm text-neutral-200">{locale.t("scene.no_key")}</p>
        <button
          class="px-3 py-2 rounded-lg text-sm bg-indigo-600 hover:bg-indigo-500 text-white transition-colors"
          onclick={onOpenSettings}
        >
          {locale.t("scene.open_settings")}
        </button>
      </div>
    {:else if hasKey}
      <section class="bg-neutral-900 rounded-xl border border-neutral-800 p-5 space-y-3">
        <div class="flex items-center justify-between gap-2">
          <h2 class="text-sm font-medium text-neutral-200">{locale.t("scene.character.title")}</h2>
          {#if confirmReset}
            <div class="flex gap-1">
              <button class="px-2 py-1 rounded-lg text-[10px] bg-red-700 hover:bg-red-600 text-white" onclick={() => { confirmReset = false; animeScene.resetScene(); }}>{locale.t("scene.character.new_scene_confirm")}</button>
              <button class="px-2 py-1 rounded-lg text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300" onclick={() => (confirmReset = false)}>{locale.t("scene.cost.cancel")}</button>
            </div>
          {:else}
            <button class="px-2 py-1 rounded-lg text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300" onclick={() => (confirmReset = true)}>{locale.t("scene.character.new_scene")}</button>
          {/if}
        </div>
        <div>
          <label class="text-[10px] text-neutral-400 block mb-0.5" for="scene-character-name">{locale.t("scene.character.name")}</label>
          <input
            id="scene-character-name"
            class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-sm text-neutral-100"
            bind:value={animeScene.characterName}
            onchange={() => animeScene.saveSettings()}
          />
        </div>
        <label class="flex items-start gap-2 select-none {animeScene.designLocked ? 'cursor-not-allowed' : 'cursor-pointer'}">
          <input
            type="checkbox"
            class="w-4 h-4 mt-0.5 rounded accent-amber-500"
            checked={animeScene.isMinor}
            disabled={animeScene.designLocked}
            onchange={(e) => animeScene.setMinor((e.target as HTMLInputElement).checked)}
          />
          <span>
            <span class="text-xs text-neutral-200 block">{locale.t("scene.character.minor")}</span>
            <span class="text-[10px] text-neutral-500">{locale.t("scene.character.minor_desc")}</span>
          </span>
        </label>
      </section>

      <SceneVoicePanel />
      <SceneScriptPanel />
    {/if}
  </div>
</div>

<SceneCostDialog />
