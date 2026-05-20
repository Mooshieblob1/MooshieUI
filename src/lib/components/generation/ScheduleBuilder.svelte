<script lang="ts">
  import { generation } from "../../stores/generation.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";

  type Mode = "fromto" | "from" | "to" | "range";

  let mode = $state<Mode>("fromto");

  // Swap (SwarmUI fromto) inputs
  let swapPivot = $state(0.5);
  let swapBefore = $state("");
  let swapAfter = $state("");
  let swapSeparator = $state<"||" | "|" | ",">("||");

  // Schedule (MooshieUI) inputs
  let schedText = $state("");
  let schedStart = $state(0.5);
  let schedEnd = $state(0.8);

  function clamp01(v: number): number {
    if (isNaN(v)) return 0;
    return Math.max(0, Math.min(1, v));
  }

  function fmt(v: number): string {
    return locale.formatDecimalTrimmed(clamp01(v), 2);
  }

  /**
   * Pick the least-intrusive separator for SwarmUI fromto — if either side
   * already contains `,` we prefer `|`; if either already contains `|` we
   * prefer `||`. User can override via the dropdown.
   */
  const autoSeparator = $derived.by(() => {
    const combined = `${swapBefore} ${swapAfter}`;
    if (combined.includes("||")) return ",";
    if (combined.includes("|")) return "||";
    return "||";
  });

  // Keep dropdown in sync with auto-pick as the user types, but only if they
  // haven't manually changed it recently. Simpler: always drive from auto, allow
  // override via dropdown.
  $effect(() => {
    swapSeparator = autoSeparator;
  });

  const output = $derived.by(() => {
    if (mode === "fromto") {
      const before = swapBefore.trim();
      const after = swapAfter.trim();
      if (!before || !after) return "";
      const sep = swapSeparator === "," ? ", " : ` ${swapSeparator} `;
      return `<fromto[${fmt(swapPivot)}]:${before}${sep}${after}>`;
    }
    const text = schedText.trim();
    if (!text) return "";
    if (mode === "from") return `<from:${fmt(schedStart)}>${text}</from>`;
    if (mode === "to") return `<to:${fmt(schedEnd)}>${text}</to>`;
    if (mode === "range") {
      const lo = Math.min(schedStart, schedEnd);
      const hi = Math.max(schedStart, schedEnd);
      return `<range:${fmt(lo)}:${fmt(hi)}>${text}</range>`;
    }
    return "";
  });

  const description = $derived.by(() => {
    if (mode === "fromto") {
      const pivot = String(Math.round(clamp01(swapPivot) * 100));
      return locale.t("schedule.desc_fromto", { pivot });
    }
    if (mode === "from") {
      return locale.t("schedule.desc_from", { start: String(Math.round(clamp01(schedStart) * 100)) });
    }
    if (mode === "to") {
      return locale.t("schedule.desc_to", { end: String(Math.round(clamp01(schedEnd) * 100)) });
    }
    const lo = Math.min(schedStart, schedEnd);
    const hi = Math.max(schedStart, schedEnd);
    return locale.t("schedule.desc_range", {
      start: String(Math.round(clamp01(lo) * 100)),
      end: String(Math.round(clamp01(hi) * 100)),
    });
  });

  let copied = $state(false);
  let copyTimer: number | null = null;
  async function copyToClipboard() {
    if (!output) return;
    try {
      await navigator.clipboard.writeText(output);
      copied = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = window.setTimeout(() => (copied = false), 1200);
    } catch {
      /* ignore */
    }
  }

  function appendToPrompt(target: "positive" | "negative") {
    if (!output) return;
    const current =
      target === "positive" ? generation.positivePrompt : generation.negativePrompt;
    const trimmed = current.trimEnd();
    const sep = trimmed.length === 0 ? "" : trimmed.endsWith(",") ? " " : ", ";
    const next = `${trimmed}${sep}${output}`;
    if (target === "positive") generation.positivePrompt = next;
    else generation.negativePrompt = next;
  }
</script>

