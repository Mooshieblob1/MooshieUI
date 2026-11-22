import { test } from "node:test";
import assert from "node:assert/strict";
import { buildCbzBytes, buildPdfBytes, crc32 } from "../../src/lib/utils/comicExport.ts";

test("crc32 matches standard IEEE 802.3 check vector", () => {
  const bytes = new TextEncoder().encode("123456789");
  assert.equal(crc32(bytes), 0xcbf43926);
});

test("buildCbzBytes produces a valid store-mode ZIP archive with PK signatures, CRCs, and entries", () => {
  const page1 = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4]);
  const page2 = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 5, 6, 7, 8, 9]);
  const archive = buildCbzBytes([
    { name: "001.png", bytes: page1 },
    { name: "002.png", bytes: page2 },
  ]);

  const view = new DataView(archive.buffer, archive.byteOffset, archive.byteLength);
  const decoder = new TextDecoder();

  // Locate End of Central Directory (22 bytes, no comment)
  const eocdOffset = archive.length - 22;
  assert.equal(view.getUint32(eocdOffset, true), 0x06054b50);
  assert.equal(view.getUint16(eocdOffset + 8, true), 2);
  assert.equal(view.getUint16(eocdOffset + 10, true), 2);
  const cdSize = view.getUint32(eocdOffset + 12, true);
  const cdOffset = view.getUint32(eocdOffset + 16, true);
  assert.equal(cdOffset + cdSize, eocdOffset);

  // Walk Central Directory and verify Local File Headers + payloads
  let cdCursor = cdOffset;
  const extracted: Array<{ name: string; bytes: Uint8Array }> = [];
  for (let i = 0; i < 2; i++) {
    assert.equal(view.getUint32(cdCursor, true), 0x02014b50);
    const cdMethod = view.getUint16(cdCursor + 10, true);
    const cdCrc = view.getUint32(cdCursor + 16, true);
    const cdCompSize = view.getUint32(cdCursor + 20, true);
    const cdUncompSize = view.getUint32(cdCursor + 24, true);
    const cdNameLen = view.getUint16(cdCursor + 28, true);
    const localOffset = view.getUint32(cdCursor + 42, true);
    const cdName = decoder.decode(archive.subarray(cdCursor + 46, cdCursor + 46 + cdNameLen));

    assert.equal(cdMethod, 0);
    assert.equal(cdCompSize, cdUncompSize);

    // Check Local File Header
    assert.equal(view.getUint32(localOffset, true), 0x04034b50);
    const lfCrc = view.getUint32(localOffset + 14, true);
    const lfSize = view.getUint32(localOffset + 18, true);
    const lfNameLen = view.getUint16(localOffset + 26, true);
    const lfExtraLen = view.getUint16(localOffset + 28, true);
    const lfName = decoder.decode(archive.subarray(localOffset + 30, localOffset + 30 + lfNameLen));
    assert.equal(lfName, cdName);
    assert.equal(lfCrc, cdCrc);
    assert.equal(lfSize, cdUncompSize);

    const payloadStart = localOffset + 30 + lfNameLen + lfExtraLen;
    const payload = archive.subarray(payloadStart, payloadStart + lfSize);
    assert.equal(crc32(payload), cdCrc);
    extracted.push({ name: lfName, bytes: payload });

    cdCursor += 46 + cdNameLen;
  }

  assert.equal(cdCursor, eocdOffset);
  assert.equal(extracted[0].name, "001.png");
  assert.deepEqual(extracted[0].bytes, page1);
  assert.equal(extracted[1].name, "002.png");
  assert.deepEqual(extracted[1].bytes, page2);

  assert.throws(() => buildCbzBytes([]));
});

test("buildPdfBytes produces a valid PDF 1.4 with DCTDecode images and exact xref offsets", () => {
  const jpeg1 = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0xff, 0xd9]);
  const jpeg2 = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0xaa, 0xbb, 0xff, 0xd9]);
  const pdf = buildPdfBytes([
    { jpegBytes: jpeg1, width: 1448, height: 2048 },
    { jpegBytes: jpeg2, width: 2048, height: 1448 },
  ]);

  const latin1 = new TextDecoder("latin1").decode(pdf);
  assert.ok(latin1.startsWith("%PDF-1.4\n"));
  assert.ok(latin1.endsWith("%%EOF\n"));
  assert.ok(latin1.includes("/Count 2"));
  assert.equal((latin1.match(/\/Filter \/DCTDecode/g) ?? []).length, 2);
  assert.ok(latin1.includes("/Width 1448 /Height 2048"));
  assert.ok(latin1.includes("/Width 2048 /Height 1448"));

  // Check proportional aspect ratio in MediaBox (portrait 1448x2048 -> ~595.38 x 842; landscape -> 842 x ~595.38)
  const boxes = [...latin1.matchAll(/\/MediaBox \[0 0 ([0-9.]+) ([0-9.]+)\]/g)].map((m) => ({
    w: Number(m[1]),
    h: Number(m[2]),
  }));
  assert.equal(boxes.length, 2);
  assert.ok(Math.abs(boxes[0].w / boxes[0].h - 1448 / 2048) < 1e-4);
  assert.ok(Math.abs(boxes[1].w / boxes[1].h - 2048 / 1448) < 1e-4);

  // Verify startxref and every xref byte offset
  const startXrefMatch = latin1.match(/startxref\n(\d+)\n%%EOF\n$/);
  assert.ok(startXrefMatch, "startxref present");
  const xrefOffset = Number(startXrefMatch[1]);
  assert.equal(latin1.slice(xrefOffset, xrefOffset + 5), "xref\n");

  const xrefSection = latin1.slice(xrefOffset);
  const entryMatches = [...xrefSection.matchAll(/^(\d{10}) (\d{5}) ([fn]) $/gm)];
  // 1 free entry (0) + 2 catalog/pages + 2*3 page/content/image = 9 entries
  assert.equal(entryMatches.length, 9);
  assert.equal(entryMatches[0][3], "f");
  for (let id = 1; id < entryMatches.length; id++) {
    assert.equal(entryMatches[id][3], "n");
    const objOffset = Number(entryMatches[id][1]);
    assert.ok(
      latin1.startsWith(`${id} 0 obj\n`, objOffset),
      `object ${id} starts at xref offset ${objOffset}`,
    );
  }

  assert.throws(() => buildPdfBytes([]));
});
