<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { animeScene } from "../../stores/animeScene.svelte.js";
  import type { VoiceBrief, VoicePreview } from "../../types/scene.js";

  const briefFields: { key: keyof VoiceBrief; labelKey: string; placeholderKey: string }[] = [
    { key: "language", labelKey: "scene.voice.brief_language", placeholderKey: "scene.voice.brief_language_hint" },
    { key: "gender", labelKey: "scene.voice.brief_gender", placeholderKey: "scene.voice.brief_gender_hint" },
    { key: "age", labelKey: "scene.voice.brief_age", placeholderKey: "scene.voice.brief_age_hint" },
    { key: "quality", labelKey: "scene.voice.brief_quality", placeholderKey: "scene.voice.brief_quality_hint" },
    { key: "persona", labelKey: "scene.voice.brief_persona", placeholderKey: "scene.voice.brief_persona_hint" },
    { key: "emotions", labelKey: "scene.voice.brief_emotions", placeholderKey: "scene.voice.brief_emotions_hint" },
    { key: "delivery", labelKey: "scene.voice.brief_delivery", placeholderKey: "scene.voice.brief_delivery_hint" },
  ];

  let previewNames = $state<Record<string, string>>({});
  let confirmDeleteId = $state<string | null>(null);

  const previewLength = $derived(animeScene.previewText.trim().length);
  const previewLengthOk = $derived(previewLength >= 100 && previewLength <= 1000);

  function setBrief(key: keyof VoiceBrief, value: string) {
    animeScene.brief = { ...animeScene.brief, [key]: value };
    animeScene.saveSettings();
  }

  function nameFor(preview: VoicePreview, index: number): string {
    return previewNames[preview.generated_voice_id] ?? `${animeScene.characterName.trim() || locale.t("scene.voice.default_name")} ${index + 1}`;
  }
</script>

