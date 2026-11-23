/**
 * Gaussian splat files: read a standard 3DGS binary PLY (what ComfyUI's
 * `SplatToFile3D` writes) into flat arrays the viewer can upload, and write
 * the compact `.splat` format (32 bytes per Gaussian) for export.
 *
 * Coordinates are kept as written. TripoSplat/Idol output uses the usual 3DGS
 * convention: +Y points down and the camera looks along +Z.
 */

export interface SplatData {
  count: number;
  /** xyz per Gaussian. */
  positions: Float32Array;
  /** Linear (already exponentiated) scale per axis. */
  scales: Float32Array;
  /** Unit quaternion, w x y z. */
  rotations: Float32Array;
  /** RGBA bytes; alpha is the activated opacity. */
  colors: Uint8Array;
}

const SH_C0 = 0.28209479177387814;

const TYPE_SIZES: Record<string, number> = {
  char: 1, int8: 1, uchar: 1, uint8: 1,
  short: 2, int16: 2, ushort: 2, uint16: 2,
  int: 4, int32: 4, uint: 4, uint32: 4,
  float: 4, float32: 4, double: 8, float64: 8,
};

type Reader = (view: DataView, offset: number) => number;

function reader(type: string): Reader {
  switch (type) {
    case "char": case "int8": return (v, o) => v.getInt8(o);
    case "uchar": case "uint8": return (v, o) => v.getUint8(o);
    case "short": case "int16": return (v, o) => v.getInt16(o, true);
    case "ushort": case "uint16": return (v, o) => v.getUint16(o, true);
    case "int": case "int32": return (v, o) => v.getInt32(o, true);
    case "uint": case "uint32": return (v, o) => v.getUint32(o, true);
    case "double": case "float64": return (v, o) => v.getFloat64(o, true);
    default: return (v, o) => v.getFloat32(o, true);
  }
}

interface Element { name: string; count: number; props: { name: string; type: string; offset: number }[]; stride: number }

/** Parse the header. Throws on anything but a binary little-endian PLY without list properties. */
function parseHeader(bytes: Uint8Array): { elements: Element[]; dataStart: number } {
  const limit = Math.min(bytes.length, 65536);
  let headerText = "";
  let end = -1;
  for (let i = 0; i < limit; i++) {
    // "end_header\n"
    if (bytes[i] === 0x0a && i >= 10 && String.fromCharCode(...bytes.subarray(i - 10, i)) === "end_header") {
      end = i + 1;
      break;
    }
  }
  if (end < 0) throw new Error("Not a PLY file (no end_header).");
  headerText = new TextDecoder().decode(bytes.subarray(0, end));
  const lines = headerText.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  if (lines[0] !== "ply") throw new Error("Not a PLY file.");
  if (!lines.some((l) => l === "format binary_little_endian 1.0")) {
    throw new Error("Only binary little-endian PLY splats are supported.");
  }
  const elements: Element[] = [];
  for (const line of lines) {
    const parts = line.split(/\s+/);
    if (parts[0] === "element") {
      elements.push({ name: parts[1], count: Number(parts[2]), props: [], stride: 0 });
    } else if (parts[0] === "property") {
      const el = elements[elements.length - 1];
      if (!el) throw new Error("PLY property before any element.");
      if (parts[1] === "list") throw new Error("PLY list properties are not supported for splats.");
      const size = TYPE_SIZES[parts[1]];
      if (!size) throw new Error(`Unknown PLY property type ${parts[1]}.`);
      el.props.push({ name: parts[2], type: parts[1], offset: el.stride });
      el.stride += size;
    }
  }
  for (const el of elements) {
    if (!Number.isSafeInteger(el.count) || el.count < 0) throw new Error("Invalid PLY element count.");
  }
  return { elements, dataStart: end };
}

const sigmoid = (x: number) => 1 / (1 + Math.exp(-x));
const clampByte = (x: number) => (x <= 0 ? 0 : x >= 255 ? 255 : Math.round(x));

