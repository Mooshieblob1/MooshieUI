<script lang="ts">
  import { untrack } from "svelte";
  import { locale } from "../../stores/locale.svelte.js";
  import { animeScene } from "../../stores/animeScene.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";

  interface Props {
    /** A fal.ai key is saved, so a draft can be completed. */
    hasKey: boolean;
  }

  let { hasKey }: Props = $props();

  const small = "px-2 py-1 rounded-lg text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300 disabled:opacity-40";
  // Clips whose info was already asked for, so a refresh never repeats a call.
  const requested = new Set<string>();

  $effect(() => {
    for (const clip of animeScene.clips) {
      if (requested.has(clip.filename)) continue;
      requested.add(clip.filename);
      untrack(() => void animeScene.loadClipInfo(clip.filename));
    }
  });

  async function exactVoice(filename: string) {
    const saved = await animeScene.exactVoice(filename);
    if (saved) {
      await gallery.addPersistedImage(saved.video_filename, { duration_seconds: saved.duration_seconds, fps: saved.fps }, true);
    }
  }

  function expiry(unix: number | null): string {
    return unix ? new Date(unix * 1000).toLocaleString() : "";
  }
</script>

{#if animeScene.clips.length > 0}
  <section class="bg-neutral-900 rounded-xl border border-neutral-800 p-5 space-y-3">
    <div class="space-y-1">
      <h2 class="text-sm font-medium text-neutral-200">{locale.t("scene.clips.title")}</h2>
      <p class="text-[10px] text-neutral-500">{locale.t("scene.clips.desc")}</p>
    </div>
    <ul class="space-y-2">
      {#each animeScene.clips as clip (clip.filename)}
        {@const info = animeScene.clipInfo[clip.filename]}
        {@const busy = animeScene.clipBusy === clip.filename}
        <li class="rounded-lg border border-neutral-800 bg-neutral-950/40 p-3 space-y-2">
          <div class="flex items-start justify-between gap-2">
            <div class="min-w-0">
              <p class="text-xs text-neutral-200 truncate" title={clip.filename}>{clip.filename}</p>
              {#if info}
                <p class="text-[10px] text-neutral-500">
                  {locale.t(info.draft ? "scene.clips.summary_draft" : "scene.clips.summary", {
                    model: info.model_label,
                    seconds: info.seconds,
                    resolution: info.draft ? "480p" : info.resolution,
                  })}
                </p>
              {/if}
            </div>
            <button class={small} onclick={() => animeScene.removeClip(clip.filename)}>
              {locale.t("scene.clips.forget")}
            </button>
          </div>
          {#if info}
            <div class="flex flex-wrap gap-2">
              {#if info.upgrade}
                <button class={small} disabled={busy || !hasKey} onclick={() => void animeScene.upgradeDraft(clip.filename)}>
                  {locale.t("scene.clips.upgrade", { usd: info.upgrade.usd.toFixed(2) })}
                </button>
              {/if}
              {#if info.exact_voice}
                <button class={small} disabled={busy} onclick={() => void exactVoice(clip.filename)}>
                  {busy ? locale.t("scene.clips.working") : locale.t("scene.clips.exact_voice")}
                </button>
              {/if}
            </div>
            {#if info.upgrade}
              <p class="text-[10px] text-neutral-500">{locale.t("scene.clips.upgrade_until", { date: expiry(info.draft_expires_unix) })}</p>
            {:else if info.draft && info.draft_expires_unix && info.draft_expires_unix * 1000 < Date.now()}
              <p class="text-[10px] text-neutral-500">{locale.t("scene.clips.upgrade_expired")}</p>
            {/if}
            {#if info.exact_voice}
              <p class="text-[10px] text-neutral-500">{locale.t("scene.clips.exact_voice_desc")}</p>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
    {#if animeScene.clipError}
      <p class="text-xs text-red-400 whitespace-pre-wrap">{animeScene.clipError}</p>
    {/if}
  </section>
{/if}
