/**
 * Style Creator: draw random artist combinations, generate them side by side
 * on one pinned seed, and save the one the user picks as an Artist Style with
 * that image as its thumbnail.
 *
 * The draw is pure random over the Artists index. No LLM is involved.
 *
 * A round holds one or two cards. Each card is submitted through the normal
 * `submitGeneration()` path with `skipActiveStyles` so the user's active
 * styles do not leak into the comparison, and the finished image is routed
 * back from `finalizeOutputImages` in App.svelte through `resolve()` and
 * `record()`, the same hook shape the artist previews use.
 *
 * A combination is keyed order-insensitively (sorted slugs, weight suffix when
 * it is not 1), so the same artists in another order are never offered twice.
 * The key set is persisted, and every saved style is blocked as well.
 */
import { generation } from "./generation.svelte.js";
import { gallery } from "./gallery.svelte.js";
import {
  bakedArtistWeight,
  fragmentForStyles,
  resizeImageToDataUrl,
  styles,
  type StyleArtist,
} from "./styles.svelte.js";
import { styleEditors } from "./styleEditors.svelte.js";
import { artistInsert } from "./artistInsert.svelte.js";
import { locale } from "./locale.svelte.js";
import { artistFavourites } from "../artist-gallery/favourites.svelte.js";
import { submitGeneration } from "../utils/generationSubmit.js";
import { classifyGenerationError } from "../utils/generationErrors.js";
import { artistIndexKey, artistTagBodiesMatch, stripArtistSigil } from "../utils/artistTag.js";
import { isBelowThresholdCount } from "../artist-gallery/counts.js";
import type { ArtistSearchHit } from "../artist-gallery/types.js";
import type { OutputImage } from "../types/index.js";

const STORAGE_KEY = "mooshieui.styleCreator.v1";
/** Redraw attempts before a round is declared exhausted. */
const MAX_DRAW_ATTEMPTS = 100;
const MIN_COUNT = 1;
const MAX_COUNT = 10;

export type StyleCreatorPhase = "idle" | "generating" | "choosing";

export interface RoundCard {
  /** Order-insensitive combination key. Empty for the anchor-alone card. */
  key: string;
  artists: StyleArtist[];
  /** Pre-filled style name. Empty for the anchor-alone card. */
  name: string;
  /** False for the anchor-alone card, which is a reference, not a candidate. */
  saveable: boolean;
  image: OutputImage | null;
}

export interface StyleCreatorRound {
  seed: string;
  cards: RoundCard[];
}

interface KeyEntry {
  slug: string;
  weight: number;
}

function keyEntry(slug: string, weight: number): string {
  const s = slug.toLowerCase();
  return weight === 1 ? s : `${s}:${weight}`;
}

/**
 * One entry per artist, lowercased, sorted, joined with `+`. Sorting is what
 * makes `a + b` and `b + a` the same combination.
 */
export function combinationKey(entries: KeyEntry[]): string {
  return entries
    .map((e) => keyEntry(e.slug, e.weight))
    .sort()
    .join("+");
}

/** A hand-typed artist has no slug, so its index key stands in for one. */
function artistSlug(a: StyleArtist): string {
  return a.slug ?? artistIndexKey(a.tag);
}

function artistKeyEntries(artists: StyleArtist[], overallWeight: number): KeyEntry[] {
  return artists.map((a) => ({
    slug: artistSlug(a),
    weight: bakedArtistWeight(a, overallWeight),
  }));
}

/** Uniform 0.5 to 1.5 in steps of 0.1, for the Vary weights option. */
function randomWeight(): number {
  return (5 + Math.floor(Math.random() * 11)) / 10;
}

/** Human form of a tag for the pre-filled style name: no sigil, no escapes, spaces. */
function displayName(tag: string): string {
  return stripArtistSigil(tag)
    .replace(/\\([()[\]+-])/g, "$1")
    .replace(/_/g, " ")
    .trim();
}

/** Partial Fisher-Yates over a copy: n distinct items, uniform, no mutation. */
function sampleWithoutReplacement<T>(pool: T[], n: number): T[] {
  const copy = pool.slice();
  const take = Math.min(n, copy.length);
  for (let i = 0; i < take; i++) {
    const j = i + Math.floor(Math.random() * (copy.length - i));
    const tmp = copy[i];
    copy[i] = copy[j];
    copy[j] = tmp;
  }
  return copy.slice(0, take);
}

