<script module lang="ts">
  type AnimaTag = { n: string; c: number };

  let animaArtistTagLookupPromise: Promise<Record<string, string>> | null = null;

  function normalizeArtistTagForLookup(tagName: string): string {
    return tagName.toLowerCase().replace(/ /g, "_").replace(/@/g, "");
  }

  async function getAnimaArtistTagLookup(): Promise<Record<string, string>> {
    if (!animaArtistTagLookupPromise) {
      animaArtistTagLookupPromise = import("../../assets/anima-tags.json")
        .then((module) => {
          const lookup: Record<string, string> = {};
          for (const tag of module.default as AnimaTag[]) {
            if (tag.c === 1) {
              lookup[normalizeArtistTagForLookup(tag.n)] = tag.n;
            }
          }
          return lookup;
        })
        .catch((error) => {
          animaArtistTagLookupPromise = null;
          throw error;
        });
    }

    return animaArtistTagLookupPromise;
  }
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import type { InterrogationResult, TagResult } from "../../types/index.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { promptAssistant } from "../../stores/promptAssistant.svelte.js";
  import { savedCharacters, type SavedCharacter } from "../../stores/savedCharacters.svelte.js";
  import { escapeArtistParens, isArtistTagToken } from "../../utils/artistTag.js";
  import { getConfig } from "../../utils/api.js";
  import { mapLlmError } from "../../utils/llmError.js";
  import { normalizeTag, pickCandidates, type DetectTags } from "../../utils/characterDetect.js";

  interface Props {
    result: InterrogationResult | null;
    loading: boolean;
    stage: string | null;
    downloadProgress: { downloaded: number; total: number; filename: string } | null;
    imagePreviewUrl: string | null;
    error: string | null;
    onclose: () => void;
  }

  let { result, loading, stage, downloadProgress, imagePreviewUrl, error, onclose }: Props = $props();

  // Track checked state per tag per category
  let checkedCharacter = $state<Record<string, boolean>>({});
  let checkedArtist = $state<Record<string, boolean>>({});
  let checkedGeneral = $state<Record<string, boolean>>({});
  let checkedCopyright = $state<Record<string, boolean>>({});

  /** Replace mode: true = replace entire prompt, false = append to existing */
  let replaceMode = $state(true);
  /** When replacing, preserve existing artist tags (@ prefixed, not @_@) from the current prompt */
  let preserveArtistTags = $state(false);

  // Initialize checked states when result changes
  $effect(() => {
    if (result) {
      checkedCharacter = Object.fromEntries(result.character_tags.map((t) => [t.name, true]));
      checkedArtist = Object.fromEntries(result.artist_tags.map((t) => [t.name, true]));
      checkedGeneral = Object.fromEntries(result.general_tags.map((t) => [t.name, true]));
      checkedCopyright = Object.fromEntries(result.copyright_tags.map((t) => [t.name, false]));
    }
  });

  /** A saved character the prompt assistant found in the image. */
  type SavedSuggestion = { character: SavedCharacter; covered: string[] };

  // Saved character detection (Settings > Interrogator > Detect saved characters)
  let detectEnabled = $state(false);
  let detecting = $state(false);
  let detectDone = $state(false);
  let detectError = $state<string | null>(null);
  let suggestions = $state<SavedSuggestion[]>([]);
  let checkedSaved = $state<Record<string, boolean>>({});
  /** Tag names each picked character switched off, so unpicking it can switch them back on. */
  let hiddenBy: Record<string, string[]> = {};
  /** Bumped per run, so a reply for an earlier image is dropped. */
  let detectRun = 0;

  const savedCount = $derived(savedCharacters.currentCharacters.length);

  $effect(() => {
    const r = result;
    untrack(() => void detectSavedCharacters(r));
  });

  async function detectSavedCharacters(r: InterrogationResult | null) {
    const run = ++detectRun;
    suggestions = [];
    checkedSaved = {};
    hiddenBy = {};
    detectError = null;
    detectDone = false;
    detecting = false;
    if (!r) return;
    let enabled = false;
    try {
      enabled = (await getConfig()).interrogator_detect_characters === true;
    } catch { /* keep it off */ }
    if (run !== detectRun) return;
    detectEnabled = enabled;
    const saved = savedCharacters.currentCharacters;
    if (!enabled || saved.length === 0 || !promptAssistant.isAvailable) return;
    const tags: DetectTags = {
      character: r.character_tags.map((t) => t.name),
      general: r.general_tags.map((t) => t.name),
      copyright: r.copyright_tags.map((t) => t.name),
    };
    if (tags.character.length === 0 && tags.general.length === 0) return;
    const candidates = pickCandidates(saved, tags).map((i) => saved[i]);
    detecting = true;
    try {
      const found = await promptAssistant.detectCharacters(tags, candidates);
      if (run !== detectRun) return;
      suggestions = found.map((m) => ({ character: candidates[m.index], covered: m.covered }));
      detectDone = true;
    } catch (e) {
      if (run !== detectRun) return;
      const msg = e instanceof Error ? e.message : String(e);
      detectError = msg.includes("invalid_character_detect")
        ? locale.t("generation.interrogate.character_match_invalid")
        : mapLlmError(msg);
    } finally {
      if (run === detectRun) detecting = false;
    }
  }

  /**
   * Pick or unpick a suggested character. Picking switches off the tags its
   * saved prompt already covers, so Apply doesn't write them twice; unpicking
   * switches back on the ones it switched off, unless another picked
   * character still covers them.
   */
  function toggleSaved(s: SavedSuggestion) {
    if (!result) return;
    const id = s.character.id;
    const on = !checkedSaved[id];
    checkedSaved = { ...checkedSaved, [id]: on };
    const covered = new Set(s.covered);
    if (on) {
      const hidden: string[] = [];
      const off = (map: Record<string, boolean>, list: TagResult[]) => {
        const next = { ...map };
        for (const t of list) {
          if (next[t.name] && covered.has(normalizeTag(t.name))) {
            next[t.name] = false;
            hidden.push(t.name);
          }
        }
        return next;
      };
      checkedCharacter = off(checkedCharacter, result.character_tags);
      checkedGeneral = off(checkedGeneral, result.general_tags);
      checkedCopyright = off(checkedCopyright, result.copyright_tags);
      hiddenBy = { ...hiddenBy, [id]: hidden };
    } else {
      const restore = new Set(hiddenBy[id] ?? []);
      const { [id]: _, ...rest } = hiddenBy;
      hiddenBy = rest;
      const stillCovered = new Set(
        suggestions.filter((o) => checkedSaved[o.character.id]).flatMap((o) => o.covered),
      );
      const back = (map: Record<string, boolean>, list: TagResult[]) => {
        const next = { ...map };
        for (const t of list) {
          if (restore.has(t.name) && !stillCovered.has(normalizeTag(t.name))) next[t.name] = true;
        }
        return next;
      };
      checkedCharacter = back(checkedCharacter, result.character_tags);
      checkedGeneral = back(checkedGeneral, result.general_tags);
      checkedCopyright = back(checkedCopyright, result.copyright_tags);
    }
  }

  // Rating display
  const ratingLabel = $derived.by(() => {
    if (!result || result.rating_tags.length === 0) return null;
    const top = result.rating_tags[0];
    return top;
  });

  function ratingColor(name: string): string {
    if (name.includes("general") || name.includes("safe")) return "bg-green-600";
    if (name.includes("sensitive")) return "bg-amber-600";
    if (name.includes("questionable")) return "bg-orange-600";
    if (name.includes("explicit")) return "bg-red-600";
    return "bg-neutral-600";
  }

  // Download progress display
  const downloadPercent = $derived(
    downloadProgress && downloadProgress.total > 0
      ? Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)
      : 0
  );

  // Collapsed sections
  let collapsedSections = $state<Record<string, boolean>>({});

  function toggleCollapse(section: string) {
    collapsedSections = { ...collapsedSections, [section]: !collapsedSections[section] };
  }

  // Select all / deselect all
  function selectAll() {
    if (result) {
      checkedCharacter = Object.fromEntries(result.character_tags.map((t) => [t.name, true]));
      checkedArtist = Object.fromEntries(result.artist_tags.map((t) => [t.name, true]));
      checkedGeneral = Object.fromEntries(result.general_tags.map((t) => [t.name, true]));
      checkedCopyright = Object.fromEntries(result.copyright_tags.map((t) => [t.name, true]));
    }
  }

  function deselectAll() {
    if (result) {
      checkedCharacter = Object.fromEntries(result.character_tags.map((t) => [t.name, false]));
      checkedArtist = Object.fromEntries(result.artist_tags.map((t) => [t.name, false]));
      checkedGeneral = Object.fromEntries(result.general_tags.map((t) => [t.name, false]));
      checkedCopyright = Object.fromEntries(result.copyright_tags.map((t) => [t.name, false]));
      checkedSaved = {};
      hiddenBy = {};
    }
  }

  const anyChecked = $derived.by(() => {
    return (
      Object.values(checkedSaved).some(Boolean) ||
      Object.values(checkedCharacter).some(Boolean) ||
      Object.values(checkedArtist).some(Boolean) ||
      Object.values(checkedGeneral).some(Boolean) ||
      Object.values(checkedCopyright).some(Boolean)
    );
  });

  /** Format an artist tag for Anima models with the `@` prefix. */
  function formatArtistTagForAnima(tagName: string, lookup?: Record<string, string>): string {
    const normalized = normalizeArtistTagForLookup(tagName);
    const animaMatch = lookup?.[normalized];
    if (animaMatch) {
      return animaMatch;
    }
    // Not in the anima list, so build the tag here: the sigil comes from
    // the store rather than a literal, and parens are escaped so the
    // prompt round-trips through the scheduler.
    return generation.animaArtistTagPrefix + escapeArtistParens(normalized);
  }

  async function handleApply() {
    if (!result) return;

    const isAnima = generation.isAnima;
    const artistTags: string[] = [];
    const otherTags: string[] = [];
    let animaArtistTagLookup: Record<string, string> | undefined;

    if (isAnima && result.artist_tags.some((tag) => checkedArtist[tag.name])) {
      try {
        animaArtistTagLookup = await getAnimaArtistTagLookup();
      } catch {
        animaArtistTagLookup = undefined;
      }
    }

    // Collect checked artist tags
    for (const tag of result.artist_tags) {
      if (checkedArtist[tag.name]) {
        if (isAnima) {
          artistTags.push(formatArtistTagForAnima(tag.name, animaArtistTagLookup));
        } else {
          artistTags.push(tag.name.replace(/_/g, " "));
        }
      }
    }

    // Collect checked character tags (before general)
    for (const tag of result.character_tags) {
      if (checkedCharacter[tag.name]) {
        otherTags.push(tag.name.replace(/_/g, " "));
      }
    }

    // Collect checked general tags
    for (const tag of result.general_tags) {
      if (checkedGeneral[tag.name]) {
        otherTags.push(tag.name.replace(/_/g, " "));
      }
    }

    // Collect checked copyright tags
    for (const tag of result.copyright_tags) {
      if (checkedCopyright[tag.name]) {
        otherTags.push(tag.name.replace(/_/g, " "));
      }
    }

    let prompt: string;

    if (replaceMode) {
      // Replace mode: start fresh, optionally preserving existing artist tags
      const parts: string[] = [];

      if (preserveArtistTags) {
        // Extract artist tags from the current prompt. The sigil is
        // mode-dependent, so NovelAI mode falls back to the artist index to
        // tell a bare artist tag apart from any other danbooru tag. `@_@` is
        // an emoticon tag, never an artist, so it is excluded either way.
        const existing = generation.positivePrompt.trim();
        if (existing) {
          const prefix = generation.artistTagPrefix;
          const index = gallery.artistIndexReady ? gallery.artistTagIndex : null;
          const existingTags = existing.split(",").map((t) => t.trim()).filter(Boolean);
          const preserved = existingTags.filter(
            (t) => isArtistTagToken(t, prefix, index) && !t.startsWith("@_@"),
          );
          if (preserved.length > 0) parts.push(preserved.join(", "));
        }
      }

      if (artistTags.length > 0) parts.push(artistTags.join(", "));
      if (otherTags.length > 0) parts.push(otherTags.join(", "));
      prompt = parts.filter(Boolean).join(", ");
    } else {
      // Append mode: artist tags prepend, character+general append
      prompt = generation.positivePrompt.trim();

      if (artistTags.length > 0) {
        const artistStr = artistTags.join(", ");
        prompt = prompt ? `${artistStr}, ${prompt}` : artistStr;
      }

      if (otherTags.length > 0) {
        const otherStr = otherTags.join(", ");
        prompt = prompt ? `${prompt}, ${otherStr}` : otherStr;
      }
    }

    generation.positivePrompt = prompt;
    // Picked saved characters go in as the Characters tab's Insert puts them:
    // a NovelAI character box when there is room, else the prompt, with
    // their UC. One already in the prompt is skipped.
    for (const s of suggestions) {
      if (checkedSaved[s.character.id]) savedCharacters.insert(s.character);
    }
    onclose();
  }

  async function handleCopy() {
    if (!result) return;

    const allTags: string[] = [];
    for (const tag of result.artist_tags) {
      if (checkedArtist[tag.name]) allTags.push(tag.name);
    }
    for (const tag of result.character_tags) {
      if (checkedCharacter[tag.name]) allTags.push(tag.name);
    }
    for (const tag of result.general_tags) {
      if (checkedGeneral[tag.name]) allTags.push(tag.name);
    }
    for (const tag of result.copyright_tags) {
      if (checkedCopyright[tag.name]) allTags.push(tag.name);
    }

    const text = allTags.join(", ");
    if (navigator.clipboard?.writeText) {
      try {
        await navigator.clipboard.writeText(text);
        return;
      } catch { /* fall through */ }
    }
    // Fallback for insecure (HTTP) contexts
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  }

  async function handleCopyAIBooru() {
    if (!result) return;

    const allTags: string[] = [];
    for (const tag of result.artist_tags) {
      if (checkedArtist[tag.name]) allTags.push(tag.name);
    }
    for (const tag of result.character_tags) {
      if (checkedCharacter[tag.name]) allTags.push(tag.name);
    }
    for (const tag of result.general_tags) {
      if (checkedGeneral[tag.name]) allTags.push(tag.name);
    }
    for (const tag of result.copyright_tags) {
      if (checkedCopyright[tag.name]) allTags.push(tag.name);
    }

    // AIBooru format: each tag uses underscores instead of spaces, tags
    // separated by spaces (no commas).
    const text = allTags.map((t) => t.replace(/\s+/g, "_")).join(" ");
    if (navigator.clipboard?.writeText) {
      try {
        await navigator.clipboard.writeText(text);
        return;
      } catch { /* fall through */ }
    }
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }

  /** Teleport element to document.body so it escapes overflow/transform containers */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      }
    };
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- Backdrop (portaled to body to escape overflow containers) -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  use:portal
  class="fixed inset-0 bg-black/70 z-50 flex items-center justify-center p-4"
  onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}
