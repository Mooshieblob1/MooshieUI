<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { animeScene, type SceneShot } from "../../stores/animeScene.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { novelai } from "../../stores/novelai.svelte.js";
  import { readImageMetadata } from "../../utils/api.js";
  import GalleryPickerModal from "../gallery/GalleryPickerModal.svelte";
  import type { ImageRole } from "../../types/scene.js";
  import type { OutputImage } from "../../types/index.js";

  interface Props {
    hasKey: boolean;
    onOpenSettings: () => void;
  }

  let { hasKey, onOpenSettings }: Props = $props();

  let pickerOpen = $state(false);
  let pickError = $state<string | null>(null);

  const caps = $derived(animeScene.capability);
  const slotsLeft = $derived(Math.max(0, (caps?.max_images ?? 30) - animeScene.references.length));
  const input = "w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100";
  const label = "text-[10px] text-neutral-400 block mb-0.5";
  const small = "px-2 py-1 rounded-lg text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300 disabled:opacity-40";

  // A finished render leaves the page's list once the queue lets it go.
  $effect(() => {
    void progress.pendingPrompts;
    animeScene.syncActiveJobs();
  });

  function attach(images: OutputImage[]) {
    pickerOpen = false;
    pickError = null;
    const usable = images
      .filter((image) => image.gallery_filename)
      .map((image) => ({ filename: image.gallery_filename!, thumb: image.thumbnailUrl ?? image.url ?? null }));
    if (usable.length < images.length) pickError = locale.t("scene.video.not_saved");
    animeScene.addReferences(usable);
  }

  function roleValue(role: ImageRole): string {
    return role.kind === "shot" ? `shot:${role.shot}` : role.kind;
  }

  function parseRole(value: string): ImageRole {
    if (value.startsWith("shot:")) return { kind: "shot", shot: Number(value.slice(5)) };
    return value === "character" ? { kind: "character" } : { kind: "location" };
  }

  function lineText(id: string | null): string {
    const line = animeScene.lines.find((l) => l.id === id);
    if (!line) return "";
    return line.text.length > 40 ? `${line.text.slice(0, 40)}…` : line.text;
  }

  function setLead(shot: SceneShot, value: string) {
    const n = Number(value);
    animeScene.updateShot(shot.id, { lead: Number.isFinite(n) ? Math.min(10, Math.max(0, n)) : 0 });
  }

  /** Mirrors `cloud::scene::keyframe::size_for`. */
  const KEYFRAME_SIZES: Record<string, { width: number; height: number }> = {
    "16:9": { width: 1216, height: 832 },
    "9:16": { width: 832, height: 1216 },
    "4:3": { width: 1152, height: 896 },
    "3:4": { width: 896, height: 1152 },
    "1:1": { width: 1024, height: 1024 },
    "21:9": { width: 1472, height: 640 },
  };
  const keyframeSize = $derived(KEYFRAME_SIZES[animeScene.video.aspect] ?? KEYFRAME_SIZES["16:9"]);
  const keyframeAnlas = $derived(animeScene.keyframeAnlas(keyframeSize.width, keyframeSize.height));
  const pendingShots = $derived(new Set(Object.values(animeScene.pendingKeyframes)));
  const canKeyframe = $derived(
    novelai.apiKeyConfigured && animeScene.characterReference !== null && animeScene.video.characterTags.trim() !== "",
  );
  let copyingTags = $state(false);

  /** Fill the character tags from the character image's own prompt. */
  async function copyCharacterTags() {
    const ref = animeScene.characterReference;
    if (!ref) return;
    copyingTags = true;
    try {
      const meta = await readImageMetadata(ref.filename);
      const prompt = meta?.positive_prompt?.trim();
      if (prompt) animeScene.updateVideo({ characterTags: prompt });
      else animeScene.failKeyframe(locale.t("scene.keyframe.no_prompt"));
    } catch {
      animeScene.failKeyframe(locale.t("scene.keyframe.no_prompt"));
    } finally {
      copyingTags = false;
    }
  }

  function setSeed(value: string) {
    const n = Number.parseInt(value, 10);
    animeScene.updateVideo({ seed: value.trim() === "" || !Number.isFinite(n) ? null : Math.max(0, n) });
  }
</script>