/** Read a 3DGS PLY. Accepts SH colors (f_dc_*) or plain red/green/blue bytes. */
export function parseSplatPly(buffer: ArrayBuffer): SplatData {
  const bytes = new Uint8Array(buffer);
  const { elements, dataStart } = parseHeader(bytes);
  let offset = dataStart;
  let vertex: Element | undefined;
  for (const el of elements) {
    if (el.name === "vertex") { vertex = el; break; }
    offset += el.count * el.stride;
  }
  if (!vertex) throw new Error("The PLY has no vertex element.");
  const count = vertex.count;
  if (count <= 0) throw new Error("The PLY has no Gaussians.");
  if (offset + count * vertex.stride > bytes.length) throw new Error("The PLY file is truncated.");

  const prop = (name: string) => vertex!.props.find((p) => p.name === name);
  const accessor = (name: string) => {
    const p = prop(name);
    return p ? { offset: p.offset, read: reader(p.type) } : null;
  };
  const x = accessor("x"), y = accessor("y"), z = accessor("z");
  if (!x || !y || !z) throw new Error("The PLY has no positions.");
  const dc = [accessor("f_dc_0"), accessor("f_dc_1"), accessor("f_dc_2")];
  const rgb = [accessor("red"), accessor("green"), accessor("blue")];
  const opacity = accessor("opacity");
  const scale = [accessor("scale_0"), accessor("scale_1"), accessor("scale_2")];
  const rot = [accessor("rot_0"), accessor("rot_1"), accessor("rot_2"), accessor("rot_3")];
  const hasDc = dc.every(Boolean);
  const hasRgb = rgb.every(Boolean);
  const hasScale = scale.every(Boolean);
  const hasRot = rot.every(Boolean);

  const view = new DataView(buffer);
  const positions = new Float32Array(count * 3);
  const scales = new Float32Array(count * 3);
  const rotations = new Float32Array(count * 4);
  const colors = new Uint8Array(count * 4);
  for (let i = 0; i < count; i++) {
    const row = offset + i * vertex.stride;
    positions[i * 3] = x.read(view, row + x.offset);
    positions[i * 3 + 1] = y.read(view, row + y.offset);
    positions[i * 3 + 2] = z.read(view, row + z.offset);
    for (let k = 0; k < 3; k++) {
      scales[i * 3 + k] = hasScale ? Math.exp(scale[k]!.read(view, row + scale[k]!.offset)) : 0.005;
      if (hasDc) colors[i * 4 + k] = clampByte((0.5 + SH_C0 * dc[k]!.read(view, row + dc[k]!.offset)) * 255);
      else if (hasRgb) colors[i * 4 + k] = clampByte(rgb[k]!.read(view, row + rgb[k]!.offset));
      else colors[i * 4 + k] = 200;
    }
    colors[i * 4 + 3] = opacity ? clampByte(sigmoid(opacity.read(view, row + opacity.offset)) * 255) : 255;
    let qw = 1, qx = 0, qy = 0, qz = 0;
    if (hasRot) {
      qw = rot[0]!.read(view, row + rot[0]!.offset);
      qx = rot[1]!.read(view, row + rot[1]!.offset);
      qy = rot[2]!.read(view, row + rot[2]!.offset);
      qz = rot[3]!.read(view, row + rot[3]!.offset);
      const n = Math.hypot(qw, qx, qy, qz) || 1;
      qw /= n; qx /= n; qy /= n; qz /= n;
    }
    rotations[i * 4] = qw;
    rotations[i * 4 + 1] = qx;
    rotations[i * 4 + 2] = qy;
    rotations[i * 4 + 3] = qz;
  }
  return { count, positions, scales, rotations, colors };
}

/** Number of Gaussians in a PLY, read from its header only. */
export function plyVertexCount(buffer: ArrayBuffer): number {
  const { elements } = parseHeader(new Uint8Array(buffer, 0, Math.min(buffer.byteLength, 65536)));
  return elements.find((e) => e.name === "vertex")?.count ?? 0;
}

/**
 * The `.splat` format (antimatter15): per Gaussian, position f32x3, scale
 * f32x3, RGBA u8x4, rotation u8x4 (w x y z mapped from [-1, 1] to [0, 255]).
 * Most web splat viewers open it.
 */
export function toSplatBytes(data: SplatData): Uint8Array {
  const out = new ArrayBuffer(data.count * 32);
  const f = new Float32Array(out);
  const u = new Uint8Array(out);
  for (let i = 0; i < data.count; i++) {
    f[i * 8] = data.positions[i * 3];
    f[i * 8 + 1] = data.positions[i * 3 + 1];
    f[i * 8 + 2] = data.positions[i * 3 + 2];
    f[i * 8 + 3] = data.scales[i * 3];
    f[i * 8 + 4] = data.scales[i * 3 + 1];
    f[i * 8 + 5] = data.scales[i * 3 + 2];
    u.set(data.colors.subarray(i * 4, i * 4 + 4), i * 32 + 24);
    for (let k = 0; k < 4; k++) u[i * 32 + 28 + k] = clampByte(data.rotations[i * 4 + k] * 128 + 128);
  }
  return u;
}

/** Opacity-weighted centre and a robust radius (95th percentile), for framing the camera. */
export function splatBounds(data: SplatData): { center: [number, number, number]; radius: number } {
  let sx = 0, sy = 0, sz = 0, sw = 0;
  for (let i = 0; i < data.count; i++) {
    const w = data.colors[i * 4 + 3] / 255;
    if (w < 0.05) continue;
    sx += data.positions[i * 3] * w;
    sy += data.positions[i * 3 + 1] * w;
    sz += data.positions[i * 3 + 2] * w;
    sw += w;
  }
  const center: [number, number, number] = sw > 0 ? [sx / sw, sy / sw, sz / sw] : [0, 0, 0];
  const step = Math.max(1, Math.floor(data.count / 20000));
  const distances: number[] = [];
  for (let i = 0; i < data.count; i += step) {
    if (data.colors[i * 4 + 3] < 13) continue;
    distances.push(Math.hypot(
      data.positions[i * 3] - center[0],
      data.positions[i * 3 + 1] - center[1],
      data.positions[i * 3 + 2] - center[2],
    ));
  }
  distances.sort((a, b) => a - b);
  const radius = distances.length ? distances[Math.min(distances.length - 1, Math.floor(distances.length * 0.95))] : 1;
  return { center, radius: radius > 1e-6 ? radius : 1 };
}
