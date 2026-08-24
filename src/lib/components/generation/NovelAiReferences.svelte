<script lang="ts">
  /**
   * Vibe Transfer and Precise Reference.
   *
   * NovelAI rejects a request carrying both systems, so the store clears one
   * when the other gains an image and the payload builder enforces the same
   * precedence. This panel makes that visible rather than letting a populated
   * section be silently dropped: whichever list lost out is dimmed, with the
   * reason spelled out above both.
   */
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import {
    NOVELAI_MAX_VIBES,
    NOVELAI_MAX_DIRECTOR_REFERENCES,
    NOVELAI_REFERENCE_DESCRIPTIONS,
  } from "../../utils/novelaiModels.js";
  import { fileToNovelAiBase64, novelAiBase64ToSrc } from "../../utils/novelaiImage.js";
  import { VIBE_ENCODE_COST } from "../../utils/novelaiCost.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import type { NovelAiVibe } from "../../types/index.js";

  let busy = $state(false);

  const vibes = $derived(generation.novelaiSettings.vibes);
  /**
   * Whether this vibe already has a token NovelAI will accept as is.
   *
   * A token is minted for one model at one extraction level, so moving
   * either invalidates it and the next generation pays for a fresh encode.
   * A vibe that arrived as a bare token has no image to re-encode from and
   * is always good.
   */
  const isEncoded = (vibe: NovelAiVibe) =>
    !!vibe.encoding &&
    (!vibe.image ||
      (vibe.encoded_model === generation.checkpoint &&
        vibe.encoded_information_extracted === vibe.information_extracted));
  const references = $derived(generation.novelaiSettings.director_references);
  const anySupported = $derived(
    generation.supportsNovelAiVibeTransfer || generation.supportsNovelAiPreciseReference,
  );
  const referenceFull = $derived(references.length >= NOVELAI_MAX_DIRECTOR_REFERENCES || busy);
  const vibeFull = $derived(vibes.length >= NOVELAI_MAX_VIBES || busy);

  async function addFrom(files: FileList | null, target: "vibe" | "reference") {
    const file = files?.[0];
    if (!file) return;
    busy = true;
    try {
      const base64 = await fileToNovelAiBase64(file);
      if (!base64) return;
      if (target === "vibe") generation.addNovelAiVibe(base64);
      else generation.addNovelAiDirectorReference(base64);
    } finally {
      busy = false;
    }
  }
</script>