function clampCount(n: unknown): number {
  const v = typeof n === "number" && Number.isFinite(n) ? Math.round(n) : 3;
  return Math.max(MIN_COUNT, Math.min(MAX_COUNT, v));
}

class StyleCreatorStore {
  // Controls (persisted).
  count = $state(3);
  favouritesOnly = $state(false);
  varyWeights = $state(false);
  pairInNai = $state(true);
  anchor = $state<StyleArtist[]>([]);
  anchorStyleId = $state<string | null>(null);
  history = $state<Set<string>>(new Set());

  // Round state (session only).
  phase = $state<StyleCreatorPhase>("idle");
  running = $state(false);
  round = $state<StyleCreatorRound | null>(null);
  error = $state<string | null>(null);
  note = $state<string | null>(null);

  /** promptId -> card index for the round in flight. */
  private pending = new Map<string, number>();
  /** Bumped per round so a late submit from a stopped round cannot land. */
  private roundSerial = 0;
  // The anchor-alone card is identical between rounds whenever the seed and
  // the anchor have not changed, so its image is reused instead of re-billed.
  private lastAnchorSignature: string | null = null;
  private lastAnchorImage: OutputImage | null = null;
  private pendingAnchorSignature: string | null = null;

  constructor() {
    this.load();
  }

  /** Two cards everywhere except NovelAI with "Two per round" switched off. */
  get cardsPerRound(): number {
    return !generation.isNovelAi || this.pairInNai ? 2 : 1;
  }

  get historyCount(): number {
    return this.history.size;
  }

  setCount(n: number): void {
    this.count = clampCount(n);
    this.saveSettings();
  }

  setFavouritesOnly(v: boolean): void {
    this.favouritesOnly = v;
    this.saveSettings();
  }

  setVaryWeights(v: boolean): void {
    this.varyWeights = v;
    this.saveSettings();
  }

  setPairInNai(v: boolean): void {
    this.pairInNai = v;
    this.saveSettings();
  }

  /**
   * Edit a card's pre-filled style name. The round object is reassigned rather
   * than mutated: `$state` proxies it, and a fresh object keeps the panel's
   * input in step without a two-way binding into proxied state.
   */
  setCardName(index: number, name: string): void {
    const round = this.round;
    if (!round) return;
    this.round = {
      seed: round.seed,
      cards: round.cards.map((c, i) => (i === index ? { ...c, name } : c)),
    };
  }

  /**
   * The artists available to draw from: the index deduped by slug, minus
   * low-post-count entries, minus anything already in the anchor or in the
   * user's positive prompt, filtered to favourites when asked.
   */
  private buildPool(): ArtistSearchHit[] {
    const anchorSlugs = new Set(this.anchor.map((a) => artistSlug(a).toLowerCase()));
    const anchorTags = this.anchor.map((a) => a.tag);
    const promptTags = artistInsert.existingArtistTags();
    const seen = new Set<string>();
    const pool: ArtistSearchHit[] = [];
    for (const hit of gallery.artistTagIndex.values()) {
      if (isBelowThresholdCount(hit)) continue;
      const slug = hit.slug.toLowerCase();
      if (seen.has(slug)) continue;
      seen.add(slug);
      if (anchorSlugs.has(slug)) continue;
      if (anchorTags.some((t) => artistTagBodiesMatch(t, hit.tag))) continue;
      if (promptTags.some((t) => artistTagBodiesMatch(t, hit.tag))) continue;
      if (this.favouritesOnly && !artistFavourites.isFavourite(hit.slug)) continue;
      pool.push(hit);
    }
    return pool;
  }

  /**
   * Combinations that must not be offered: everything already judged, plus
   * every saved style. Saved styles stay blocked after Clear history, because
   * the user already has that combination.
   */
  private blockedKeys(): Set<string> {
    const blocked = new Set(this.history);
    for (const style of styles.styles) {
      if (style.artists.length === 0) continue;
      blocked.add(combinationKey(artistKeyEntries(style.artists, style.overallWeight)));
    }
    return blocked;
  }

