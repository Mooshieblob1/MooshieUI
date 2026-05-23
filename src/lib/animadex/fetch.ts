/**
 * Animadex API fetch adapter (characters mode only).
 *
 * Desktop Tauri and browser/server mode route JSON through a locked-down proxy
 * because animadex.net does not allow cross-origin fetches from the app origin.
 * Image URLs on blobs.animadex.net load via plain <img src> (no proxy needed).
 */
import { ipcInvoke, isBrowserMode, isTauri } from "../utils/ipc.js";

export const ANIMADEX_ORIGIN = "https://animadex.net/";

export function proxiedAnimadexApiUrl(apiPath: string): string {
  const clean = apiPath.replace(/^\//, "");
  if (isBrowserMode) {
    return `/internal-api/_animadex/${clean}`;
  }
  return `${ANIMADEX_ORIGIN}${clean}`;
}

export const animadexFetch: typeof fetch | undefined =
  isTauri || isBrowserMode
    ? (async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
        const url =
          typeof input === "string"
            ? input
            : input instanceof URL
              ? input.href
              : input.url;

        if (url.startsWith(ANIMADEX_ORIGIN)) {
          const path = url.slice(ANIMADEX_ORIGIN.length);
          if (!path.startsWith("api/characters/")) {
            return new Response("animadex fetch: path not allowed", { status: 400 });
          }
          if (isBrowserMode) {
            return globalThis.fetch(proxiedAnimadexApiUrl(path), init);
          }
          try {
            const body = await ipcInvoke<string>("animadex_proxy_fetch", { path });
            return new Response(body, {
              status: 200,
              headers: { "content-type": "application/json" },
            });
          } catch (e) {
            return new Response(String(e), { status: 502 });
          }
        }

        return globalThis.fetch(input as RequestInfo, init);
      }) as typeof fetch
    : undefined;