>
  <!-- Modal card -->
  <div class="bg-neutral-900 border border-neutral-700 rounded-2xl shadow-2xl w-full {imagePreviewUrl ? 'max-w-4xl' : 'max-w-3xl'} max-h-[85vh] flex flex-col">
    <!-- Header -->
    <div class="flex items-center gap-3 px-5 py-3 border-b border-neutral-700">
      <h2 class="text-lg font-semibold text-neutral-100 flex-1">{locale.t('generation.interrogate.title')}</h2>
      <button
        onclick={onclose}
        class="w-8 h-8 flex items-center justify-center rounded-lg hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18"/><path d="M6 6l12 12"/></svg>
      </button>
    </div>

    <!-- Body: the interrogated image stays in view beside the tags (above them on narrow screens) -->
    <div class="flex-1 min-h-0 flex flex-col md:flex-row">
    {#if imagePreviewUrl}
      <div class="shrink-0 flex justify-center px-5 pt-4 md:pr-0 md:pb-4 md:w-72 md:block">
        <img src={imagePreviewUrl} alt="" class="max-h-48 md:max-h-[60vh] max-w-full md:w-full object-contain rounded-lg border border-neutral-700 bg-neutral-950" />
      </div>
    {/if}
    <div class="flex-1 min-w-0 overflow-y-auto px-5 py-4 space-y-4">
      {#if downloadProgress && !downloadProgress.filename.includes("done")}
        <!-- Download progress -->
        <div class="space-y-2">
          <p class="text-sm text-neutral-300">{locale.t('generation.interrogate.downloading_model')}</p>
          <div class="h-2 bg-neutral-700 rounded-full overflow-hidden">
            <div class="h-full bg-indigo-600 rounded-full transition-all" style="width: {downloadPercent}%"></div>
          </div>
          <p class="text-xs text-neutral-500">{downloadProgress.filename} — {downloadPercent}%</p>
        </div>
      {:else if loading}
        <!-- Loading spinner with stage info -->
        <div class="flex flex-col items-center justify-center py-12 gap-3">
          <div class="flex items-center gap-3">
            <svg class="animate-spin h-5 w-5 text-indigo-400" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.37 0 0 5.37 0 12h4z"></path>
            </svg>
            <span class="text-neutral-300 text-sm">
              {#if stage === "loading_model"}
                {locale.t('generation.interrogate.loading_model')}
              {:else if stage === "running_inference"}
                {locale.t('generation.interrogate.running_inference')}
              {:else}
                {locale.t('generation.interrogate.preparing')}
              {/if}
            </span>
          </div>
          <p class="text-[10px] text-neutral-500">{locale.t('generation.interrogate.cpu_hint')}</p>
        </div>
      {:else if error}
        <!-- Error state -->
        <div class="flex flex-col items-center justify-center py-12 gap-3">
          <svg xmlns="http://www.w3.org/2000/svg" class="w-8 h-8 text-red-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 8v4"/><path d="M12 16h.01"/></svg>
          <p class="text-sm text-red-400">{locale.t('generation.interrogate.failed')}</p>
          <p class="text-xs text-neutral-500 max-w-md text-center break-all">{error}</p>
        </div>
      {:else if result}
        <!-- Rating badge -->
        {#if ratingLabel}
          <div class="flex items-center gap-2">
            <span class="text-xs text-neutral-400">{locale.t('generation.interrogate.rating')}</span>
            <span class="px-2 py-0.5 rounded-full text-xs font-medium text-white {ratingColor(ratingLabel.name)}">
              {ratingLabel.name.replace(/_/g, " ")}
            </span>
            <span class="text-xs text-neutral-500">({locale.formatPercent(ratingLabel.confidence * 100, 1)})</span>
          </div>
        {/if}

        <!-- Saved characters the prompt assistant found -->
        {#if detectEnabled && savedCount > 0}
          <div>
            <p class="flex items-center gap-1 text-sm font-medium text-teal-400 mb-2">
              {locale.t('generation.interrogate.saved_characters', { count: suggestions.length })}
            </p>
            {#if !promptAssistant.isAvailable}
              <p class="text-xs text-neutral-500">
                {locale.t('generation.interrogate.character_match_needs_llm')}
                <button onclick={() => { promptAssistant.setupModalOpen = true; }} class="underline text-neutral-400 hover:text-neutral-200">{locale.t('generation.interrogate.set_up_assistant')}</button>
              </p>
            {:else if detecting}
              <div class="flex items-center gap-2 text-xs text-neutral-400">
                <svg class="animate-spin h-3.5 w-3.5 text-teal-400" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.37 0 0 5.37 0 12h4z"></path>
                </svg>
                {promptAssistant.stage === "loading_model"
                  ? locale.t('generation.interrogate.loading_assistant')
                  : locale.t('generation.interrogate.matching_characters')}
              </div>
            {:else if detectError}
              <p class="text-xs text-red-400 break-words">
                {detectError}
                <button onclick={() => void detectSavedCharacters(result)} class="underline text-neutral-400 hover:text-neutral-200 ml-1">{locale.t('common.retry')}</button>
              </p>
            {:else if suggestions.length > 0}
              <div class="flex flex-wrap gap-1.5">
                {#each suggestions as s (s.character.id)}
                  <button
                    onclick={() => toggleSaved(s)}
                    title={s.character.prompt}
                    class="inline-flex items-center gap-1.5 pl-0.5 pr-2 py-0.5 rounded-full text-xs transition-colors border {checkedSaved[s.character.id] ? 'bg-teal-600/20 border-teal-500/50 text-teal-300' : 'bg-neutral-800 border-neutral-700 text-neutral-400 hover:text-neutral-200'}"
                  >
                    {#if s.character.thumbnail}
                      <img src={s.character.thumbnail} alt="" class="w-5 h-5 rounded-full object-cover" />
                    {:else}
                      <span class="w-1"></span>
                    {/if}
                    {s.character.name}
                  </button>
                {/each}
              </div>
              <p class="text-[10px] text-neutral-500 mt-1.5">{locale.t('generation.interrogate.saved_characters_hint')}</p>
            {:else if detectDone}
              <p class="text-xs text-neutral-500">{locale.t('generation.interrogate.no_character_match')}</p>
            {/if}
          </div>
        {/if}

        <!-- Character tags -->
        {#if result.character_tags.length > 0}
          <div>
            <button onclick={() => toggleCollapse("character")} class="flex items-center gap-1 text-sm font-medium text-indigo-400 mb-2 hover:text-indigo-300">
              <svg class="w-3 h-3 transition-transform {collapsedSections['character'] ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M7 10l5 5 5-5z"/></svg>
              {locale.t('generation.interrogate.characters', { count: result.character_tags.length })}
            </button>
            {#if !collapsedSections["character"]}
              <div class="flex flex-wrap gap-1.5">
                {#each result.character_tags as tag}
                  <button
                    onclick={() => (checkedCharacter = { ...checkedCharacter, [tag.name]: !checkedCharacter[tag.name] })}
                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs transition-colors border {checkedCharacter[tag.name] ? 'bg-indigo-600/20 border-indigo-500/50 text-indigo-300' : 'bg-neutral-800 border-neutral-700 text-neutral-500'}"
                  >
                    {tag.name.replace(/_/g, " ")}
                    <span class="text-[10px] opacity-60">{locale.formatPercent(tag.confidence * 100, 0)}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        <!-- Artist tags -->
        {#if result.artist_tags.length > 0}
          <div>
            <button onclick={() => toggleCollapse("artist")} class="flex items-center gap-1 text-sm font-medium text-purple-400 mb-2 hover:text-purple-300">
              <svg class="w-3 h-3 transition-transform {collapsedSections['artist'] ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M7 10l5 5 5-5z"/></svg>
              {locale.t('generation.interrogate.artists', { count: result.artist_tags.length })}
            </button>
            {#if !collapsedSections["artist"]}
              <div class="flex flex-wrap gap-1.5">
                {#each result.artist_tags as tag}
                  <button
                    onclick={() => (checkedArtist = { ...checkedArtist, [tag.name]: !checkedArtist[tag.name] })}
                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs transition-colors border {checkedArtist[tag.name] ? 'bg-purple-600/20 border-purple-500/50 text-purple-300' : 'bg-neutral-800 border-neutral-700 text-neutral-500'}"
                  >
                    {tag.name.replace(/_/g, " ")}
                    <span class="text-[10px] opacity-60">{locale.formatPercent(tag.confidence * 100, 0)}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        <!-- General tags -->
        {#if result.general_tags.length > 0}
          <div>
            <button onclick={() => toggleCollapse("general")} class="flex items-center gap-1 text-sm font-medium text-neutral-300 mb-2 hover:text-neutral-200">
              <svg class="w-3 h-3 transition-transform {collapsedSections['general'] ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M7 10l5 5 5-5z"/></svg>
              {locale.t('generation.interrogate.general', { count: result.general_tags.length })}
            </button>
            {#if !collapsedSections["general"]}
              <div class="flex flex-wrap gap-1.5">
                {#each result.general_tags as tag}
                  <button
                    onclick={() => (checkedGeneral = { ...checkedGeneral, [tag.name]: !checkedGeneral[tag.name] })}
                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs transition-colors border {checkedGeneral[tag.name] ? 'bg-neutral-600/30 border-neutral-500/50 text-neutral-200' : 'bg-neutral-800 border-neutral-700 text-neutral-500'}"
                  >
                    {tag.name.replace(/_/g, " ")}
                    <span class="text-[10px] opacity-60">{locale.formatPercent(tag.confidence * 100, 0)}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        <!-- Copyright tags -->
        {#if result.copyright_tags.length > 0}
          <div>
            <button onclick={() => toggleCollapse("copyright")} class="flex items-center gap-1 text-sm font-medium text-neutral-400 mb-2 hover:text-neutral-300">
              <svg class="w-3 h-3 transition-transform {collapsedSections['copyright'] ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M7 10l5 5 5-5z"/></svg>
              {locale.t('generation.interrogate.copyright_label', { count: result.copyright_tags.length })}
            </button>
            {#if !collapsedSections["copyright"]}
              <div class="flex flex-wrap gap-1.5">
                {#each result.copyright_tags as tag}
                  <button
                    onclick={() => (checkedCopyright = { ...checkedCopyright, [tag.name]: !checkedCopyright[tag.name] })}
                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs transition-colors border {checkedCopyright[tag.name] ? 'bg-neutral-600/30 border-neutral-500/50 text-neutral-200' : 'bg-neutral-800 border-neutral-700 text-neutral-500'}"
                  >
                    {tag.name.replace(/_/g, " ")}
                    <span class="text-[10px] opacity-60">{locale.formatPercent(tag.confidence * 100, 0)}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      {/if}
    </div>
    </div>

    <!-- Footer -->
    {#if result && !loading}
      <div class="flex flex-col gap-2 px-5 py-3 border-t border-neutral-700">
        <div class="flex items-center gap-4">
          <label class="flex items-center gap-1.5 text-xs text-neutral-300 cursor-pointer">
            <input type="radio" name="apply-mode" checked={replaceMode} onchange={() => { replaceMode = true; }} class="accent-indigo-500" />
            {locale.t('generation.interrogate.replace_prompt')}
          </label>
          <label class="flex items-center gap-1.5 text-xs text-neutral-300 cursor-pointer">
            <input type="radio" name="apply-mode" checked={!replaceMode} onchange={() => { replaceMode = false; }} class="accent-indigo-500" />
            {locale.t('generation.interrogate.append_prompt')}
          </label>
          {#if replaceMode}
            <label class="flex items-center gap-1.5 text-xs text-neutral-400 cursor-pointer ml-2">
              <input type="checkbox" bind:checked={preserveArtistTags} class="accent-indigo-500" />
              {locale.t('generation.interrogate.keep_artist_tags')}
            </label>
          {/if}
        </div>
        <div class="flex items-center justify-between">
          <div class="flex gap-2">
            <button onclick={selectAll} class="text-xs text-neutral-400 hover:text-neutral-200 transition-colors">{locale.t('generation.interrogate.select_all')}</button>
            <span class="text-neutral-600">|</span>
            <button onclick={deselectAll} class="text-xs text-neutral-400 hover:text-neutral-200 transition-colors">{locale.t('generation.interrogate.deselect_all')}</button>
          </div>
          <div class="flex gap-2">
            <button
              onclick={handleCopy}
              class="px-3 py-1.5 text-sm rounded-lg bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
            >{locale.t('common.copy')}</button>
            <button
              onclick={handleCopyAIBooru}
              class="px-3 py-1.5 text-sm rounded-lg bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
            >{locale.t('generation.interrogate.copy_aibooru')}</button>
            <button
              onclick={handleApply}
              disabled={!anyChecked}
              class="px-3 py-1.5 text-sm rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 disabled:cursor-not-allowed text-white transition-colors"
            >{locale.t('generation.interrogate.apply_to_prompt')}</button>
          </div>
        </div>
      </div>
    {/if}
  </div>
</div>