  private drawCard(pool: ArtistSearchHit[], count: number, blocked: Set<string>): RoundCard | null {
    for (let attempt = 0; attempt < MAX_DRAW_ATTEMPTS; attempt++) {
      const drawn: StyleArtist[] = sampleWithoutReplacement(pool, count).map((hit) => ({
        // Anima checkpoints keep the `@` sigil, NovelAI and the rest do not.
        tag: generation.animaArtistTagPrefix + stripArtistSigil(hit.tag),
        slug: hit.slug,
        weight: this.varyWeights ? randomWeight() : 1,
      }));
      if (drawn.length === 0) return null;
      const artists = [...this.anchor.map((a) => ({ ...a })), ...drawn];
      const key = combinationKey(artistKeyEntries(artists, 1));
      if (blocked.has(key)) continue;
      return { key, artists, name: this.suggestName(drawn), saveable: true, image: null };
    }
    return null;
  }

  /** Anchor (or the style it came from) plus the drawn tag bodies, joined. */
  private suggestName(drawn: StyleArtist[]): string {
    const source = this.anchorStyleId ? styles.getById(this.anchorStyleId) : null;
    const head = source ? [source.name] : this.anchor.map((a) => displayName(a.tag));
    return [...head, ...drawn.map((a) => displayName(a.tag))].filter(Boolean).join(" + ");
  }

  private anchorCard(): RoundCard {
    return {
      key: "",
      artists: this.anchor.map((a) => ({ ...a })),
      name: "",
      saveable: false,
      image: null,
    };
  }

  /** The user's seed when pinned, otherwise one random seed for the round. */
  private roundSeed(): string {
    const numeric = parseInt(generation.seed, 10);
    if (!isNaN(numeric) && numeric !== -1) return String(numeric >>> 0);
    return String((Math.random() * 0x100000000) >>> 0);
  }

  private nextRound(): void {
    if (!this.running) return;
    this.error = null;
    this.note = null;
    this.pendingAnchorSignature = null;
    const pool = this.buildPool();
    if (pool.length === 0) {
      this.endRound(locale.t("style_creator.pool_empty"));
      return;
    }
    let count = this.count;
    if (pool.length < count) {
      count = pool.length;
      this.note = locale.t("style_creator.pool_short", { count: pool.length });
    }
    const blocked = this.blockedKeys();
    const wantTwo = this.cardsPerRound === 2;
    const cards: RoundCard[] = [];
    if (this.anchor.length > 0) {
      // The anchor alone is the reference the extras are judged against.
      if (wantTwo) cards.push(this.anchorCard());
      const drawn = this.drawCard(pool, count, blocked);
      if (!drawn) {
        this.endRound(locale.t("style_creator.exhausted"));
        return;
      }
      cards.push(drawn);
    } else {
      const first = this.drawCard(pool, count, blocked);
      if (!first) {
        this.endRound(locale.t("style_creator.exhausted"));
        return;
      }
      cards.push(first);
      if (wantTwo) {
        // A second card that cannot be drawn is dropped silently: one card is
        // still a usable round, and the exhausted message belongs to the case
        // where nothing at all could be drawn.
        const second = this.drawCard(pool, count, new Set([...blocked, first.key]));
        if (second) cards.push(second);
      }
    }
    const round: StyleCreatorRound = { seed: this.roundSeed(), cards };
    this.roundSerial++;
    const serial = this.roundSerial;
    this.pending.clear();
    this.round = round;
    this.phase = "generating";
    void this.submitRound(round, serial);
  }

  /** Submit each card left to right on the round's seed. */
  private async submitRound(round: StyleCreatorRound, serial: number): Promise<void> {
    for (let i = 0; i < round.cards.length; i++) {
      if (!this.running || this.roundSerial !== serial) return;
      const card = round.cards[i];
      try {
        const fragment = fragmentForStyles(
          [{ artists: card.artists, overallWeight: 1 }],
          generation.isNovelAi,
        );
        const params = generation.toParams({
          extraPositive: fragment,
          skipActiveStyles: true,
          seed: round.seed,
          overrides: { mode: "txt2img", input_image: null, mask_image: null },
        });
        params.batch_size = 1;
        if (!card.saveable) {
          // Identical params mean an identical image: reuse the last anchor
          // render instead of paying for it again.
          const signature = JSON.stringify(params);
          if (signature === this.lastAnchorSignature && this.lastAnchorImage) {
            this.record(i, this.lastAnchorImage);
            continue;
          }
          this.pendingAnchorSignature = signature;
        }
        const promptId = await submitGeneration(params);
        if (!this.running || this.roundSerial !== serial) return;
        this.pending.set(promptId, i);
      } catch (err) {
        this.submitFailed(err);
        return;
      }
    }
  }

