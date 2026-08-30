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
  import {
    directorTools,
    DIRECTOR_TOOLS,
    DEFRY_MAX,
    toolAlwaysCostsAnlas,
  } from "../../stores/directorTools.svelte.js";
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
      // Started from the lightbox, the result belongs there: what is on screen
      // is the source, and the point of the click is to see what replaced it.
      // Only when it is already open, so a card's DT button does not force a
      // lightbox the user did not ask for.
      if (gallery.lightboxOpen) gallery.markLightboxFollow(result.prompt_id);
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
    if (e.key === "Escape") {
      close();
      return;
    }
    // Ctrl+Enter runs the selected tool, the same as clicking its button.  At
    // the window rather than on one field, so it works wherever focus sits
    // inside the modal; run() is the one that decides whether there is
    // anything to run.
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void run();
    }
  }
</script>

<svelte:window onkeydown={directorTools.isOpen ? onKeydown : undefined} />

{#if directorTools.isOpen}
  <div
    data-modal-open
    class="fixed inset-0 z-70 flex items-center justify-center bg-black/70 p-4 sm:p-8"
    onclick={(e) => {
      if (e.currentTarget === e.target) close();
    }}
    role="presentation"
  >
    <div
      class="flex max-h-[90vh] w-full max-w-[880px] flex-col overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-6 shadow-2xl sm:p-7"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      aria-label={locale.t("novelai.director.title")}
    >
      <div class="mb-4 flex items-start justify-between gap-3">
        <div>
          <h2 class="text-lg font-semibold text-neutral-100">
            {locale.t("novelai.director.title")}
          </h2>
          <p class="mt-1 text-sm text-neutral-400">
            {locale.t("novelai.director.subtitle")}
          </p>
        </div>
        <!-- Sized to a comfortable pointer target rather than to the glyph: this
             is the only close affordance besides Escape and the backdrop. -->
        <button
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-neutral-600 text-base text-neutral-300 hover:bg-neutral-800"
          aria-label={locale.t("common.cancel")}
          onclick={close}
        >
          ✕
        </button>
      </div>

      <div class="flex flex-col gap-4 sm:flex-row">
        {#if directorTools.previewUrl}
          <img
            src={directorTools.previewUrl}
            alt={locale.t("novelai.director.source")}
            class="h-32 w-32 shrink-0 self-start rounded-lg border border-neutral-700 object-cover sm:h-40 sm:w-40"
          />
        {/if}
        <div class="grid flex-1 grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
          {#each DIRECTOR_TOOLS as tool (tool)}
            <button
              class="rounded-lg border px-3 py-2.5 text-left text-sm transition-colors {directorTools.tool ===
              tool
                ? 'border-indigo-500/60 bg-neutral-800/60 text-neutral-100'
                : 'border-neutral-700 bg-neutral-950 text-neutral-300 hover:bg-neutral-800'}"
              disabled={directorTools.busy}
              onclick={() => directorTools.selectTool(tool)}
            >
              <span class="block font-medium">
                {locale.t(`novelai.director.tool.${toolKey(tool)}`)}
              </span>
              <!-- Per-tool because only one rule is knowable here: background
                   removal is billed on every plan at every size. What the rest
                   cost depends on the source image's pixel size, which nothing
                   on this side carries, so the badge states the threshold. -->
              <span
                class="mt-1 inline-block rounded px-1.5 py-0.5 text-[11px] font-medium {toolAlwaysCostsAnlas(
                  tool,
                )
                  ? 'bg-amber-500/15 text-amber-300'
                  : 'bg-neutral-700/50 text-neutral-400'}"
              >
                {toolAlwaysCostsAnlas(tool)
                  ? locale.t("novelai.director.cost_anlas")
                  : locale.t("novelai.director.cost_free_1mp")}
              </span>
              <span class="mt-1.5 block text-xs leading-snug text-neutral-500">
                {locale.t(`novelai.director.tool.${toolKey(tool)}_hint`)}
              </span>
            </button>
          {/each}
        </div>
      </div>

      <!-- Shown up front rather than in the fine print at the bottom: a user
           weighing an Anlas balance wants the cost before picking a tool, and
           the default selection happens to be the one tool nobody gets free.
           It also carries the size rule the per-tool badges cannot state in
           full, since an upscaled source is charged on every plan. -->
      <p
        class="mt-4 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-200"
      >
        {locale.t("novelai.director.anlas_note")}
      </p>

      {#if directorTools.tool === "emotion"}
        <!-- Required: the mood is everything before the `;;` the backend joins
             on, and the endpoint has nothing to work from without it. NovelAI's
             accepted moods are not documented, so this is a free text box
             rather than a dropdown of guesses. -->
        <label class="mt-4 block">
          <span class="text-sm font-medium text-neutral-300">
            {locale.t("novelai.director.mood")}
          </span>
          <input
            class="mt-1.5 w-full rounded-lg border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
            placeholder={locale.t("novelai.director.mood_placeholder")}
            disabled={directorTools.busy}
            bind:value={directorTools.mood}
          />
          <span class="mt-1.5 block text-xs text-neutral-500">
            {locale.t("novelai.director.mood_hint")}
          </span>
        </label>
      {/if}

      {#if directorTools.takesExtras}
        <label class="mt-4 block">
          <span class="text-sm font-medium text-neutral-300">
            {locale.t("novelai.director.prompt")}
          </span>
          <textarea
            class="mt-1.5 min-h-[5rem] w-full resize-y rounded-lg border border-neutral-700 bg-neutral-950 p-3 text-sm text-neutral-100 placeholder:text-neutral-600 focus:border-indigo-500 focus:outline-none"
            placeholder={locale.t("novelai.director.prompt_placeholder")}
            disabled={directorTools.busy}
            bind:value={directorTools.prompt}
          ></textarea>
        </label>

        <div class="mt-4">
          <div class="flex items-center justify-between">
            <span class="text-sm font-medium text-neutral-300">
              {locale.t("novelai.director.defry")}
            </span>
            <span class="tabular-nums text-sm text-neutral-400">{directorTools.defry}</span>
          </div>
          <input
            type="range"
            min="0"
            max={DEFRY_MAX}
            step="1"
            class="mt-1.5 w-full accent-indigo-500"
            aria-label={locale.t("novelai.director.defry")}
            disabled={directorTools.busy}
            bind:value={directorTools.defry}
          />
          <span class="mt-1.5 block text-xs text-neutral-500">
            {locale.t("novelai.director.defry_hint")}
          </span>
        </div>
      {/if}

      {#if directorTools.error}
        <div class="mt-4 rounded-lg border border-red-500/40 bg-red-500/10 p-3 text-sm text-red-200">
          {directorTools.error}
        </div>
      {/if}

      <div class="mt-5 flex items-center justify-end gap-2">
        <button
          class="rounded-lg border border-neutral-600 px-4 py-2 text-sm text-neutral-300 hover:bg-neutral-800"
          onclick={close}
        >
          {locale.t("common.cancel")}
        </button>
        <button
          class="rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-500 disabled:opacity-40"
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
