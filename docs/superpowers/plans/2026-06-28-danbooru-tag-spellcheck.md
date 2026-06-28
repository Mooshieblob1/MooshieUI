# Danbooru Tag-Aware Spell Checker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Underline prompt tags that are not real Danbooru tag names/aliases, and offer "did you mean" fuzzy suggestions on right-click, replacing the useless native English spell checker.

**Architecture:** Pure tokenizing/fuzzy logic lives in a new `promptSpellcheck.ts` util; the active tag corpus, a normalized known-tag `Set`, and the fuzzy lookup live on the existing `autocomplete` store; `PromptTextarea.svelte` adds a transparent-text underline overlay (same trick as the scheduling backdrop) whose unknown-tag spans handle right-click via the existing `ContextMenu.svelte`. A settings toggle (default ON) gates the whole feature; native browser spellcheck is disabled on the textarea in all states.

**Tech Stack:** Svelte 5 runes (`*.svelte.ts` class singletons, no `svelte/store`), TypeScript, Tailwind (no `<style>` blocks), Vite build as the validation gate.

## Global Constraints

- **No test framework exists.** Validation is `npm run build` (must end in `✓ built in`) plus the manual checks each task specifies. There is no vitest/jest; do not add one.
- **Windows git:** prefix every git command with `git -c core.hooksPath=/dev/null` (the bash pre-commit hook hangs in PowerShell).
- **No `Co-Authored-By` trailers** in any commit message.
- **i18n parity is a build-blocking rule:** every key added to `src/lib/locales/en.ts` must exist in all 10 other locale files with matching `{placeholder}` names. Canonical file: `src/lib/locales/en.ts`.
- **Svelte conventions:** Tailwind only (no `<style>` blocks), `onclick`/`oncontextmenu` not `on:click`, read shared state directly from the store singleton, reassign arrays with spread (no `.push()` on `$state` arrays), call `saveSettings()` explicitly after mutations, guard persisted fields with `!== undefined`/`=== false`.
- **Stores:** no imports from `svelte/store`; use `$state`/`get` accessors. `autocomplete` is a leaf utility store — do not import feature stores into it.
- **Work branch:** `feat/danbooru-tag-spellcheck` (already created; the design spec is already committed there).

---

## File Structure

- **Create** `src/lib/utils/promptSpellcheck.ts` — pure logic: Damerau-Levenshtein, unknown-tag range detection, overlay piece builder. No Svelte, no store imports.
- **Modify** `src/lib/stores/autocomplete.svelte.ts` — add `spellcheckEnabled` state + persistence, `_knownTagSet` + `isKnownTag()`, `suggestSimilar()`.
- **Modify** `src/lib/components/generation/PromptTextarea.svelte` — underline overlay layer, right-click menu wiring, `spellcheck={false}` on the textarea.
- **Modify** `src/lib/components/settings/SettingsPage.svelte` — toggle under the existing clickable-overlay switch.
- **Modify** all 11 `src/lib/locales/*.ts` — 3 new keys each.

Dependency order: i18n + store state → pure util → store fuzzy → overlay visual → right-click → settings toggle.

---

### Task 1: i18n keys in all locales

