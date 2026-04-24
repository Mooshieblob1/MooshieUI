<script lang="ts">
  import { styles, resizeImageToDataUrl, type ArtistStyle, type StyleArtist } from "../../stores/styles.svelte.js";
  import { artistFavourites } from "../../artist-gallery/favourites.svelte.js";
  import { gallery } from "../../stores/gallery.svelte.js";
  import { autocomplete } from "../../stores/autocomplete.svelte.js";
  import { generation } from "../../stores/generation.svelte.js";

  interface Props {
    styleId: string;
    onclose: () => void;
  }

  let { styleId, onclose }: Props = $props();

  const style = $derived(styles.getById(styleId));

  let newTagInput = $state("");
  let searchQuery = $state("");
  let thumbnailError = $state<string | null>(null);
  let thumbnailBusy = $state(false);
  let fileInput: HTMLInputElement | null = $state(null);

  function displayTag(tag: string): string {
    return tag.replace(/^@/, "").replace(/\\([()\[\]])/g, "$1");
  }

  /** Favourites that are not already part of this style. */
  const favouriteCandidates = $derived.by(() => {
    if (!style) return [];
    const used = new Set(style.artists.map((a) => a.tag.trim().toLowerCase()));
    const q = searchQuery.trim().toLowerCase();
    const slugs = Object.keys(artistFavourites.favourites);
    const filtered = q ? slugs.filter((s) => s.toLowerCase().includes(q)) : slugs;
    return filtered
      .filter((slug) => !used.has(slug.toLowerCase()) && !used.has(`@${slug}`.toLowerCase()))
      .sort()
      .slice(0, 40);
  });

  /** Gallery artist index matches (up to 20) — excluding favourites already shown and artists already in the style. */
  const gallerySuggestions = $derived.by(() => {
    if (!style) return [] as { slug: string; tag: string }[];
    const q = searchQuery.trim().toLowerCase();
    if (!q || q.length < 2) return [];
    const used = new Set(style.artists.map((a) => a.tag.trim().toLowerCase()));
    const favSet = new Set(Object.keys(artistFavourites.favourites).map((s) => s.toLowerCase()));
    const out: { slug: string; tag: string }[] = [];
    for (const [key, hit] of gallery.artistTagIndex) {
      if (out.length >= 20) break;
      if (!key.includes(q)) continue;
      const slugLower = hit.slug.toLowerCase();
      if (used.has(slugLower) || used.has(`@${hit.slug}`.toLowerCase())) continue;
      if (favSet.has(slugLower)) continue;
      if (!out.some((o) => o.slug === hit.slug)) out.push({ slug: hit.slug, tag: hit.tag });
    }
    return out;
  });

  /**
   * Autocomplete artist-category tags (c === 1) from the active autocomplete
   * taglist. When an Anima model is active, the autocomplete store is already
   * swapped to the anima list, so this naturally surfaces Anima artists.
   */
  const autocompleteSuggestions = $derived.by(() => {
    if (!style) return [] as { name: string; postCount: number }[];
    const q = searchQuery.trim();
    if (q.length < 2) return [];
    const used = new Set(style.artists.map((a) => a.tag.trim().toLowerCase().replace(/^@/, "")));
    const favSet = new Set(Object.keys(artistFavourites.favourites).map((s) => s.toLowerCase()));
    const gallerySet = new Set(gallerySuggestions.map((g) => g.slug.toLowerCase()));
    const results = autocomplete.search(q, 30);
    const out: { name: string; postCount: number }[] = [];
    for (const tag of results) {
      if (tag.c !== 1) continue; // artist category only
      const key = tag.n.toLowerCase();
      if (used.has(key) || favSet.has(key) || gallerySet.has(key)) continue;
      out.push({ name: tag.n, postCount: tag.p });
      if (out.length >= 15) break;
    }
    return out;
  });

  /**
   * Normalize an artist tag for insertion. Anima-style models use an `@` prefix
   * on artist tags; other architectures (SDXL, Pony, Illustrious, etc.) take
   * raw danbooru-style names. Strips a leading `@` when not on Anima.
   */
  function normalizeArtistTag(tag: string): string {
    if (generation.isAnima) return tag.startsWith("@") ? tag : `@${tag}`;
    return tag.replace(/^@/, "");
  }

  function addArtist(tag: string, slug?: string) {
    if (!style) return;
    const normalized = normalizeArtistTag(tag.trim());
    if (!normalized) return;
    styles.addArtist(style.id, { tag: normalized, slug, weight: 1.0 });
  }

  function addFromFreeText() {
    if (!style) return;
    const raw = newTagInput.trim();
    if (!raw) return;
    // Allow comma-separated bulk-add.
    for (const part of raw.split(",")) {
      const t = part.trim();
      if (t) addArtist(t);
    }
    newTagInput = "";
  }

  function updateArtistWeight(index: number, value: number) {
    if (!style) return;
    styles.updateArtist(style.id, index, { weight: value });
  }

  function removeArtist(index: number) {
    if (!style) return;
    styles.removeArtist(style.id, index);
  }

  function setName(value: string) {
    if (!style) return;
    styles.update(style.id, { name: value });
  }

  function setOverallWeight(value: number) {
    if (!style) return;
    styles.update(style.id, { overallWeight: value });
  }

  // ---------------------------------------------------------------------------
  // Thumbnail
  // ---------------------------------------------------------------------------

  async function setThumbnailFromLastGen() {
    if (!style) return;
    thumbnailError = null;
    thumbnailBusy = true;
    try {
      const latest = gallery.sessionImages[0] ?? gallery.images[0];
      if (!latest) {
        thumbnailError = "No recent generation to use";
        return;
      }
      const url = latest.fullImageUrl ?? latest.thumbnailUrl ?? latest.url;
      if (!url) {
        thumbnailError = "Latest image has no accessible URL";
        return;
      }
      const resp = await fetch(url);
      if (!resp.ok) throw new Error(`Fetch failed (${resp.status})`);
      const blob = await resp.blob();
      const dataUrl = await resizeImageToDataUrl(blob);
      styles.setThumbnail(style.id, dataUrl);
    } catch (e) {
      thumbnailError = e instanceof Error ? e.message : String(e);
    } finally {
      thumbnailBusy = false;
    }
  }

  async function onFilePicked(e: Event) {
    if (!style) return;
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    thumbnailError = null;
    thumbnailBusy = true;
    try {
      const dataUrl = await resizeImageToDataUrl(file);
      styles.setThumbnail(style.id, dataUrl);
    } catch (e) {
      thumbnailError = e instanceof Error ? e.message : String(e);
    } finally {
      thumbnailBusy = false;
    }
  }

  function clearThumbnail() {
    if (!style) return;
    styles.setThumbnail(style.id, null);
  }
