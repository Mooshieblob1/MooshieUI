<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { animeScene, type SceneLine } from "../../stores/animeScene.svelte.js";

  const languages = ["ja", "en", "zh", "ko", "de", "fr", "es", "it", "pt", "pl", "ru"];

  // Object URLs for playback, loaded from disk when a take is first shown.
  let urls = $state<Record<string, string | null>>({});

  $effect(() => {
    for (const line of animeScene.lines) {
      for (const take of line.takes) {
        if (!(take.takeId in urls)) {
          urls = { ...urls, [take.takeId]: null };
          void animeScene.takeUrl(take.takeId).then((url) => {
            urls = { ...urls, [take.takeId]: url };
          });
        }
      }
    }
  });

  const pendingLineIds = $derived(
    animeScene.lines.filter((l) => l.takes.length === 0 && l.text.trim() !== "").map((l) => l.id),
  );

  function lineLocked(line: SceneLine): boolean {
    return line.blocked !== null && animeScene.isMinor;
  }

  function setSpeech<K extends keyof typeof animeScene.speech>(key: K, value: (typeof animeScene.speech)[K]) {
    animeScene.speech = { ...animeScene.speech, [key]: value };
    animeScene.saveSettings();
  }
</script>

<section class="bg-neutral-900 rounded-xl border border-neutral-800 p-5 space-y-4">
  <h2 class="text-sm font-medium text-neutral-200">{locale.t("scene.script.title")}</h2>

  <div class="grid gap-3 sm:grid-cols-3">
    <div>
      <label class="text-[10px] text-neutral-400 block mb-0.5" for="scene-language">{locale.t("scene.script.language")}</label>
      <select
        id="scene-language"
        class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-xs text-neutral-100"
        value={animeScene.speech.language_code}
        onchange={(e) => setSpeech("language_code", (e.target as HTMLSelectElement).value)}
      >
        {#each languages as code (code)}
          <option value={code}>{code}</option>
        {/each}
      </select>
    </div>
    <div>
      <label class="text-[10px] text-neutral-400 block mb-0.5" for="scene-stability">
        {locale.t("scene.script.stability", { value: animeScene.speech.stability.toFixed(2) })}
      </label>
      <input
        id="scene-stability"
        type="range" min="0" max="1" step="0.05"
        class="w-full accent-indigo-500"
        value={animeScene.speech.stability}
        onchange={(e) => setSpeech("stability", Number((e.target as HTMLInputElement).value))}
      />
    </div>
    <div>
      <label class="text-[10px] text-neutral-400 block mb-0.5" for="scene-similarity">
        {locale.t("scene.script.similarity", { value: animeScene.speech.similarity.toFixed(2) })}
      </label>
      <input
        id="scene-similarity"
        type="range" min="0" max="1" step="0.05"
        class="w-full accent-indigo-500"
        value={animeScene.speech.similarity}
        onchange={(e) => setSpeech("similarity", Number((e.target as HTMLInputElement).value))}
      />
    </div>
  </div>
  <label class="flex items-start gap-2 cursor-pointer select-none">
    <input
      type="checkbox"
      class="w-4 h-4 mt-0.5 rounded accent-indigo-500"
      checked={animeScene.speech.normalize_text}
      onchange={(e) => setSpeech("normalize_text", (e.target as HTMLInputElement).checked)}
    />
    <span>
      <span class="text-xs text-neutral-200 block">{locale.t("scene.script.normalize")}</span>
      <span class="text-[10px] text-neutral-500">{locale.t("scene.script.normalize_desc")}</span>
    </span>
  </label>

  {#if animeScene.lines.length === 0}
    <p class="text-xs text-neutral-500">{locale.t("scene.script.empty")}</p>
  {/if}

  <ol class="space-y-3">
    {#each animeScene.lines as line, i (line.id)}
      <li class="rounded-lg border border-neutral-800 bg-neutral-950/40 p-3 space-y-2">
        <div class="flex items-center justify-between gap-2">
          <span class="text-[10px] text-neutral-500">{locale.t("scene.script.line_n", { n: i + 1 })}</span>
          <div class="flex gap-1">
            <button class="px-1.5 py-0.5 rounded text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300 disabled:opacity-40" disabled={i === 0} onclick={() => animeScene.moveLine(line.id, -1)} aria-label={locale.t("scene.script.move_up")}>↑</button>
            <button class="px-1.5 py-0.5 rounded text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300 disabled:opacity-40" disabled={i === animeScene.lines.length - 1} onclick={() => animeScene.moveLine(line.id, 1)} aria-label={locale.t("scene.script.move_down")}>↓</button>
            <button class="px-1.5 py-0.5 rounded text-[10px] bg-neutral-800 hover:bg-neutral-700 text-neutral-300" onclick={() => animeScene.removeLine(line.id)}>{locale.t("scene.script.remove")}</button>
          </div>
        </div>
        <textarea
          rows="2"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 disabled:opacity-60"
          aria-label={locale.t("scene.script.line_text")}
          placeholder={locale.t("scene.script.line_text")}
          value={line.text}
          disabled={lineLocked(line)}
          onchange={(e) => animeScene.updateLine(line.id, { text: (e.target as HTMLTextAreaElement).value })}
        ></textarea>
        <input
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-2 py-1 text-xs text-neutral-100 placeholder-neutral-500 disabled:opacity-60"
          aria-label={locale.t("scene.script.delivery")}
          placeholder={locale.t("scene.script.delivery_hint")}
          value={line.delivery}
          disabled={lineLocked(line)}
          onchange={(e) => animeScene.updateLine(line.id, { delivery: (e.target as HTMLInputElement).value })}
        />
        {#if line.blocked}
          <p class="text-[10px] text-amber-400" role="status">
            {line.blocked}
            {#if lineLocked(line)}{locale.t("scene.script.blocked_final")}{/if}
          </p>
        {/if}
        {#each line.takes as take (take.takeId)}
          <div class="flex flex-wrap items-center gap-2">
            <input
              type="radio"
              name={`take-${line.id}`}
              class="accent-indigo-500"
              checked={line.chosenTakeId === take.takeId}
              onchange={() => animeScene.chooseTake(line.id, take.takeId)}
              aria-label={locale.t("scene.script.use_take", { n: take.takeIndex + 1 })}
            />
            <span class="text-[10px] text-neutral-400">{locale.t("scene.script.take_n", { n: take.takeIndex + 1 })}</span>
            {#if urls[take.takeId]}
              <audio controls src={urls[take.takeId]} class="h-8 max-w-full"></audio>
            {/if}
            {#if take.credits !== null}
              <span class="text-[10px] text-neutral-500">{locale.t("scene.script.take_credits", { credits: take.credits })}</span>
            {/if}
          </div>
        {/each}
        {#if line.takes.length > 0 && !lineLocked(line)}
          <button
            class="px-2 py-1 rounded-lg text-[10px] bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 text-neutral-300 transition-colors"
            disabled={animeScene.rendering || !animeScene.voiceId}
            onclick={() => { void animeScene.renderTakes([line.id], true); }}
          >
            {locale.t("scene.script.another_take")}
          </button>
        {/if}
      </li>
    {/each}
  </ol>

  <div class="flex flex-wrap gap-2">
    <button
      class="px-3 py-2 rounded-lg text-sm bg-neutral-800 hover:bg-neutral-700 text-neutral-200 transition-colors"
      onclick={() => animeScene.addLine()}
    >
      {locale.t("scene.script.add_line")}
    </button>
    <button
      class="px-3 py-2 rounded-lg text-sm bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 disabled:cursor-not-allowed text-white transition-colors"
      disabled={animeScene.rendering || !animeScene.voiceId || pendingLineIds.length === 0}
      onclick={() => { void animeScene.renderTakes(pendingLineIds, false); }}
    >
      {animeScene.rendering ? locale.t("scene.script.rendering") : locale.t("scene.script.render", { count: pendingLineIds.length })}
    </button>
  </div>
  {#if !animeScene.voiceId}
    <p class="text-[10px] text-neutral-500">{locale.t("scene.script.pick_voice_first")}</p>
  {/if}
  {#if animeScene.renderError}
    <p class="text-[10px] text-red-400">{animeScene.renderError}</p>
  {/if}
</section>
