// Always the absolute CDN origin. In browser/LAN mode `cdnFetch`/`proxiedCdnUrl`
// rewrite this to the `/internal-api/_cdn/...` proxy AND append the auth token;
// pre-rewriting to the proxy path here would bypass that token injection and get
// rejected with a 401 by the LAN auth gate (proxy_request_authed).
const CDN_BASE = "https://cdn.mooshieblob.com";

class ConnectionStore {
  connected = $state(false);
  serverUrl = $state("http://127.0.0.1:8188");
  /** Manifest URL for the Anima artist gallery. Proxied in browser mode to avoid CORS. */
  artistGalleryManifestUrl = $state(
    `${CDN_BASE}/20260425_anima_all_artists/indices/manifest.json`,
  );
}

export const connection = new ConnectionStore();