<section class="bg-neutral-900 rounded-xl border border-neutral-800 p-5 space-y-4">
  <div class="flex items-center justify-between gap-2">
    <h2 class="text-sm font-medium text-neutral-200">{locale.t("scene.voice.title")}</h2>
    <button
      class="px-2 py-1 rounded-lg text-[10px] bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 text-neutral-300 transition-colors"
      disabled={animeScene.accountLoading}
      onclick={() => { void animeScene.refreshAccount(); }}
    >
      {locale.t("scene.voice.refresh")}
    </button>
  </div>

  {#if animeScene.accountError}
    <p class="text-[10px] text-red-400">{animeScene.accountError}</p>
  {/if}
  {#if animeScene.subscription}
    {@const sub = animeScene.subscription}
    <p class="text-[10px] text-neutral-500">
      {#if animeScene.remainingCredits !== null}
        {locale.t("scene.voice.credits", { credits: animeScene.remainingCredits.toLocaleString() })}
      {/if}
      {#if sub.voice_slots_used !== null && sub.voice_limit !== null}
        · {locale.t("scene.voice.slots", { used: sub.voice_slots_used, limit: sub.voice_limit })}
      {/if}
    </p>
  {/if}

  <div>
    <label class="text-xs text-neutral-400 block mb-1" for="scene-voice-select">{locale.t("scene.voice.pick")}</label>
    <div class="flex gap-2">
      <select
        id="scene-voice-select"
        class="flex-1 min-w-0 bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100"
        value={animeScene.voiceId ?? ""}
        onchange={(e) => animeScene.selectVoice((e.target as HTMLSelectElement).value || null)}
      >
        <option value="">{locale.t("scene.voice.none")}</option>
        {#each animeScene.voices as voice (voice.voice_id)}
          <option value={voice.voice_id}>{voice.name}{voice.category ? ` (${voice.category})` : ""}</option>
        {/each}
      </select>
      {#if animeScene.voiceId}
        {#if confirmDeleteId === animeScene.voiceId}
          <button
            class="shrink-0 px-3 py-2 rounded-lg text-xs bg-red-700 hover:bg-red-600 text-white transition-colors"
            onclick={() => { const id = confirmDeleteId!; confirmDeleteId = null; void animeScene.deleteVoice(id); }}
          >
            {locale.t("scene.voice.delete_confirm")}
          </button>
          <button
            class="shrink-0 px-3 py-2 rounded-lg text-xs bg-neutral-800 hover:bg-neutral-700 text-neutral-300 transition-colors"
            onclick={() => (confirmDeleteId = null)}
          >
            {locale.t("scene.cost.cancel")}
          </button>
        {:else}
          <button
            class="shrink-0 px-3 py-2 rounded-lg text-xs bg-neutral-800 hover:bg-neutral-700 text-neutral-300 transition-colors"
            onclick={() => (confirmDeleteId = animeScene.voiceId)}
          >
            {locale.t("scene.voice.delete")}
          </button>
        {/if}
      {/if}
    </div>
    {#if confirmDeleteId}
      <p class="text-[10px] text-amber-400 mt-1">{locale.t("scene.voice.delete_warning")}</p>
    {/if}
  </div>

  <div class="border-t border-neutral-800 pt-4 space-y-3">
    <h3 class="text-xs font-medium text-neutral-300">{locale.t("scene.voice.design_title")}</h3>

    {#if animeScene.designLocked}
      <div class="rounded-lg border border-amber-800/60 bg-amber-950/30 p-3 space-y-1" role="status">
        <p class="text-xs text-amber-200">{locale.t("scene.voice.design_locked")}</p>
        {#if animeScene.designLockedMessage}
          <p class="text-[10px] text-amber-300/80">{animeScene.designLockedMessage}</p>
        {/if}
      </div>
    {:else}
      <p class="text-[10px] text-neutral-500">{locale.t("scene.voice.design_desc")}</p>
      <div class="grid gap-2 sm:grid-cols-2">
        {#each briefFields as field (field.key)}
          <div class={field.key === "delivery" ? "sm:col-span-2" : ""}>
            <label class="text-[10px] text-neutral-400 block mb-0.5" for={`scene-brief-${field.key}`}>{locale.t(field.labelKey)}</label>
            <input
              id={`scene-brief-${field.key}`}
              class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 placeholder-neutral-500"
              value={animeScene.brief[field.key]}
              placeholder={locale.t(field.placeholderKey)}
              onchange={(e) => setBrief(field.key, (e.target as HTMLInputElement).value)}
            />
          </div>
        {/each}
      </div>
      <div>
        <label class="text-[10px] text-neutral-400 block mb-0.5" for="scene-preview-text">{locale.t("scene.voice.preview_text")}</label>
        <textarea
          id="scene-preview-text"
          rows="3"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100 placeholder-neutral-500"
          placeholder={locale.t("scene.voice.preview_text_hint")}
          bind:value={animeScene.previewText}
          onchange={() => animeScene.saveSettings()}
        ></textarea>
        <p class="text-[10px] {previewLengthOk ? 'text-neutral-500' : 'text-amber-400'}">
          {locale.t("scene.voice.preview_length", { count: previewLength })}
        </p>
      </div>
      {#if animeScene.slotsFull}
        <p class="text-[10px] text-amber-400">{locale.t("scene.voice.slots_full")}</p>
      {/if}
      <button
        class="px-3 py-2 rounded-lg text-sm bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 disabled:cursor-not-allowed text-white transition-colors"
        disabled={animeScene.designing || !previewLengthOk}
        onclick={() => { void animeScene.designVoice(); }}
      >
        {animeScene.designing ? locale.t("scene.voice.designing") : locale.t("scene.voice.design")}
      </button>
      {#if animeScene.designBlocked}
        <p class="text-[10px] text-amber-400" role="status">{animeScene.designBlocked}</p>
      {/if}
    {/if}

    {#if animeScene.designError}
      <p class="text-[10px] text-red-400">{animeScene.designError}</p>
    {/if}

    {#if animeScene.previews.length > 0}
      <div class="space-y-2">
        {#each animeScene.previews as preview, i (preview.generated_voice_id)}
          <div class="flex flex-wrap items-center gap-2 rounded-lg bg-neutral-800/60 p-2">
            <audio controls src={animeScene.previewUrl(preview)} class="h-8 max-w-full"></audio>
            <input
              class="flex-1 min-w-32 bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1 text-xs text-neutral-100"
              aria-label={locale.t("scene.voice.voice_name")}
              value={nameFor(preview, i)}
              oninput={(e) => (previewNames = { ...previewNames, [preview.generated_voice_id]: (e.target as HTMLInputElement).value })}
            />
            <button
              class="px-2 py-1 rounded-lg text-xs bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 text-white transition-colors"
              disabled={animeScene.savingVoice || animeScene.slotsFull}
              onclick={() => { void animeScene.savePreview(preview, nameFor(preview, i)); }}
            >
              {locale.t("scene.voice.save_preview")}
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</section>
