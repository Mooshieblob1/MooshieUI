# Artist Gallery — portable Svelte 5 module

Zero-dependency module (aside from Svelte 5 runes) that renders a searchable
gallery of Anima-preview artist tags. Images + index JSON are served from any
HTTPS origin that matches the shape produced by `scripts/r2_build_indices.py`.

Designed to drop, unchanged, into:

- The MooshieUI desktop shell (Tauri webview — already uses `fetch()` for HTTPS).
- A standalone public Svelte site (no Tauri at all).

## Integration

```svelte
<script lang="ts">
  import { ArtistGalleryPage } from "$lib/artist-gallery";

  const manifestUrl =
    "https://cdn.mooshieblob.com/20260325_anima_all_artists/indices/manifest.json";

  function insertTagIntoPrompt(tag: string) {
    // Integrator wires this up; omit the prop if you don't want the button.
  }
</script>

<ArtistGalleryPage {manifestUrl} oninsertTag={insertTagIntoPrompt} />
```

For inline previews next to an autocomplete suggestion:

```svelte
<script lang="ts">
  import { ArtistHoverPreview } from "$lib/artist-gallery";
</script>

<ArtistHoverPreview {manifestUrl} slugOrTag={currentTag} />
```

## Headless usage

```ts
import { createArtistGalleryClient } from "$lib/artist-gallery";

const client = createArtistGalleryClient({ manifestUrl });
const hits = await client.search("dairi", { limit: 10 });
const entry = await client.getArtist(hits[0].slug);
console.log(entry?.imageUrl);
```

## Contract with the publisher

The manifest URL must resolve to a JSON document shaped like `ArtistManifest`
(see [types.ts](./types.ts)). Sibling files:

- `shards/<bucket>.json` — one file per first-char slug bucket.
- `search.json` — flat typeahead index sorted by `postCount` desc.

All image URLs in each shard are absolute, computed at build time from
`imageBaseUrl + objectKey`. No per-image metadata fetch is required.

## Non-goals

- No write / generation APIs. The module is read-only.
- No Tauri, Electron, or Node deps.
- No CSP-unsafe patterns (pure `fetch` + `<img src>`).