<section class="bg-neutral-900 rounded-xl border border-neutral-800 p-5 space-y-4">
  <div class="flex items-baseline justify-between gap-2">
    <h2 class="text-sm font-medium text-neutral-200">{locale.t("scene.video.title")}</h2>
    {#if caps}
      <span class="text-[10px] text-neutral-500">{caps.label}</span>
    {/if}
  </div>

  {#if !hasKey}
    <p class="text-xs text-neutral-300">{locale.t("scene.video.no_key")}</p>
    <button class="px-3 py-2 rounded-lg text-sm bg-indigo-600 hover:bg-indigo-500 text-white transition-colors" onclick={onOpenSettings}>
      {locale.t("scene.open_settings")}
    </button>
  {:else}
    <p class="text-[10px] text-neutral-500">{locale.t("scene.video.desc")}</p>

    <!-- References -->
    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <h3 class="text-xs font-medium text-neutral-300">{locale.t("scene.video.references")}</h3>
        <button class={small} disabled={slotsLeft === 0} onclick={() => (pickerOpen = true)}>
          {locale.t("scene.video.add_references")}
        </button>
      </div>
      <p class="text-[10px] text-neutral-500">{locale.t("scene.video.references_desc")}</p>
      {#if pickError}<p class="text-[10px] text-amber-400">{pickError}</p>{/if}
      {#if animeScene.references.length === 0}
        <p class="text-xs text-neutral-500">{locale.t("scene.video.no_references")}</p>
      {:else}
        <div class="grid gap-2 grid-cols-2 sm:grid-cols-4">
          {#each animeScene.references as ref, i (ref.filename)}
            <div class="rounded-lg border border-neutral-800 bg-neutral-950 p-1.5 space-y-1">
              {#if ref.thumb}
                <img src={ref.thumb} alt={ref.filename} class="w-full aspect-square object-cover rounded" />
              {:else}
                <p class="text-[10px] text-neutral-400 break-all">{ref.filename}</p>
              {/if}
              <label class="sr-only" for="scene-ref-role-{i}">{locale.t("scene.video.role")}</label>
              <select
                id="scene-ref-role-{i}"
                class={input}
                value={roleValue(ref.role)}
                onchange={(e) => animeScene.setReferenceRole(ref.filename, parseRole((e.target as HTMLSelectElement).value))}
              >
                <option value="character">{locale.t("scene.video.role_character")}</option>
                <option value="location">{locale.t("scene.video.role_location")}</option>
                {#each animeScene.shots as _, s (s)}
                  <option value="shot:{s}">{locale.t("scene.video.role_shot", { n: s + 1 })}</option>
                {/each}
              </select>
              <button class="{small} w-full" onclick={() => animeScene.removeReference(ref.filename)}>
                {locale.t("scene.video.remove")}
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Keyframes -->
    <div class="space-y-2">
      <h3 class="text-xs font-medium text-neutral-300">{locale.t("scene.keyframe.title")}</h3>
      <p class="text-[10px] text-neutral-500">{locale.t("scene.keyframe.desc")}</p>
      {#if !novelai.apiKeyConfigured}
        <p class="text-[10px] text-amber-400">{locale.t("scene.keyframe.no_key")}</p>
      {/if}
      <div>
        <div class="flex items-center justify-between gap-2 mb-0.5">
          <label class="text-[10px] text-neutral-400" for="scene-character-tags">{locale.t("scene.keyframe.character_tags")}</label>
          <button class={small} disabled={!animeScene.characterReference || copyingTags} onclick={() => void copyCharacterTags()}>
            {locale.t("scene.keyframe.copy_tags")}
          </button>
        </div>
        <textarea
          id="scene-character-tags"
          rows="2"
          class={input}
          placeholder={locale.t("scene.keyframe.character_tags_ph")}
          value={animeScene.video.characterTags}
          onchange={(e) => animeScene.updateVideo({ characterTags: (e.target as HTMLTextAreaElement).value })}
        ></textarea>
        {#if animeScene.isMinor}
          <p class="text-[10px] text-neutral-500">{locale.t("scene.keyframe.minor_note")}</p>
        {/if}
      </div>
      {#if animeScene.keyframeError}
        <p class="text-xs text-red-400">{animeScene.keyframeError}</p>
      {/if}
    </div>

    <!-- Shots -->
    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <h3 class="text-xs font-medium text-neutral-300">{locale.t("scene.video.shots")}</h3>
        <button class={small} onclick={() => animeScene.addShot()}>{locale.t("scene.video.add_shot")}</button>
      </div>
      <p class="text-[10px] text-neutral-500">{locale.t("scene.video.shots_desc")}</p>
      {#if animeScene.readyLines.length === 0}
        <p class="text-[10px] text-amber-400">{locale.t("scene.video.no_takes")}</p>
      {/if}
      {#each animeScene.shots as shot, i (shot.id)}
        {@const span = animeScene.plan?.shots[i]}
        <div class="rounded-lg border border-neutral-800 bg-neutral-950 p-3 space-y-2">
          <div class="flex items-center justify-between gap-2">
            <span class="text-xs text-neutral-300">
              {locale.t("scene.video.shot_n", { n: i + 1 })}
              {#if span}<span class="text-neutral-500"> · {span.start.toFixed(1)}–{span.end.toFixed(1)} s</span>{/if}
            </span>
            <div class="flex gap-1">
              <button class={small} disabled={i === 0} onclick={() => animeScene.moveShot(shot.id, -1)} aria-label={locale.t("scene.video.move_up")}>↑</button>
              <button class={small} disabled={i === animeScene.shots.length - 1} onclick={() => animeScene.moveShot(shot.id, 1)} aria-label={locale.t("scene.video.move_down")}>↓</button>
              <button class={small} onclick={() => animeScene.removeShot(shot.id)}>{locale.t("scene.video.remove")}</button>
            </div>
          </div>
          <div class="grid gap-2 sm:grid-cols-3">
            <div class="sm:col-span-2">
              <label class={label} for="scene-shot-line-{shot.id}">{locale.t("scene.video.shot_line")}</label>
              <select
                id="scene-shot-line-{shot.id}"
                class={input}
                value={shot.lineId ?? ""}
                onchange={(e) => {
                  const v = (e.target as HTMLSelectElement).value;
                  animeScene.updateShot(shot.id, { lineId: v || null });
                }}
              >
                <option value="">{locale.t("scene.video.shot_silent")}</option>
                {#each animeScene.readyLines as line (line.id)}
                  <option value={line.id}>{lineText(line.id)}</option>
                {/each}
              </select>
            </div>
            <div>
              <label class={label} for="scene-shot-lead-{shot.id}">
                {shot.lineId ? locale.t("scene.video.shot_pause") : locale.t("scene.video.shot_length")}
              </label>
              <input
                id="scene-shot-lead-{shot.id}"
                type="number" min="0" max="10" step="0.1"
                class={input}
                value={shot.lead}
                onchange={(e) => setLead(shot, (e.target as HTMLInputElement).value)}
              />
            </div>
          </div>
          <div>
            <label class={label} for="scene-shot-framing-{shot.id}">{locale.t("scene.video.shot_framing")}</label>
            <input
              id="scene-shot-framing-{shot.id}"
              class={input}
              placeholder={locale.t("scene.video.shot_framing_ph")}
              value={shot.framing}
              onchange={(e) => animeScene.updateShot(shot.id, { framing: (e.target as HTMLInputElement).value })}
            />
          </div>
          <div class="flex items-end gap-2">
            <div class="flex-1">
              <label class={label} for="scene-shot-keyframe-{shot.id}">{locale.t("scene.keyframe.shot_tags")}</label>
              <input
                id="scene-shot-keyframe-{shot.id}"
                class={input}
                placeholder={locale.t("scene.keyframe.shot_tags_ph")}
                value={shot.keyframeTags}
                onchange={(e) => animeScene.updateShot(shot.id, { keyframeTags: (e.target as HTMLInputElement).value })}
              />
            </div>
            <button
              class="px-2 py-1.5 rounded-lg text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-200 disabled:opacity-40 whitespace-nowrap"
              disabled={!canKeyframe || pendingShots.has(shot.id)}
              onclick={() => void animeScene.makeKeyframe(shot.id, keyframeSize)}
            >
              {pendingShots.has(shot.id)
                ? locale.t("scene.keyframe.running")
                : locale.t("scene.keyframe.make", { anlas: keyframeAnlas })}
            </button>
          </div>
          <div>
            <label class={label} for="scene-shot-action-{shot.id}">{locale.t("scene.video.shot_action")}</label>
            <input
              id="scene-shot-action-{shot.id}"
              class={input}
              placeholder={locale.t("scene.video.shot_action_ph")}
              value={shot.action}
              onchange={(e) => animeScene.updateShot(shot.id, { action: (e.target as HTMLInputElement).value })}
            />
          </div>
        </div>
      {/each}
    </div>

    <!-- Scene details -->
    <div class="space-y-2">
      <h3 class="text-xs font-medium text-neutral-300">{locale.t("scene.video.details")}</h3>
      <div class="grid gap-2 sm:grid-cols-2">
        <div class="sm:col-span-2">
          <label class={label} for="scene-traits">{locale.t("scene.video.traits")}</label>
          <input id="scene-traits" class={input} placeholder={locale.t("scene.video.traits_ph")} value={animeScene.video.characterTraits}
            onchange={(e) => animeScene.updateVideo({ characterTraits: (e.target as HTMLInputElement).value })} />
        </div>
        <div>
          <label class={label} for="scene-start">{locale.t("scene.video.starting_state")}</label>
          <input id="scene-start" class={input} value={animeScene.video.startingState}
            onchange={(e) => animeScene.updateVideo({ startingState: (e.target as HTMLInputElement).value })} />
        </div>
        <div>
          <label class={label} for="scene-end">{locale.t("scene.video.ending_state")}</label>
          <input id="scene-end" class={input} value={animeScene.video.endingState}
            onchange={(e) => animeScene.updateVideo({ endingState: (e.target as HTMLInputElement).value })} />
        </div>
        <div>
          <label class={label} for="scene-ambience">{locale.t("scene.video.ambience")}</label>
          <input id="scene-ambience" class={input} placeholder={locale.t("scene.video.ambience_ph")} value={animeScene.video.ambience}
            onchange={(e) => animeScene.updateVideo({ ambience: (e.target as HTMLInputElement).value })} />
        </div>
        <div>
          <label class={label} for="scene-video-language">{locale.t("scene.video.language")}</label>
          <input id="scene-video-language" class={input} value={animeScene.video.language}
            onchange={(e) => animeScene.updateVideo({ language: (e.target as HTMLInputElement).value })} />
        </div>
        <div>
          <label class={label} for="scene-tail">{locale.t("scene.video.tail")}</label>
          <input id="scene-tail" type="number" min="0" max="10" step="0.1" class={input} value={animeScene.video.tail}
            onchange={(e) => {
              const n = Number((e.target as HTMLInputElement).value);
              animeScene.updateVideo({ tail: Number.isFinite(n) ? Math.min(10, Math.max(0, n)) : 1 });
            }} />
        </div>
        <label class="flex items-center gap-2 select-none cursor-pointer self-end pb-1.5">
          <input type="checkbox" class="w-4 h-4 rounded accent-indigo-500" checked={animeScene.video.continuous}
            onchange={(e) => animeScene.updateVideo({ continuous: (e.target as HTMLInputElement).checked })} />
          <span class="text-xs text-neutral-300">{locale.t("scene.video.continuous")}</span>
        </label>
      </div>
    </div>

    <!-- Render settings -->
    {#if caps}
      <div class="grid gap-2 sm:grid-cols-4">
        <div>
          <label class={label} for="scene-resolution">{locale.t("scene.video.resolution")}</label>
          <select id="scene-resolution" class={input} value={animeScene.video.resolution}
            onchange={(e) => animeScene.updateVideo({ resolution: (e.target as HTMLSelectElement).value })}>
            {#each caps.resolutions as r (r)}<option value={r}>{r}</option>{/each}
          </select>
        </div>
        <div>
          <label class={label} for="scene-aspect">{locale.t("scene.video.aspect")}</label>
          <select id="scene-aspect" class={input} value={animeScene.video.aspect}
            onchange={(e) => animeScene.updateVideo({ aspect: (e.target as HTMLSelectElement).value })}>
            {#each caps.aspect_ratios as a (a)}<option value={a}>{a}</option>{/each}
          </select>
        </div>
        <div>
          <label class={label} for="scene-seed">{locale.t("scene.video.seed")}</label>
          <input id="scene-seed" type="number" min="0" class={input} placeholder={locale.t("scene.video.seed_random")}
            value={animeScene.video.seed ?? ""} onchange={(e) => setSeed((e.target as HTMLInputElement).value)} />
        </div>
        {#if caps.draft}
          <label class="flex items-center gap-2 select-none cursor-pointer self-end pb-1.5">
            <input type="checkbox" class="w-4 h-4 rounded accent-indigo-500" checked={animeScene.video.draft}
              onchange={(e) => animeScene.updateVideo({ draft: (e.target as HTMLInputElement).checked })} />
            <span class="text-xs text-neutral-300">{locale.t("scene.video.draft")}</span>
          </label>
        {/if}
      </div>
      {#if caps.draft && animeScene.video.draft}
        <p class="text-[10px] text-neutral-500">{locale.t("scene.video.draft_desc")}</p>
      {/if}
      <label class="flex items-start gap-2 select-none cursor-pointer">
        <input type="checkbox" class="w-4 h-4 mt-0.5 rounded accent-indigo-500" checked={animeScene.video.mouthMap}
          onchange={(e) => animeScene.updateVideo({ mouthMap: (e.target as HTMLInputElement).checked })} />
        <span>
          <span class="text-xs text-neutral-300 block">{locale.t("scene.video.mouth_map")}</span>
          <span class="text-[10px] text-neutral-500">{locale.t("scene.video.mouth_map_desc")}</span>
        </span>
      </label>
    {/if}

    <!-- Plan and render -->
    <div class="flex flex-wrap gap-2">
      <button
        class="px-3 py-2 rounded-lg text-sm bg-neutral-800 hover:bg-neutral-700 text-neutral-200 transition-colors disabled:opacity-40"
        disabled={animeScene.planning || animeScene.shots.length === 0}
        onclick={() => void animeScene.planScene()}
      >
        {animeScene.planning ? locale.t("scene.video.planning") : locale.t("scene.video.preview")}
      </button>
      <button
        class="px-3 py-2 rounded-lg text-sm bg-indigo-600 hover:bg-indigo-500 text-white transition-colors disabled:opacity-40"
        disabled={animeScene.starting || animeScene.planning || animeScene.shots.length === 0}
        onclick={() => void animeScene.generateVideo()}
      >
        {animeScene.starting ? locale.t("scene.video.starting") : locale.t("scene.video.generate")}
      </button>
    </div>
    {#if animeScene.videoError}
      <p class="text-xs text-red-400 whitespace-pre-wrap">{animeScene.videoError}</p>
    {/if}
    {#if animeScene.activeJobs.length > 0}
      <p class="text-xs text-indigo-300">{locale.t("scene.video.running", { count: animeScene.activeJobs.length })}</p>
    {/if}
    <p class="text-[10px] text-neutral-500">{locale.t("scene.cost.cancel_note")}</p>

    {#if animeScene.plan}
      {@const plan = animeScene.plan}
      <div class="space-y-1">
        <p class="text-xs text-neutral-300">
          {locale.t("scene.video.plan_summary", {
            seconds: plan.seconds,
            track: plan.track_seconds.toFixed(1),
            usd: plan.estimate.usd.toFixed(2),
            resolution: plan.estimate.resolution,
          })}
        </p>
        {#if animeScene.video.mouthMap && plan.mouth_measured + plan.mouth_missing > 0}
          <div class="flex flex-wrap items-center gap-2">
            <p class="text-[10px] text-neutral-400">
              {locale.t("scene.video.mouth_status", { measured: plan.mouth_measured, total: plan.mouth_measured + plan.mouth_missing })}
            </p>
            {#if plan.mouth_missing > 0}
              <button class={small} disabled={animeScene.measuring} onclick={() => void animeScene.measureMouth()}>
                {animeScene.measuring ? locale.t("scene.video.mouth_measuring") : locale.t("scene.video.mouth_measure")}
              </button>
            {/if}
          </div>
        {/if}
        {#if animeScene.mouthError}
          <p class="text-xs text-red-400 whitespace-pre-wrap">{animeScene.mouthError}</p>
        {/if}
        <div class="flex items-center justify-between gap-2">
          <label class={label} for="scene-prompt-preview">{locale.t("scene.video.prompt", { version: plan.builder_version })}</label>
          {#if animeScene.promptOverride !== null}
            <button class={small} onclick={() => (animeScene.promptOverride = null)}>{locale.t("scene.video.prompt_reset")}</button>
          {/if}
        </div>
        <textarea
          id="scene-prompt-preview"
          rows="12"
          class="{input} font-mono"
          value={animeScene.promptOverride ?? plan.prompt}
          oninput={(e) => animeScene.setPromptOverride((e.target as HTMLTextAreaElement).value)}
        ></textarea>
        <p class="text-[10px] text-neutral-500">{locale.t("scene.video.prompt_desc")}</p>
      </div>
    {/if}
  {/if}
</section>

<GalleryPickerModal
  open={pickerOpen}
  multiple
  max={slotsLeft}
  title={locale.t("scene.video.pick_title")}
  onselect={attach}
  onclose={() => (pickerOpen = false)}
/>
