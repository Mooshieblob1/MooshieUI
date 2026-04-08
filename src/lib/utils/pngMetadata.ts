/**
 * Client-side PNG metadata reader.
 *
 * Reads metadata from PNG files without requiring server round-trips.
 * Supports:
 *  - PNG tEXt / iTXt chunks (SwarmUI JSON + A1111 fallback)
 *  - Stealth alpha-channel LSB encoding (SwarmUI-compatible)
 */

// ---------------------------------------------------------------------------
// PNG chunk reader
// ---------------------------------------------------------------------------

/** Read a 4-byte big-endian uint from a DataView. */
function readU32(view: DataView, offset: number): number {
  return view.getUint32(offset, false);
}

/** Read a Latin-1 null-terminated string starting at offset. Returns [string, bytesConsumed]. */
function readNullTerminated(data: Uint8Array, offset: number): [string, number] {
  let end = offset;
  while (end < data.length && data[end] !== 0) end++;
  const str = new TextDecoder("latin1").decode(data.subarray(offset, end));
  return [str, end - offset + 1]; // +1 for null byte
}

interface PngChunk {
  type: string;
  data: Uint8Array;
}

/** Iterate over PNG chunks (skipping the 8-byte signature). */
function* iterChunks(buf: ArrayBuffer): Generator<PngChunk> {
  const view = new DataView(buf);
  let offset = 8; // skip PNG signature
  while (offset + 8 <= buf.byteLength) {
    const length = readU32(view, offset);
    const typeBytes = new Uint8Array(buf, offset + 4, 4);
    const type = String.fromCharCode(...typeBytes);
    const data = new Uint8Array(buf, offset + 8, length);
    yield { type, data };
    offset += 12 + length; // 4(length) + 4(type) + data + 4(crc)
  }
}

// ---------------------------------------------------------------------------
// SwarmUI JSON parser (mirrors Rust parse_swarmui_json)
// ---------------------------------------------------------------------------

