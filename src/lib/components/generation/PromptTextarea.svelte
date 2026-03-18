<script lang="ts">
  import { onMount } from "svelte";
  import tagsData from "../../assets/danbooru-tags.json";

  interface TagEntry {
    n: string; // name
    c: number; // category (0=general, 1=artist, 3=copyright, 4=character, 5=meta)
    p: number; // post count
    a?: string[]; // aliases
  }

  interface Props {
    value: string;
    placeholder?: string;
    rows?: number;
    minHeight?: string;
  }

  let { value = $bindable(), placeholder = "", rows = 4, minHeight = "min-h-25" }: Props = $props();

  const tags: TagEntry[] = tagsData as TagEntry[];

  let textareaEl = $state<HTMLTextAreaElement | null>(null);
  let suggestions = $state<TagEntry[]>([]);
  let selectedIndex = $state(0);
  let showSuggestions = $state(false);
  let dropdownTop = $state(0);
  let dropdownLeft = $state(0);

  const CATEGORY_COLORS: Record<number, string> = {
    0: "text-indigo-300",   // general
    1: "text-red-400",      // artist
    3: "text-purple-400",   // copyright
    4: "text-green-400",    // character
    5: "text-orange-400",   // meta
  };

  function formatCount(count: number): string {
    if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M`;
    if (count >= 1_000) return `${(count / 1_000).toFixed(0)}k`;
    return String(count);
  }

  function getCurrentTagFragment(): { fragment: string; start: number; end: number } | null {
    if (!textareaEl) return null;
    const pos = textareaEl.selectionStart;
    const text = value;

    // Find the start of the current tag (after the last comma before cursor)
    let start = text.lastIndexOf(",", pos - 1) + 1;
    // Find the end of the current tag (next comma after cursor, or end of string)
    let end = text.indexOf(",", pos);
    if (end === -1) end = text.length;

    const fragment = text.substring(start, end).trim();
    return { fragment, start, end };
  }

  function updateSuggestions() {
    const result = getCurrentTagFragment();
    if (!result || result.fragment.length < 1) {
      showSuggestions = false;
      suggestions = [];
      return;
    }

    const query = result.fragment.toLowerCase().replace(/\s+/g, "_");

    // Skip if the fragment looks like a weight expression
    if (/^\(.*:\d/.test(result.fragment)) {
      showSuggestions = false;
      return;
    }

    // Collect all matches, then sort by post count
    const prefixMatches: TagEntry[] = [];
    const containsMatches: TagEntry[] = [];
    const aliasMatches: TagEntry[] = [];

    for (const tag of tags) {
      if (tag.n.startsWith(query)) {
        prefixMatches.push(tag);
      } else if (tag.n.includes(query)) {
        containsMatches.push(tag);
      } else if (tag.a?.some((a) => a.startsWith(query) || a.includes(query))) {
        aliasMatches.push(tag);
      }
    }

    // Sort each group by post count (highest first), then take top 10
    const byCount = (a: TagEntry, b: TagEntry) => b.p - a.p;
    prefixMatches.sort(byCount);
    containsMatches.sort(byCount);
    aliasMatches.sort(byCount);

    suggestions = [...prefixMatches, ...containsMatches, ...aliasMatches].slice(0, 10);
    selectedIndex = 0;
    showSuggestions = suggestions.length > 0;

    if (showSuggestions) {
      positionDropdown();
    }
  }

  function positionDropdown() {
    if (!textareaEl) return;
    const rect = textareaEl.getBoundingClientRect();
    // Position below the textarea
    dropdownTop = rect.bottom + 4;
    dropdownLeft = rect.left;
  }

  function acceptSuggestion(tag: TagEntry) {
    const result = getCurrentTagFragment();
    if (!result || !textareaEl) return;

    const before = value.substring(0, result.start);
    const after = value.substring(result.end);

    // Add the tag with proper spacing
    const needsLeadingSpace = before.length > 0 && !before.endsWith(" ") && !before.endsWith(",");
    const prefix = needsLeadingSpace ? " " : "";
    const tagText = tag.n.replace(/_/g, " ");
    const suffix = after.startsWith(",") ? "" : ", ";

    value = before + prefix + tagText + suffix + after.replace(/^,\s*/, "");

    showSuggestions = false;

    // Set cursor position after the inserted tag
    const cursorPos = (before + prefix + tagText + suffix).length;
    requestAnimationFrame(() => {
      textareaEl?.focus();
      textareaEl?.setSelectionRange(cursorPos, cursorPos);
    });
  }

  function handleKeydown(e: KeyboardEvent) {
    // Tag weight adjustment: Ctrl+Up/Down on selected text
    if ((e.ctrlKey || e.metaKey) && (e.key === "ArrowUp" || e.key === "ArrowDown") && textareaEl) {
      const start = textareaEl.selectionStart;
      const end = textareaEl.selectionEnd;
      if (start !== end) {
        e.preventDefault();
        adjustWeight(e.key === "ArrowUp" ? 0.05 : -0.05, start, end);
        return;
      }
    }

    // Autocomplete navigation
    if (showSuggestions) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        selectedIndex = (selectedIndex + 1) % suggestions.length;
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        selectedIndex = (selectedIndex - 1 + suggestions.length) % suggestions.length;
        return;
      }
      if (e.key === "Tab" || (e.key === "Enter" && !e.ctrlKey && !e.metaKey)) {
        e.preventDefault();
        acceptSuggestion(suggestions[selectedIndex]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        showSuggestions = false;
        return;
      }
    }
  }

  function adjustWeight(delta: number, start: number, end: number) {
    if (!textareaEl) return;
    let selected = value.substring(start, end);

    // Check if selection is already a weighted tag: (tag:weight)
    const weightMatch = selected.match(/^\((.+):(\d+\.?\d*)\)$/);

    let newText: string;
    let newWeight: number;

    if (weightMatch) {
      const tagName = weightMatch[1];
      const currentWeight = parseFloat(weightMatch[2]);
      newWeight = Math.round((currentWeight + delta) * 100) / 100;
      newWeight = Math.max(0, Math.min(2, newWeight));
      if (Math.abs(newWeight - 1.0) < 0.001) {
        // Weight is 1.0, just use the raw tag
        newText = tagName;
      } else {
        newText = `(${tagName}:${newWeight.toFixed(2)})`;
      }
    } else {
      // Wrap in weight syntax
      newWeight = Math.round((1.0 + delta) * 100) / 100;
      newText = `(${selected}:${newWeight.toFixed(2)})`;
    }

    value = value.substring(0, start) + newText + value.substring(end);

    // Re-select the full weighted text
    requestAnimationFrame(() => {
      textareaEl?.focus();
      textareaEl?.setSelectionRange(start, start + newText.length);
    });
  }

  function handleInput() {
    // Small delay to let the value update
    requestAnimationFrame(updateSuggestions);
  }

  function handleClick() {
    requestAnimationFrame(updateSuggestions);
  }

  function handleBlur() {
    // Delay to allow click on suggestion to fire first
    setTimeout(() => {
      showSuggestions = false;
    }, 200);
  }
</script>

<div class="relative">
  <textarea
    bind:this={textareaEl}
    bind:value
    {placeholder}
    {rows}
    class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 resize-y focus:outline-none focus:border-indigo-500 transition-colors {minHeight}"
    onkeydown={handleKeydown}
    oninput={handleInput}
    onclick={handleClick}
    onblur={handleBlur}
  ></textarea>

  {#if showSuggestions}
    <div
      class="fixed z-50 w-80 max-h-60 overflow-y-auto bg-neutral-800 border border-neutral-600 rounded-lg shadow-xl"
      style="top: {dropdownTop}px; left: {dropdownLeft}px;"
    >
      {#each suggestions as tag, i}
        <button
          class="w-full text-left px-3 py-1.5 text-sm flex items-center justify-between gap-2 transition-colors cursor-pointer
            {i === selectedIndex ? 'bg-indigo-600/40 text-white' : 'text-neutral-300 hover:bg-neutral-700'}"
          onmousedown={(e) => { e.preventDefault(); acceptSuggestion(tag); }}
          onmouseenter={() => { selectedIndex = i; }}
        >
          <span class={CATEGORY_COLORS[tag.c] ?? "text-neutral-300"}>
            {tag.n.replace(/_/g, " ")}
          </span>
          <span class="text-xs text-neutral-500 shrink-0">{formatCount(tag.p)}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>
