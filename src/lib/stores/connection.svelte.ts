import { isBrowserMode } from "../utils/ipc.js";

const CDN_BASE = isBrowserMode
  ? "/internal-api/_cdn"
  : "https://cdn.mooshieblob.com";

class ConnectionStore {
  connected = $state(false);
  serverUrl = $state("http://127.0.0.1:8188");
  /** Manifest URL for the Anima artist gallery. Proxied in browser mode to avoid CORS. */
  artistGalleryManifestUrl = $state(
    `${CDN_BASE}/20260325_anima_all_artists/indices/manifest.json`,
  );
}

export const connection = new ConnectionStore();
