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

test("SwarmUI parameters zip string-array loras with loraweights and loratencweights", async () => {
  const payload = JSON.stringify({
    sui_image_params: {
      model: "redLilyIllu_v10",
      loras: ["ctd mofu/ctdmofu", "style/second"],
      loraweights: ["0.8", 0.5],
      loratencweights: ["0.6"],
    },
  });
  const meta = await readPngMetadataClientSide(png([["parameters", payload]]));
  assert.ok(meta);
  assert.deepEqual(JSON.parse(meta.loras), [
    { name: "ctd mofu/ctdmofu", strength_model: 0.8, strength_clip: 0.6 },
    { name: "style/second", strength_model: 0.5, strength_clip: 0.5 },
  ]);

  const defaultPayload = JSON.stringify({
    sui_image_params: {
      loras: ["only_name"],
    },
  });
  const defaultMeta = await readPngMetadataClientSide(png([["parameters", defaultPayload]]));
  assert.ok(defaultMeta);
  assert.deepEqual(JSON.parse(defaultMeta.loras), [
    { name: "only_name", strength_model: 1.0, strength_clip: 1.0 },
  ]);
});

test("NativeInfillingRequest reads inpaint strength without touching add_original_image", async () => {
  const comment = JSON.stringify({
    request_type: "NativeInfillingRequest",
    add_original_image: false,
    inpaintImg2ImgStrength: 0.65,
    img2img: { strength: 0.65, color_correct: true },
  });
  const meta = await readPngMetadataClientSide(
    png([
      ["Software", "NovelAI"],
      ["Source", "NovelAI Diffusion V5 657484A5"],
      ["Comment", comment],
    ]),
  );
  assert.ok(meta);
  assert.equal(meta.mooshie_novelai_request_type, "NativeInfillingRequest");
  assert.equal(meta.mooshie_novelai_inpaint_strength, "0.65");
  assert.equal(meta.mooshie_novelai_add_original_image, undefined);
});
