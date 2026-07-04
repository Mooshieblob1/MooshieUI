<script lang="ts">
  import { locale } from "../../stores/locale.svelte.js";
  import { exportLogsContent } from "../../utils/api.js";
  import type { FriendlyError } from "../../errors/types.js";
  import ReportErrorModal from "./ReportErrorModal.svelte";

  let { error, compact = false }: { error: FriendlyError; compact?: boolean } = $props();

  let detailsOpen = $state(false);
  let copied = $state(false);
  let showReport = $state(false);

  async function copyDiagnostics() {
    try {
      const logs = await exportLogsContent();
      await navigator.clipboard.writeText(logs || error.raw);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      // Clipboard unavailable; ignore.
    }
  }
</script>

<div class="rounded-lg border border-neutral-700 bg-neutral-900 p-4 text-sm text-neutral-200">
  <h3 class="text-base font-semibold text-neutral-100">{error.title}</h3>
  <p class="mt-1 text-neutral-300">{error.what}</p>

  {#if !compact}
    <p class="mt-2 text-neutral-400"><span class="text-neutral-500">{locale.t("errors.card.why_label")}:</span> {error.why}</p>

    {#if error.fixes.length}
      <p class="mt-3 text-neutral-300">{locale.t("errors.card.fix_label")}</p>
      <ol class="mt-1 list-decimal space-y-1 pl-5 text-neutral-300">
        {#each error.fixes as fix}
          <li>{fix}</li>
        {/each}
      </ol>
    {/if}
  {/if}

  <div class="mt-3 flex flex-wrap items-center gap-2">
    {#if error.reportable}
      <button
        onclick={() => (showReport = true)}
        class="rounded-lg bg-indigo-600 px-3 py-1.5 text-white transition-colors hover:bg-indigo-500"
      >
        {locale.t("errors.card.report")}
      </button>
    {/if}
    <button
      onclick={() => (detailsOpen = !detailsOpen)}
      class="rounded-lg bg-neutral-800 px-3 py-1.5 text-neutral-300 transition-colors hover:bg-neutral-700"
    >
      {locale.t("errors.card.details")}
    </button>
  </div>

  {#if detailsOpen}
    <div class="mt-3 rounded-lg bg-neutral-950 p-3">
      <pre class="max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs text-neutral-400">{error.raw}</pre>
      <button
        onclick={copyDiagnostics}
        class="mt-2 rounded-lg bg-neutral-800 px-3 py-1.5 text-xs text-neutral-300 transition-colors hover:bg-neutral-700"
      >
        {copied ? locale.t("errors.card.copied") : locale.t("errors.card.copy_diagnostics")}
      </button>
    </div>
  {/if}
</div>

{#if showReport}
  <ReportErrorModal {error} onclose={() => (showReport = false)} />
{/if}