**Files:**
- Modify: `src/lib/locales/en.ts` (after line 1400, the `clickable_overlay_desc` entry)
- Modify: `src/lib/locales/de.ts`, `es.ts`, `fr.ts`, `it.ts`, `ja.ts`, `ko.ts`, `pt.ts`, `ru.ts`, `zh.ts`, `zh-tw.ts` (after each file's `settings.autocomplete.clickable_overlay_desc` entry)

**Interfaces:**
- Produces three flat string keys consumed by Tasks 5–7:
  - `settings.autocomplete.spellcheck`
  - `settings.autocomplete.spellcheck_desc`
  - `generation.prompt.spellcheck_no_suggestions`

The locale files are flat `Record<string, string>` maps; physical placement does not affect runtime, so insert all three keys together after each file's `clickable_overlay_desc` line. Placeholder count is zero for all three, so parity is automatic.

- [ ] **Step 1: Add the keys to `en.ts`**

Insert immediately after the `"settings.autocomplete.clickable_overlay_desc": "...",` line:

```ts
  "settings.autocomplete.spellcheck": "Tag spell check",
  "settings.autocomplete.spellcheck_desc": "Underline tags that aren't known Danbooru tags and suggest corrections on right-click.",
  "generation.prompt.spellcheck_no_suggestions": "No suggestions",
```

- [ ] **Step 2: Add translated keys to the 10 other locale files**

In each file, insert after that file's `"settings.autocomplete.clickable_overlay_desc": "...",` line. Use exactly these values:

`de.ts`:
```ts
  "settings.autocomplete.spellcheck": "Tag-Rechtschreibprüfung",
  "settings.autocomplete.spellcheck_desc": "Unterstreicht Tags, die keine bekannten Danbooru-Tags sind, und schlägt beim Rechtsklick Korrekturen vor.",
  "generation.prompt.spellcheck_no_suggestions": "Keine Vorschläge",
```

`es.ts`:
```ts
  "settings.autocomplete.spellcheck": "Corrector ortográfico de etiquetas",
  "settings.autocomplete.spellcheck_desc": "Subraya las etiquetas que no son etiquetas Danbooru conocidas y sugiere correcciones al hacer clic derecho.",
  "generation.prompt.spellcheck_no_suggestions": "Sin sugerencias",
```

`fr.ts`:
```ts
  "settings.autocomplete.spellcheck": "Vérification orthographique des tags",
  "settings.autocomplete.spellcheck_desc": "Souligne les tags qui ne sont pas des tags Danbooru connus et propose des corrections par clic droit.",
  "generation.prompt.spellcheck_no_suggestions": "Aucune suggestion",
```

`it.ts`:
```ts
  "settings.autocomplete.spellcheck": "Controllo ortografico dei tag",
  "settings.autocomplete.spellcheck_desc": "Sottolinea i tag che non sono tag Danbooru conosciuti e suggerisce correzioni con il clic destro.",
  "generation.prompt.spellcheck_no_suggestions": "Nessun suggerimento",
```

`ja.ts`:
```ts
  "settings.autocomplete.spellcheck": "タグスペルチェック",
  "settings.autocomplete.spellcheck_desc": "既知の Danbooru タグではないタグに下線を引き、右クリックで修正候補を表示します。",
  "generation.prompt.spellcheck_no_suggestions": "候補なし",
```

`ko.ts`:
```ts
  "settings.autocomplete.spellcheck": "태그 맞춤법 검사",
  "settings.autocomplete.spellcheck_desc": "알려진 Danbooru 태그가 아닌 태그에 밑줄을 긋고 마우스 오른쪽 클릭 시 수정 제안을 표시합니다.",
  "generation.prompt.spellcheck_no_suggestions": "제안 없음",
```

`pt.ts`:
```ts
  "settings.autocomplete.spellcheck": "Verificação ortográfica de tags",
  "settings.autocomplete.spellcheck_desc": "Sublinha tags que não são tags Danbooru conhecidas e sugere correções ao clicar com o botão direito.",
  "generation.prompt.spellcheck_no_suggestions": "Sem sugestões",
```

`ru.ts`:
```ts
  "settings.autocomplete.spellcheck": "Проверка орфографии тегов",
  "settings.autocomplete.spellcheck_desc": "Подчёркивает теги, не являющиеся известными тегами Danbooru, и предлагает исправления по правому клику.",
  "generation.prompt.spellcheck_no_suggestions": "Нет предложений",
```

`zh.ts`:
```ts
  "settings.autocomplete.spellcheck": "标签拼写检查",
  "settings.autocomplete.spellcheck_desc": "为非已知 Danbooru 标签的标签添加下划线，并在右键单击时建议更正。",
  "generation.prompt.spellcheck_no_suggestions": "无建议",
```

`zh-tw.ts`:
```ts
  "settings.autocomplete.spellcheck": "標籤拼字檢查",
  "settings.autocomplete.spellcheck_desc": "為非已知 Danbooru 標籤的標籤加上底線，並在右鍵點擊時建議更正。",
  "generation.prompt.spellcheck_no_suggestions": "無建議",
```

- [ ] **Step 3: Verify parity and build**

Run: `npm run build`
Expected: ends with `✓ built in`. (A missing key in any file does not fail the build, but the pre-commit i18n gate checks parity; confirm all 11 files contain the three keys.)

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/locales/
git -c core.hooksPath=/dev/null commit -m "i18n: add tag spell-check strings"
```

---

### Task 2: `spellcheckEnabled` state + persistence

**Files:**
- Modify: `src/lib/stores/autocomplete.svelte.ts` (state field near line 39; `loadSettings` ~302; `saveSettings` ~341; `collectPrefs` ~471)

**Interfaces:**
- Produces `autocomplete.spellcheckEnabled: boolean` (default `true`), persisted under the existing `autocomplete-settings` store key and synced via `collectPrefs`. Consumed by Tasks 5, 6, 7.

- [ ] **Step 1: Add the state field**

After the `clickableOverlayEnabled` field (line 39), add:

```ts
  /** Whether the Danbooru tag-aware spell checker (underlines + right-click suggestions) is on */
  spellcheckEnabled = $state(true);
```

- [ ] **Step 2: Hydrate in `loadSettings`**

In `loadSettings`, next to the existing `if (saved.clickableOverlayEnabled === false) ...` line, add:

```ts
        if (saved.spellcheckEnabled === false) this.spellcheckEnabled = false;
```

- [ ] **Step 3: Persist in `saveSettings` and `collectPrefs`**

In `saveSettings`, add to the `ipcStore.set` object (next to `clickableOverlayEnabled`):

```ts
        spellcheckEnabled: this.spellcheckEnabled,
```

In `collectPrefs`, add to the returned object (next to `clickableOverlayEnabled`):

```ts
      spellcheckEnabled: this.spellcheckEnabled,
```

- [ ] **Step 4: Build**

Run: `npm run build`
Expected: ends with `✓ built in`.

- [ ] **Step 5: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/stores/autocomplete.svelte.ts
git -c core.hooksPath=/dev/null commit -m "feat: add spellcheckEnabled setting (default on)"
```

---

### Task 3: Known-tag set + `isKnownTag()`

**Files:**
- Modify: `src/lib/stores/autocomplete.svelte.ts` (private fields near line 47; `indexOne` ~69; `rebuildSearchIndex` ~114; new public method)

**Interfaces:**
- Produces `autocomplete.isKnownTag(name: string): boolean`. Returns `true` for blank input and `true` for every input while the corpus index is empty/not-yet-built (so an unbuilt index never underlines the whole prompt). Consumed by Task 4's caller in Task 5.

The known set must be populated wherever `_searchEntries` is, including the chunked large-corpus path, and swapped in atomically with the other indexes. `indexOne` already computes `nameLower` and `aliasesLower`; thread the set through so each entry is added once.

- [ ] **Step 1: Add the field**

After the `_aliasFirstChar` field (line 51), add:

```ts
  /** Normalized known tag names + aliases for spell-check membership tests. */
  private _knownTagSet: Set<string> = new Set();
```

- [ ] **Step 2: Populate in `indexOne`**

Change `indexOne`'s signature to also receive the set, and add names/aliases to it. Replace the method header and add the inserts:

```ts
  private indexOne(
    tag: TagEntry,
    nameBuckets: Map<string, SearchEntry[]>,
    aliasBuckets: Map<string, SearchEntry[]>,
    knownSet: Set<string>,
  ): SearchEntry {
    const nameLower = tag.n.toLowerCase();
    const aliasesLower = tag.a ? tag.a.map((alias) => alias.toLowerCase()) : [];
    const entry: SearchEntry = { tag, nameLower, aliasesLower };

    if (nameLower.length > 0) {
      knownSet.add(this.normalizeQuery(nameLower));
    }
    for (const alias of aliasesLower) {
      if (alias.length > 0) knownSet.add(this.normalizeQuery(alias));
    }
```

(Leave the rest of `indexOne` — the bucket pushes and `return entry` — unchanged.)

- [ ] **Step 3: Thread the set through `rebuildSearchIndex`**

In `rebuildSearchIndex`, create a local set, pass it into both `indexOne` call sites, and assign it on each atomic swap. Replace the method body so both the sync and chunked paths build `knownSet`:

```ts
  private rebuildSearchIndex(tags: TagEntry[]): void {
    const version = ++this._indexVersion;
    const SYNC_THRESHOLD = 20000;
    const nameBuckets = new Map<string, SearchEntry[]>();
    const aliasBuckets = new Map<string, SearchEntry[]>();
    const knownSet = new Set<string>();

    if (tags.length <= SYNC_THRESHOLD) {
      const entries: SearchEntry[] = new Array(tags.length);
      for (let i = 0; i < tags.length; i++) {
        entries[i] = this.indexOne(tags[i], nameBuckets, aliasBuckets, knownSet);
      }
      this._searchEntries = entries;
      this._nameFirstChar = nameBuckets;
      this._aliasFirstChar = aliasBuckets;
      this._knownTagSet = knownSet;
      return;
    }

    const entries: SearchEntry[] = new Array(tags.length);
    const CHUNK = 8000;
    const buildChunk = (start: number) => {
      if (version !== this._indexVersion) return;
      const end = Math.min(start + CHUNK, tags.length);
      for (let i = start; i < end; i++) {
        entries[i] = this.indexOne(tags[i], nameBuckets, aliasBuckets, knownSet);
      }
      if (end < tags.length) {
        setTimeout(() => buildChunk(end), 0);
        return;
      }
      if (version !== this._indexVersion) return;
      this._searchEntries = entries;
      this._nameFirstChar = nameBuckets;
      this._aliasFirstChar = aliasBuckets;
      this._knownTagSet = knownSet;
    };
    buildChunk(0);
  }
```

- [ ] **Step 4: Add `isKnownTag`**

Add a public method (e.g. directly after `search()`):

```ts
  /**
   * True if `name` normalizes to a known tag name or alias in the active corpus.
   * Blank input and an empty/not-yet-built index both return true so nothing is
   * spuriously flagged as misspelled.
   */
  isKnownTag(name: string): boolean {
    if (this._knownTagSet.size === 0) return true;
    const q = this.normalizeQuery(name);
    if (!q) return true;
    return this._knownTagSet.has(q);
  }
```

- [ ] **Step 5: Build**

Run: `npm run build`
Expected: ends with `✓ built in`.

- [ ] **Step 6: Manual sanity check (browser console after `npm run tauri dev`)**

In the running app's devtools console, the store is reachable through the prompt; sanity-confirm logic by reasoning over the corpus: `1girl` is a real Danbooru tag (→ `isKnownTag` true), `1gril` is not (→ false). No automated assertion exists; this is a reasoning check.

- [ ] **Step 7: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/stores/autocomplete.svelte.ts
git -c core.hooksPath=/dev/null commit -m "feat: build known-tag set and add isKnownTag"
```

---

### Task 4: Pure spell-check util

**Files:**
- Create: `src/lib/utils/promptSpellcheck.ts`

**Interfaces:**
- Consumes `getPromptClickableSegments` from `./promptClickableRanges.ts` (returns `{start, end, kind: "text"|"tag"|"weighted", clickable}[]`, already skipping inert ranges).
- Produces:
  - `damerauLevenshtein(a: string, b: string, max: number): number` — optimal-string-alignment distance; returns `max + 1` as soon as the best possible distance exceeds `max` (early exit).
  - `interface UnknownTagRange { start: number; end: number; name: string }`
  - `getUnknownTagRanges(raw: string, isKnown: (name: string) => boolean, caretOffset: number): UnknownTagRange[]` — tag/weighted tokens whose extracted name is not known, excluding the token under `caretOffset` (pass `-1` to exclude nothing).
  - `interface SpellcheckPiece { start: number; end: number; unknown: boolean; name: string | null }`
  - `buildSpellcheckPieces(textLength: number, ranges: UnknownTagRange[]): SpellcheckPiece[]` — gap-free cover of `[0, textLength)` alternating known (transparent) and unknown pieces, used to render the overlay.

These types/signatures are relied on verbatim by Task 5 (overlay) and Task 6 (right-click).

- [ ] **Step 1: Create the file**

```ts
import { getPromptClickableSegments } from "./promptClickableRanges.ts";

export interface UnknownTagRange {
  start: number;
  end: number;
  name: string;
}

export interface SpellcheckPiece {
  start: number;
  end: number;
  unknown: boolean;
  name: string | null;
}

/**
 * Damerau-Levenshtein (optimal string alignment) distance with early exit.
 * Returns `max + 1` once the minimum achievable distance is already > max,
 * so callers can cheaply reject far-apart strings.
 */
export function damerauLevenshtein(a: string, b: string, max: number): number {
  const al = a.length;
  const bl = b.length;
  if (Math.abs(al - bl) > max) return max + 1;
  if (al === 0) return bl <= max ? bl : max + 1;
  if (bl === 0) return al <= max ? al : max + 1;

  let prevPrev = new Array<number>(bl + 1).fill(0);
  let prev = new Array<number>(bl + 1);
  let curr = new Array<number>(bl + 1);
  for (let j = 0; j <= bl; j++) prev[j] = j;

  for (let i = 1; i <= al; i++) {
    curr[0] = i;
    let rowMin = curr[0];
    for (let j = 1; j <= bl; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      let val = Math.min(
        prev[j] + 1, // deletion
        curr[j - 1] + 1, // insertion
        prev[j - 1] + cost, // substitution
      );
      if (
        i > 1 &&
        j > 1 &&
        a[i - 1] === b[j - 2] &&
        a[i - 2] === b[j - 1]
      ) {
        val = Math.min(val, prevPrev[j - 2] + 1); // transposition
      }
      curr[j] = val;
      if (val < rowMin) rowMin = val;
    }
    if (rowMin > max) return max + 1;
    const tmp = prevPrev;
    prevPrev = prev;
    prev = curr;
    curr = tmp;
  }
  return prev[bl] <= max ? prev[bl] : max + 1;
}

/** Extract the bare tag name from a clickable segment's raw text. */
function extractName(raw: string, start: number, end: number, weighted: boolean): string {
  const text = raw.slice(start, end);
  if (!weighted) return text;
  // Strip one wrapper layer: (name:1.2) -> name, {name} -> name, [name] -> name.
  const inner = text.slice(1, -1);
  const m = inner.match(/^(.*):\d*\.?\d+$/);
  return m ? m[1] : inner;
}

/**
 * Tag/weighted tokens whose name is not a known tag, excluding the token the
 * caret is currently inside (so in-progress typing is never flagged).
 * Pass caretOffset = -1 to exclude nothing (e.g. when a selection is active).
 */
export function getUnknownTagRanges(
  raw: string,
  isKnown: (name: string) => boolean,
  caretOffset: number,
): UnknownTagRange[] {
  if (!raw) return [];
  const out: UnknownTagRange[] = [];
  for (const seg of getPromptClickableSegments(raw)) {
    if (seg.kind !== "tag" && seg.kind !== "weighted") continue;
    if (caretOffset >= seg.start && caretOffset <= seg.end) continue;
    const name = extractName(raw, seg.start, seg.end, seg.kind === "weighted");
    if (!isKnown(name)) {
      out.push({ start: seg.start, end: seg.end, name });
    }
  }
  return out;
}

/** Cover [0, textLength) with alternating known/unknown pieces for the overlay. */
export function buildSpellcheckPieces(
  textLength: number,
  ranges: UnknownTagRange[],
): SpellcheckPiece[] {
  const pieces: SpellcheckPiece[] = [];
  let cursor = 0;
  for (const r of ranges) {
    if (r.start > cursor) {
      pieces.push({ start: cursor, end: r.start, unknown: false, name: null });
    }
    pieces.push({ start: r.start, end: r.end, unknown: true, name: r.name });
    cursor = r.end;
  }
  if (cursor < textLength) {
    pieces.push({ start: cursor, end: textLength, unknown: false, name: null });
  }
  return pieces;
}
```

- [ ] **Step 2: Build (type-check)**

Run: `npm run build`
Expected: ends with `✓ built in`. (Catches any signature/type mismatch.)

- [ ] **Step 3: Reasoning check against examples**

Confirm by hand:
- `damerauLevenshtein("1gril", "1girl", 2)` → `1` (one transposition).
- `damerauLevenshtein("cat", "elephant", 3)` → `4` (length diff 5 > 3 → early `max+1` = 4).
- `getUnknownTagRanges("1girl, 1gril", name => name === "1girl", -1)` → one range covering `1gril` (offsets 7–11).
- `getUnknownTagRanges("1girl, 1gr", name => name === "1girl", 9)` → `[]` (caret 9 is inside the `1gr` token, excluded).

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/utils/promptSpellcheck.ts
git -c core.hooksPath=/dev/null commit -m "feat: add prompt spell-check util (fuzzy + unknown-tag ranges)"
```

---

### Task 5: Fuzzy `suggestSimilar()` on the store

**Files:**
- Modify: `src/lib/stores/autocomplete.svelte.ts` (import line 1–4; new method after `isKnownTag`)

**Interfaces:**
- Consumes `damerauLevenshtein` from `../utils/promptSpellcheck.js`.
- Produces `autocomplete.suggestSimilar(name: string, limit?: number): TagEntry[]` — up to `limit` (default 6) corpus tags closest to `name`, ranked by `(distance asc, post count desc)`. Called only on right-click (Task 6). Returns `[]` for blank input.

- [ ] **Step 1: Import the distance function**

At the top of the file, after the existing imports, add:

```ts
import { damerauLevenshtein } from "../utils/promptSpellcheck.js";
```

- [ ] **Step 2: Add the method**

After `isKnownTag`, add:

```ts
  /**
   * Fuzzy "did you mean" matches for an unknown tag. Damerau-Levenshtein over the
   * active corpus, length-windowed and early-exited so it stays cheap even on large
   * custom lists. Only ever called on a right-click, never per keystroke.
   */
  suggestSimilar(name: string, limit = 6): TagEntry[] {
    const q = this.normalizeQuery(name);
    if (!q) return [];
    const maxDist = q.length <= 4 ? 1 : q.length <= 8 ? 2 : 3;

    const scored: { tag: TagEntry; dist: number }[] = [];
    const entries = this._searchEntries;
    for (let i = 0; i < entries.length; i++) {
      const n = entries[i].nameLower;
      if (Math.abs(n.length - q.length) > maxDist) continue;
      const d = damerauLevenshtein(q, n, maxDist);
      if (d <= maxDist) scored.push({ tag: entries[i].tag, dist: d });
    }

    scored.sort((a, b) => a.dist - b.dist || b.tag.p - a.tag.p);
    return scored.slice(0, Math.max(1, limit)).map((s) => s.tag);
  }
```

- [ ] **Step 3: Build**

Run: `npm run build`
Expected: ends with `✓ built in`.

- [ ] **Step 4: Reasoning check**

For the builtin corpus, `suggestSimilar("1gril")` should rank `1girl` first (distance 1, high post count). `suggestSimilar("blue_eys")` should surface `blue_eyes`.

- [ ] **Step 5: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/stores/autocomplete.svelte.ts
git -c core.hooksPath=/dev/null commit -m "feat: add suggestSimilar fuzzy tag lookup"
```

---

### Task 6: Underline overlay + native spellcheck off

**Files:**
- Modify: `src/lib/components/generation/PromptTextarea.svelte` (imports ~8–18; `$derived` block ~497–500; `syncScroll` ~520; overlay markup ~730–756; `<textarea>` ~714–728)

**Interfaces:**
- Consumes `getUnknownTagRanges`, `buildSpellcheckPieces`, types `UnknownTagRange`/`SpellcheckPiece` from `../../utils/promptSpellcheck.js`; `autocomplete.spellcheckEnabled`, `autocomplete.isKnownTag` from the store.
- Produces a `spellcheckPieces` `$derived` and a scroll-synced underline overlay div (`spellcheckOverlayEl`). The unknown-piece spans (and their hit handling) are extended in Task 7.

This task delivers the **visual** underline only; right-click is Task 7. The overlay layers like the existing clickable overlay (transparent text, scroll-synced). Underlined spans use `text-decoration` so the squiggle paints over the real textarea glyphs.

- [ ] **Step 1: Import the util**

In the `<script>` import block, after the `getPromptClickableSegments` import, add:

```ts
  import {
    getUnknownTagRanges,
    buildSpellcheckPieces,
    type SpellcheckPiece,
  } from "../../utils/promptSpellcheck.js";
```

- [ ] **Step 2: Add the overlay element ref**

Next to `let clickOverlayEl = $state<HTMLDivElement | null>(null);` (line 45), add:

```ts
  let spellcheckOverlayEl = $state<HTMLDivElement | null>(null);
```

- [ ] **Step 3: Derive the unknown ranges and pieces**

After the existing `clickableSegments`/`showClickableOverlay` derivations (line 500), add:

```ts
  // Exclude the token under the caret only when there's no active selection.
  const spellcheckCaret = $derived(selectionStart === selectionEnd ? selectionStart : -1);
  const spellcheckRanges = $derived(
    autocomplete.spellcheckEnabled
      ? getUnknownTagRanges(value, (n) => autocomplete.isKnownTag(n), spellcheckCaret)
      : [],
  );
  const showSpellcheckOverlay = $derived(
    autocomplete.spellcheckEnabled && spellcheckRanges.length > 0,
  );
  const spellcheckPieces = $derived(
    showSpellcheckOverlay ? buildSpellcheckPieces(value.length, spellcheckRanges) : [],
  );
```

- [ ] **Step 4: Keep the overlay scroll-synced**

In `syncScroll`, after the `clickOverlayEl` block, add:

```ts
    if (textareaEl && spellcheckOverlayEl) {
      spellcheckOverlayEl.scrollTop = textareaEl.scrollTop;
      spellcheckOverlayEl.scrollLeft = textareaEl.scrollLeft;
    }
```

- [ ] **Step 5: Disable native spellcheck on the textarea**

On the `<textarea>` element (line 714), add the attribute (place it next to `bind:value`):

```svelte
      spellcheck={false}
```

- [ ] **Step 6: Render the underline overlay**

Immediately after the closing `{/if}` of the `{#if showClickableOverlay}` block (after line 756, before the outer `</div>` at 757), add:

```svelte
    {#if showSpellcheckOverlay}
      <div
        bind:this={spellcheckOverlayEl}
        aria-hidden="true"
        class="absolute inset-0 overflow-hidden rounded-lg px-3 py-2 text-sm leading-5 whitespace-pre-wrap break-words border border-transparent select-none"
        style="pointer-events: none; color: transparent; z-index: 3; right: {scrollbarWidth}px;"
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
```

- [ ] **Step 7: Build**

Run: `npm run build`
Expected: ends with `✓ built in`.

- [ ] **Step 8: Manual check (`npm run tauri dev`)**

Type `1girl, 1gril, blue eyes` in the positive prompt. After moving the caret out of `1gril`, a red wavy underline appears under `1gril` only (`1girl` and `blue eyes` stay clean). While the caret is inside a token you are typing, that token is not underlined. Toggling does nothing yet (Task 7's settings switch) — confirm the underline tracks edits and scrolls with the text.

- [ ] **Step 9: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/components/generation/PromptTextarea.svelte
git -c core.hooksPath=/dev/null commit -m "feat: underline unknown tags and disable native spellcheck"
```

---

### Task 7: Right-click "did you mean" menu

**Files:**
- Modify: `src/lib/components/generation/PromptTextarea.svelte` (imports; new state + handlers; the unknown-piece span in the overlay; ContextMenu markup)

**Interfaces:**
- Consumes `autocomplete.suggestSimilar`, `ContextMenu` + `ContextMenuItem`, `locale.t`, the existing `undoStack`/`value`/`textareaEl` machinery, and `formatTagForPrompt`.
- Produces a working right-click flow: right-click an underlined tag → menu of suggestions → pick → in-place replacement with undo.

- [ ] **Step 1: Import ContextMenu and its item type**

In the import block, add:

```ts
  import ContextMenu, { type ContextMenuItem } from "../ui/ContextMenu.svelte";
```

- [ ] **Step 2: Add menu state**

Near the other `$state` declarations (after `spellcheckOverlayEl`), add:

```ts
  let spellMenuVisible = $state(false);
  let spellMenuX = $state(0);
  let spellMenuY = $state(0);
  let spellMenuItems = $state<ContextMenuItem[]>([]);
```

- [ ] **Step 3: Add the right-click + replacement handlers**

Add these functions in the `<script>` (e.g. after `handleClickableSegmentMouseDown`):

```ts
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
```

- [ ] **Step 4: Wire the unknown-piece span to right-click**

Replace the unknown-piece `<span>` added in Task 6 (the `{#if piece.unknown}` branch) with one that captures contextmenu and enables pointer events:

```svelte
          {#if piece.unknown}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <span
              class="pointer-events-auto cursor-context-menu underline decoration-wavy decoration-red-500 underline-offset-2"
              style="color: transparent; text-decoration-skip-ink: none;"
              oncontextmenu={(event) => openSpellMenu(event, piece)}
            >{value.slice(piece.start, piece.end)}</span>
          {:else}
```

- [ ] **Step 5: Render the ContextMenu**

After the `{#if showSuggestions}...{/if}` dropdown block near the end of the markup (after line 794, before the final `</div>`), add:

```svelte
  <ContextMenu
    items={spellMenuItems}
    x={spellMenuX}
    y={spellMenuY}
    visible={spellMenuVisible}
    onclose={() => (spellMenuVisible = false)}
  />
```

- [ ] **Step 6: Build**

Run: `npm run build`
Expected: ends with `✓ built in`.

- [ ] **Step 7: Manual check (`npm run tauri dev`)**

Type `1gril, blue eyes`. Right-click the underlined `1gril`: a menu lists suggestions (e.g. `1girl`) ranked by closeness/popularity. Picking `1girl` replaces only that token (`1girl, blue eyes`), caret lands after it, and Ctrl+Z restores `1gril`. Right-clicking a known tag or plain prose still shows the OS native menu (no custom menu). Right-click on an unknown tag with no close match shows a single disabled-feeling "No suggestions" entry.

- [ ] **Step 8: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/components/generation/PromptTextarea.svelte
git -c core.hooksPath=/dev/null commit -m "feat: right-click did-you-mean menu for unknown tags"
```

---

### Task 8: Settings toggle

**Files:**
- Modify: `src/lib/components/settings/SettingsPage.svelte` (after the clickable-overlay toggle `<label>`, ~line 3241)

**Interfaces:**
- Consumes `autocomplete.spellcheckEnabled` + `autocomplete.saveSettings()`, and the i18n keys from Task 1. No new outputs.

- [ ] **Step 1: Add the toggle**

Immediately after the closing `</label>` of the existing `clickable_overlay` toggle (line 3241), add:

```svelte
            <label class="flex items-center justify-between gap-3 cursor-pointer">
              <div>
                <p class="text-sm text-neutral-200">{locale.t('settings.autocomplete.spellcheck')}</p>
                <p class="text-[11px] text-neutral-500 mt-0.5">{locale.t('settings.autocomplete.spellcheck_desc')}</p>
              </div>
              <button
                class="relative w-10 h-5 rounded-full transition-colors shrink-0 {autocomplete.spellcheckEnabled ? 'bg-indigo-600' : 'bg-neutral-700'}"
                onclick={() => { autocomplete.spellcheckEnabled = !autocomplete.spellcheckEnabled; autocomplete.saveSettings(); }}
                role="switch"
                aria-checked={autocomplete.spellcheckEnabled}
                aria-label={locale.t('settings.autocomplete.spellcheck')}
                title={locale.t('settings.autocomplete.spellcheck')}
              >
                <span class="absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform {autocomplete.spellcheckEnabled ? 'translate-x-5' : ''}"></span>
              </button>
            </label>
```

- [ ] **Step 2: Build**

Run: `npm run build`
Expected: ends with `✓ built in`.

- [ ] **Step 3: Manual check (`npm run tauri dev`)**

In Settings → autocomplete section, the new "Tag spell check" toggle appears under "clickable tag overlay", defaults ON. Turning it OFF removes all underlines and the custom right-click menu (native menu returns, with no English squiggles since `spellcheck={false}` is permanent). Turning it back ON restores underlines. Reload the app: the toggle state persists.

- [ ] **Step 4: Commit**

```bash
git -c core.hooksPath=/dev/null add src/lib/components/settings/SettingsPage.svelte
git -c core.hooksPath=/dev/null commit -m "feat: settings toggle for tag spell check"
```

---

## Final Verification

- [ ] `npm run build` ends in `✓ built in`.
- [ ] All 11 locale files contain the 3 new keys.
- [ ] Manual end-to-end: underline appears on settled unknown tags only; right-click suggests + replaces in place with working undo; known tags/prose untouched; toggle off kills underlines + custom menu and shows no English squiggles; setting persists across reload; scheduling/LoRA/preset/region syntax never underlined.

## Self-Review Notes

- **Spec coverage:** detection (Tasks 3–4, 6), right-click suggestions (Tasks 5, 7), toggle default-ON (Tasks 2, 8), native spellcheck off (Task 6), i18n parity (Task 1), corpus-awareness incl. empty-index guard (Task 3 `isKnownTag`), caret exclusion + weighted/inert handling (Task 4). All spec sections map to a task.
- **No unit-test steps** because the repo has no test framework (per Global Constraints); verification is `npm run build` + the manual checks the spec's Validation section prescribes, plus reasoning checks for the pure functions.
- **Type consistency:** `UnknownTagRange`/`SpellcheckPiece`/`damerauLevenshtein`/`getUnknownTagRanges`/`buildSpellcheckPieces` (Task 4) are used with identical names/signatures in Tasks 5–7; `spellcheckEnabled`/`isKnownTag`/`suggestSimilar` store members are named identically wherever consumed.
