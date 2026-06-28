# Danbooru Tag-Aware Spell Checker — Design

**Issue:** [#390](https://github.com/Mooshieblob1/MooshieUI/issues/390)
**Date:** 2026-06-28
**Status:** Approved

## Goal

Replace the useless native English spell checker on the prompt textarea with a
Danbooru tag-aware one. Tags that are not real Danbooru tag names (or aliases) get
a red wavy underline, and right-clicking an underlined tag opens an in-app
"did you mean" menu of close matches. Picking one replaces that tag in place.

This turns the browser's red squiggles (which flag every booru tag like `1girl`
as a misspelling and offer English suggestions) into squiggles that mean something:
"this is not a known tag."

## Scope

**In scope (core):**
- Detect unknown tags in the positive/negative prompt textareas and underline them.
- Right-click an underlined tag → context menu of fuzzy ("did you mean") suggestions.
- A settings toggle, default **ON**.
- Disable native browser spellcheck on the prompt textarea in all states.

**Out of scope (deferred, per brainstorming):**
- e621 / cross-booru tag translation.
- Suggesting while a tag is still being typed (only "settled" tags are checked).
- Underlining inside scheduling / region / segment / preset / LoRA syntax (the
  tokenizer already treats those as inert, so they are skipped for free).
- Any new dependency. Fuzzy matching is implemented in-repo.

## Existing infrastructure this builds on

All of these already exist and are reused rather than reinvented:

- **`getPromptClickableSegments(raw)`** in [promptClickableRanges.ts](../../../src/lib/utils/promptClickableRanges.ts)
  tokenizes the prompt into `{start, end, kind, clickable}` segments where `kind` is
  `"text" | "tag" | "weighted"`. It already skips inert ranges (scheduling, region,
  preset, LoRA) and splits on commas at brace/paren depth 0. The tag/weighted
  segments are exactly the tokens we want to spell-check.
- **Backdrop overlay pattern** in [PromptTextarea.svelte](../../../src/lib/components/generation/PromptTextarea.svelte)
  (`showBackdrop` / `highlightedHtml` and the clickable overlay): a div layered over
  the textarea with `color: transparent`, scroll-synced, rendering styled spans whose
  geometry matches the textarea text. Underlines use the same trick — a transparent-text
  span whose `text-decoration` is still painted in its own color.
- **`ContextMenu.svelte`** ([ui/ContextMenu.svelte](../../../src/lib/components/ui/ContextMenu.svelte)):
  takes `items`, `x`, `y`, `visible`, `onclose`; handles viewport clamping, click-outside,
  Escape, and scroll/blur close. Drop-in for the suggestion menu.