  private submitFailed(err: unknown): void {
    const message = err instanceof Error ? err.message : String(err);
    const classified = classifyGenerationError(message);
    const detail =
      classified.messageKey === "generation.toast.failed"
        ? message
        : locale.t(classified.messageKey, classified.params);
    const text = locale.t("style_creator.failed", { error: detail });
    gallery.showToast(text, "error");
    this.endRound(text);
  }

  /** Consume a promptId mapping. Returns the card index, or null if not ours. */
  resolve(promptId: string): number | null {
    const index = this.pending.get(promptId);
    if (index === undefined) return null;
    this.pending.delete(promptId);
    return index;
  }

  record(index: number, image: OutputImage): void {
    const round = this.round;
    if (!round || this.phase !== "generating") return;
    const card = round.cards[index];
    if (!card) return;
    if (!card.saveable && this.pendingAnchorSignature) {
      this.lastAnchorSignature = this.pendingAnchorSignature;
      this.lastAnchorImage = image;
      this.pendingAnchorSignature = null;
    }
    const next: StyleCreatorRound = {
      seed: round.seed,
      cards: round.cards.map((c, i) => (i === index ? { ...c, image } : c)),
    };
    this.round = next;
    if (next.cards.every((c) => c.image !== null)) this.phase = "choosing";
  }

  /** Drop the round and stop the loop, showing `message` in the panel. */
  private endRound(message: string | null): void {
    this.pending.clear();
    this.pendingAnchorSignature = null;
    this.round = null;
    this.phase = "idle";
    this.running = false;
    this.error = message;
    this.note = null;
  }

  /** A backend execution error carrying a prompt id from this round. */
  fail(promptId: string, message: string): void {
    if (!this.pending.has(promptId)) return;
    this.endRound(locale.t("style_creator.failed", { error: message }));
  }

  /** The queue was cleared or an error arrived with no prompt id. */
  failAll(): void {
    if (this.phase !== "generating") return;
    this.endRound(
      locale.t("style_creator.failed", { error: locale.t("style_creator.interrupted") }),
    );
  }

  /** Record every drawn card of the round as judged and clear it. */
  private finishRound(): void {
    const round = this.round;
    if (round) {
      const keys = round.cards.map((c) => c.key).filter((k) => k.length > 0);
      if (keys.length > 0) this.history = new Set([...this.history, ...keys]);
    }
    this.pending.clear();
    this.pendingAnchorSignature = null;
    this.round = null;
    this.phase = "idle";
    this.saveSettings();
  }

  /**
   * Save the picked card as a style and move on. The next round starts before
   * the thumbnail resize is awaited, so the loop is never blocked by it.
   * Picking the anchor-alone card saves nothing.
   */
  async pick(index: number, edit = false): Promise<void> {
    if (this.phase !== "choosing") return;
    const round = this.round;
    if (!round) return;
    const card = round.cards[index];
    if (!card || !card.image) return;
    const image = card.image;
    const name = card.name;
    const artists = card.artists.map((a) => ({ ...a }));
    const saveable = card.saveable;
    this.finishRound();
    if (!saveable) {
      if (this.running) this.nextRound();
      return;
    }
    const style = styles.create(name, artists);
    if (edit) styleEditors.openStyle(style.id);
    if (this.running) this.nextRound();
    try {
      const source = image.sessionBlob ?? image.fullImageUrl ?? image.url;
      if (source) {
        const dataUrl = await resizeImageToDataUrl(source);
        styles.setThumbnail(style.id, dataUrl);
      }
    } catch (e) {
      console.error("styleCreator: thumbnail failed", e);
    }
    gallery.showToast(locale.t("style_creator.saved", { name: style.name }), "success");
  }

  /** Judge the round without saving anything. */
  skip(): void {
    if (this.phase !== "choosing") return;
    this.finishRound();
    if (this.running) this.nextRound();
  }