function parseSwarmUIJson(text: string): Record<string, string> | null {
  try {
    const root = JSON.parse(text);
    if (typeof root !== "object" || root === null) return null;

    const params: Record<string, string> = {};
    const imageParams = root.sui_image_params;
    if (imageParams && typeof imageParams === "object") {
      const reverseMap: [string, string][] = [
        ["prompt", "positive_prompt"],
        ["negativeprompt", "negative_prompt"],
        ["model", "model"],
        ["vae", "vae"],
        ["seed", "seed"],
        ["steps", "steps"],
        ["cfgscale", "cfg"],
        ["sampler", "sampler"],
        ["scheduler", "scheduler"],
        ["denoise", "denoise"],
        ["generationmode", "mode"],
        ["loras", "loras"],
        ["upscalemodel", "upscale_model"],
        ["upscalescale", "upscale_scale"],
        ["upscaledenoise", "upscale_denoise"],
      ];

      for (const [swarm, internal] of reverseMap) {
        if (swarm in imageParams) {
          const v = imageParams[swarm];
          const s = typeof v === "string" ? v : JSON.stringify(v);
          if (s) params[internal] = s;
        }
      }

      const w = imageParams.width;
      const h = imageParams.height;
      if (w !== undefined && h !== undefined) {
        const ws = typeof w === "number" ? String(w) : String(w);
        const hs = typeof h === "number" ? String(h) : String(h);
        if (ws && hs) params.size = `${ws}x${hs}`;
      }
    }

    const extra = root.sui_extra_data;
    if (extra && typeof extra === "object") {
      if (extra.date) params.date = String(extra.date);
      if (extra.generation_time) params.generation_time = String(extra.generation_time);
    }

    const mooshie = root.mooshie_extra;
    if (mooshie && typeof mooshie === "object") {
      for (const [key, value] of Object.entries(mooshie)) {
        if (key === "software") continue;
        const s = typeof value === "string" ? value : JSON.stringify(value);
        if (s) params[`mooshie_${key}`] = s;
      }
    }

    return Object.keys(params).length > 0 ? params : null;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// A1111 parameter parser (mirrors Rust parse_a1111_params)
// ---------------------------------------------------------------------------

function parseA1111Params(text: string): Record<string, string> {
  const params: Record<string, string> = {};
  const lines = text.split("\n");
  if (lines.length === 0) return params;

  const positiveLines: string[] = [];
  const negativeLines: string[] = [];
  let settingsLine: string | null = null;
  let inNegative = false;

  for (const line of lines) {
    if (line.startsWith("Negative prompt: ")) {
      inNegative = true;
      negativeLines.push(line.slice("Negative prompt: ".length));
    } else if (!inNegative && settingsLine === null) {
      if (line.startsWith("Steps:") || line.startsWith("Sampler:") || line.startsWith("CFG")) {
        settingsLine = line;
      } else {
        positiveLines.push(line);
      }
    } else if (inNegative) {
      if (line.startsWith("Steps:") || line.startsWith("Sampler:") || line.startsWith("CFG")) {
        settingsLine = line;
        inNegative = false;
      } else {
        negativeLines.push(line);
      }
    }
  }

  params.positive_prompt = positiveLines.join("\n");
  if (negativeLines.length > 0) {
    params.negative_prompt = negativeLines.join("\n");
  }

  if (settingsLine) {
    const settingsMap: Record<string, string> = {};
    let currentKey = "";
    let currentValue = "";

    for (const part of settingsLine.split(", ")) {
      const colonPos = part.indexOf(": ");
      if (colonPos !== -1) {
        if (currentKey) settingsMap[currentKey] = currentValue;
        currentKey = part.slice(0, colonPos);
        currentValue = part.slice(colonPos + 2);
      } else if (currentKey) {
        currentValue += ", " + part;
      }
    }
    if (currentKey) settingsMap[currentKey] = currentValue;

    const keyMap: Record<string, string> = {
      Steps: "steps",
      Sampler: "sampler",
      "CFG scale": "cfg",
      Seed: "seed",
      Size: "size",
      Model: "model",
      VAE: "vae",
      "Denoising strength": "denoise",
      Scheduler: "scheduler",
    };

    for (const [a1111Key, internalKey] of Object.entries(keyMap)) {
      if (settingsMap[a1111Key]) params[internalKey] = settingsMap[a1111Key];
    }
  }

  return params;
}

// ---------------------------------------------------------------------------
// Text chunk reader
// ---------------------------------------------------------------------------

/** Read metadata from PNG tEXt/iTXt chunks. */
function readTextChunks(buf: ArrayBuffer): Record<string, string> | null {
  for (const chunk of iterChunks(buf)) {
    if (chunk.type === "tEXt") {
      const [keyword, consumed] = readNullTerminated(chunk.data, 0);
      if (keyword === "parameters") {
        const text = new TextDecoder("latin1").decode(chunk.data.subarray(consumed)).trim();
        if (text.startsWith("{")) {
          const parsed = parseSwarmUIJson(text);
          if (parsed) return parsed;
        }
        return parseA1111Params(text);
      }
    } else if (chunk.type === "iTXt") {
      const [keyword, consumed] = readNullTerminated(chunk.data, 0);
      if (keyword === "parameters") {
        // iTXt: keyword\0 compressionFlag(1) compressionMethod(1) languageTag\0 translatedKeyword\0 text
        let pos = consumed;
        pos += 2; // skip compression flag + method
        while (pos < chunk.data.length && chunk.data[pos] !== 0) pos++;
        pos++; // skip null
        while (pos < chunk.data.length && chunk.data[pos] !== 0) pos++;
        pos++; // skip null
        const text = new TextDecoder("utf-8").decode(chunk.data.subarray(pos)).trim();
        if (text.startsWith("{")) {
          const parsed = parseSwarmUIJson(text);
          if (parsed) return parsed;
        }
        return parseA1111Params(text);
      }
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// Stealth alpha reader (mirrors Rust read_stealth_alpha)
// ---------------------------------------------------------------------------

const STEALTH_MAGIC = new TextEncoder().encode("stealth_pngcomp");

/** Column-major alpha offset for stealth data. */
function colmajorAlphaOffset(bitIdx: number, width: number, height: number): number {
  const x = Math.floor(bitIdx / height);
  const y = bitIdx % height;
  // For 8-bit RGBA: 4 bytes/pixel, alpha at offset 3
  return (y * width + x) * 4 + 3;
}

/** Async version: read stealth alpha and decompress. */
async function readStealthAlphaAsync(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
): Promise<Record<string, string> | null> {
  const pixelCount = width * height;
  const magicBits = STEALTH_MAGIC.length * 8;

  if (pixelCount < magicBits + 32) return null;

  // Read and verify magic header
  const magicBytes = new Uint8Array(STEALTH_MAGIC.length);
  let bitIdx = 0;
  for (let i = 0; i < magicBytes.length; i++) {
    let byteVal = 0;
    for (let bitPos = 7; bitPos >= 0; bitPos--) {
      const off = colmajorAlphaOffset(bitIdx, width, height);
      if (off >= rgba.length) return null;
      const bit = rgba[off] & 1;
      byteVal |= bit << bitPos;
      bitIdx++;
    }
    magicBytes[i] = byteVal;
  }

  for (let i = 0; i < STEALTH_MAGIC.length; i++) {
    if (magicBytes[i] !== STEALTH_MAGIC[i]) return null;
  }

  // Read 32-bit length
  let lenVal = 0;
  for (let bitPos = 31; bitPos >= 0; bitPos--) {
    const off = colmajorAlphaOffset(bitIdx, width, height);
    if (off >= rgba.length) return null;
    const bit = rgba[off] & 1;
    lenVal |= bit << bitPos;
    bitIdx++;
  }
  lenVal = lenVal >>> 0;

  const dataBits = lenVal;
  if (dataBits === 0 || dataBits % 8 !== 0) return null;
  const dataBytesLen = dataBits / 8;
  if (bitIdx + dataBits > pixelCount) return null;
  if (dataBytesLen > 10 * 1024 * 1024) return null;

  const compressed = new Uint8Array(dataBytesLen);
  for (let i = 0; i < dataBytesLen; i++) {
    let byteVal = 0;
    for (let bitPos = 7; bitPos >= 0; bitPos--) {
      const off = colmajorAlphaOffset(bitIdx, width, height);
      if (off >= rgba.length) return null;
      const bit = rgba[off] & 1;
      byteVal |= bit << bitPos;
      bitIdx++;
    }
    compressed[i] = byteVal;
  }

  // Decompress gzip using web Streams API
  try {
    const ds = new DecompressionStream("gzip");
    const writer = ds.writable.getWriter();
    writer.write(compressed);
    writer.close();

    const reader = ds.readable.getReader();
    const chunks: Uint8Array[] = [];
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
    }

    const totalLen = chunks.reduce((acc, c) => acc + c.length, 0);
    const result = new Uint8Array(totalLen);
    let offset = 0;
    for (const chunk of chunks) {
      result.set(chunk, offset);
      offset += chunk.length;
    }

    const jsonText = new TextDecoder("utf-8").decode(result).trim();
    return parseSwarmUIJson(jsonText);
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Read metadata from a PNG file's ArrayBuffer, entirely client-side.
 * Tries PNG text chunks first (fast), then stealth alpha (requires decoding pixels).
 */
export async function readPngMetadataClientSide(
  buf: ArrayBuffer
): Promise<Record<string, string> | null> {
  // 1. Try text chunks (fast — no pixel decoding needed)
  const textResult = readTextChunks(buf);
  if (textResult) return textResult;

  // 2. Try stealth alpha (requires pixel decoding via Canvas)
  try {
    const blob = new Blob([buf], { type: "image/png" });
    const bitmap = await createImageBitmap(blob);
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    ctx.drawImage(bitmap, 0, 0);
    const imageData = ctx.getImageData(0, 0, bitmap.width, bitmap.height);
    bitmap.close();
    return readStealthAlphaAsync(imageData.data, imageData.width, imageData.height);
  } catch {
    return null;
  }
}
