<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { on } from "svelte/events";
  import { autocomplete, type TagEntry } from "../../stores/autocomplete.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";
  import { promptPresets } from "../../stores/promptPresets.svelte.js";
  import {
    renderHighlightedPrompt,
    hasSchedulingTags,
    hasPresetTokens,
    hasLoraWordInPrompt,
    findPromptInertRangeContaining,
  } from "../../utils/promptSchedule.js";
  import {
    getPromptClickableSegments,
    type PromptClickableSegment,
  } from "../../utils/promptClickableRanges.js";
  import { adjustWeightText, isNonNumericWeightSelection } from "../../utils/promptWeightAdjust.js";
  import {
    getUnknownTagRanges,
    buildSpellcheckPieces,
    type SpellcheckPiece,
  } from "../../utils/promptSpellcheck.js";
  import { SYNTAX_ANGLE_LOOKBEHIND } from "../../utils/promptSyntaxEscape.js";
  import { CLIP_CHUNK_SIZE, estimatePromptTokens, promptTokenLimit } from "../../utils/promptTokens.js";
  import ContextMenu, { type ContextMenuItem } from "../ui/ContextMenu.svelte";

  interface Props {
    value: string;
    placeholder?: string;
    rows?: number;
    minHeight?: string;
    storageKey?: string;
    /**
     * Danbooru tag assistance: the completion dropdown, the unknown-tag
     * spellcheck underline, and the clickable-tag overlay. One flag rather than
     * three, because all three read the same tag index and are wrong in exactly
     * the same places — a prose prompt (H3 video) is not made of tags, so
     * completing, underlining and linking them all misfire together.
     */
    tagAssist?: boolean;
    /**
     * Highlight prompt words inserted via a LoRA's trigger-word chip
     * (tracked per-LoRA in `generation.loras[].insertedWords`). Only the
     * positive-prompt instance should set this — trigger words are only
     * ever inserted into `generation.positivePrompt`, so lighting up the
     * same literal text in the negative prompt or region boxes would be
     * a false positive.
     */
    highlightLoraWords?: boolean;
  }

  let {
    value = $bindable(),
    placeholder = "",
    rows = 4,
    minHeight = "min-h-25",
    storageKey,
    tagAssist = true,
    highlightLoraWords = false,
  }: Props = $props();

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
  let clickMirrorEl = $state<HTMLDivElement | null>(null);
  let spellcheckOverlayEl = $state<HTMLDivElement | null>(null);
  let spellMenuVisible = $state(false);
  let spellMenuX = $state(0);
  let spellMenuY = $state(0);
  let spellMenuItems = $state<ContextMenuItem[]>([]);
  let scrollbarWidth = $state(0);
  let overlayScrollTop = $state(0);
  let overlayScrollLeft = $state(0);

  /**
   * The overlays mirror the textarea character for character, so their content box has
   * to match its content box exactly: a sub-pixel difference moves wrap points and the
   * mirror gains or loses a line. The textarea reserves a scrollbar gutter, so the
   * overlays must reserve an identical one. `scrollbar-gutter: stable` lets the engine
   * do that at full precision (it applies to `overflow: hidden` boxes too). The JS
   * fallback can only ever be an integer approximation, because offsetWidth and
   * clientWidth are rounded — measurably wrong at 125%/150% display scaling.
   */
  const supportsScrollbarGutter =
    typeof CSS !== "undefined" &&
    typeof CSS.supports === "function" &&
    CSS.supports("scrollbar-gutter", "stable");
  const gutterStyle = $derived(
    supportsScrollbarGutter ? "scrollbar-gutter: stable;" : `right: ${scrollbarWidth}px;`,
  );

  function updateScrollbarWidth() {
    if (!supportsScrollbarGutter && textareaEl) {
      const computedStyle = window.getComputedStyle(textareaEl);
      const borderLeft = parseFloat(computedStyle.borderLeftWidth) || 0;
      const borderRight = parseFloat(computedStyle.borderRightWidth) || 0;
      const width = textareaEl.offsetWidth - textareaEl.clientWidth - (borderLeft + borderRight);
      scrollbarWidth = Math.max(0, width);
    }
  }
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
  // The +0.05/-0.05 buttons can numerically adjust A1111/NAI/InvokeAI numeric
  // forms and plain text, but not recognized-but-non-numeric forms (InvokeAI
  // emphasis marks, NAI brace wrap) — grey them out for those.
  const canAdjustWeight = $derived(
    hasSelection && !isNonNumericWeightSelection(value.substring(selectionStart, selectionEnd)),
  );

  // Estimated CLIP tokens for the current prompt, gauged against the 75-token
  // chunk boundary the text encoder splits on.
  const tokenCount = $derived(estimatePromptTokens(value));
  const tokenLimit = $derived(promptTokenLimit(tokenCount));
  const tokenOverflow = $derived(tokenCount > CLIP_CHUNK_SIZE);

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

    // Scheduling, preset, LoRA, and region blocks use commas inside their syntax.
    const inertRange = findPromptInertRangeContaining(text, pos);
    if (inertRange) {
      const token = text.substring(inertRange.start, inertRange.end);
      const leadingWhitespace = token.match(/^\s*/)?.[0].length ?? 0;
      const trailingWhitespace = token.match(/\s*$/)?.[0].length ?? 0;
      const trimmedStart = inertRange.start + leadingWhitespace;
      const trimmedEnd = Math.max(trimmedStart, inertRange.end - trailingWhitespace);
      const fragment = text.substring(trimmedStart, trimmedEnd);
      return {
        fragment,
        start: inertRange.start,
        end: inertRange.end,
        trimmedStart,
        trimmedEnd,
      };
    }

    // Find the start of the current tag (after the last comma or newline before cursor)
    let start = Math.max(text.lastIndexOf(",", pos - 1), text.lastIndexOf("\n", pos - 1)) + 1;
    // Find the end of the current tag (next comma or newline after cursor, or end of string).
    // Newlines delimit tags too — otherwise accepting a suggestion on its own line would
    // replace everything up to the next comma, swallowing tags on the lines below.
    let end = text.length;
    const commaEnd = text.indexOf(",", pos);
    if (commaEnd !== -1) end = commaEnd;
    const newlineEnd = text.indexOf("\n", pos);
    if (newlineEnd !== -1 && newlineEnd < end) end = newlineEnd;

    const token = text.substring(start, end);
    const leadingWhitespace = token.match(/^\s*/)?.[0].length ?? 0;
    const trailingWhitespace = token.match(/\s*$/)?.[0].length ?? 0;
    const trimmedStart = start + leadingWhitespace;
    const trimmedEnd = Math.max(trimmedStart, end - trailingWhitespace);
    const fragment = text.substring(trimmedStart, trimmedEnd);

    return { fragment, start, end, trimmedStart, trimmedEnd };
  }

  function updateSuggestions() {
    if (!tagAssist) {
      showSuggestions = false;
      suggestions = [];
      return;
    }
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
      .replace(new RegExp(`^${SYNTAX_ANGLE_LOOKBEHIND}<(?:from|to|range):[\\d.]+(?::[\\d.]+)?>`, "i"), "")
      .replace(/<\/(?:from|to|range)>$/i, "")
      .replace(new RegExp(`^${SYNTAX_ANGLE_LOOKBEHIND}<fromto\\[[\\d.]+\\]:`, "i"), "")
      .replace(/>$/i, "");

    if (searchFragment.length < 1) {
      showSuggestions = false;
      suggestions = [];
      return;
    }

    // Skip only while typing an in-progress weight expression like `(tag:1.2`.
    // Anchoring to the caret (no closing paren, ending at the number) means a
    // completed `(tag:1.2)` followed by a fresh tag still autocompletes.
    if (/^\([^)]*:\d[\d.]*$/.test(searchFragment)) {
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
    const schedPrefixMatch = result.fragment.match(
      new RegExp(`^(${SYNTAX_ANGLE_LOOKBEHIND}<(from|to|range):[\\d.]+(?::[\\d.]+)?>)`, "i"),
    );
    const schedSuffixMatch = result.fragment.match(/(<\/(from|to|range)>)$/i);
    const schedPrefix = schedPrefixMatch?.[1] ?? "";
    const schedType = schedPrefixMatch?.[2] ?? "";
    // Auto-close if there's an open tag but no closing tag yet
    const schedSuffix = schedSuffixMatch?.[1] ?? (schedType ? `</${schedType}>` : "");
    // SwarmUI syntax: <fromto[0.5]:tag — preserve the prefix (no closing tag needed)
    const swarmPrefixMatch = !schedPrefix
      ? result.fragment.match(new RegExp(`^(${SYNTAX_ANGLE_LOOKBEHIND}<fromto\\[[\\d.]+\\]:)`, "i"))
      : null;
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
    if (!textareaEl || !canAdjustWeight) return;
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
      if (start !== end && !isNonNumericWeightSelection(value.substring(start, end))) {
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
    const selected = value.substring(start, end);

    // Edit the number in place for A1111 (tag:w), NAI N::tag::, and InvokeAI
    // (tag)N; wrap plain text. Returns null for recognized-but-non-numeric
    // forms (emphasis marks, brace wrap) — the buttons grey out for those, but
    // guard here too in case Ctrl+Up/Down reaches it.
    const newText = adjustWeightText(selected, delta);
    if (newText === null) return;

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

  // Words inserted into the prompt via an enabled LoRA's trigger-word chip,
  // lowercased for case-insensitive matching against the raw prompt text.
  const activeLoraWords = $derived(
    highlightLoraWords
      ? new Set(
          generation.loras
            .filter((l) => l.enabled && l.name)
            .flatMap((l) => l.insertedWords ?? [])
            .map((w) => w.trim().toLowerCase())
            .filter(Boolean),
        )
      : new Set<string>(),
  );

  // Reactive: detect if current value has scheduling tags, inline preset tokens, or LoRA trigger words
  const showBackdrop = $derived(
    hasSchedulingTags(value) || hasPresetTokens(value) || hasLoraWordInPrompt(value, activeLoraWords),
  );

  // Reactive: render highlighted HTML for the backdrop overlay
  const highlightedHtml = $derived(
    showBackdrop ? renderHighlightedPrompt(value, promptPresets.slugs, activeLoraWords) : "",
  );
  const clickableSegments = $derived(
    tagAssist && autocomplete.clickableOverlayEnabled ? getPromptClickableSegments(value) : [],
  );
  const showClickableOverlay = $derived(
    tagAssist && autocomplete.clickableOverlayEnabled && clickableSegments.length > 0,
  );

  /** One painted box. Coordinates are relative to the mirror's border box, unscrolled. */
  interface HighlightRect {
    left: number;
    top: number;
    width: number;
    height: number;
  }
  /** A tag and the boxes covering it — more than one when it straddles a line break. */
  interface HighlightGroup {
    segment: PromptClickableSegment;
    rects: HighlightRect[];
  }
  let highlightGroups = $state<HighlightGroup[]>([]);

  // Exclude the token under the caret only when there's no active selection.
  const spellcheckCaret = $derived(selectionStart === selectionEnd ? selectionStart : -1);
  const spellcheckRanges = $derived(
    tagAssist && autocomplete.spellcheckEnabled
      ? getUnknownTagRanges(value, (n) => autocomplete.isKnownTag(n), spellcheckCaret)
      : [],
  );
  const showSpellcheckOverlay = $derived(
    tagAssist && autocomplete.spellcheckEnabled && spellcheckRanges.length > 0,
  );
  const spellcheckPieces = $derived(
    showSpellcheckOverlay ? buildSpellcheckPieces(value.length, spellcheckRanges) : [],
  );

  // Dismiss the right-click suggestion menu on any prompt edit: its items close over
  // fixed offsets, so editing the prompt while it is open would splice at stale positions.
  // Only `value` may trigger this — reading spellMenuVisible reactively would make opening
  // the menu (which sets it true) re-run the effect and immediately close it again.
  $effect(() => {
    void value;
    untrack(() => {
      if (spellMenuVisible) spellMenuVisible = false;
    });
  });

  /** The mirror holds the whole prompt as one text node; that node is the layout source. */
  function mirrorTextNode(): Text | null {
    const node = clickMirrorEl?.firstChild ?? null;
    return node && node.nodeType === Node.TEXT_NODE ? (node as Text) : null;
  }

  /**
   * Measure where each clickable tag actually sits and store the boxes to paint.
   *
   * The overlay used to draw one <span> per segment directly over the textarea. Splitting
   * the mirrored text into separate inline boxes makes the engine accumulate advance
   * widths per box instead of across the whole run, so at some container widths it picks
   * different line-break points than the textarea does — and from the first divergent
   * break onwards every hover box was drawn a line down and hundreds of pixels sideways.
   * The mirror is now a single unbroken text node, which cannot break differently, and
   * the boxes are painted separately from Range rectangles measured against it.
   */
  function measureHighlights() {
    const node = mirrorTextNode();
    if (!node || !clickMirrorEl) {
      highlightGroups = [];
      return;
    }
    // Store unscrolled coordinates relative to the mirror's border box, so scrolling
    // only has to translate the paint layer instead of re-measuring every tag.
    const origin = clickMirrorEl.getBoundingClientRect();
    const dx = clickMirrorEl.scrollLeft - origin.left;
    const dy = clickMirrorEl.scrollTop - origin.top;
    const range = document.createRange();
    const groups: HighlightGroup[] = [];
    for (const segment of clickableSegments) {
      // The mirror can still hold the previous value for a frame after an edit.
      if (!segment.clickable || segment.end > node.length) continue;
      range.setStart(node, segment.start);
      range.setEnd(node, segment.end);
      const rects: HighlightRect[] = [];
      for (const box of range.getClientRects()) {
        if (box.width <= 0 || box.height <= 0) continue;
        rects.push({
          left: box.left + dx,
          top: box.top + dy,
          width: box.width,
          height: box.height,
        });
      }
      if (rects.length > 0) groups.push({ segment, rects });
    }
    highlightGroups = groups;
  }

  function handleClickableSegmentMouseDown(event: MouseEvent, segment: PromptClickableSegment) {
    if (!textareaEl || !segment.clickable) return;
    // Right-click is handled via oncontextmenu; don't clobber the selection here.
    if (event.button !== 0) return;

    // Second click on an already-selected segment: place the caret at the exact
    // clicked character instead of re-selecting the whole segment. The first click
    // keeps the whole-segment select (the weight-button workflow).
    if (textareaEl.selectionStart === segment.start && textareaEl.selectionEnd === segment.end) {
      const caret = getCaretFromPoint(segment, event.clientX, event.clientY);
      if (caret >= 0) {
        event.preventDefault();
        event.stopPropagation();
        textareaEl.focus();
        textareaEl.setSelectionRange(caret, caret);
        syncSelectionRange();
        return;
      }
      // Caret unresolved: fall through to whole-segment select (still usable).
    }

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

  /**
   * Map a viewport point to an absolute character offset inside `segment`.
   *
   * A Range over [i, i+1) on the mirror gives the exact box of a single character, so
   * walking the segment's characters resolves the caret without document hit testing —
   * which would have to see through the `pointer-events: none` mirror. Segments are
   * single tags, so the scan is short. Returns -1 when the point is on none of the
   * lines the segment occupies (the caller falls back to selecting the whole segment).
   */
  function getCaretFromPoint(
    segment: PromptClickableSegment,
    clientX: number,
    clientY: number,
  ): number {
    const node = mirrorTextNode();
    if (!node || segment.end > node.length) return -1;
    const range = document.createRange();
    let caret = -1;
    for (let i = segment.start; i < segment.end; i++) {
      range.setStart(node, i);
      range.setEnd(node, i + 1);
      const box = range.getClientRects()[0];
      if (!box) continue;
      if (clientY < box.top || clientY >= box.bottom) continue;
      caret = clientX < box.left + box.width / 2 ? i : i + 1;
      // Past the right edge of this glyph the caret can still move on; stop once the
      // point falls within it, so the last matching character on the line wins.
      if (clientX < box.right) break;
    }
    return caret;
  }

  /**
   * Right-click on a clickable tag span: if it maps to an unknown-tag spellcheck
   * piece, open the suggestion menu; otherwise let the native context menu show.
   */
  function handleClickableSegmentContextMenu(event: MouseEvent, segment: PromptClickableSegment) {
    const piece = spellcheckPieces.find(
      (p) => p.unknown && p.name !== null && p.start === segment.start && p.end === segment.end,
    );
    if (piece) openSpellMenu(event, piece);
  }

  /**
   * Right-click anywhere in the textarea: covers underlined segments even when the
   * clickable overlay is disabled. WebView2/Chromium moves the caret to the click
   * point before `contextmenu` fires, so hit-test the caret against unknown pieces.
   */
  function handleTextareaContextMenu(event: MouseEvent) {
    if (!textareaEl || !tagAssist || !autocomplete.spellcheckEnabled) return;
    const caret = textareaEl.selectionStart;
    const piece = spellcheckPieces.find(
      (p) => p.unknown && p.name !== null && caret >= p.start && caret <= p.end,
    );
    if (piece) openSpellMenu(event, piece);
  }

  function replaceRange(start: number, end: number, replacement: string) {
    if (!textareaEl) return;
    undoStack = [...undoStack, value];
    redoStack = [];
    const before = value.substring(0, start);
    const after = value.substring(end);
    value = before + replacement + after;
    const caret = before.length + replacement.length;
    requestAnimationFrame(() => {
      textareaEl?.focus();
      textareaEl?.setSelectionRange(caret, caret);
      syncSelectionRange();
    });
  }

  function openSpellMenu(event: MouseEvent, piece: SpellcheckPiece) {
    if (!textareaEl || !piece.name) return;
    event.preventDefault();
    event.stopPropagation();

    // Select the offending tag so the replacement target is visible.
    textareaEl.focus();
    textareaEl.setSelectionRange(piece.start, piece.end);
    syncSelectionRange();

    const suggestions = autocomplete.suggestSimilar(piece.name);
    if (suggestions.length === 0) {
      spellMenuItems = [
        {
          label: locale.t("generation.prompt.spellcheck_no_suggestions"),
          action: () => {},
        },
      ];
    } else {
      spellMenuItems = suggestions.map((tag) => ({
        label: tag.n.replace(/_/g, " "),
        action: () => replaceRange(piece.start, piece.end, formatTagForPrompt(tag.n)),
      }));
    }
    spellMenuX = event.clientX;
    spellMenuY = event.clientY;
    spellMenuVisible = true;
  }

  function syncScroll() {
    if (!textareaEl) return;
    if (backdropEl) {
      backdropEl.scrollTop = textareaEl.scrollTop;
      backdropEl.scrollLeft = textareaEl.scrollLeft;
    }
    // The click mirror is deliberately not scrolled: it paints nothing, and Range
    // rectangles come from layout, so they are correct for text below the fold too.
    // It could not track the textarea anyway — a textarea is inline-level, so the
    // wrapper the mirror fills is a few pixels taller and clamps scrollTop lower.
    if (spellcheckOverlayEl) {
      spellcheckOverlayEl.scrollTop = textareaEl.scrollTop;
      spellcheckOverlayEl.scrollLeft = textareaEl.scrollLeft;
    }
    // The highlight boxes hold unscrolled coordinates, so the layer translates instead.
    overlayScrollTop = textareaEl.scrollTop;
    overlayScrollLeft = textareaEl.scrollLeft;
  }

  // Restore saved height and persist future resize changes via ResizeObserver.
  let resizeObserver: ResizeObserver | null = null;
  $effect(() => {
    if (!textareaEl) return;

    // Restore saved height on mount if storageKey is provided.
    if (storageKey) {
      const saved = localStorage.getItem(storageKey);
      if (saved) {
        textareaEl.style.height = saved;
        resizeStyle = `height: ${saved};`;
      }
    }

    // Observe future user-driven resizes.
    resizeObserver?.disconnect();
    resizeObserver = new ResizeObserver(() => {
      if (!textareaEl) return;
      if (storageKey) {
        const h = textareaEl.style.height;
        if (h && h !== "" && h !== "0px") {
          localStorage.setItem(storageKey, h);
          resizeStyle = `height: ${h};`;
        }
      }
      updateScrollbarWidth();
      measureHighlights();
    });
    resizeObserver.observe(textareaEl);
  });

  // Keep scrollbar width, scroll position and highlight geometry in step with the text.
  // Measuring inside rAF lets Svelte flush the mirror first; the reads there are
  // untracked, so the dependencies this effect needs are listed explicitly.
  $effect(() => {
    void value;
    void clickableSegments;
    void clickMirrorEl;
    requestAnimationFrame(() => {
      updateScrollbarWidth();
      syncScroll();
      measureHighlights();
    });
  });

  // Text metrics also change when the webfont swaps in and when the UI font scale is
  // changed (`--font-scale` on the root element). Neither resizes the textarea's own
  // box, so the ResizeObserver above never fires for them and the boxes would go stale.
  $effect(() => {
    let cancelled = false;
    void document.fonts?.ready.then(() => {
      if (!cancelled) measureHighlights();
    });
    const rootObserver = new MutationObserver(() => measureHighlights());
    rootObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["style"],
    });
    return () => {
      cancelled = true;
      rootObserver.disconnect();
    };
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

  $effect(() => {
    // Reset the category filter whenever the panel closes so a fresh tag always
    // starts from "all". Otherwise a category chosen for one tag stays active and
    // silently hides the panel for later tags with no match in it (issue #342).
    if (!showSuggestions) {
      categoryFilter = null;
    }
  });
</script>

<div class="relative">
  <!-- Always rendered so the textarea never shifts; buttons stay disabled until
       text is selected, which guards against inserting empty weight wrappers. -->
  <div class="mb-2 flex items-center gap-1.5">
    <button
      type="button"
      disabled={!canAdjustWeight}
      class="shrink-0 rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-300 enabled:hover:border-indigo-500 enabled:hover:text-indigo-300 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
      title={canAdjustWeight ? locale.t('generation.prompt.weight_up') : locale.t('generation.prompt.weight_select_hint')}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => adjustSelectedWeight(0.05)}
    >
      +0.05
    </button>
    <button
      type="button"
      disabled={!canAdjustWeight}
      class="shrink-0 rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-300 enabled:hover:border-indigo-500 enabled:hover:text-indigo-300 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
      title={canAdjustWeight ? locale.t('generation.prompt.weight_down') : locale.t('generation.prompt.weight_select_hint')}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => adjustSelectedWeight(-0.05)}
    >
      -0.05
    </button>
    <button
      type="button"
      disabled={!hasSelection}
      class="shrink-0 rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-300 enabled:hover:border-indigo-500 enabled:hover:text-indigo-300 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
      title={hasSelection ? locale.t('generation.prompt.wrap_stronger') : locale.t('generation.prompt.weight_select_hint')}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => wrapSelection("brace")}
    >
      &#123;&#125;
    </button>
    <button
      type="button"
      disabled={!hasSelection}
      class="shrink-0 rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-300 enabled:hover:border-indigo-500 enabled:hover:text-indigo-300 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
      title={hasSelection ? locale.t('generation.prompt.wrap_weaker') : locale.t('generation.prompt.weight_select_hint')}
      onmousedown={(e) => e.preventDefault()}
      onclick={() => wrapSelection("bracket")}
    >
      []
    </button>
    {#if !hasSelection}
      <span class="min-w-0 truncate text-[10px] text-neutral-600" title={locale.t('generation.prompt.weight_select_hint')}>{locale.t('generation.prompt.weight_select_hint')}</span>
    {/if}
    <span
      class="ml-auto shrink-0 rounded bg-neutral-900/70 px-1 tabular-nums text-[10px] {tokenOverflow ? 'text-amber-400' : 'text-neutral-500'}"
      title={locale.t('generation.prompt.tokens_tip')}
    >{tokenCount}/{tokenLimit}</span>
  </div>
  <div class="relative">
    {#if showBackdrop}
      <div
        bind:this={backdropEl}
        class="absolute inset-0 pointer-events-none overflow-hidden rounded-lg px-3 py-2 text-sm leading-5 whitespace-pre-wrap break-words border border-transparent"
        style="color: transparent; z-index: 0; {gutterStyle}"
      >{@html highlightedHtml}</div>
    {/if}

    <textarea
      bind:this={textareaEl}
      bind:value
      spellcheck={false}
      {placeholder}
      {rows}
      class="w-full border border-neutral-700 rounded-lg px-3 py-2 text-sm leading-5 text-neutral-100 placeholder-neutral-500 resize-y focus:outline-none focus:border-indigo-500 transition-colors break-words {minHeight} {showBackdrop ? 'bg-transparent' : 'bg-neutral-800'}"
      style="position: relative; z-index: 1; scrollbar-gutter: stable; {resizeStyle}{showBackdrop ? 'caret-color: #e5e5e5;' : ''}"
      onkeydown={handleKeydown}
      oninput={handleInput}
      onclick={handleClick}
        onselect={syncSelectionRange}
        onkeyup={syncSelectionRange}
      onblur={handleBlur}
      onscroll={syncScroll}
      oncontextmenu={handleTextareaContextMenu}
    ></textarea>

    {#if showClickableOverlay}
      <!--
        Measurement mirror. One unbroken text node, so it cannot pick different line
        breaks than the textarea (per-segment spans could, and did). Nothing is drawn
        here: the boxes are painted by the sibling layer below from Range rectangles
        measured against this text, which keeps them glued to the real glyphs.
      -->
      <div
        bind:this={clickMirrorEl}
        aria-hidden="true"
        class="absolute inset-0 overflow-hidden rounded-lg px-3 py-2 text-sm leading-5 whitespace-pre-wrap break-words border border-transparent select-none"
        style="pointer-events: none; color: transparent; z-index: 2; {gutterStyle}"
      >{value}</div>

      <div
        aria-hidden="true"
        class="absolute inset-0 overflow-hidden rounded-lg"
        style="pointer-events: none; z-index: 2;"
      >
        <div
          style="position: absolute; left: 0; top: 0; transform: translate({-overlayScrollLeft}px, {-overlayScrollTop}px);"
        >
          {#each highlightGroups as group (group.segment.start + ':' + group.segment.end + ':' + group.segment.kind)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="group"
              onmousedown={(event) => handleClickableSegmentMouseDown(event, group.segment)}
              oncontextmenu={(event) => handleClickableSegmentContextMenu(event, group.segment)}
            >
              {#each group.rects as rect, i (i)}
                <!-- A tag split across a line break paints one box per line; grouping
                     them means hovering either fragment lights the whole tag. -->
                <div
                  class="absolute pointer-events-auto cursor-pointer rounded-[4px] transition-colors {selectionStart === group.segment.start && selectionEnd === group.segment.end
                    ? group.segment.kind === 'weighted'
                      ? 'bg-amber-400/28 shadow-[inset_0_0_0_1px_rgba(252,211,77,0.8),0_0_10px_rgba(251,191,36,0.35)]'
                      : 'bg-indigo-400/24 shadow-[inset_0_0_0_1px_rgba(165,180,252,0.8),0_0_10px_rgba(129,140,248,0.35)]'
                    : group.segment.kind === 'weighted'
                      ? 'bg-amber-500/16 group-hover:bg-amber-500/22 shadow-[inset_0_0_0_1px_rgba(245,158,11,0.4)] group-hover:shadow-[inset_0_0_0_1px_rgba(251,191,36,0.55)]'
                      : 'group-hover:bg-indigo-500/18 group-hover:shadow-[inset_0_0_0_1px_rgba(129,140,248,0.5)]'}"
                  style="left: {rect.left}px; top: {rect.top}px; width: {rect.width}px; height: {rect.height}px;"
                ></div>
              {/each}
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if showSpellcheckOverlay}
      <div
        bind:this={spellcheckOverlayEl}
        aria-hidden="true"
        class="absolute inset-0 overflow-hidden rounded-lg px-3 py-2 text-sm leading-5 whitespace-pre-wrap break-words border border-transparent select-none"
        style="pointer-events: none; color: transparent; z-index: 3; {gutterStyle}"
      >
        {#each spellcheckPieces as piece (piece.start + ':' + piece.end)}
          {#if piece.unknown}
            <span
              class="underline decoration-wavy decoration-red-500 underline-offset-2"
              style="color: transparent; text-decoration-skip-ink: none;"
            >{value.slice(piece.start, piece.end)}</span>
          {:else}
            <span style="color: transparent;">{value.slice(piece.start, piece.end)}</span>
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
  <ContextMenu
    items={spellMenuItems}
    x={spellMenuX}
    y={spellMenuY}
    visible={spellMenuVisible}
    onclose={() => (spellMenuVisible = false)}
  />
</div>
