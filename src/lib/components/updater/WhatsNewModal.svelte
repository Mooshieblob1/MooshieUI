<script lang="ts">
  import DOMPurify from "dompurify";
  import { marked } from "marked";
  import { portal } from "../../utils/portal.js";
  import { openExternalUrl } from "../../utils/openExternal.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { whatsNew } from "../../stores/whatsNew.svelte.js";

  const CHANGELOG_URL = "https://github.com/Mooshieblob1/MooshieUI/blob/main/CHANGELOG.md";
  const appVersion = __APP_VERSION__ ?? "dev";

  function renderMarkdown(markdown: string): string {
    const html = marked.parse(markdown, { async: false, breaks: true, gfm: true }) as string;
    return DOMPurify.sanitize(html);
  }

  function handleClose(): void {
    whatsNew.dismiss(appVersion);
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (whatsNew.open && e.key === "Escape") {
      e.preventDefault();
      handleClose();
    }
  }}
/>

{#if whatsNew.open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    use:portal
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
    onclick={(e) => { if (e.target === e.currentTarget) handleClose(); }}
    onkeydown={(e) => { if (e.key === "Escape") handleClose(); }}
    role="dialog"
    aria-modal="true"
    aria-labelledby="whats-new-title"
    tabindex="-1"
  >
    <div class="flex max-h-[85vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-neutral-700 bg-neutral-900 shadow-2xl">
      <div class="flex items-center justify-between gap-3 border-b border-neutral-800 px-5 py-4">
        <div class="flex items-center gap-2.5 min-w-0">
          <div class="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-indigo-500/15 text-indigo-400">
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M12 3l1.912 5.813a2 2 0 0 0 1.275 1.275L21 12l-5.813 1.912a2 2 0 0 0-1.275 1.275L12 21l-1.912-5.813a2 2 0 0 0-1.275-1.275L3 12l5.813-1.912a2 2 0 0 0 1.275-1.275L12 3z" />
            </svg>
          </div>
          <h2 id="whats-new-title" class="truncate text-base font-semibold text-neutral-100">
            {locale.t("whats_new.title")}
          </h2>
          {#if whatsNew.releases.length > 0}
            <span class="shrink-0 rounded-md border border-indigo-500/30 bg-indigo-500/15 px-2 py-0.5 font-mono text-xs font-medium text-indigo-300">
              v{whatsNew.releases[0].version}
            </span>
          {/if}
        </div>
        <button
          type="button"
          onclick={handleClose}
          class="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-neutral-400 transition-colors hover:bg-neutral-800 hover:text-neutral-200 cursor-pointer"
          aria-label={locale.t("common.close")}
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      </div>

      <div class="flex-1 space-y-5 overflow-y-auto px-5 py-4">
        {#if whatsNew.releases.length === 0}
          <p class="py-6 text-center text-sm text-neutral-500">{locale.t("whats_new.empty")}</p>
        {:else}
          {#each whatsNew.releases as release, i (release.version)}
            <section class={i > 0 ? "border-t border-neutral-800 pt-5" : ""}>
              <div class="mb-2.5 flex items-center gap-2">
                <span class="rounded border px-1.5 py-0.5 font-mono text-xs font-medium {i === 0 ? 'border-indigo-500/30 bg-indigo-500/15 text-indigo-300' : 'border-neutral-700 bg-neutral-800 text-neutral-300'}">
                  v{release.version}
                </span>
                <h3 class="text-sm font-semibold text-neutral-100">{release.heading}</h3>
              </div>
              <div
                class="text-sm leading-relaxed text-neutral-300 [&_h1]:mt-3 [&_h1]:mb-1.5 [&_h1]:text-sm [&_h1]:font-semibold [&_h1]:text-neutral-100 [&_h2]:mt-3 [&_h2]:mb-1.5 [&_h2]:text-xs [&_h2]:font-semibold [&_h2]:uppercase [&_h2]:tracking-wider [&_h2]:text-indigo-300 [&_h3]:mt-3 [&_h3]:mb-1.5 [&_h3]:text-xs [&_h3]:font-semibold [&_h3]:uppercase [&_h3]:tracking-wider [&_h3]:text-indigo-300 [&_p]:my-1.5 [&_ul]:my-1.5 [&_ul]:list-disc [&_ul]:space-y-1 [&_ul]:pl-5 [&_ol]:my-1.5 [&_ol]:list-decimal [&_ol]:space-y-1 [&_ol]:pl-5 [&_li]:text-neutral-300 [&_strong]:font-semibold [&_strong]:text-neutral-100 [&_code]:rounded [&_code]:bg-neutral-800 [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-xs [&_code]:text-indigo-200 [&_a]:text-indigo-400 [&_a]:underline [&_a]:underline-offset-2 hover:[&_a]:text-indigo-300"
              >
                {@html renderMarkdown(release.markdown)}
              </div>
            </section>
          {/each}
        {/if}
      </div>

      <div class="flex items-center justify-between gap-3 border-t border-neutral-800 bg-neutral-900 px-5 py-3.5">
        <button
          type="button"
          onclick={() => void openExternalUrl(CHANGELOG_URL)}
          class="inline-flex items-center gap-1.5 text-xs font-medium text-neutral-400 transition-colors hover:text-indigo-300 cursor-pointer"
        >
          <span>{locale.t("whats_new.full_changelog")}</span>
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
            <polyline points="15 3 21 3 21 9" />
            <line x1="10" y1="14" x2="21" y2="3" />
          </svg>
        </button>

        <button
          type="button"
          onclick={handleClose}
          class="rounded-lg bg-indigo-600 px-4 py-1.5 text-sm font-medium text-white transition-colors hover:bg-indigo-500 cursor-pointer"
        >
          {whatsNew.manual ? locale.t("common.close") : locale.t("whats_new.got_it")}
        </button>
      </div>
    </div>
  </div>
{/if}
