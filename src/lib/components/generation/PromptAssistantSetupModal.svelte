<script lang="ts">
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import type { LlmCatalogEntry, LlmVariant } from "../../types/index.js";

  let { onClose, onInstalled }: { onClose: () => void; onInstalled?: () => void } =
    $props();

  const hw = $derived(promptAssistant.hardware);
  const catalog = $derived(promptAssistant.catalog);

  let selectedId = $state(promptAssistant.selectedModelId ?? "");
  let selectedVariant = $state("");

  $effect(() => {
    if (!selectedId && promptAssistant.selectedModelId) {
      selectedId = promptAssistant.selectedModelId;
    }
    if (selectedId && !selectedVariant) {
      selectedVariant = promptAssistant.defaultVariantKey(selectedId);
    }
  });

  function variantKey(v: LlmVariant): string {
    return `gguf:${v.quant}`;
  }

  function variantLabel(v: LlmVariant): string {
    return `GGUF ${v.quant}`;
  }

  function fits(v: LlmVariant): boolean {
    const vram = hw?.total_vram_mb ?? 0;
    // CPU path: allow if system RAM can hold it.
    if (vram < 2000) return (hw?.system_ram_mb ?? 0) * 0.6 >= v.vram_mb;
    return v.vram_mb <= vram;
  }

  function isInstalled(id: string): boolean {
    return promptAssistant.status?.installed_models.includes(id) ?? false;
  }

  let error = $state<string | null>(null);

  async function download() {
    if (!selectedId || !selectedVariant) return;
    error = null;
    try {
      promptAssistant.selectedModelId = selectedId;
      await promptAssistant.download(selectedId, selectedVariant);
      onInstalled?.();
      onClose();
    } catch (e: any) {
      error = String(e);
    }
  }

  function hardwareLabel(): string {
    if (!hw) return locale.t("prompt_assistant.detecting_hardware");
    if (hw.gpus.length === 0) {
      return locale.t("prompt_assistant.cpu_only");
    }
    const g = hw.gpus[0];
    const vramGb = (g.vram_mb / 1024).toFixed(0);
    return `${g.name} — ${vramGb} GB VRAM`;
  }
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
  onclick={onClose}
  role="presentation"
>
  <div
    class="max-h-[85vh] w-full max-w-2xl overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl"
    onclick={(e) => e.stopPropagation()}
    role="dialog"
    aria-modal="true"
  >
    <div class="mb-3 flex items-center justify-between">
      <h2 class="text-lg font-semibold text-neutral-100">
        {locale.t("prompt_assistant.setup_title")}
      </h2>
      <button
        class="rounded-lg px-2 py-1 text-neutral-400 hover:bg-neutral-800"
        onclick={onClose}
        aria-label={locale.t("common.close")}
      >
        ✕
      </button>
    </div>

    <!-- Hardware banner -->
    <div class="mb-4 rounded-lg border border-neutral-700 bg-neutral-800/50 px-3 py-2 text-sm text-neutral-300">
      {hardwareLabel()}
    </div>

    {#if error}
      <div class="mb-3 rounded-lg border border-red-700 bg-red-900/30 px-3 py-2 text-sm text-red-300">
        {error}
      </div>
    {/if}

    <!-- Model cards -->
    <div class="space-y-3">
      {#each catalog as entry (entry.id)}
        {@const recommended = entry.id === promptAssistant.recommendedModelId}
        {@const installed = isInstalled(entry.id)}
        <button
          class="w-full rounded-lg border p-3 text-left transition-colors {selectedId ===
          entry.id
            ? 'border-[var(--theme-accent-500)] bg-neutral-800'
            : 'border-neutral-700 bg-neutral-800/30 hover:bg-neutral-800/60'}"
          onclick={() => {
            selectedId = entry.id;
            selectedVariant = promptAssistant.defaultVariantKey(entry.id);
          }}
        >
          <div class="flex items-center justify-between">
            <span class="font-medium text-neutral-100">{entry.name}</span>
            <span class="flex gap-1">
              {#if recommended}
                <span class="rounded bg-[var(--theme-accent-600)] px-1.5 py-0.5 text-[10px] text-black">
                  {locale.t("prompt_assistant.recommended")}
                </span>
              {/if}
              {#if installed}
                <span class="rounded bg-green-700 px-1.5 py-0.5 text-[10px] text-white">
                  {locale.t("prompt_assistant.installed")}
                </span>
              {/if}
            </span>
          </div>
          <p class="mt-1 text-xs text-neutral-400">{entry.best_for}</p>
          <p class="mt-1 text-[11px] text-green-400">{entry.pros}</p>
          <p class="text-[11px] text-amber-400/80">{entry.cons}</p>

          {#if selectedId === entry.id}
            <div class="mt-2 flex flex-wrap gap-1.5">
              {#each entry.variants as v}
                {@const selectable = fits(v)}
                <button
                  disabled={!selectable}
                  title={!selectable
                    ? locale.t("prompt_assistant.needs_vram", {
                        gb: (v.vram_mb / 1024).toFixed(1),
                      })
                    : ""}
                  class="rounded border px-2 py-0.5 text-[10px] {selectedVariant ===
                  variantKey(v)
                    ? 'border-[var(--theme-accent-500)] text-neutral-100'
                    : 'border-neutral-600 text-neutral-400'} {selectable
                    ? 'hover:text-neutral-200'
                    : 'opacity-40'}"
                  onclick={(e) => {
                    e.stopPropagation();
                    if (selectable) selectedVariant = variantKey(v);
                  }}
                >
                  {variantLabel(v)} · {(v.size_mb / 1024).toFixed(1)} GB
                </button>
              {/each}
            </div>
          {/if}
        </button>
      {/each}
    </div>

    <!-- Download progress / action -->
    <div class="mt-4">
      {#if promptAssistant.isDownloading}
        {@const p = promptAssistant.downloadProgress}
        <div class="text-sm text-neutral-300">
          {locale.t("prompt_assistant.downloading")}
          {#if p && p.total > 0}
            <div class="mt-1 h-2 w-full overflow-hidden rounded bg-neutral-700">
              <div
                class="h-full bg-[var(--theme-accent-500)]"
                style="width: {((p.downloaded / p.total) * 100).toFixed(0)}%"
              ></div>
            </div>
            <span class="text-[11px] text-neutral-500">
              {(p.downloaded / 1024 / 1024).toFixed(0)} /
              {(p.total / 1024 / 1024).toFixed(0)} MB
            </span>
          {/if}
        </div>
      {:else}
        <button
          class="w-full rounded-lg bg-[var(--theme-accent-600)] px-4 py-2 font-medium text-black hover:bg-[var(--theme-accent-500)] disabled:opacity-50"
          disabled={!selectedId || !selectedVariant}
          onclick={download}
        >
          {isInstalled(selectedId)
            ? locale.t("prompt_assistant.use_model")
            : locale.t("prompt_assistant.download_install")}
        </button>
      {/if}
    </div>
  </div>
</div>