<div class="flex h-full flex-col overflow-y-auto px-3 py-2 text-neutral-200">
  <div class="mb-3">
    <h2 class="text-sm font-semibold text-neutral-100">{locale.t("bottom_panel.tab.schedule")}</h2>
    <p class="text-[11px] text-neutral-500">{locale.t("schedule.intro")}</p>
  </div>

  <!-- Mode tabs -->
  <div class="mb-3 flex flex-wrap gap-1">
    {#each [
      { id: "fromto", label: locale.t("schedule.mode_swap"), hint: locale.t("schedule.mode_swap_hint") },
      { id: "from", label: locale.t("schedule.mode_from"), hint: locale.t("schedule.mode_from_hint") },
      { id: "to", label: locale.t("schedule.mode_to"), hint: locale.t("schedule.mode_to_hint") },
      { id: "range", label: locale.t("schedule.mode_range"), hint: locale.t("schedule.mode_range_hint") },
    ] as m (m.id)}
      <button
        type="button"
        class="rounded-md border px-2.5 py-1 text-[11px] transition-colors {mode === m.id
          ? 'border-indigo-500 bg-indigo-500/10 text-indigo-200'
          : 'border-neutral-700 bg-neutral-800 text-neutral-400 hover:text-neutral-200'}"
        onclick={() => (mode = m.id as Mode)}
      >
        {m.label}
        <span class="ml-1 text-[9px] text-neutral-500">{m.hint}</span>
      </button>
    {/each}
  </div>

  {#if mode === "fromto"}
    <section class="mb-3 space-y-2 rounded-lg border border-neutral-800 bg-neutral-950/50 p-3">
      <p class="text-[11px] text-neutral-400">{locale.t("schedule.swap_desc")}</p>
      <div>
        <label for="sch-swap-before" class="mb-1 block text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("schedule.before")}</label>
        <input
          id="sch-swap-before"
          type="text"
          bind:value={swapBefore}
          placeholder={locale.t("schedule.before_placeholder")}
          class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
        />
      </div>
      <div>
        <label for="sch-swap-after" class="mb-1 block text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("schedule.after")}</label>
        <input
          id="sch-swap-after"
          type="text"
          bind:value={swapAfter}
          placeholder={locale.t("schedule.after_placeholder")}
          class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
        />
      </div>
      <div class="flex items-center gap-3">
        <label class="flex flex-1 items-center gap-2 text-[11px] text-neutral-400">
          <span class="w-14 shrink-0">{locale.t("schedule.pivot")}</span>
          <input
            type="range"
            min="0"
            max="1"
            step="0.05"
            bind:value={swapPivot}
            class="flex-1 accent-indigo-500"
          />
          <span class="w-10 shrink-0 text-right font-mono text-[11px] text-neutral-300">{fmt(swapPivot)}</span>
        </label>
        <label class="flex items-center gap-1 text-[11px] text-neutral-400">
          {locale.t("schedule.sep")}
          <select
            bind:value={swapSeparator}
            class="rounded border border-neutral-700 bg-neutral-800 px-1.5 py-1 text-[11px] text-neutral-200 focus:border-indigo-500 focus:outline-none"
          >
            <option value="||">||</option>
            <option value="|">|</option>
            <option value=",">,</option>
          </select>
        </label>
      </div>
    </section>
  {:else}
    <section class="mb-3 space-y-2 rounded-lg border border-neutral-800 bg-neutral-950/50 p-3">
      <div>
        <label for="sch-text" class="mb-1 block text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("schedule.text")}</label>
        <input
          id="sch-text"
          type="text"
          bind:value={schedText}
          placeholder={locale.t("schedule.text_placeholder")}
          class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
        />
      </div>
      {#if mode === "from"}
        <label class="flex items-center gap-2 text-[11px] text-neutral-400">
          <span class="w-14 shrink-0">{locale.t("schedule.start")}</span>
          <input type="range" min="0" max="1" step="0.05" bind:value={schedStart} class="flex-1 accent-indigo-500" />
          <span class="w-10 shrink-0 text-right font-mono text-[11px] text-neutral-300">{fmt(schedStart)}</span>
        </label>
      {:else if mode === "to"}
        <label class="flex items-center gap-2 text-[11px] text-neutral-400">
          <span class="w-14 shrink-0">{locale.t("schedule.end")}</span>
          <input type="range" min="0" max="1" step="0.05" bind:value={schedEnd} class="flex-1 accent-indigo-500" />
          <span class="w-10 shrink-0 text-right font-mono text-[11px] text-neutral-300">{fmt(schedEnd)}</span>
        </label>
      {:else if mode === "range"}
        <label class="flex items-center gap-2 text-[11px] text-neutral-400">
          <span class="w-14 shrink-0">{locale.t("schedule.start")}</span>
          <input type="range" min="0" max="1" step="0.05" bind:value={schedStart} class="flex-1 accent-indigo-500" />
          <span class="w-10 shrink-0 text-right font-mono text-[11px] text-neutral-300">{fmt(schedStart)}</span>
        </label>
        <label class="flex items-center gap-2 text-[11px] text-neutral-400">
          <span class="w-14 shrink-0">{locale.t("schedule.end")}</span>
          <input type="range" min="0" max="1" step="0.05" bind:value={schedEnd} class="flex-1 accent-indigo-500" />
          <span class="w-10 shrink-0 text-right font-mono text-[11px] text-neutral-300">{fmt(schedEnd)}</span>
        </label>
      {/if}
    </section>
  {/if}

  <!-- Preview -->
  <section class="mb-3 rounded-lg border border-neutral-800 bg-neutral-950/70 p-3">
    <div class="mb-1 flex items-center justify-between">
      <h3 class="text-[10px] uppercase tracking-wide text-neutral-500">{locale.t("schedule.preview")}</h3>
      <span class="text-[10px] text-neutral-500">{description}</span>
    </div>
    {#if output}
      <code class="block wrap-break-word rounded border border-neutral-800 bg-black/40 px-2 py-1.5 font-mono text-[12px] text-indigo-200">
        {output}
      </code>
    {:else}
      <p class="text-[11px] italic text-neutral-600">{locale.t("schedule.preview_empty")}</p>
    {/if}
  </section>

  <!-- Actions -->
  <div class="mb-2 flex flex-wrap gap-2">
    <button
      type="button"
      class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500 disabled:opacity-40 disabled:hover:border-neutral-700"
      onclick={() => appendToPrompt("positive")}
      disabled={!output}
    >{locale.t("schedule.append_positive")}</button>
    <button
      type="button"
      class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500 disabled:opacity-40 disabled:hover:border-neutral-700"
      onclick={() => appendToPrompt("negative")}
      disabled={!output}
    >{locale.t("schedule.append_negative")}</button>
    <button
      type="button"
      class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500 disabled:opacity-40 disabled:hover:border-neutral-700"
      onclick={copyToClipboard}
      disabled={!output}
    >
      {copied ? locale.t("schedule.copied") : locale.t("common.copy")}
    </button>
  </div>

  <!-- Cheat-sheet -->
  <details class="mt-2 rounded-lg border border-neutral-800 bg-neutral-950/50 p-3 text-[11px] text-neutral-400">
    <summary class="cursor-pointer text-neutral-300">{locale.t("schedule.syntax_reference")}</summary>
    <dl class="mt-2 space-y-1.5 font-mono text-[11px]">
      <div>
        <dt class="text-indigo-300">&lt;fromto[N]:A || B&gt;</dt>
        <dd class="text-neutral-400">{locale.t("schedule.syntax_fromto")}</dd>
      </div>
      <div>
        <dt class="text-indigo-300">&lt;from:N&gt;text&lt;/from&gt;</dt>
        <dd class="text-neutral-400">{locale.t("schedule.syntax_from")}</dd>
      </div>
      <div>
        <dt class="text-indigo-300">&lt;to:N&gt;text&lt;/to&gt;</dt>
        <dd class="text-neutral-400">{locale.t("schedule.syntax_to")}</dd>
      </div>
      <div>
        <dt class="text-indigo-300">&lt;range:A:B&gt;text&lt;/range&gt;</dt>
        <dd class="text-neutral-400">{locale.t("schedule.syntax_range")}</dd>
      </div>
    </dl>
  </details>
</div>
