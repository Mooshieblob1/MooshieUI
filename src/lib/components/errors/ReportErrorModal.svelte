<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { reportError } from "../../errors/reportError.js";
  import type { FriendlyError } from "../../errors/types.js";

  let {
    error,
    onclose,
    generic = false,
  }: { error: FriendlyError; onclose: () => void; generic?: boolean } = $props();

  let userNote = $state("");
  let submitting = $state(false);
  let copiedHint = $state(false);

  async function submit() {
    submitting = true;
    try {
      await reportError(error, userNote);
      copiedHint = true;
      setTimeout(onclose, 1500);
    } catch {
      submitting = false;
    }
  }
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
  onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}
  onkeydown={(e) => { if (e.key === "Escape") onclose(); }}
  role="dialog"
  aria-modal="true"
  tabindex="-1"
>
  <div class="w-full max-w-md space-y-4 rounded-xl border border-neutral-700 bg-neutral-900 p-6 shadow-2xl">
    <h3 class="text-base font-semibold text-neutral-100">{locale.t(generic ? "errors.report.title_generic" : "errors.report.title")}</h3>
    <p class="text-sm text-neutral-400">{locale.t(generic ? "errors.report.intro_generic" : "errors.report.intro")}</p>

    <div>
      <label class="mb-1 block text-xs text-neutral-400">{locale.t("errors.report.note_label")}</label>
      <textarea
        bind:value={userNote}
        placeholder={locale.t("errors.report.note_placeholder")}
        rows="4"
        class="w-full resize-y rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
      ></textarea>
    </div>

    {#if copiedHint}
      <p class="text-xs text-emerald-400">{locale.t("errors.report.copied_hint")}</p>
    {/if}

    <div class="flex justify-end gap-3 pt-1">
      <button onclick={onclose} class="rounded-lg bg-neutral-800 px-4 py-2 text-sm text-neutral-300 hover:bg-neutral-700">
        {locale.t("common.cancel")}
      </button>
      <button
        onclick={submit}
        disabled={submitting}
        class="rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {submitting ? locale.t("errors.report.opening") : locale.t("errors.report.submit")}
      </button>
    </div>
  </div>
</div>
