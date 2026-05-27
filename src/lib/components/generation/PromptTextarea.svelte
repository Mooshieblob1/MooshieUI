<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { on } from "svelte/events";
  import { autocomplete, type TagEntry } from "../../stores/autocomplete.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import { renderHighlightedPrompt, hasSchedulingTags, hasPresetTokens } from "../../utils/promptSchedule.js";
  import {
    getPromptClickableSegments,
    type PromptClickableSegment,
  } from "../../utils/promptClickableRanges.js";

  interface Props {
    value: string;
    placeholder?: string;
    rows?: number;
    minHeight?: string;
    storageKey?: string;
  }

  let { value = $bindable(), placeholder = "", rows = 4, minHeight = "min-h-25", storageKey }: Props = $props();

  // Restored height is applied as inline style (set in $effect on mount).
  let resizeStyle = $state("");

  /** Format a tag name for insertion into the prompt. Escapes parentheses for models that take raw tags. */
  function formatTagForPrompt(name: string): string {
    return name.replace(/_/g, " ").replace(/\(/g, "\\(").replace(/\)/g, "\\)");
  }

  /** Format a tag name for display in the dropdown (always clean, no escapes). */
  function formatTagForDisplay(name: string): string {
    return name.replace(/_/g, " ");
  }

  let textareaEl = $state<HTMLTextAreaElement | null>(null);
  let backdropEl = $state<HTMLDivElement | null>(null);
  let clickOverlayEl = $state<HTMLDivElement | null>(null);
  let suggestions = $state<TagEntry[]>([]);
  let selectedIndex = $state(0);
  let showSuggestions = $state(false);
  let dropdownTop = $state(0);
  let dropdownLeft = $state(0);
  let suggestionTimer: ReturnType<typeof setTimeout> | null = null;

  const DROPDOWN_WIDTH = 320; // w-80
  const DROPDOWN_MAX_HEIGHT = 240; // max-h-60
  const VIEWPORT_MARGIN = 8;
  const PANEL_GAP = 8;
  const SUGGEST_DEBOUNCE_MS = 60;

  // Undo/redo stacks for autocomplete insertions
  let undoStack = $state<string[]>([]);
  let redoStack = $state<string[]>([]);
  let categoryFilter = $state<number | null>(null);
  let selectionStart = $state(0);
  let selectionEnd = $state(0);
  const hasSelection = $derived(selectionStart !== selectionEnd);

  const categoryOptions = $derived([
    { value: null, label: locale.t("generation.prompt.category_all") },
    { value: 0, label: locale.t("generation.prompt.category_general") },
    { value: 1, label: locale.t("generation.prompt.category_artist") },
    { value: 3, label: locale.t("generation.prompt.category_copyright") },
    { value: 4, label: locale.t("generation.prompt.category_character") },
    { value: 5, label: locale.t("generation.prompt.category_meta") },
  ]);

  const CATEGORY_COLORS: Record<number, string> = {
    0: "text-indigo-300",   // general
    1: "text-red-400",      // artist
    3: "text-purple-400",   // copyright
    4: "text-green-400",    // character
    5: "text-orange-400",   // meta
  };

  function formatCount(count: number): string {
    return locale.formatCompactCount(count);
  }

  function formatTagCount(tag: { p: number; b?: number }): string {
    if (tag.b) return `<${tag.b === 1 ? 50 : tag.b}`;
    return formatCount(tag.p);
  }

  function getCurrentTagFragment(): {
    fragment: string;
    start: number;
    end: number;
    trimmedStart: number;
    trimmedEnd: number;
  } | null {
    if (!textareaEl) return null;
    const pos = textareaEl.selectionStart;
    const text = value;

    // Check if cursor is inside a <fromto[...]...> block — if so, use block
    // boundaries instead of comma-splitting (commas are part of fromto syntax).
    const fromtoRe = /<fromto\[[^\]]*\]:[^>]*>/g;
    let ftMatch: RegExpExecArray | null;
    while ((ftMatch = fromtoRe.exec(text)) !== null) {
      const ftStart = ftMatch.index;
      const ftEnd = ftStart + ftMatch[0].length;
      if (pos > ftStart && pos <= ftEnd) {
        // Cursor is inside this fromto block — use the whole block as the fragment
        const token = text.substring(ftStart, ftEnd);
        const leadingWhitespace = token.match(/^\s*/)?.[0].length ?? 0;
        const trailingWhitespace = token.match(/\s*$/)?.[0].length ?? 0;
        const trimmedStart = ftStart + leadingWhitespace;
        const trimmedEnd = Math.max(trimmedStart, ftEnd - trailingWhitespace);
        const fragment = text.substring(trimmedStart, trimmedEnd);
        return { fragment, start: ftStart, end: ftEnd, trimmedStart, trimmedEnd };
      }
    }

    // Find the start of the current tag (after the last comma before cursor)
    let start = text.lastIndexOf(",", pos - 1) + 1;
    // Find the end of the current tag (next comma after cursor, or end of string)
    let end = text.indexOf(",", pos);
    if (end === -1) end = text.length;

    const token = text.substring(start, end);
    const leadingWhitespace = token.match(/^\s*/)?.[0].length ?? 0;
    const trailingWhitespace = token.match(/\s*$/)?.[0].length ?? 0;
    const trimmedStart = start + leadingWhitespace;
    const trimmedEnd = Math.max(trimmedStart, end - trailingWhitespace);
    const fragment = text.substring(trimmedStart, trimmedEnd);

    return { fragment, start, end, trimmedStart, trimmedEnd };
  }

  function updateSuggestions() {
    const result = getCurrentTagFragment();
    const pos = textareaEl?.selectionStart ?? 0;
    
    if (!result || pos < result.trimmedStart) {
      showSuggestions = false;
      suggestions = [];
      return;
    }

    // Search based on text from tag start to cursor position (supports mid-prompt editing)
    let searchFragment = value.substring(result.trimmedStart, pos).replace(/\s+$/, "");

    // Strip scheduling tag syntax so autocomplete works inside scheduling blocks
    // MooshieUI: <from:0.2>tag</from>, <to:0.8>tag</to>, <range:0.2:0.8>tag</range>
    // SwarmUI:   <fromto[0.5]:before, after>
    searchFragment = searchFragment
      .replace(/^<(?:from|to|range):[\d.]+(?::[\d.]+)?>/i, "")
      .replace(/<\/(?:from|to|range)>$/i, "")
      .replace(/^<fromto\[[\d.]+\]:/i, "")
      .replace(/>$/i, "");

    if (searchFragment.length < 1) {
      showSuggestions = false;
      suggestions = [];
      return;
    }

    // Skip if the fragment looks like a weight expression
    if (/^\(.*:\d/.test(searchFragment)) {
      showSuggestions = false;
      suggestions = [];
      return;
    }

    suggestions = autocomplete.search(searchFragment, autocomplete.maxResults, categoryFilter);
    selectedIndex = 0;
    showSuggestions = suggestions.length > 0;

    if (showSuggestions) {
      positionDropdown();
    }
  }

  function positionDropdown() {
    if (!textareaEl) return;
    const rect = textareaEl.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;
    const textareaCenterX = rect.left + rect.width / 2;
    const isLeftPanel = textareaCenterX < viewportWidth / 2;

    const minLeft = VIEWPORT_MARGIN;
    const maxLeft = Math.max(VIEWPORT_MARGIN, viewportWidth - VIEWPORT_MARGIN - DROPDOWN_WIDTH);
    const desiredDropdownLeft = isLeftPanel
      ? rect.right + PANEL_GAP
      : rect.left - DROPDOWN_WIDTH - PANEL_GAP;
    dropdownLeft = Math.min(Math.max(desiredDropdownLeft, minLeft), maxLeft);

    const minTop = VIEWPORT_MARGIN;
    const maxTop = Math.max(VIEWPORT_MARGIN, viewportHeight - VIEWPORT_MARGIN - DROPDOWN_MAX_HEIGHT);
    const desiredDropdownTop = rect.top;
    dropdownTop = Math.min(Math.max(desiredDropdownTop, minTop), maxTop);
  }

  function acceptSuggestion(tag: TagEntry) {
    const result = getCurrentTagFragment();
    if (!result || !textareaEl) return;

    // Push current value to undo stack before modifying
    undoStack = [...undoStack, value];
    redoStack = [];

    const before = value.substring(0, result.start);
    const leadingWhitespace = value.substring(result.start, result.trimmedStart);
    const trailingWhitespace = value.substring(result.trimmedEnd, result.end);
    const after = value.substring(result.end);
    const rawTagText = formatTagForPrompt(tag.n);

    // Detect scheduling wrapper in the current fragment and preserve it
    // MooshieUI XML syntax: <from:0.2>tag</from>
    const schedPrefixMatch = result.fragment.match(/^(<(from|to|range):[\d.]+(?::[\d.]+)?>)/i);
    const schedSuffixMatch = result.fragment.match(/(<\/(from|to|range)>)$/i);
    const schedPrefix = schedPrefixMatch?.[1] ?? "";
    const schedType = schedPrefixMatch?.[2] ?? "";
    // Auto-close if there's an open tag but no closing tag yet
    const schedSuffix = schedSuffixMatch?.[1] ?? (schedType ? `</${schedType}>` : "");
    // SwarmUI syntax: <fromto[0.5]:tag — preserve the prefix (no closing tag needed)
    const swarmPrefixMatch = !schedPrefix ? result.fragment.match(/^(<fromto\[[\d.]+\]:)/i) : null;
    const swarmPrefix = swarmPrefixMatch?.[1] ?? "";
    // Trailing > from SwarmUI second entry (e.g. "blue eyes>")
    const swarmSuffix = !schedSuffix && result.fragment.match(/>$/) ? ">" : "";
    const tagText = (schedPrefix || swarmPrefix) + rawTagText + (schedSuffix || swarmSuffix);

    const needsCommaSuffix = !/^\s*,/.test(after);
    const suffix = needsCommaSuffix ? ", " : "";

    value = before + leadingWhitespace + tagText + trailingWhitespace + suffix + after;

    showSuggestions = false;

    // Set cursor position after the inserted tag (before trailing suffix)
    const cursorPos = (before + leadingWhitespace + tagText + trailingWhitespace + suffix).length;
    requestAnimationFrame(() => {
      textareaEl?.focus();
      textareaEl?.setSelectionRange(cursorPos, cursorPos);
      syncSelectionRange();
    });
  }

  function syncSelectionRange() {
    if (!textareaEl) return;
    selectionStart = textareaEl.selectionStart;
    selectionEnd = textareaEl.selectionEnd;
  }

  function wrapSelection(braceKey: "brace" | "bracket") {
    if (!textareaEl || selectionStart === selectionEnd) return;
    const start = selectionStart;
    const end = selectionEnd;
    const selected = value.substring(start, end);

    if (braceKey === "bracket" && selected.startsWith("{") && selected.endsWith("}")) {
      undoStack = [...undoStack, value];
      redoStack = [];
      const inner = selected.slice(1, -1);
      value = value.substring(0, start) + inner + value.substring(end);
      requestAnimationFrame(() => {
        textareaEl?.focus();
        textareaEl?.setSelectionRange(start, start + inner.length);
        syncSelectionRange();
      });
      return;
    }

    if (braceKey === "brace" && selected.startsWith("[") && selected.endsWith("]")) {
      undoStack = [...undoStack, value];
      redoStack = [];
      const inner = selected.slice(1, -1);
      value = value.substring(0, start) + inner + value.substring(end);
      requestAnimationFrame(() => {
        textareaEl?.focus();
        textareaEl?.setSelectionRange(start, start + inner.length);
        syncSelectionRange();
      });
      return;
    }

    const open = braceKey === "brace" ? "{" : "[";
    const close = braceKey === "brace" ? "}" : "]";
    const wrapped = `${open}${selected}${close}`;
    undoStack = [...undoStack, value];
    redoStack = [];
    value = value.substring(0, start) + wrapped + value.substring(end);
    requestAnimationFrame(() => {
      textareaEl?.focus();
      textareaEl?.setSelectionRange(start, start + wrapped.length);
      syncSelectionRange();
    });
  }

  function adjustSelectedWeight(delta: number) {
    if (!textareaEl || selectionStart === selectionEnd) return;
    undoStack = [...undoStack, value];
    redoStack = [];
    adjustWeight(delta, selectionStart, selectionEnd);
  }

  function undo() {
    if (undoStack.length === 0) return;
    redoStack = [...redoStack, value];
    const prev = undoStack[undoStack.length - 1];
    undoStack = undoStack.slice(0, -1);
    value = prev;
    requestAnimationFrame(syncSelectionRange);
  }

  function redo() {
    if (redoStack.length === 0) return;
    undoStack = [...undoStack, value];
    const next = redoStack[redoStack.length - 1];
    redoStack = redoStack.slice(0, -1);
    value = next;
    requestAnimationFrame(syncSelectionRange);
  }

  function handleKeydown(e: KeyboardEvent) {
    // Undo/redo for autocomplete: Ctrl+Z / Ctrl+Y
    if ((e.ctrlKey || e.metaKey) && e.key === "z" && !e.shiftKey) {
      if (undoStack.length > 0) {
        e.preventDefault();
        undo();
        return;
      }
    }
    if ((e.ctrlKey || e.metaKey) && (e.key === "y" || (e.key === "z" && e.shiftKey))) {
      if (redoStack.length > 0) {
        e.preventDefault();
        redo();
        return;
      }
    }

    // Tag weight adjustment: Ctrl+Up/Down on selected text
    if ((e.ctrlKey || e.metaKey) && (e.key === "ArrowUp" || e.key === "ArrowDown") && textareaEl) {
      const start = textareaEl.selectionStart;
      const end = textareaEl.selectionEnd;
      if (start !== end) {
        e.preventDefault();
        // Push current value to undo stack before modifying
        undoStack = [...undoStack, value];
        redoStack = [];
        adjustWeight(e.key === "ArrowUp" ? 0.05 : -0.05, start, end);
        return;
      }
    }

    // NAI-style bracket weighting: { / } wraps selection to increase, [ / ] to decrease.
    // If selected text is already wrapped in {} and user presses [ or ], strip {}s first.
    if ((e.key === "{" || e.key === "}" || e.key === "[" || e.key === "]") && textareaEl) {
      const start = textareaEl.selectionStart;
      const end = textareaEl.selectionEnd;
      if (start !== end) {
        e.preventDefault();
        const braceKey = (e.key === "{" || e.key === "}") ? "brace" : "bracket";
        wrapSelection(braceKey);
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
      if (e.key === "Tab" || (e.key === "Enter" && !e.ctrlKey && !e.metaKey && !e.shiftKey)) {
        e.preventDefault();
        acceptSuggestion(suggestions[selectedIndex]);
        return;
      }
      if (e.key === "Enter" && e.shiftKey) {
        // Let Shift+Enter insert a newline (default textarea behavior)
        showSuggestions = false;
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
      syncSelectionRange();
    });
  }

  function scheduleUpdateSuggestions() {
    if (suggestionTimer !== null) {
      clearTimeout(suggestionTimer);
    }
    suggestionTimer = setTimeout(() => {
      suggestionTimer = null;
      updateSuggestions();
    }, SUGGEST_DEBOUNCE_MS);
  }

  function handleInput() {
    redoStack = [];
    syncSelectionRange();
    scheduleUpdateSuggestions();
  }

  function handleClick() {
    requestAnimationFrame(() => {
      syncSelectionRange();
      scheduleUpdateSuggestions();
    });
  }

  function handleBlur() {
    if (suggestionTimer !== null) {
      clearTimeout(suggestionTimer);
      suggestionTimer = null;
    }

    // Delay to allow click on suggestion to fire first
    setTimeout(() => {
      showSuggestions = false;
    }, 200);
  }

  // Reactive: detect if current value has scheduling tags or inline preset tokens
  const showBackdrop = $derived(hasSchedulingTags(value) || hasPresetTokens(value));

  // Reactive: render highlighted HTML for the backdrop overlay
  const highlightedHtml = $derived(showBackdrop ? renderHighlightedPrompt(value, promptPresets.slugs) : "");
  const clickableSegments = $derived(
    autocomplete.clickableOverlayEnabled ? getPromptClickableSegments(value) : [],
  );
  const showClickableOverlay = $derived(autocomplete.clickableOverlayEnabled && clickableSegments.length > 0);

  function handleClickableSegmentMouseDown(event: MouseEvent, segment: PromptClickableSegment) {
    if (!textareaEl || !segment.clickable) return;

    event.preventDefault();
    event.stopPropagation();

    if (suggestionTimer !== null) {
      clearTimeout(suggestionTimer);
      suggestionTimer = null;
    }

    showSuggestions = false;
    suggestions = [];
    textareaEl.focus();
    textareaEl.setSelectionRange(segment.start, segment.end);
    syncSelectionRange();
  }

  function syncScroll() {
    if (textareaEl && backdropEl) {
      backdropEl.scrollTop = textareaEl.scrollTop;
      backdropEl.scrollLeft = textareaEl.scrollLeft;
    }
    if (textareaEl && clickOverlayEl) {
      clickOverlayEl.scrollTop = textareaEl.scrollTop;
      clickOverlayEl.scrollLeft = textareaEl.scrollLeft;
    }
  }

  // Restore saved height and persist future resize changes via ResizeObserver.
  let resizeObserver: ResizeObserver | null = null;
  $effect(() => {
    if (!textareaEl || !storageKey) return;
    // Restore saved height on mount.
    const saved = localStorage.getItem(storageKey);
    if (saved) {
      textareaEl.style.height = saved;
      resizeStyle = `height: ${saved};`;
    }
    // Observe future user-driven resizes.
    resizeObserver?.disconnect();
    resizeObserver = new ResizeObserver(() => {
      if (!textareaEl || !storageKey) return;
      const h = textareaEl.style.height;
      if (h && h !== "" && h !== "0px") {
        localStorage.setItem(storageKey, h);
        resizeStyle = `height: ${h};`;
      }
    });
    resizeObserver.observe(textareaEl);
  });

  onDestroy(() => {
    if (suggestionTimer !== null) {
      clearTimeout(suggestionTimer);
      suggestionTimer = null;
    }
    resizeObserver?.disconnect();
  });

  /** Teleport overlays to body so fixed positioning escapes panel overflow/transform containers. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  // Portalled dropdown nodes cannot rely on delegated listeners, so bind directly.
  function bindCategoryButton(node: HTMLButtonElement, value: number | null) {
    const offMouseDown = on(node, "mousedown", (e) => e.preventDefault());
    const offClick = on(node, "click", () => {
      categoryFilter = value;
    });
    return {
      update(nextValue: number | null) {
        value = nextValue;
      },
      destroy() {
        offMouseDown();
        offClick();
      },
    };
  }

  function bindSuggestionButton(node: HTMLButtonElement, initial: { tag: TagEntry; index: number }) {
    let current = initial;
    const offMouseDown = on(node, "mousedown", (e) => {
      e.preventDefault();
      acceptSuggestion(current.tag);
    });
    const offMouseEnter = on(node, "mouseenter", () => {
      selectedIndex = current.index;
    });
    return {
      update(next: { tag: TagEntry; index: number }) {
        current = next;
      },
      destroy() {
        offMouseDown();
        offMouseEnter();
      },
    };
  }

  $effect(() => {
    if (!showSuggestions) return;
    const reposition = () => positionDropdown();
    window.addEventListener("scroll", reposition, true);
    window.addEventListener("resize", reposition);
    return () => {
      window.removeEventListener("scroll", reposition, true);
      window.removeEventListener("resize", reposition);
    };
  });

  $effect(() => {
    // Only re-run when categoryFilter changes; avoid re-tracking value/showSuggestions
    // (handleInput already drives updateSuggestions on typing).
    categoryFilter;
    untrack(() => {
      if (showSuggestions) {
        updateSuggestions();
      }
    });
  });
</script>

<div class="relative">
  {#if hasSelection}
    <div class="mb-2 flex flex-wrap gap-1.5">
      <button
        type="button"
        class="rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
        title={locale.t('generation.prompt.weight_up')}
        onmousedown={(e) => e.preventDefault()}
        onclick={() => adjustSelectedWeight(0.05)}
      >
        +0.05
      </button>
      <button
        type="button"
        class="rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
        title={locale.t('generation.prompt.weight_down')}
        onmousedown={(e) => e.preventDefault()}
        onclick={() => adjustSelectedWeight(-0.05)}
      >
        -0.05
      </button>
      <button
        type="button"
        class="rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
        title={locale.t('generation.prompt.wrap_stronger')}
        onmousedown={(e) => e.preventDefault()}
        onclick={() => wrapSelection("brace")}
      >
        &#123;&#125;
      </button>
      <button
        type="button"
        class="rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-300 hover:border-indigo-500 hover:text-indigo-300 transition-colors"
        title={locale.t('generation.prompt.wrap_weaker')}
        onmousedown={(e) => e.preventDefault()}
        onclick={() => wrapSelection("bracket")}
      >
        []
      </button>
    </div>
  {/if}
  <div class="relative">
    {#if showBackdrop}
      <div
        bind:this={backdropEl}
        class="absolute inset-0 pointer-events-none overflow-hidden rounded-lg px-3 py-2 text-sm leading-5 whitespace-pre-wrap break-words border border-transparent"
        style="color: transparent; z-index: 0;"
      >{@html highlightedHtml}</div>
    {/if}

    <textarea
      bind:this={textareaEl}
      bind:value
      {placeholder}
      {rows}
      class="w-full border border-neutral-700 rounded-lg px-3 py-2 text-sm leading-5 text-neutral-100 placeholder-neutral-500 resize-y focus:outline-none focus:border-indigo-500 transition-colors break-words {minHeight} {showBackdrop ? 'bg-transparent' : 'bg-neutral-800'}"
      style="position: relative; z-index: 1; {resizeStyle}{showBackdrop ? 'caret-color: #e5e5e5;' : ''}"
      onkeydown={handleKeydown}
      oninput={handleInput}
      onclick={handleClick}
        onselect={syncSelectionRange}
        onkeyup={syncSelectionRange}
      onblur={handleBlur}
      onscroll={syncScroll}
    ></textarea>

    {#if showClickableOverlay}
      <div
        bind:this={clickOverlayEl}
        aria-hidden="true"
        class="absolute inset-0 overflow-hidden rounded-lg px-3 py-2 text-sm leading-5 whitespace-pre-wrap break-words border border-transparent select-none"
        style="pointer-events: none; color: transparent; z-index: 2;"
      >
        {#each clickableSegments as segment (segment.start + ':' + segment.end + ':' + segment.kind)}
          {#if segment.clickable}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <span
              class="pointer-events-auto cursor-pointer rounded-[4px] transition-colors box-decoration-clone {selectionStart === segment.start && selectionEnd === segment.end
                ? segment.kind === 'weighted'
                  ? 'bg-amber-400/28 shadow-[inset_0_0_0_1px_rgba(252,211,77,0.8),0_0_10px_rgba(251,191,36,0.35)]'
                  : 'bg-indigo-400/24 shadow-[inset_0_0_0_1px_rgba(165,180,252,0.8),0_0_10px_rgba(129,140,248,0.35)]'
                : segment.kind === 'weighted'
                  ? 'bg-amber-500/16 hover:bg-amber-500/22 shadow-[inset_0_0_0_1px_rgba(245,158,11,0.4)] hover:shadow-[inset_0_0_0_1px_rgba(251,191,36,0.55)]'
                  : 'hover:bg-indigo-500/18 hover:shadow-[inset_0_0_0_1px_rgba(129,140,248,0.5)]'}"
              style="color: transparent;"
              onmousedown={(event) => handleClickableSegmentMouseDown(event, segment)}
            >{value.slice(segment.start, segment.end)}</span>
          {:else}
            <span style="color: transparent;">{value.slice(segment.start, segment.end)}</span>
          {/if}
        {/each}
      </div>
    {/if}
  </div>

  {#if showSuggestions}
    <div
      use:portal
      class="fixed z-[200] w-80 max-h-60 overflow-y-auto bg-neutral-800 border border-neutral-600 rounded-lg shadow-xl"
      style="top: {dropdownTop}px; left: {dropdownLeft}px;"
    >
      <div class="sticky top-0 z-10 border-b border-neutral-700 bg-neutral-800/95 p-2 backdrop-blur-sm">
        <div class="flex flex-wrap gap-1">
          {#each categoryOptions as option (option.value ?? "all")}
            <button
              type="button"
              class="rounded-full border px-2 py-0.5 text-[10px] transition-colors cursor-pointer {categoryFilter === option.value
                ? 'border-indigo-500/60 bg-indigo-500/15 text-indigo-200'
                : 'border-neutral-700 text-neutral-400 hover:border-neutral-500 hover:text-neutral-200'}"
              use:bindCategoryButton={option.value}
            >
              {option.label}
            </button>
          {/each}
        </div>
      </div>
      {#each suggestions as tag, i (tag.n)}
        <button
          type="button"
          class="w-full text-left px-3 py-1.5 text-sm flex items-center justify-between gap-2 transition-colors cursor-pointer
            {i === selectedIndex ? 'bg-indigo-600/40 text-white' : 'text-neutral-300 hover:bg-neutral-700'}"
          use:bindSuggestionButton={{ tag, index: i }}
        >
          <span class={CATEGORY_COLORS[tag.c] ?? "text-neutral-300"}>
            {formatTagForDisplay(tag.n)}
          </span>
          <span class="text-xs text-neutral-500 shrink-0">{formatTagCount(tag)}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>
