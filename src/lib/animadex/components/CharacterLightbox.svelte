<script lang="ts">
  import type { AnimadexCharacter } from "../types.js";
  import { buildCharacterInsertText } from "../characterInsert.js";
  import { locale } from "../../stores/locale.svelte.js";

  interface Props {
    character: AnimadexCharacter;
    onclose: () => void;
    oninsertCharacter?: (character: AnimadexCharacter) => void;
    onprev?: () => void;
    onnext?: () => void;
  }

  let { character, onclose, oninsertCharacter, onprev, onnext }: Props = $props();

  let copied = $state(false);

  function onBackdropKey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
    if (e.key === "ArrowLeft") {
      e.preventDefault();
      onprev?.();
    }
    if (e.key === "ArrowRight") {
      e.preventDefault();
      onnext?.();
    }
  }

  async function copyText(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      window.setTimeout(() => {
        copied = false;
      }, 1500);
    } catch {
      /* clipboard may be unavailable */
    }
  }

  const allTagsText = $derived(buildCharacterInsertText(character, "all"));
  const nameCopyrightText = $derived(buildCharacterInsertText(character, "name_copyright"));
</script>

<svelte:window onkeydown={onBackdropKey} />

<div
  class="fixed inset-0 z-50 flex items-center justify-center overflow-hidden bg-black/90 p-3 backdrop-blur-md sm:p-5"
  role="dialog"
  aria-modal="true"
  aria-label={locale.t("animadex.lightbox.aria", { name: character.name })}
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label={locale.t("animadex.lightbox.close_aria")}
    onclick={onclose}
  ></button>

  {#if onprev}
    <button
      type="button"
      onclick={onprev}
      class="absolute left-2 top-1/2 z-20 flex h-10 w-10 -translate-y-1/2 items-center justify-center rounded-full border border-neutral-700 bg-neutral-900/90 text-neutral-200 hover:border-indigo-500 sm:left-4"
      aria-label={locale.t("animadex.lightbox.prev_aria")}
    >←</button>
  {/if}
  {#if onnext}
    <button
      type="button"
      onclick={onnext}
      class="absolute right-2 top-1/2 z-20 flex h-10 w-10 -translate-y-1/2 items-center justify-center rounded-full border border-neutral-700 bg-neutral-900/90 text-neutral-200 hover:border-indigo-500 sm:right-4"
      aria-label={locale.t("animadex.lightbox.next_aria")}
    >→</button>
  {/if}

  <div
    class="relative z-10 grid w-full max-h-[min(96vh,920px)] max-w-[min(72rem,calc(100vw-2.5rem))] grid-cols-1 gap-5 overflow-x-hidden overflow-y-auto overscroll-contain rounded-[var(--app-shell-radius)] border border-neutral-700 bg-neutral-900 p-4 shadow-2xl shadow-black/50 sm:p-5 md:grid-cols-[minmax(0,1.05fr)_minmax(0,1fr)] md:items-start"
  >
    <div class="flex min-w-0 flex-col items-center justify-center">
      {#if character.has_image && character.img_url}
        <img
          src={character.img_url}
          alt={character.name}
          class="max-h-[min(72vh,560px)] w-auto max-w-full rounded-lg object-contain"
        />
      {:else}
        <div class="flex aspect-3/4 w-full max-w-sm items-center justify-center rounded-lg bg-neutral-800 text-sm text-neutral-500">
          {locale.t("animadex.no_preview")}
        </div>
      {/if}
    </div>

    <div class="flex min-w-0 flex-col gap-3 text-sm text-neutral-200">
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0 flex-1">
          <h2 class="text-lg font-semibold break-words text-neutral-100">{character.name}</h2>
          <p class="text-xs break-words text-neutral-500">{character.copyright_name}</p>
        </div>
        <button
          type="button"
          class="text-neutral-500 hover:text-neutral-200"
          onclick={onclose}
          aria-label={locale.t("animadex.lightbox.close_aria")}
        >✕</button>
      </div>

      <section>
        <h3 class="mb-1 text-[10px] font-medium uppercase tracking-wide text-neutral-500">
          {locale.t("animadex.lightbox.character_tags")}
        </h3>
        <p class="rounded bg-neutral-800 px-2 py-1.5 font-mono text-xs leading-relaxed break-words whitespace-normal">{nameCopyrightText}</p>
      </section>

      {#if character.tags.length > 0}
        <section>
          <h3 class="mb-1 text-[10px] font-medium uppercase tracking-wide text-neutral-500">
            {locale.t("animadex.lightbox.appearance_tags", { count: character.tags.length })}
          </h3>
          <div class="flex flex-wrap gap-1">
            {#each character.tags as tag (tag)}
              <span class="max-w-full rounded bg-neutral-800 px-1.5 py-0.5 text-xs break-words text-neutral-300">{tag}</span>
            {/each}
          </div>
        </section>
      {/if}

      {#if character.loras.length > 0}
        <section>
          <h3 class="mb-1 text-[10px] font-medium uppercase tracking-wide text-neutral-500">
            {locale.t("animadex.lightbox.loras")}
          </h3>
          <ul class="space-y-1 text-xs">
            {#each character.loras as lora (lora.id)}
              <li class="min-w-0">
                <a
                  href={lora.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  class="break-words text-indigo-400 hover:underline"
                >{lora.name}</a>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      <div class="mt-auto flex flex-wrap gap-2 pt-2 max-w-full">
        <button
          type="button"
          class="rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs hover:border-indigo-500"
          onclick={() => copyText(nameCopyrightText)}
        >
          {copied ? locale.t("animadex.copied") : locale.t("animadex.copy_tags")}
        </button>
        <button
          type="button"
          class="rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs hover:border-indigo-500"
          onclick={() => copyText(allTagsText)}
        >
          {locale.t("animadex.copy_all_tags")}
        </button>
        {#if oninsertCharacter}
          <button
            type="button"
            class="rounded-lg bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
            onclick={() => oninsertCharacter(character)}
          >
            {locale.t("animadex.insert_prompt")}
          </button>
        {/if}
        {#if character.url}
          <a
            href={character.url}
            target="_blank"
            rel="noopener noreferrer"
            class="rounded-lg border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs hover:border-indigo-500"
          >
            {locale.t("animadex.danbooru")}
          </a>
        {/if}
      </div>
    </div>
  </div>
</div>