  start(): void {
    if (this.running) return;
    if (!gallery.artistIndexReady) {
      this.error = locale.t("style_creator.index_loading");
      return;
    }
    this.error = null;
    this.running = true;
    // A round left in "choosing" keeps its cards: Start resumes the loop only
    // after that round is judged.
    if (this.phase === "idle") this.nextRound();
  }

  /**
   * Stop the loop. A round waiting to be judged stays on screen; a round still
   * generating is abandoned, its images still land in the gallery, and nothing
   * is added to the history.
   */
  stop(): void {
    this.running = false;
    this.error = null;
    this.note = null;
    if (this.phase === "generating") {
      this.pending.clear();
      this.pendingAnchorSignature = null;
      this.round = null;
      this.phase = "idle";
    }
  }

  /**
   * Add a hand-typed anchor artist. Returns false when the text was not found
   * in the Artists index, in which case it is still kept as typed.
   */
  addAnchorTag(raw: string): boolean {
    const text = stripArtistSigil(raw.trim());
    if (!text) return true;
    const hit = gallery.artistIndexReady
      ? gallery.artistTagIndex.get(artistIndexKey(text))
      : undefined;
    const tag = generation.animaArtistTagPrefix + (hit ? stripArtistSigil(hit.tag) : text);
    if (this.anchor.some((a) => artistTagBodiesMatch(a.tag, tag))) return hit !== undefined;
    this.anchor = [...this.anchor, { tag, slug: hit?.slug, weight: 1 }];
    // Editing the row by hand breaks the link to the style it came from.
    this.anchorStyleId = null;
    this.saveSettings();
    return hit !== undefined;
  }

  removeAnchor(index: number): void {
    this.anchor = this.anchor.filter((_, i) => i !== index);
    this.anchorStyleId = null;
    this.saveSettings();
  }

  /** Replace the anchor row with a saved style's artists at their baked weights. */
  loadAnchorFromStyle(styleId: string): void {
    const style = styles.getById(styleId);
    if (!style) return;
    this.anchor = style.artists.map((a) => ({
      tag: a.tag,
      slug: a.slug,
      weight: bakedArtistWeight(a, style.overallWeight),
    }));
    this.anchorStyleId = style.id;
    this.saveSettings();
  }

  clearAnchor(): void {
    this.anchor = [];
    this.anchorStyleId = null;
    this.saveSettings();
  }

  clearHistory(): void {
    this.history = new Set();
    this.saveSettings();
  }

  private load(): void {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const data = JSON.parse(raw);
      if (data.count !== undefined) this.count = clampCount(data.count);
      if (data.favouritesOnly !== undefined) this.favouritesOnly = !!data.favouritesOnly;
      if (data.varyWeights !== undefined) this.varyWeights = !!data.varyWeights;
      if (data.pairInNai !== undefined) this.pairInNai = !!data.pairInNai;
      if (data.anchor?.artists !== undefined && Array.isArray(data.anchor.artists)) {
        this.anchor = data.anchor.artists
          .filter((a: any) => a && typeof a.tag === "string" && a.tag.trim())
          .map((a: any) => ({
            tag: a.tag,
            slug: typeof a.slug === "string" ? a.slug : undefined,
            weight: typeof a.weight === "number" && Number.isFinite(a.weight) ? a.weight : 1,
          }));
      }
      if (data.anchor?.styleId !== undefined) {
        this.anchorStyleId = typeof data.anchor.styleId === "string" ? data.anchor.styleId : null;
      }
      if (data.history !== undefined && Array.isArray(data.history)) {
        this.history = new Set(data.history.filter((k: unknown) => typeof k === "string"));
      }
    } catch (e) {
      console.error("styleCreator: load failed", e);
    }
  }

  saveSettings(): void {
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
          count: this.count,
          favouritesOnly: this.favouritesOnly,
          varyWeights: this.varyWeights,
          pairInNai: this.pairInNai,
          anchor: {
            artists: this.anchor.map((a) => ({ tag: a.tag, slug: a.slug, weight: a.weight })),
            styleId: this.anchorStyleId,
          },
          history: [...this.history],
        }),
      );
    } catch (e) {
      console.error("styleCreator: save failed", e);
    }
  }
}

export const styleCreator = new StyleCreatorStore();
