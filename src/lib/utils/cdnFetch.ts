/**
 * CDN fetch adapter.
 *
 * In browser (server) mode the frontend hits `/internal-api/_cdn/...`, which
 * the embedded axum server proxies to `https://cdn.mooshieblob.com/...` with
 * CORS headers — so ordinary `fetch` works.
 *
 * In Tauri desktop mode the webview origin is something like
 * `tauri://localhost`, and a direct `fetch("https://cdn.mooshieblob.com/...")`
 * is blocked by CORS (the CDN doesn't whitelist the tauri origin). We route
 * those requests through the `cdn_proxy_fetch` Tauri command, which uses the
 * shared reqwest client on the Rust side and isn't subject to CORS.
 *
 * Image loads via `<img src>` bypass CORS and work in both modes without any
 * special handling — only JSON `fetch` calls need this adapter.
 */
import { ipcInvoke, isTauri } from "./ipc.js";

const CDN_PREFIX = "https://cdn.mooshieblob.com/";

/**
 * A drop-in `fetch` for the artist gallery client. In app mode, CDN URLs are
 * routed through the `cdn_proxy_fetch` Tauri command; everything else falls
 * back to the platform `fetch`. In browser/server mode we don't need an
 * adapter — callers can pass `undefined` and default `fetch` does the right
 * thing.
 */
export const cdnFetch: typeof fetch | undefined = isTauri
  ? (async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const url =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.href
            : input.url;

      if (url.startsWith(CDN_PREFIX)) {
        try {
          const path = url.slice(CDN_PREFIX.length);
          const body = await ipcInvoke<string>("cdn_proxy_fetch", { path });
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
