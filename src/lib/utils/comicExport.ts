// Pure binary builders for exporting a multi-page comic book as CBZ (store-mode
// ZIP archive) or PDF 1.4 (DCTDecode JPEG pages). Zero external dependencies and
// no DOM globals in the byte builders so Node tests can validate them directly.

export interface CbzPageInput {
  name: string;
  bytes: Uint8Array;
}

export interface PdfPageInput {
  jpegBytes: Uint8Array;
  width: number;
  height: number;
}

const CRC_TABLE = Uint32Array.from({ length: 256 }, (_, value) => {
  for (let bit = 0; bit < 8; bit++) {
    value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  }
  return value >>> 0;
});

/** Standard IEEE 802.3 CRC-32 used by ZIP archives. */
export function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) {
    crc = CRC_TABLE[(crc ^ bytes[i]) & 255] ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function concatBytes(chunks: Uint8Array[]): Uint8Array {
  let total = 0;
  for (const c of chunks) total += c.length;
  const out = new Uint8Array(total);
  let pos = 0;
  for (const c of chunks) {
    out.set(c, pos);
    pos += c.length;
  }
  return out;
}

/**
 * Build a valid store-mode ZIP archive (CBZ) from named page image buffers.
 * Uses compression method 0 (STORE) since PNG/JPEG images are already compressed.
 */
export function buildCbzBytes(pages: CbzPageInput[]): Uint8Array {
  if (pages.length === 0) throw new Error("Cannot build CBZ with zero pages");
  const encoder = new TextEncoder();
  const bodyChunks: Uint8Array[] = [];
  const centralChunks: Uint8Array[] = [];
  const usedNames = new Set<string>();
  let offset = 0;

  for (let i = 0; i < pages.length; i++) {
    const page = pages[i];
    const cleanName = page.name.replace(/^[/\\]+/, "").replace(/\.\./g, "_") || `${String(i + 1).padStart(3, "0")}.png`;
    if (usedNames.has(cleanName)) throw new Error(`Duplicate CBZ entry name: ${cleanName}`);
    usedNames.add(cleanName);

    const nameBytes = encoder.encode(cleanName);
    const data = page.bytes;
    const sum = crc32(data);

    // Local file header (30 bytes + filename)
    const localHeader = new Uint8Array(30 + nameBytes.length);
    const lv = new DataView(localHeader.buffer);
    lv.setUint32(0, 0x04034b50, true); // Local file header signature
    lv.setUint16(4, 20, true); // Version needed to extract (2.0)
    lv.setUint16(6, 0x0800, true); // UTF-8 filename flag
    lv.setUint16(8, 0, true); // Compression method: 0 = STORE
    lv.setUint16(10, 0, true); // Last mod time
    lv.setUint16(12, 33, true); // Last mod date (1980-01-01)
    lv.setUint32(14, sum, true); // CRC-32
    lv.setUint32(18, data.length, true); // Compressed size
    lv.setUint32(22, data.length, true); // Uncompressed size
    lv.setUint16(26, nameBytes.length, true); // File name length
    lv.setUint16(28, 0, true); // Extra field length
    localHeader.set(nameBytes, 30);

    // Central directory header (46 bytes + filename)
    const central = new Uint8Array(46 + nameBytes.length);
    const cv = new DataView(central.buffer);
    cv.setUint32(0, 0x02014b50, true); // Central directory signature
    cv.setUint16(4, 20, true); // Version made by
    cv.setUint16(6, 20, true); // Version needed
    cv.setUint16(8, 0x0800, true); // UTF-8 filename flag
    cv.setUint16(10, 0, true); // Compression method: 0 = STORE
    cv.setUint16(12, 0, true); // Last mod time
    cv.setUint16(14, 33, true); // Last mod date
    cv.setUint32(16, sum, true); // CRC-32
    cv.setUint32(20, data.length, true); // Compressed size
    cv.setUint32(24, data.length, true); // Uncompressed size
    cv.setUint16(28, nameBytes.length, true); // File name length
    cv.setUint16(30, 0, true); // Extra field length
    cv.setUint16(32, 0, true); // File comment length
    cv.setUint16(34, 0, true); // Disk number start
    cv.setUint16(36, 0, true); // Internal file attributes
    cv.setUint32(38, 0, true); // External file attributes
    cv.setUint32(42, offset, true); // Relative offset of local header
    central.set(nameBytes, 46);

    bodyChunks.push(localHeader, data);
    centralChunks.push(central);
    offset += localHeader.length + data.length;
  }

  let centralSize = 0;
  for (const c of centralChunks) centralSize += c.length;

  // End of central directory record (22 bytes)
  const eocd = new Uint8Array(22);
  const ev = new DataView(eocd.buffer);
  ev.setUint32(0, 0x06054b50, true); // EOCD signature
  ev.setUint16(4, 0, true); // Disk number
  ev.setUint16(6, 0, true); // Start disk
  ev.setUint16(8, pages.length, true); // Entries on disk
  ev.setUint16(10, pages.length, true); // Total entries
  ev.setUint32(12, centralSize, true); // Size of central directory
  ev.setUint32(16, offset, true); // Offset of central directory
  ev.setUint16(20, 0, true); // Comment length

  return concatBytes([...bodyChunks, ...centralChunks, eocd]);
}

export function buildCbzBlob(pages: CbzPageInput[]): Blob {
  const bytes = buildCbzBytes(pages);
  return new Blob([bytes as BlobPart], { type: "application/vnd.comicbook+zip" });
}

/**
 * Build a minimal PDF 1.4 document embedding each page as a /DCTDecode JPEG
 * Image XObject scaled proportionally to a standard 842pt long edge (A4).
 */
export function buildPdfBytes(pages: PdfPageInput[]): Uint8Array {
  if (pages.length === 0) throw new Error("Cannot build PDF with zero pages");
  const encoder = new TextEncoder();
  const chunks: Uint8Array[] = [];
  let cursor = 0;

  const pushBytes = (b: Uint8Array) => {
    chunks.push(b);
    cursor += b.length;
  };
  const pushText = (s: string) => {
    pushBytes(encoder.encode(s));
  };

  // PDF 1.4 header + binary marker comment
  pushBytes(new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d, 0x31, 0x2e, 0x34, 0x0a, 0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a]));

  const totalObjects = 2 + pages.length * 3;
  const offsets = new Array<number>(totalObjects + 1).fill(0);
  const kidsRefs = pages.map((_, i) => `${3 + i * 3} 0 R`).join(" ");

  // 1: Catalog
  offsets[1] = cursor;
  pushText("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

  // 2: Pages
  offsets[2] = cursor;
  pushText(`2 0 obj\n<< /Type /Pages /Kids [${kidsRefs}] /Count ${pages.length} >>\nendobj\n`);

  const longEdgePt = 842;
  for (let i = 0; i < pages.length; i++) {
    const page = pages[i];
    const wPx = Math.max(1, Math.round(page.width));
    const hPx = Math.max(1, Math.round(page.height));
    const isLandscape = wPx > hPx;
    const wPt = isLandscape ? longEdgePt : Number(((longEdgePt * wPx) / hPx).toFixed(2));
    const hPt = isLandscape ? Number(((longEdgePt * hPx) / wPx).toFixed(2)) : longEdgePt;

    const pageId = 3 + i * 3;
    const contentId = pageId + 1;
    const imageId = pageId + 2;

    // Page object
    offsets[pageId] = cursor;
    pushText(
      `${pageId} 0 obj\n` +
        `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${wPt} ${hPt}] ` +
        `/Resources << /XObject << /Im0 ${imageId} 0 R >> >> /Contents ${contentId} 0 R >>\n` +
        `endobj\n`,
    );

    // Page content stream drawing /Im0 across the full MediaBox
    const contentStr = `q\n${wPt} 0 0 ${hPt} 0 0 cm\n/Im0 Do\nQ\n`;
    const contentBytes = encoder.encode(contentStr);
    offsets[contentId] = cursor;
    pushText(`${contentId} 0 obj\n<< /Length ${contentBytes.length} >>\nstream\n`);
    pushBytes(contentBytes);
    pushText("\nendstream\nendobj\n");

    // JPEG Image XObject (/DCTDecode)
    offsets[imageId] = cursor;
    pushText(
      `${imageId} 0 obj\n` +
        `<< /Type /XObject /Subtype /Image /Width ${wPx} /Height ${hPx} ` +
        `/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length ${page.jpegBytes.length} >>\n` +
        `stream\n`,
    );
    pushBytes(page.jpegBytes);
    pushText("\nendstream\nendobj\n");
  }

  // Exact xref table
  const xrefOffset = cursor;
  pushText(`xref\n0 ${totalObjects + 1}\n`);
  pushText("0000000000 65535 f \n");
  for (let id = 1; id <= totalObjects; id++) {
    pushText(`${String(offsets[id]).padStart(10, "0")} 00000 n \n`);
  }

  pushText(
    `trailer\n<< /Size ${totalObjects + 1} /Root 1 0 R >>\nstartxref\n${xrefOffset}\n%%EOF\n`,
  );

  return concatBytes(chunks);
}

export function buildPdfBlob(pages: PdfPageInput[]): Blob {
  const bytes = buildPdfBytes(pages);
  return new Blob([bytes as BlobPart], { type: "application/pdf" });
}
