/**
 * WASM AVIF decoder fallback for the artist gallery.
 *
 * The gallery dataset (index v2) is AVIF-only — the CDN has no WebP/JPEG
 * variants of those objects.  WebView2 (Windows) and WKWebView (macOS) decode
 * AVIF natively, but WebKitGTK on Linux is routinely built without AVIF
 * support, so every artist image renders as a broken-image glyph there.
 *
 * This module decodes AVIF in WebAssembly and re-encodes to a format the
 * webview can definitely draw.  It is imported dynamically and only on
 * webviews that fail the native-AVIF probe, so Windows/macOS never load the
 * ~1.2 MB decoder.
 *
 * Decodes run on the main thread rather than in a Worker: `module.decode()` is
 * synchronous anyway, and a Worker adds a failure mode (module-worker support,
 * `worker-src` CSP) on the exact platform this code exists to rescue.  Callers
 * get one decode at a time with a yield in between so the browser can paint.
 */

import decodeAvif, { init as initAvifDecoder } from "@jsquash/avif/decode";
import avifDecWasmUrl from "@jsquash/avif/codec/dec/avif_dec.wasm?url";

let _ready: Promise<void> | null = null;

/** Compile and initialise the decoder once per session. */
function ensureDecoder(): Promise<void> {
  if (!_ready) {
    _ready = (async () => {
      const response = await fetch(avifDecWasmUrl);
      if (!response.ok) {
        throw new Error(`AVIF decoder wasm HTTP ${response.status}`);
      }
      const wasmModule = await WebAssembly.compile(await response.arrayBuffer());
      await initAvifDecoder({
        // Instantiating the module ourselves avoids Emscripten's own
        // `fetch(locateFile(...))`, which guesses the wasm path from
        // `import.meta.url` and needs an `application/wasm` content type.
        // Emscripten wants the *instance* handed back, not the module —
        // jSquash's own `initEmscriptenModule` helper does the same thing.
        // The params are annotated because jSquash's `ModuleOpts` type refers
        // to an ambient namespace its package never actually ships.
        instantiateWasm: (
          imports: WebAssembly.Imports,
          successCallback: (instance: WebAssembly.Instance) => void,
        ) => {
          const instance = new WebAssembly.Instance(wasmModule, imports);
          successCallback(instance);
          return instance.exports;
        },
      });
    })().catch((e) => {
      _ready = null; // let a later image retry
      throw e;
    });
  }
  return _ready;
}

/** Serialises decodes and yields between them so the UI stays responsive. */
let _queue: Promise<unknown> = Promise.resolve();

/**
 * Decode AVIF bytes and re-encode them as a blob the webview can render.
 * Prefers WebP; webviews without a WebP encoder fall back to PNG per the
 * canvas spec — bigger, but correct, and the result is cached so the work
 * happens once per image.
 */
export function decodeAvifToBlob(bytes: ArrayBuffer): Promise<Blob> {
  const run = _queue.then(
    () => _transcode(bytes),
    () => _transcode(bytes),
  );
  _queue = run.then(
    () => new Promise((r) => setTimeout(r, 0)),
    () => new Promise((r) => setTimeout(r, 0)),
  );
  return run;
}

async function _transcode(bytes: ArrayBuffer): Promise<Blob> {
  await ensureDecoder();

  const image = await decodeAvif(bytes);
  if (!image) throw new Error("AVIF decode returned null");

  const canvas = document.createElement("canvas");
  canvas.width = image.width;
  canvas.height = image.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("2D canvas context unavailable");
  ctx.putImageData(image, 0, 0);

  return new Promise<Blob>((resolve, reject) => {
    canvas.toBlob(
      (blob) => {
        if (blob) resolve(blob);
        else reject(new Error("canvas.toBlob produced no blob"));
      },
      "image/webp",
      0.9,
    );
  });
}
