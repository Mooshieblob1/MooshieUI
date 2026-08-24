<script lang="ts">
  /**
   * The NovelAI Director Tools form, in one app-wide modal.
   *
   * App-wide rather than a panel section because the tools run on an image the
   * user is already looking at (a session output, the live preview, a gallery
   * entry) and none of those live next to the generation settings.
   *
   * The `novelaiAugment` call lives here and not in the store, the same way
   * `NaiEnhanceModal` owns its rewrite: `directorTools` is a feature store and
   * keeps to being a state machine over this one form.
   *
   * Nothing here waits for the result. The backend reports through the same
   * synthetic prompt id and `comfyui:*` events a NovelAI generation uses, so
   * once the id is handed to `progress.enqueue` the images arrive in the
   * session grid and the gallery on their own.
   */
  import { directorTools, DIRECTOR_TOOLS, DEFRY_MAX } from "../../stores/directorTools.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { progress } from "../../stores/progress.svelte.js";
  import { novelaiAugment, type DirectorTool } from "../../utils/api.js";
  import {
    imageBytesToBase64,
    loadOutputImageForGenerationInput,
  } from "../../utils/galleryActions.js";

  /** `bg-removal` is the wire name; i18n keys cannot carry the hyphen. */
  function toolKey(tool: DirectorTool): string {
    return tool.replace(/-/g, "_");
  }

  async function run() {
    const source = directorTools.source;
    if (!source || !directorTools.canRun) return;
    directorTools.busy = true;
    directorTools.error = null;
    try {
      // The same loader img2img and inpainting use, so a JXL gallery entry is
      // decoded to PNG here rather than being read off disk raw.
      const { bytes } = await loadOutputImageForGenerationInput(source, "director_input.png");
      const result = await novelaiAugment({
        tool: directorTools.tool,
        image: imageBytesToBase64(bytes),
        defry: directorTools.defry,
        prompt: directorTools.prompt.trim(),
        mood: directorTools.mood.trim(),
      });
      // "image_edit" and not "img2img": the result is a pass over an existing
      // image, and tagging it img2img would overwrite that tab's last output.
      progress.enqueue(result.prompt_id, false, "image_edit", null);
      directorTools.dismiss();
      gallery.showToast(locale.t("novelai.director.started"), "success");
    } catch (e) {
      console.error("NovelAI Director Tool failed:", e);
      directorTools.error = String(e);
    } finally {
      directorTools.busy = false;
    }
  }

  function close() {
    directorTools.dismiss();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }

  function onFieldKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      run();
    }
  }
</script>

<svelte:window onkeydown={directorTools.isOpen ? onKeydown : undefined} />