</script>

<div
  class="fixed inset-0 z-200 flex items-center justify-center bg-black/80 backdrop-blur-sm"
  role="dialog"
  aria-modal="true"
  aria-label="Edit Style"
>
  <button
    type="button"
    class="absolute inset-0 h-full w-full cursor-default"
    aria-label="Close"
    onclick={onclose}
  ></button>

  {#if style}
    <div class="relative z-10 w-full max-w-3xl max-h-[92vh] overflow-y-auto rounded-xl border border-neutral-700 bg-neutral-900 p-5 shadow-2xl">
      <div class="mb-4 flex items-center justify-between gap-3">
        <h2 class="text-sm font-semibold text-neutral-100">Edit Style</h2>
        <button
          type="button"
          class="text-neutral-500 hover:text-neutral-200 text-lg leading-none"
          onclick={onclose}
          aria-label="Close"
        >✕</button>
      </div>

      <!-- Name + overall weight + thumbnail -->
      <section class="mb-5 grid grid-cols-1 gap-4 md:grid-cols-[auto_1fr]">
        <!-- Thumbnail -->
        <div class="flex flex-col gap-2">
          <div class="relative h-32 w-32 overflow-hidden rounded-lg border border-neutral-700 bg-neutral-950 flex items-center justify-center">
            {#if style.thumbnail}
              <img src={style.thumbnail} alt="Style thumbnail" class="h-full w-full object-cover" />
            {:else}
              <span class="text-[10px] text-neutral-500">No thumbnail</span>
            {/if}
            {#if thumbnailBusy}
              <div class="absolute inset-0 flex items-center justify-center bg-black/60 text-[10px] text-neutral-300">Loading…</div>
            {/if}
          </div>
          <div class="flex flex-col gap-1">
            <button
              type="button"
              class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-200 hover:border-indigo-500 disabled:opacity-50"
              onclick={setThumbnailFromLastGen}
              disabled={thumbnailBusy}
            >Use last gen</button>
            <button
              type="button"
              class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-200 hover:border-indigo-500 disabled:opacity-50"
              onclick={() => fileInput?.click()}
              disabled={thumbnailBusy}
            >Upload…</button>
            {#if style.thumbnail}
              <button
                type="button"
                class="rounded border border-neutral-700 bg-neutral-800 px-2 py-1 text-[11px] text-neutral-400 hover:text-red-300"
                onclick={clearThumbnail}
                disabled={thumbnailBusy}
              >Clear</button>
            {/if}
            <input
              bind:this={fileInput}
              type="file"
              accept="image/png,image/jpeg,image/webp"
              class="hidden"
              onchange={onFilePicked}
            />
          </div>
          {#if thumbnailError}
            <p class="text-[10px] text-red-400">{thumbnailError}</p>
          {/if}
        </div>

        <div class="space-y-3">
          <div>
            <label for="sty-name" class="mb-1 block text-[10px] uppercase tracking-wide text-neutral-500">Name</label>
            <input
              id="sty-name"
              type="text"
              value={style.name}
              oninput={(e) => setName((e.currentTarget as HTMLInputElement).value)}
              placeholder="My signature style"
              class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
            />
          </div>

          <div>
            <div class="mb-1 flex items-center justify-between text-[10px] uppercase tracking-wide text-neutral-500">
              <label for="sty-overall">Overall weight</label>
              <span class="font-mono text-neutral-300">{style.overallWeight.toFixed(2)}</span>
            </div>
            <input
              id="sty-overall"
              type="range"
              min="0"
              max="2"
              step="0.05"
              value={style.overallWeight}
              oninput={(e) => setOverallWeight(parseFloat((e.currentTarget as HTMLInputElement).value))}
              class="w-full"
            />
            <p class="mt-0.5 text-[10px] text-neutral-500">Multiplies every artist's individual weight when the style is active.</p>
          </div>
        </div>
      </section>

      <!-- Artists in this style -->
      <section class="mb-5">
        <h3 class="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400">
          Artists ({style.artists.length})
        </h3>
        {#if style.artists.length === 0}
          <p class="rounded border border-dashed border-neutral-800 bg-neutral-950/50 p-3 text-center text-[11px] text-neutral-500">
            No artists yet. Add some from your favourites, the gallery, or paste tags below.
          </p>
        {:else}
          <div class="space-y-1.5">
            {#each style.artists as artist, i (artist.tag + i)}
              {@const effective = (artist.weight * style.overallWeight).toFixed(2)}
              <div class="flex items-center gap-2 rounded border border-neutral-800 bg-neutral-950/60 px-2 py-1.5">
                <span class="flex-1 truncate font-mono text-[11px] text-red-300">@{displayTag(artist.tag)}</span>
                <input
                  type="range"
                  min="0"
                  max="2"
                  step="0.05"
                  value={artist.weight}
                  oninput={(e) => updateArtistWeight(i, parseFloat((e.currentTarget as HTMLInputElement).value))}
                  class="w-28"
                  aria-label={`Weight for ${displayTag(artist.tag)}`}
                />
                <span class="w-10 shrink-0 text-right font-mono text-[10px] text-neutral-400">{artist.weight.toFixed(2)}</span>
                <span class="w-14 shrink-0 text-right font-mono text-[10px] text-indigo-300" title="Effective weight (per-artist × overall)">= {effective}</span>
                <button
                  type="button"
                  class="rounded px-1.5 py-0.5 text-[11px] text-neutral-500 hover:bg-red-500/10 hover:text-red-300"
                  onclick={() => removeArtist(i)}
                  title="Remove"
                  aria-label={`Remove ${displayTag(artist.tag)}`}
                >✕</button>
              </div>
            {/each}
          </div>
        {/if}
      </section>

      <!-- Add artists -->
      <section class="mb-3 space-y-3">
        <h3 class="text-xs font-medium uppercase tracking-wide text-neutral-400">Add artists</h3>

        <!-- Search favourites + gallery -->
        <div>
          <input
            type="text"
            bind:value={searchQuery}
            placeholder="Search favourites or artist gallery…"
            class="w-full rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
          />

          {#if favouriteCandidates.length > 0}
            <div class="mt-2">
              <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">From favourites</p>
              <div class="flex flex-wrap gap-1">
                {#each favouriteCandidates as slug (slug)}
                  {@const cat = artistFavourites.categoryOf(slug)}
                  <button
                    type="button"
                    class="inline-flex items-center gap-1 rounded-full border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-[10px] text-neutral-300 hover:border-indigo-500 hover:text-indigo-200"
                    onclick={() => addArtist(slug, slug)}
                    title={`Add ${generation.isAnima ? "@" : ""}${slug}`}
                  >
                    {#if cat}
                      <span class="h-2 w-2 rounded-full" style="background-color: {cat.color}" aria-hidden="true"></span>
                    {/if}
                    <span class="font-mono">+ {generation.isAnima ? "@" : ""}{slug}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          {#if gallerySuggestions.length > 0}
            <div class="mt-2">
              <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">From gallery</p>
              <div class="flex flex-wrap gap-1">
                {#each gallerySuggestions as hit (hit.slug)}
                  <button
                    type="button"
                    class="inline-flex items-center gap-1 rounded-full border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-[10px] text-neutral-300 hover:border-indigo-500 hover:text-indigo-200"
                    onclick={() => addArtist(hit.tag, hit.slug)}
                    title={`Add ${hit.tag}`}
                  >
                    <span class="font-mono">+ {generation.isAnima ? hit.tag : displayTag(hit.tag)}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          {#if autocompleteSuggestions.length > 0}
            <div class="mt-2">
              <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">
                From autocomplete{generation.isAnima ? " (Anima)" : ""}
              </p>
              <div class="flex flex-wrap gap-1">
                {#each autocompleteSuggestions as hit (hit.name)}
                  <button
                    type="button"
                    class="inline-flex items-center gap-1 rounded-full border border-neutral-700 bg-neutral-800 px-2 py-0.5 text-[10px] text-red-300 hover:border-indigo-500 hover:text-indigo-200"
                    onclick={() => addArtist(hit.name)}
                    title={`Add ${generation.isAnima ? "@" : ""}${hit.name} — ${hit.postCount} posts`}
                  >
                    <span class="font-mono">+ {generation.isAnima ? "@" : ""}{hit.name.replace(/_/g, " ")}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          {#if searchQuery.trim() && favouriteCandidates.length === 0 && gallerySuggestions.length === 0 && autocompleteSuggestions.length === 0}
            <p class="mt-2 text-[10px] text-neutral-500">No matches. Use the free-text field below to add any tag.</p>
          {/if}
        </div>

        <!-- Free-text add -->
        <div>
          <p class="mb-1 text-[10px] uppercase tracking-wide text-neutral-500">Paste tags</p>
          <div class="flex gap-2">
            <input
              type="text"
              bind:value={newTagInput}
              placeholder="@artist_1, @artist_2, …"
              onkeydown={(e) => { if (e.key === "Enter") { e.preventDefault(); addFromFreeText(); } }}
              class="flex-1 rounded border border-neutral-700 bg-neutral-800 px-2 py-1.5 text-sm text-neutral-100 placeholder-neutral-500 focus:border-indigo-500 focus:outline-none"
            />
            <button
              type="button"
              class="rounded border border-neutral-700 bg-neutral-800 px-3 py-1.5 text-xs text-neutral-200 hover:border-indigo-500"
              onclick={addFromFreeText}
            >Add</button>
          </div>
        </div>
      </section>

      <div class="flex justify-end">
        <button
          type="button"
          class="rounded bg-indigo-600 px-4 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
          onclick={onclose}
        >Done</button>
      </div>
    </div>
  {:else}
    <div class="relative z-10 rounded-xl border border-neutral-700 bg-neutral-900 p-5 text-sm text-neutral-300">
      Style not found.
      <button class="ml-3 text-indigo-400 hover:text-indigo-300" onclick={onclose}>Close</button>
    </div>
  {/if}
</div>
