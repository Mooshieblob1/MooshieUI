<script lang="ts">
  import type { InterrogationResult, TagResult } from "../../types/index.js";
  import { generation } from "../../stores/generation.svelte.js";
  import animaTags from "../../assets/anima-tags.json";

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

  // Initialize checked states when result changes
  $effect(() => {
    if (result) {
      checkedCharacter = Object.fromEntries(result.character_tags.map((t) => [t.name, true]));
      checkedArtist = Object.fromEntries(result.artist_tags.map((t) => [t.name, true]));
      checkedGeneral = Object.fromEntries(result.general_tags.map((t) => [t.name, true]));
      checkedCopyright = Object.fromEntries(result.copyright_tags.map((t) => [t.name, false]));
    }
  });

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
    }
  }

  const anyChecked = $derived.by(() => {
    return (
      Object.values(checkedCharacter).some(Boolean) ||
      Object.values(checkedArtist).some(Boolean) ||
      Object.values(checkedGeneral).some(Boolean) ||
      Object.values(checkedCopyright).some(Boolean)
    );
  });

  /** Format an artist tag for Anima models with @ prefix */
  function formatArtistTagForAnima(tagName: string): string {
    // Cross-reference against anima tag list
    const normalized = tagName.toLowerCase().replace(/ /g, "_");
    const animaMatch = (animaTags as { n: string; c: number }[]).find(
      (t) => t.c === 1 && t.n.toLowerCase().replace(/@/g, "") === normalized
    );
    if (animaMatch) {
      return animaMatch.n;
    }
    // Not in anima list — format with @ prefix and escape parens
    const escaped = normalized.replace(/\(/g, "\\(").replace(/\)/g, "\\)");
    return `@${escaped}`;
  }

  function handleApply() {
    if (!result) return;

    const isAnima = generation.isAnima;
    const artistTags: string[] = [];
    const otherTags: string[] = [];

    // Collect checked artist tags
    for (const tag of result.artist_tags) {
      if (checkedArtist[tag.name]) {
        if (isAnima) {
          artistTags.push(formatArtistTagForAnima(tag.name));
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

    // Smart insertion: artist tags prepend, character+general append
    let prompt = generation.positivePrompt.trim();

    if (artistTags.length > 0) {
      const artistStr = artistTags.join(", ");
      prompt = prompt ? `${artistStr}, ${prompt}` : artistStr;
    }

    if (otherTags.length > 0) {
      const otherStr = otherTags.join(", ");
      prompt = prompt ? `${prompt}, ${otherStr}` : otherStr;
    }

    generation.positivePrompt = prompt;
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

    await navigator.clipboard.writeText(allTags.join(", "));
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
  <div class="bg-neutral-900 border border-neutral-700 rounded-2xl shadow-2xl w-full max-w-3xl max-h-[85vh] flex flex-col">
    <!-- Header -->
    <div class="flex items-center gap-3 px-5 py-3 border-b border-neutral-700">
      {#if imagePreviewUrl}
        <img src={imagePreviewUrl} alt="" class="w-16 h-16 rounded-lg object-cover shrink-0 border border-neutral-600" />
      {/if}
      <h2 class="text-lg font-semibold text-neutral-100 flex-1">Image Tags</h2>
      <button
        onclick={onclose}
        class="w-8 h-8 flex items-center justify-center rounded-lg hover:bg-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors"
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18"/><path d="M6 6l12 12"/></svg>
      </button>
    </div>

    <!-- Body -->
    <div class="flex-1 overflow-y-auto px-5 py-4 space-y-4">
      {#if downloadProgress && !downloadProgress.filename.includes("done")}
        <!-- Download progress -->
        <div class="space-y-2">
          <p class="text-sm text-neutral-300">Downloading interrogator model...</p>
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
                Loading model (first time may take a while)...
              {:else if stage === "running_inference"}
                Running inference...
              {:else}
                Preparing...
              {/if}
            </span>
          </div>
          <p class="text-[10px] text-neutral-500">CPU inference can take 10-30 seconds</p>
        </div>
      {:else if error}
        <!-- Error state -->
        <div class="flex flex-col items-center justify-center py-12 gap-3">
          <svg xmlns="http://www.w3.org/2000/svg" class="w-8 h-8 text-red-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 8v4"/><path d="M12 16h.01"/></svg>
          <p class="text-sm text-red-400">Interrogation failed</p>
          <p class="text-xs text-neutral-500 max-w-md text-center break-all">{error}</p>
        </div>
      {:else if result}
        <!-- Rating badge -->
        {#if ratingLabel}
          <div class="flex items-center gap-2">
            <span class="text-xs text-neutral-400">Rating:</span>
            <span class="px-2 py-0.5 rounded-full text-xs font-medium text-white {ratingColor(ratingLabel.name)}">
              {ratingLabel.name.replace(/_/g, " ")}
            </span>
            <span class="text-xs text-neutral-500">({(ratingLabel.confidence * 100).toFixed(1)}%)</span>
          </div>
        {/if}

        <!-- Character tags -->
        {#if result.character_tags.length > 0}
          <div>
            <button onclick={() => toggleCollapse("character")} class="flex items-center gap-1 text-sm font-medium text-indigo-400 mb-2 hover:text-indigo-300">
              <svg class="w-3 h-3 transition-transform {collapsedSections['character'] ? '-rotate-90' : ''}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M7 10l5 5 5-5z"/></svg>
              Characters ({result.character_tags.length})
            </button>
            {#if !collapsedSections["character"]}
              <div class="flex flex-wrap gap-1.5">
                {#each result.character_tags as tag}
                  <button
                    onclick={() => (checkedCharacter = { ...checkedCharacter, [tag.name]: !checkedCharacter[tag.name] })}
                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs transition-colors border {checkedCharacter[tag.name] ? 'bg-indigo-600/20 border-indigo-500/50 text-indigo-300' : 'bg-neutral-800 border-neutral-700 text-neutral-500'}"
                  >
                    {tag.name.replace(/_/g, " ")}
                    <span class="text-[10px] opacity-60">{(tag.confidence * 100).toFixed(0)}%</span>
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
              Artists ({result.artist_tags.length})
            </button>
            {#if !collapsedSections["artist"]}
              <div class="flex flex-wrap gap-1.5">
                {#each result.artist_tags as tag}
                  <button
                    onclick={() => (checkedArtist = { ...checkedArtist, [tag.name]: !checkedArtist[tag.name] })}
                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs transition-colors border {checkedArtist[tag.name] ? 'bg-purple-600/20 border-purple-500/50 text-purple-300' : 'bg-neutral-800 border-neutral-700 text-neutral-500'}"
                  >
                    {tag.name.replace(/_/g, " ")}
                    <span class="text-[10px] opacity-60">{(tag.confidence * 100).toFixed(0)}%</span>
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
              General ({result.general_tags.length})
            </button>
            {#if !collapsedSections["general"]}
              <div class="flex flex-wrap gap-1.5">
                {#each result.general_tags as tag}
                  <button
                    onclick={() => (checkedGeneral = { ...checkedGeneral, [tag.name]: !checkedGeneral[tag.name] })}
                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs transition-colors border {checkedGeneral[tag.name] ? 'bg-neutral-600/30 border-neutral-500/50 text-neutral-200' : 'bg-neutral-800 border-neutral-700 text-neutral-500'}"
                  >
                    {tag.name.replace(/_/g, " ")}
                    <span class="text-[10px] opacity-60">{(tag.confidence * 100).toFixed(0)}%</span>
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
              Copyright ({result.copyright_tags.length})
            </button>
            {#if !collapsedSections["copyright"]}
              <div class="flex flex-wrap gap-1.5">
                {#each result.copyright_tags as tag}
                  <button
                    onclick={() => (checkedCopyright = { ...checkedCopyright, [tag.name]: !checkedCopyright[tag.name] })}
                    class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs transition-colors border {checkedCopyright[tag.name] ? 'bg-neutral-600/30 border-neutral-500/50 text-neutral-200' : 'bg-neutral-800 border-neutral-700 text-neutral-500'}"
                  >
                    {tag.name.replace(/_/g, " ")}
                    <span class="text-[10px] opacity-60">{(tag.confidence * 100).toFixed(0)}%</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      {/if}
    </div>

    <!-- Footer -->
    {#if result && !loading}
      <div class="flex items-center justify-between px-5 py-3 border-t border-neutral-700">
        <div class="flex gap-2">
          <button onclick={selectAll} class="text-xs text-neutral-400 hover:text-neutral-200 transition-colors">Select All</button>
          <span class="text-neutral-600">|</span>
          <button onclick={deselectAll} class="text-xs text-neutral-400 hover:text-neutral-200 transition-colors">Deselect All</button>
        </div>
        <div class="flex gap-2">
          <button
            onclick={handleCopy}
            class="px-3 py-1.5 text-sm rounded-lg bg-neutral-700 hover:bg-neutral-600 text-neutral-200 transition-colors"
          >Copy</button>
          <button
            onclick={handleApply}
            disabled={!anyChecked}
            class="px-3 py-1.5 text-sm rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:opacity-40 disabled:cursor-not-allowed text-white transition-colors"
          >Apply to Prompt</button>
        </div>
      </div>
    {/if}
  </div>
</div>
