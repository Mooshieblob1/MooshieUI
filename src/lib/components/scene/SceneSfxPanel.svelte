<script lang="ts">
  import { onDestroy } from "svelte";
  import { locale } from "../../stores/locale.svelte.js";
  import InfoTip from "../ui/InfoTip.svelte";
  import type { SfxClipData, SfxClipSummary } from "../../types/scene.js";
  import {
    sceneDeleteSfx,
    sceneGenerateSfx,
    sceneListSfx,
    sceneLoadSfx,
  } from "../../utils/api.js";

  let prompt = $state("");
  let autoDuration = $state(true);
  let durationSeconds = $state(4.0);
  let promptInfluence = $state(0.3);
  let loopAudio = $state(false);

  let busy = $state(false);
  let error = $state<string | null>(null);
  let blocked = $state<string | null>(null);
  let clips = $state<SfxClipSummary[]>([]);
  let loadedUrls = $state<Record<string, string>>({});
  let loadingClipId = $state<string | null>(null);
  let confirmDeleteId = $state<string | null>(null);

  const canGenerate = $derived(!busy && prompt.trim().length > 0 && prompt.trim().length <= 1000);

  function toBlobUrl(clip: SfxClipData): string {
    const raw = atob(clip.audio_base64);
    const bytes = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i += 1) {
      bytes[i] = raw.charCodeAt(i);
    }
    return URL.createObjectURL(new Blob([bytes], { type: clip.media_type }));
  }

  function storeClipAudio(clip: SfxClipData) {
    const prev = loadedUrls[clip.summary.id];
    if (prev) URL.revokeObjectURL(prev);
    loadedUrls = { ...loadedUrls, [clip.summary.id]: toBlobUrl(clip) };
  }

  function revokeClipAudio(clipId: string) {
    const url = loadedUrls[clipId];
    if (!url) return;
    URL.revokeObjectURL(url);
    const next = { ...loadedUrls };
    delete next[clipId];
    loadedUrls = next;
  }

  async function refreshClips() {
    try {
      clips = await sceneListSfx();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  $effect(() => {
    void refreshClips();
  });

  onDestroy(() => {
    for (const url of Object.values(loadedUrls)) {
      URL.revokeObjectURL(url);
    }
  });

  async function handleGenerate() {
    if (!canGenerate) return;
    busy = true;
    error = null;
    blocked = null;
    try {
      const outcome = await sceneGenerateSfx({
        prompt: prompt.trim(),
        duration_seconds: autoDuration ? null : durationSeconds,
        prompt_influence: promptInfluence,
        loop_audio: loopAudio,
      });
      if (outcome.status === "blocked") {
        blocked = outcome.message;
        return;
      }
      storeClipAudio(outcome.value);
      clips = [outcome.value.summary, ...clips.filter((c) => c.id !== outcome.value.summary.id)];
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function handleLoad(clipId: string) {
    if (loadingClipId || loadedUrls[clipId]) return;
    loadingClipId = clipId;
    error = null;
    try {
      const data = await sceneLoadSfx(clipId);
      storeClipAudio(data);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loadingClipId = null;
    }
  }

  async function handleDownload(clip: SfxClipSummary) {
    try {
      if (!loadedUrls[clip.id]) {
        const data = await sceneLoadSfx(clip.id);
        storeClipAudio(data);
      }
      const url = loadedUrls[clip.id];
      if (!url) return;
      const a = document.createElement("a");
      a.href = url;
      a.download = `sfx-${clip.id}.mp3`;
      a.click();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function handleDelete(clipId: string) {
    confirmDeleteId = null;
    error = null;
    try {
      await sceneDeleteSfx(clipId);
      revokeClipAudio(clipId);
      clips = clips.filter((c) => c.id !== clipId);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<section class="bg-neutral-900 rounded-xl border border-neutral-800 p-5 space-y-4">
  <div class="flex items-center justify-between gap-2">
    <h2 class="text-sm font-medium text-neutral-200">{locale.t("scene.sfx.title")}</h2>
    {#if clips.length > 0}
      <span class="text-[10px] text-neutral-500 font-mono">{clips.length}</span>
    {/if}
  </div>

  <div>
    <label class="text-xs text-neutral-400 block mb-1" for="scene-sfx-prompt">
      {locale.t("scene.sfx.prompt")}
    </label>
    <textarea
      id="scene-sfx-prompt"
      rows="2"
      maxlength="1000"
      placeholder={locale.t("scene.sfx.prompt_placeholder")}
      class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 resize-y"
      bind:value={prompt}
    ></textarea>
  </div>

  <div class="grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs">
    <div class="space-y-1.5">
      <label class="flex items-center gap-2 text-neutral-300 cursor-pointer select-none">
        <input type="checkbox" class="accent-indigo-500 rounded" bind:checked={autoDuration} />
        <span>{locale.t("scene.sfx.auto_duration")}</span>
      </label>
      {#if !autoDuration}
        <label class="block text-neutral-400 text-[11px]" for="scene-sfx-duration">
          {locale.t("scene.sfx.duration", { seconds: durationSeconds.toFixed(1) })}
        </label>
        <input
          id="scene-sfx-duration"
          type="range"
          min="0.5"
          max="30"
          step="0.5"
          bind:value={durationSeconds}
          class="w-full accent-indigo-500"
        />
      {/if}
    </div>

    <div class="space-y-1.5">
      <label class="flex items-center gap-1 text-neutral-400 text-[11px]" for="scene-sfx-influence">
        <span>{locale.t("scene.sfx.influence", { percent: Math.round(promptInfluence * 100) })}</span>
        <InfoTip text={locale.t("scene.sfx.influence_tip")} />
      </label>
      <input
        id="scene-sfx-influence"
        type="range"
        min="0"
        max="1"
        step="0.05"
        bind:value={promptInfluence}
        class="w-full accent-indigo-500"
      />
      <label class="flex items-center gap-2 text-neutral-300 cursor-pointer select-none pt-1">
        <input type="checkbox" class="accent-indigo-500 rounded" bind:checked={loopAudio} />
        <span>{locale.t("scene.sfx.loop")}</span>
      </label>
    </div>
  </div>

  {#if blocked}
    <p class="text-xs text-amber-400">{blocked}</p>
  {/if}
  {#if error}
    <p class="text-xs text-red-400">{error}</p>
  {/if}

  <div class="flex items-center justify-end">
    <button
      type="button"
      disabled={!canGenerate}
      onclick={() => { void handleGenerate(); }}
      class="px-3 py-1.5 rounded-lg text-xs bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 disabled:cursor-not-allowed text-white transition-colors cursor-pointer"
    >
      {busy ? locale.t("scene.sfx.generating") : locale.t("scene.sfx.generate")}
    </button>
  </div>

  {#if clips.length === 0}
    <p class="text-[11px] text-neutral-500">{locale.t("scene.sfx.empty")}</p>
  {:else}
    <div class="space-y-2 border-t border-neutral-800 pt-3">
      {#each clips as clip (clip.id)}
        <div class="rounded-lg border border-neutral-800 bg-neutral-950/60 p-3 space-y-2">
          <div class="flex items-start justify-between gap-2">
            <div class="min-w-0">
              <p class="text-xs text-neutral-200 break-words">{clip.prompt}</p>
              <p class="text-[10px] text-neutral-500 font-mono">
                {clip.duration_seconds ? `${clip.duration_seconds.toFixed(1)}s` : locale.t("scene.sfx.auto_duration")}
                {#if clip.loop_audio}
                  · {locale.t("scene.sfx.loop")}
                {/if}
              </p>
            </div>
            <div class="flex items-center gap-1.5 shrink-0">
              <button
                type="button"
                onclick={() => { void handleDownload(clip); }}
                class="px-2 py-1 rounded text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-200 cursor-pointer"
              >
                {locale.t("scene.sfx.download")}
              </button>
              {#if confirmDeleteId === clip.id}
                <button
                  type="button"
                  onclick={() => { void handleDelete(clip.id); }}
                  class="px-2 py-1 rounded text-[10px] bg-red-700 hover:bg-red-600 text-white cursor-pointer"
                >
                  {locale.t("scene.sfx.delete_confirm")}
                </button>
                <button
                  type="button"
                  onclick={() => { confirmDeleteId = null; }}
                  class="px-2 py-1 rounded text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300 cursor-pointer"
                >
                  {locale.t("scene.sfx.cancel")}
                </button>
              {:else}
                <button
                  type="button"
                  onclick={() => { confirmDeleteId = clip.id; }}
                  class="px-2 py-1 rounded text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300 cursor-pointer"
                >
                  {locale.t("scene.sfx.delete")}
                </button>
              {/if}
            </div>
          </div>

          {#if loadedUrls[clip.id]}
            <audio class="w-full h-8" controls src={loadedUrls[clip.id]}></audio>
          {:else}
            <button
              type="button"
              disabled={loadingClipId === clip.id}
              onclick={() => { void handleLoad(clip.id); }}
              class="px-2.5 py-1 rounded text-[11px] bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 text-neutral-200 cursor-pointer"
            >
              {locale.t("scene.sfx.load")}
            </button>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</section>
