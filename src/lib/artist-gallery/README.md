# Artist Gallery

The Svelte 5 artist explorer renders searchable artist tags, preview variants,
favourites and entries without CDN previews. Images and index JSON follow the
shape produced by `scripts/r2_build_indices.py`.

The UI is integrated with MooshieUI's locale, generation and gallery stores,
CDN proxy helpers and Animadex character explorer. It is not a drop-in,
zero-dependency component for an unrelated Svelte app. The read-only
`client.ts` data client can be reused separately with a manifest URL and
optional `fetchImpl` override.

## Integration

```svelte
<script lang="ts">
  import { ArtistGalleryPage } from "$lib/artist-gallery";

  const manifestUrl =
    "https://cdn.mooshieblob.com/20260425_anima_all_artists/indices/manifest.json";

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
import { createArtistGalleryClient } from "$lib/artist-gallery/client.js";

const client = createArtistGalleryClient({ manifestUrl });
const hits = await client.search("dairi", { limit: 10 });
const entry = hits[0] ? await client.getArtist(hits[0].slug) : null;
console.log(entry?.imageUrl);
```

## Contract with the publisher

The manifest URL must resolve to a JSON document shaped like `ArtistManifest`
(see [types.ts](./types.ts)). Sibling files:

- `shards/<bucket>.json` — one file per first-char slug bucket.
- `search.json` — flat typeahead index sorted by `postCount` desc.
- An optional `noPreviewIndex` referenced by the manifest, for artists without
  CDN images. Older manifests without it return an empty no-preview list.

Version 2 entries may carry an `images` array with per-variant `hasImage`
flags; readers fall back to the single `imageUrl` for version 1. Use the
manifest's recorded paths rather than assuming every release includes all files.

All image URLs in each shard are absolute, computed at build time from
`imageBaseUrl + objectKey`. No per-image metadata fetch is required.

## App integration

- `ArtistGalleryPage` accepts artist/character insertion callbacks and optional
  `ongeneratePreview` / `previewStatus` hooks for generating missing previews.
- The shared preview recipe is exported from `previewRecipe.ts`; use it when
  matching the CDN previews locally.
- Style Creator consumes the artist index for candidate combinations and saves
  picks as Artist Styles. Its generation loop belongs to the app, not the data
  client. See the [Style Creator guide](https://github.com/Mooshieblob1/MooshieUI/wiki/Prompting-Guide#style-creator).
- The data client fetches published metadata and images; it does not publish
  to the CDN or own the generation backend.