{#if directorTools.isOpen}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 sm:p-8"
    onclick={(e) => {
      if (e.currentTarget === e.target) close();
    }}
    role="presentation"
  >
    <div
      class="flex max-h-[90vh] w-full max-w-[720px] flex-col overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-label={locale.t("novelai.director.title")}
    >
      <div class="mb-3 flex items-start justify-between gap-3">
        <div>
          <h2 class="text-sm font-semibold text-neutral-100">
            {locale.t("novelai.director.title")}
          </h2>
          <p class="mt-0.5 text-[11px] text-neutral-400">
            {locale.t("novelai.director.subtitle")}
          </p>
        </div>
        <button
          class="rounded-lg border border-neutral-600 px-2 py-0.5 text-[10px] text-neutral-300 hover:bg-neutral-800"
          onclick={close}
        >
          ✕
        </button>
      </div>

      <div class="flex gap-4">
        {#if directorTools.previewUrl}
          <img
            src={directorTools.previewUrl}
            alt={locale.t("novelai.director.source")}
            class="h-24 w-24 shrink-0 rounded-lg border border-neutral-700 object-cover"
          />
        {/if}
        <div class="grid flex-1 grid-cols-2 gap-2 sm:grid-cols-3">
          {#each DIRECTOR_TOOLS as tool (tool)}
            <button
              class="rounded-lg border px-2 py-1.5 text-left text-[11px] transition-colors {directorTools.tool ===
              tool
                ? 'border-indigo-500/60 bg-neutral-800/60 text-neutral-100'
                : 'border-neutral-700 bg-neutral-950 text-neutral-300 hover:bg-neutral-800'}"
              disabled={directorTools.busy}
              onclick={() => directorTools.selectTool(tool)}
            >
              <span class="block font-medium">
                {locale.t(`novelai.director.tool.${toolKey(tool)}`)}
              </span>
              <span class="mt-0.5 block text-[10px] leading-tight text-neutral-500">
                {locale.t(`novelai.director.tool.${toolKey(tool)}_hint`)}
              </span>
            </button>
          {/each}
        </div>
      </div>

      <!-- Shown up front rather than in the fine print at the bottom: unlike a
           generation, a Director Tool costs nothing, and that is the first
           thing a user weighing an Anlas balance wants to know. -->
      <p
        class="mt-3 rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-2 py-1.5 text-[11px] text-emerald-300"
      >
        {locale.t("novelai.director.anlas_note")}
      </p>

      {#if directorTools.tool === "emotion"}
        <!-- Required: the mood is everything before the `;;` the backend joins
             on, and the endpoint has nothing to work from without it. NovelAI's
             accepted moods are not documented, so this is a free text box
             rather than a dropdown of guesses. -->
        <label class="mt-4 block">
          <span class="text-[11px] font-medium text-neutral-300">
            {locale.t("novelai.director.mood")}
          </span>
          <input
            class="mt-1 w-full rounded-lg border border-neutral-700 bg-neutral-950 px-2 py-1.5 text-xs text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
            placeholder={locale.t("novelai.director.mood_placeholder")}
            disabled={directorTools.busy}
            bind:value={directorTools.mood}
            onkeydown={onFieldKeydown}
          />
          <span class="mt-1 block text-[10px] text-neutral-500">
            {locale.t("novelai.director.mood_hint")}
          </span>
        </label>
      {/if}

      {#if directorTools.takesExtras}
        <label class="mt-3 block">
          <span class="text-[11px] font-medium text-neutral-300">
            {locale.t("novelai.director.prompt")}
          </span>
          <textarea
            class="mt-1 min-h-[4rem] w-full resize-y rounded-lg border border-neutral-700 bg-neutral-950 p-2 text-xs text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
            placeholder={locale.t("novelai.director.prompt_placeholder")}
            disabled={directorTools.busy}
            bind:value={directorTools.prompt}
            onkeydown={onFieldKeydown}
          ></textarea>
        </label>

        <div class="mt-3">
          <div class="flex items-center justify-between">
            <span class="text-[11px] font-medium text-neutral-300">
              {locale.t("novelai.director.defry")}
            </span>
            <span class="tabular-nums text-[11px] text-neutral-400">{directorTools.defry}</span>
          </div>
          <input
            type="range"
            min="0"
            max={DEFRY_MAX}
            step="1"
            class="mt-1 w-full accent-indigo-500"
            aria-label={locale.t("novelai.director.defry")}
            disabled={directorTools.busy}
            bind:value={directorTools.defry}
          />
          <span class="mt-1 block text-[10px] text-neutral-500">
            {locale.t("novelai.director.defry_hint")}
          </span>
        </div>
      {/if}

      {#if directorTools.error}
        <div
          class="mt-3 rounded-lg border border-red-500/40 bg-red-500/10 p-2 text-[11px] text-red-200"
        >
          {directorTools.error}
        </div>
      {/if}

      <div class="mt-4 flex items-center justify-end gap-2">
        <button
          class="rounded-lg border border-neutral-600 px-3 py-1 text-xs text-neutral-300 hover:bg-neutral-800"
          onclick={close}
        >
          {locale.t("common.cancel")}
        </button>
        <button
          class="rounded-lg bg-indigo-600 px-3 py-1 text-xs text-white hover:bg-indigo-500 disabled:opacity-40"
          disabled={!directorTools.canRun}
          onclick={run}
        >
          {#if directorTools.busy}
            <span class="inline-block animate-spin">⟳</span>
            {locale.t("novelai.director.running")}
          {:else}
            {locale.t("novelai.director.run")}
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}
