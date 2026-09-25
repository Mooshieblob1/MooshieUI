<script lang="ts">
  import { onMount } from "svelte";
  import { locale } from "../../stores/locale.svelte.js";
  import { captureDiagnosticLog, reportDestination, reportError } from "../../errors/reportError.js";
  import type { FriendlyError } from "../../errors/types.js";

  let {
    error,
    onclose,
    generic = false,
  }: { error: FriendlyError; onclose: () => void; generic?: boolean } = $props();

  let userNote = $state("");
  let submitting = $state(false);
  let sentVia = $state<"service" | "github" | null>(null);
  // The report service posts whatever we send as a public GitHub issue, so the
  // log is opt-out and previewable there. The prefilled-issue path only copies
  // it to the clipboard for the user to paste themselves.
  // Unknown until the config is read; sending waits for it so the public-issue
  // notice is always shown before a report can go to the service.
  let destination = $state<"service" | "github" | null>(null);
  let includeLog = $state(true);
  let showLog = $state(false);
  let logPreview = $state<string | null>(null);

  onMount(async () => {
    destination = await reportDestination();
  });

  async function toggleLog() {
    showLog = !showLog;
    if (showLog && logPreview === null) {
      logPreview = await captureDiagnosticLog();
    }
  }

  async function submit() {
    submitting = true;
    try {
      const result = await reportError(error, userNote, {
        includeLog,
        log: logPreview ?? undefined,
      });
      sentVia = result.via;
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
    {#if destination === "service"}
      <p class="text-sm text-neutral-400">{locale.t("errors.report.intro_service")}</p>
    {:else}
      <p class="text-sm text-neutral-400">{locale.t(generic ? "errors.report.intro_generic" : "errors.report.intro")}</p>
    {/if}

    <div>
      <label for="report-note" class="mb-1 block text-xs text-neutral-400">{locale.t("errors.report.note_label")}</label>
      <textarea
        id="report-note"
        bind:value={userNote}
        placeholder={locale.t("errors.report.note_placeholder")}
        rows="4"
        class="w-full resize-y rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-2 text-sm text-neutral-100 placeholder-neutral-600 focus:border-indigo-500 focus:outline-none"
      ></textarea>
    </div>

    {#if destination === "service"}
      <div class="space-y-2">
        <label class="flex items-center gap-2 text-sm text-neutral-300">
          <input type="checkbox" bind:checked={includeLog} class="accent-indigo-500" />
          {locale.t("errors.report.include_log")}
        </label>
        <p class="text-xs text-neutral-500">{locale.t("errors.report.include_log_hint")}</p>
        <button onclick={toggleLog} class="text-xs text-indigo-400 hover:text-indigo-300">
          {showLog ? locale.t("errors.report.hide_log") : locale.t("errors.report.preview_log")}
        </button>
        {#if showLog}
          <textarea
            readonly
            rows="8"
            value={logPreview ?? locale.t("errors.report.loading_log")}
            class="w-full resize-y rounded-lg border border-neutral-700 bg-neutral-950 px-3 py-2 font-mono text-xs text-neutral-400"
          ></textarea>
        {/if}
      </div>
    {/if}

    {#if sentVia === "service"}
      <p class="text-xs text-emerald-400">{locale.t("errors.report.sent")}</p>
    {:else if sentVia === "github"}
      <p class="text-xs text-emerald-400">{locale.t("errors.report.copied_hint")}</p>
    {/if}

    <div class="flex justify-end gap-3 pt-1">
      <button onclick={onclose} class="rounded-lg bg-neutral-800 px-4 py-2 text-sm text-neutral-300 hover:bg-neutral-700">
        {locale.t("common.cancel")}
      </button>
      <button
        onclick={submit}
        disabled={submitting || destination === null}
        class="rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {#if destination === "service"}
          {submitting ? locale.t("errors.report.sending") : locale.t("errors.report.submit_service")}
        {:else}
          {submitting ? locale.t("errors.report.opening") : locale.t("errors.report.submit")}
        {/if}
      </button>
    </div>
  </div>
</div>
