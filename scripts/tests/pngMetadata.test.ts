import assert from "node:assert/strict";
import test from "node:test";
import { readPngMetadataClientSide } from "../../src/lib/utils/pngMetadata.ts";

/**
 * A minimal chunk list with the PNG signature. The client-side reader walks
 * chunks without checking CRCs or decoding pixels, so zero CRCs and an empty
 * IDAT are enough to exercise the text-chunk paths.
 */
function png(chunks: [string, string][]): ArrayBuffer {
  const parts: number[] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  const push = (type: string, data: number[]) => {
    const len = data.length;
    parts.push((len >>> 24) & 0xff, (len >>> 16) & 0xff, (len >>> 8) & 0xff, len & 0xff);
    for (const c of type) parts.push(c.charCodeAt(0));
    parts.push(...data, 0, 0, 0, 0);
  };
  push("IHDR", [0, 0, 0, 8, 0, 0, 0, 8, 8, 6, 0, 0, 0]);
  const text = (keyword: string, value: string) =>
    [...keyword, "\0", ...value].map((c) => c.charCodeAt(0) & 0xff);
  const before = chunks.filter(([k]) => k !== "MooshieUI");
  const after = chunks.filter(([k]) => k === "MooshieUI");
  for (const [k, v] of before) push("tEXt", text(k, v));
  push("IDAT", []);
  // The marker sits after IDAT, just before IEND, where the app writes it.
  for (const [k, v] of after) push("tEXt", text(k, v));
  push("IEND", []);
  return new Uint8Array(parts).buffer;
}

const NAI_CHUNKS: [string, string][] = [
  ["Title", "AI generated image"],
  ["Description", "1girl, standing"],
  ["Software", "NovelAI"],
  ["Source", "NovelAI Diffusion V4.5 4BDE2A90"],
  ["Comment", '{"prompt": "1girl, standing", "seed": 577536437, "steps": 28}'],
];

const MARKER =
  '{"software":"MooshieUI","version":"2.3.49","backend":"novelai","model":"nai-diffusion-4-5-full","generated":"2026-11-17T04:20:00+08:00","source":"ui"}';

test("NovelAI PNG with the MooshieUI marker reads as MooshieUI (NovelAI)", async () => {
  const meta = await readPngMetadataClientSide(png([...NAI_CHUNKS, ["MooshieUI", MARKER]]));
  assert.ok(meta);
  assert.equal(meta.source, "MooshieUI (NovelAI)");
  // Settings still come from NovelAI's own chunks.
  assert.equal(meta.seed, "577536437");
  assert.equal(meta.positive_prompt, "1girl, standing");
  assert.equal(meta.mooshie_backend, "novelai");
});

test("NovelAI PNG without the marker reads as NovelAI", async () => {
  const meta = await readPngMetadataClientSide(png(NAI_CHUNKS));
  assert.ok(meta);
  assert.equal(meta.source, "NovelAI");
  assert.equal(meta.seed, "577536437");
});

test("A parameters chunk is read as before, with no source label", async () => {
  const meta = await readPngMetadataClientSide(
    png([["parameters", '{"sui_image_params":{"seed":42},"mooshie_extra":{"software":"MooshieUI"}}']]),
  );
  assert.ok(meta);
  assert.equal(meta.seed, "42");
  assert.equal(meta.source, undefined);
});