<div class="space-y-4">
  {#if !anySupported}
    <div class="space-y-2">
      <span class="text-xs text-neutral-400">{locale.t("generation.novelai.references.title")}</span>
      <p class="text-[11px] text-amber-400/90">
        {locale.t("generation.novelai.references.unsupported")}
      </p>
    </div>
  {:else}
    <p class="text-[11px] text-neutral-500">
      {locale.t("generation.novelai.references.exclusive")}
    </p>

    {#if generation.supportsNovelAiPreciseReference}
      <div class="space-y-2 {vibes.length > 0 ? 'opacity-50' : ''}">
        <div class="flex items-center justify-between">
          <span class="text-xs text-neutral-400">
            {locale.t("generation.novelai.reference.title")}
            <InfoTip text={locale.t("generation.novelai.reference.tip")} />
          </span>
          <label
            class="px-2 py-1 text-[11px] rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-200 cursor-pointer {referenceFull
              ? 'opacity-40 pointer-events-none'
              : ''}"
          >
            {locale.t("generation.novelai.reference.add")}
            <input
              type="file"
              accept="image/*"
              class="hidden"
              onchange={(e) => {
                addFrom(e.currentTarget.files, "reference");
                e.currentTarget.value = "";
              }}
            />
          </label>
        </div>

        {#each references as reference, index (index)}
          <div class="rounded-lg border border-neutral-800 bg-neutral-900/60 p-2 flex gap-3">
            <img
              src={novelAiBase64ToSrc(reference.image)}
              alt=""
              class="w-16 h-16 rounded object-cover border border-neutral-800 shrink-0"
            />
            <div class="flex-1 space-y-1.5 min-w-0">
              <div class="flex items-center gap-2">
                <select
                  class="flex-1 px-2 py-1 text-[11px] rounded-md bg-neutral-950 border border-neutral-800 text-neutral-200 focus:outline-none focus:border-indigo-600"
                  value={reference.description}
                  onchange={(e) =>
                    generation.updateNovelAiDirectorReference(index, {
                      description: e.currentTarget.value,
                    })}
                >
                  {#each NOVELAI_REFERENCE_DESCRIPTIONS as option (option.value)}
                    <option value={option.value}>{locale.t(option.labelKey)}</option>
                  {/each}
                </select>
                <button
                  class="px-2 py-0.5 text-[11px] rounded-md text-neutral-400 hover:text-red-400 hover:bg-neutral-800"
                  onclick={() => generation.removeNovelAiDirectorReference(index)}
                >
                  {locale.t("generation.novelai.reference.remove")}
                </button>
              </div>
              <label class="block text-[11px] text-neutral-500">
                {locale.t("generation.novelai.reference.strength")}
                {reference.strength.toFixed(2)}
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  class="w-full accent-indigo-500"
                  value={reference.strength}
                  oninput={(e) =>
                    generation.updateNovelAiDirectorReference(index, {
                      strength: Number(e.currentTarget.value),
                    })}
                />
              </label>
              <label class="block text-[11px] text-neutral-500">
                {locale.t("generation.novelai.reference.information")}
                {reference.information_extracted.toFixed(2)}
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  class="w-full accent-indigo-500"
                  value={reference.information_extracted}
                  oninput={(e) =>
                    generation.updateNovelAiDirectorReference(index, {
                      information_extracted: Number(e.currentTarget.value),
                    })}
                />
              </label>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    {#if generation.supportsNovelAiVibeTransfer}
      <div class="space-y-2 {references.length > 0 ? 'opacity-50' : ''}">
        <div class="flex items-center justify-between">
          <span class="text-xs text-neutral-400">
            {locale.t("generation.novelai.vibe.title")}
            <InfoTip text={locale.t("generation.novelai.vibe.tip")} />
          </span>
          <label
            class="px-2 py-1 text-[11px] rounded-md bg-neutral-800 hover:bg-neutral-700 text-neutral-200 cursor-pointer {vibeFull
              ? 'opacity-40 pointer-events-none'
              : ''}"
          >
            {locale.t("generation.novelai.vibe.add")}
            <input
              type="file"
              accept="image/*"
              class="hidden"
              onchange={(e) => {
                addFrom(e.currentTarget.files, "vibe");
                e.currentTarget.value = "";
              }}
            />
          </label>
        </div>

        <p class="text-[11px] text-neutral-500">
          {locale.t("generation.novelai.vibe.cost", { anlas: VIBE_ENCODE_COST })}
        </p>

        {#each vibes as vibe, index (index)}
          <div class="rounded-lg border border-neutral-800 bg-neutral-900/60 p-2 flex gap-3">
            {#if vibe.image}
              <img
                src={novelAiBase64ToSrc(vibe.image)}
                alt=""
                class="w-16 h-16 rounded object-cover border border-neutral-800 shrink-0"
              />
            {:else}
              <div
                class="w-16 h-16 rounded border border-neutral-800 shrink-0 flex items-center justify-center text-center text-[10px] text-neutral-600"
              >
                {locale.t("generation.novelai.vibe.encoded")}
              </div>
            {/if}
            <div class="flex-1 space-y-1.5 min-w-0">
              <div class="flex items-center justify-between gap-2">
                {#if isEncoded(vibe)}
                  <span
                    class="px-1.5 py-0.5 text-[10px] rounded bg-emerald-900/40 text-emerald-300 border border-emerald-800/60"
                  >
                    {locale.t("generation.novelai.vibe.encoded")}
                  </span>
                {:else}
                  <span></span>
                {/if}
                <button
                  class="px-2 py-0.5 text-[11px] rounded-md text-neutral-400 hover:text-red-400 hover:bg-neutral-800"
                  onclick={() => generation.removeNovelAiVibe(index)}
                >
                  {locale.t("generation.novelai.vibe.remove")}
                </button>
              </div>
              <label class="block text-[11px] text-neutral-500">
                {locale.t("generation.novelai.vibe.strength")}
                {vibe.strength.toFixed(2)}
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  class="w-full accent-indigo-500"
                  value={vibe.strength}
                  oninput={(e) =>
                    generation.updateNovelAiVibe(index, {
                      strength: Number(e.currentTarget.value),
                    })}
                />
              </label>
              <label class="block text-[11px] text-neutral-500">
                {locale.t("generation.novelai.vibe.information")}
                {vibe.information_extracted.toFixed(2)}
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  class="w-full accent-indigo-500"
                  value={vibe.information_extracted}
                  oninput={(e) =>
                    generation.updateNovelAiVibe(index, {
                      information_extracted: Number(e.currentTarget.value),
                    })}
                />
              </label>
            </div>
          </div>
        {/each}

        {#if vibes.length > 0}
          <label class="flex items-center gap-2 text-xs text-neutral-300">
            <input
              type="checkbox"
              class="accent-indigo-500"
              checked={generation.novelaiSettings.normalize_reference_strength}
              onchange={(e) =>
                generation.updateNovelAiSettings({
                  normalize_reference_strength: e.currentTarget.checked,
                })}
            />
            {locale.t("generation.novelai.vibe.normalize")}
            <InfoTip text={locale.t("generation.novelai.vibe.normalize_desc")} />
          </label>
        {/if}
      </div>
    {/if}
  {/if}
</div>
