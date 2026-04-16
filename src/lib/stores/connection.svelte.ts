class ConnectionStore {
  connected = $state(false);
  serverUrl = $state("http://127.0.0.1:8188");
  /** Manifest URL for the Anima artist gallery. Points at Cloudflare R2 by default. */
  artistGalleryManifestUrl = $state(
    "https://cdn.mooshieblob.com/20260325_anima_all_artists/indices/manifest.json",
  );
}

export const connection = new ConnectionStore();