- **`autocomplete` store** ([autocomplete.svelte.ts](../../../src/lib/stores/autocomplete.svelte.ts)):
  owns the tag corpus, `normalizeQuery` (lowercase, trim, spaces→`_`, strip `\`),
  the first-char bucket indexes, and the settings persistence (`loadSettings` /
  `saveSettings` / `collectPrefs`). The new state and methods live here so detection
  and suggestions track whatever corpus is active (builtin / Anima / custom).

## Architecture

Five touchpoints:

### 1. `autocomplete` store — detection + suggestions + toggle

- **`spellcheckEnabled = $state(true)`** — persisted in `loadSettings`/`saveSettings`/
  `collectPrefs` alongside `clickableOverlayEnabled` (same `saved.x === false` guard so
  the default is ON). Default ON.
- **`_knownTagSet: Set<string>`** — built inside `rebuildSearchIndex` from every
  `nameLower` plus each entry's `aliasesLower`. For the chunked (large-corpus) path it
  is populated incrementally in the same loop that builds `_searchEntries` and swapped
  in atomically with the other indexes, so it never disagrees with the active corpus.
- **`isKnownTag(name: string): boolean`** — `_knownTagSet.has(this.normalizeQuery(name))`.
  Returns `true` for the empty/normalized-empty string so blank tokens are never flagged.
- **`suggestSimilar(name: string, limit = 6): TagEntry[]`** — fuzzy match against the
  corpus (algorithm below). Only ever called on a right-click, never per keystroke.

### 2. `promptSpellcheck.ts` — new util

`getUnknownTagRanges(raw, isKnown, caretOffset): { start, end, name }[]`

- Runs `getPromptClickableSegments(raw)`, keeps only `kind === "tag"` and
  `kind === "weighted"` segments.
- For `weighted`, extracts the inner tag name from `(name:1.2)` (reuse the same
  parse the weight code uses — strip the outer parens and the trailing `:<number>`);
  for `tag`, uses the segment text directly. Strips backslash escapes via the existing
  `normalizeQuery` path before the `isKnown` check.
- Drops the segment that contains `caretOffset` (the tag currently being edited) so
  in-progress typing is never underlined.
- Returns ranges where `isKnown(name)` is false, carrying the cleaned `name` for the
  suggestion lookup.

This keeps all string math out of the Svelte component and makes the rule independently
testable.

### 3. `PromptTextarea.svelte` — underline overlay + right-click

- **Underline overlay layer.** A new overlay div, rendered only when
  `autocomplete.spellcheckEnabled` and `unknownRanges.length > 0`, layered like the
  existing clickable overlay (scroll-synced, `right: {scrollbarWidth}px`,
  `whitespace-pre-wrap break-words`, matching padding/leading). It walks the full text
  emitting transparent spans; spans covering an unknown range get
  `text-decoration: underline wavy; text-decoration-color: <red>; text-underline-offset`
  so only the squiggle is visible over the real textarea glyphs.
  `unknownRanges` is `$derived` from `value`, `autocomplete.spellcheckEnabled`, the
  active corpus, and the caret position.
- **Right-click.** The unknown-tag spans get `pointer-events: auto` and an
  `oncontextmenu` handler (every other span in the layer is `pointer-events: none`, so
  known tags and prose fall through to the textarea's native menu). The handler
  `preventDefault()`s, selects the tag range in the textarea (so the replacement target
  is unambiguous and visible), calls `autocomplete.suggestSimilar(name)`, and opens
  `ContextMenu` at the cursor with one item per suggestion plus a disabled
  "No suggestions" item when empty. This span-level handling is deliberate: it sidesteps
  the unreliable "did the browser move the caret on right-click" question and is why
  right-click currently does nothing (the clickable overlay span eats the event and
  `App.svelte` then suppresses the native menu).
- **Replacement.** Selecting a suggestion reuses the existing insertion machinery:
  push `value` to `undoStack`, splice `formatTagForPrompt(suggestion.n)` over
  `[start, end)`, restore focus and caret after the replacement (the same pattern as
  `acceptSuggestion`). No new commas are introduced — it is an in-place swap.
- **Native spellcheck off.** Add `spellcheck={false}` to the `<textarea>` unconditionally
  (both toggle states) so the browser's English squiggles never appear on tags.

### 4. `SettingsPage.svelte` — toggle

A new toggle directly under the existing "clickable overlay" switch in the autocomplete
section, modeled exactly on it:
`autocomplete.spellcheckEnabled = !...; autocomplete.saveSettings();`. Labels via
`settings.autocomplete.spellcheck` / `settings.autocomplete.spellcheck_desc`.

### 5. i18n

New keys added to [en.ts](../../../src/lib/locales/en.ts) **and every other locale file**
(parity is a build gate):
- `settings.autocomplete.spellcheck`
- `settings.autocomplete.spellcheck_desc`
- `generation.prompt.spellcheck_no_suggestions`

The "did you mean" menu lists raw tag names (with counts), so it needs no per-tag i18n.

## Fuzzy matching (`suggestSimilar`)

Damerau-Levenshtein (optimal string alignment — handles the common transposition typo,
e.g. `1gril` → `1girl`) against `normalizeQuery(name)`, computed only on right-click.

- **Prefilter** to keep it well under a frame even on large/custom corpora: candidates
  must be within `±2` in length and `maxDist` scales with length (`1` for ≤4 chars,
  `2` for ≤8, `3` otherwise). Distance is computed with early-exit once it exceeds
  `maxDist`. The first-char buckets are *not* used to gate (the typo may be in the first
  char), but the length window plus early-exit keep the full scan cheap; if profiling on
  the largest corpus shows a stall, fall back to scanning only buckets for the query's
  first char and its near neighbors and `log()` that the candidate set was capped.
- **Rank** by `(distance asc, postCount desc)`, take top `limit` (6). Display uses the
  same `name.replace(/_/g, " ")` formatting as the autocomplete dropdown.

## Data flow

```
value changes ─┐
caret moves   ─┼─► getUnknownTagRanges(value, isKnownTag, caret) ─► $derived unknownRanges
corpus swaps  ─┘                                                        │
                                                                        ▼
                                          underline overlay spans (transparent text, red wavy)
                                                                        │ right-click on span
                                                                        ▼
                          suggestSimilar(name) ─► ContextMenu items ─► pick ─► in-place splice + undo push
```

## Edge cases

- **Empty / whitespace token** → `isKnownTag` returns true, never underlined.
- **Token under caret** → excluded, so typing `1gir|` does not flash an underline.
- **Weighted tag** `(1girl:1.2)` → inner `1girl` checked; the wrapper is not.
- **Inert syntax** (scheduling/region/segment/preset/LoRA) → already non-clickable
  segments, so skipped.
- **Prose-style prompts** (`a girl standing in a field` as one comma chunk) normalize to
  one long non-tag token and get underlined. This is expected for a *tag* checker and is
  the main reason the toggle exists; documented, not worked around.
- **Custom / Anima / empty corpus** → detection and suggestions both read the live
  `_knownTagSet` / corpus, so they follow whatever list is active. An empty corpus means
  nothing is "known"; guard so a not-yet-built index does not underline the whole prompt
  (treat an empty `_knownTagSet` as "spellcheck inactive").
- **`autocomplete.enabled === false`** → spellcheck is independent of the suggestion
  dropdown but shares the corpus; it stays governed solely by `spellcheckEnabled`.
- **Browser (web) mode** → pure frontend overlay + existing settings sync via
  `collectPrefs`/`applyServerPrefs`; no backend work, so it works identically.

## Non-functional

- **Performance:** detection runs on the same cadence as the existing overlays
  (`$derived` over `value`), is O(n) tokenization plus O(tokens) set lookups — no corpus
  scan. The only corpus scan is `suggestSimilar`, gated to right-click. No new
  per-keystroke cost.
- **No new deps.** Damerau-Levenshtein is ~30 lines in `promptSpellcheck.ts`.

## Validation

No test framework exists; validation is the standard gate:
- `npm run build` (frontend) must end in `✓ built in`.
- Locale parity check (all locale files carry the 3 new keys with matching placeholders).
- Manual: type a misspelled tag → underline appears once the token is left; right-click →
  suggestions; pick one → in-place replacement + working undo; toggle off → underlines and
  custom menu gone, no native English squiggles; verify scheduling/LoRA/preset syntax is
  never underlined.

## Open question for implementation

`getUnknownTagRanges` needs the caret offset to exclude the in-progress token. The
component already tracks `selectionStart`/`selectionEnd`; pass `selectionStart` and treat
a non-empty selection as "no excluded token."
