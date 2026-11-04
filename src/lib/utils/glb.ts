/**
 * A small binary glTF 2.0 (.glb) reader for the 3D page's mesh viewer.
 *
 * It reads what ComfyUI's SaveGLB writes (one mesh, POSITION / NORMAL /
 * TEXCOORD_0 / optional COLOR_0, uint32 indices, PNG textures embedded in the
 * binary chunk) and, more generally, every triangle primitive of every mesh,
 * ignoring node transforms. Positions and normals are turned from glTF's
 * frame (+Y up, front facing +Z) into the splat viewer's (+Y down, front
 * facing -Z) by a half turn about X, so both kinds share one orbit camera.
 */

export interface MeshTexture {
  image: ImageBitmap;
}

export interface MeshMaterial {
  baseColorFactor: [number, number, number, number];
  metallicFactor: number;
  roughnessFactor: number;
  baseColor: MeshTexture | null;
  /** glTF metallicRoughness: G = roughness, B = metallic. */
  metallicRoughness: MeshTexture | null;
  /** R = ambient occlusion (often the same image as metallicRoughness). */
  occlusion: MeshTexture | null;
  occlusionStrength: number;
  normal: MeshTexture | null;
  normalScale: number;
  unlit: boolean;
}

export interface MeshPrimitive {
  positions: Float32Array;
  normals: Float32Array | null;
  uvs: Float32Array | null;
  /** RGBA in 0..1, or null. */
  colors: Float32Array | null;
  indices: Uint32Array;
  material: MeshMaterial;
}

export interface MeshData {
  kind: "mesh";
  primitives: MeshPrimitive[];
  vertexCount: number;
  faceCount: number;
  center: [number, number, number];
  radius: number;
}

interface GltfAccessor {
  bufferView?: number;
  byteOffset?: number;
  componentType: number;
  count: number;
  type: string;
  normalized?: boolean;
}

interface GltfBufferView {
  buffer?: number;
  byteOffset?: number;
  byteLength: number;
  byteStride?: number;
}

interface GltfTextureInfo { index: number; texCoord?: number; scale?: number; strength?: number }

interface Gltf {
  accessors?: GltfAccessor[];
  bufferViews?: GltfBufferView[];
  images?: { bufferView?: number; mimeType?: string; uri?: string }[];
  textures?: { source?: number }[];
  materials?: {
    pbrMetallicRoughness?: {
      baseColorFactor?: number[];
      metallicFactor?: number;
      roughnessFactor?: number;
      baseColorTexture?: GltfTextureInfo;
      metallicRoughnessTexture?: GltfTextureInfo;
    };
    normalTexture?: GltfTextureInfo;
    occlusionTexture?: GltfTextureInfo;
    extensions?: Record<string, unknown>;
  }[];
  meshes?: { primitives: { attributes: Record<string, number>; indices?: number; material?: number; mode?: number }[] }[];
}

const COMPONENTS: Record<string, number> = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 };

const MAGIC_GLTF = 0x46546c67;
const CHUNK_JSON = 0x4e4f534a;
const CHUNK_BIN = 0x004e4942;

/** Split a .glb into its JSON and binary chunks. Throws on anything else. */
export function readGlbChunks(buffer: ArrayBuffer): { json: Gltf; bin: Uint8Array } {
  const view = new DataView(buffer);
  if (buffer.byteLength < 20 || view.getUint32(0, true) !== MAGIC_GLTF) throw new Error("Not a binary glTF file");
  if (view.getUint32(4, true) !== 2) throw new Error("Only glTF 2.0 is supported");
  const length = Math.min(view.getUint32(8, true), buffer.byteLength);
  let offset = 12;
  let json: Gltf | null = null;
  let bin = new Uint8Array(0);
  while (offset + 8 <= length) {
    const chunkLength = view.getUint32(offset, true);
    const chunkType = view.getUint32(offset + 4, true);
    const start = offset + 8;
    if (start + chunkLength > length) throw new Error("Truncated glTF chunk");
    if (chunkType === CHUNK_JSON) json = JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, start, chunkLength)));
    else if (chunkType === CHUNK_BIN && bin.length === 0) bin = new Uint8Array(buffer, start, chunkLength);
    offset = start + chunkLength + ((4 - (chunkLength % 4)) % 4);
  }
  if (!json) throw new Error("The glTF file has no JSON chunk");
  return { json, bin };
}

function readComponent(view: DataView, at: number, type: number, normalized: boolean): number {
  switch (type) {
    case 5126: return view.getFloat32(at, true);
    case 5125: return view.getUint32(at, true);
    case 5123: { const v = view.getUint16(at, true); return normalized ? v / 65535 : v; }
    case 5121: { const v = view.getUint8(at); return normalized ? v / 255 : v; }
    case 5122: { const v = view.getInt16(at, true); return normalized ? Math.max(v / 32767, -1) : v; }
    case 5120: { const v = view.getInt8(at); return normalized ? Math.max(v / 127, -1) : v; }
    default: throw new Error(`Unsupported glTF component type ${type}`);
  }
}

const COMPONENT_BYTES: Record<number, number> = { 5120: 1, 5121: 1, 5122: 2, 5123: 2, 5125: 4, 5126: 4 };

/** Read an accessor into a dense array of `components` per element. */
function readAccessor(gltf: Gltf, bin: Uint8Array, index: number): { data: Float32Array | Uint32Array; size: number } {
  const accessor = gltf.accessors?.[index];
  if (!accessor) throw new Error(`Missing glTF accessor ${index}`);
  const size = COMPONENTS[accessor.type];
  const bytes = COMPONENT_BYTES[accessor.componentType];
  if (!size || !bytes) throw new Error(`Unsupported glTF accessor ${accessor.type}`);
  const integer = accessor.componentType === 5125 || ((accessor.componentType === 5123 || accessor.componentType === 5121) && !accessor.normalized);
  const out = integer ? new Uint32Array(accessor.count * size) : new Float32Array(accessor.count * size);
  if (accessor.bufferView === undefined) return { data: out, size }; // all zeros
  const bufferView = gltf.bufferViews?.[accessor.bufferView];
  if (!bufferView || (bufferView.buffer ?? 0) !== 0) throw new Error("glTF data outside the binary chunk is not supported");
  const base = (bufferView.byteOffset ?? 0) + (accessor.byteOffset ?? 0);
  const stride = bufferView.byteStride || size * bytes;
  const end = base + stride * (accessor.count - 1) + size * bytes;
  if (accessor.count > 0 && end > bin.byteLength) throw new Error("glTF accessor runs past the binary chunk");
  const view = new DataView(bin.buffer, bin.byteOffset, bin.byteLength);
  // Fast path: tightly packed float32 / uint32 at an aligned offset.
  if (stride === size * bytes && (accessor.componentType === 5126 || accessor.componentType === 5125) && (bin.byteOffset + base) % 4 === 0) {
    const Ctor = accessor.componentType === 5126 ? Float32Array : Uint32Array;
    out.set(new Ctor(bin.buffer as ArrayBuffer, bin.byteOffset + base, accessor.count * size));
    return { data: out, size };
  }
  for (let i = 0; i < accessor.count; i++) {
    for (let c = 0; c < size; c++) {
      out[i * size + c] = readComponent(view, base + i * stride + c * bytes, accessor.componentType, !!accessor.normalized);
    }
  }
  return { data: out, size };
}

async function loadImages(gltf: Gltf, bin: Uint8Array): Promise<(ImageBitmap | null)[]> {
  return Promise.all((gltf.images ?? []).map(async (image) => {
    if (image.bufferView === undefined) return null;
    const bufferView = gltf.bufferViews?.[image.bufferView];
    if (!bufferView) return null;
    const start = bufferView.byteOffset ?? 0;
    if (start + bufferView.byteLength > bin.byteLength) return null;
    const blob = new Blob([bin.slice(start, start + bufferView.byteLength)], { type: image.mimeType || "image/png" });
    try {
      // Raw texel values: normal and ORM maps are data, not colours.
      return await createImageBitmap(blob, { premultiplyAlpha: "none", colorSpaceConversion: "none" });
    } catch {
      try { return await createImageBitmap(blob); } catch { return null; }
    }
  }));
}

/** Parse a .glb into GPU-ready arrays in the splat viewer's frame. */
export async function parseGlb(buffer: ArrayBuffer): Promise<MeshData> {
  const { json: gltf, bin } = readGlbChunks(buffer);
  const images = await loadImages(gltf, bin);
  const texture = (info?: GltfTextureInfo): MeshTexture | null => {
    if (!info) return null;
    const source = gltf.textures?.[info.index]?.source;
    const image = source === undefined ? null : images[source];
    return image ? { image } : null;
  };
  const materials = (gltf.materials ?? []).map((m): MeshMaterial => {
    const pbr = m.pbrMetallicRoughness ?? {};
    const factor = pbr.baseColorFactor ?? [1, 1, 1, 1];
    return {
      baseColorFactor: [factor[0] ?? 1, factor[1] ?? 1, factor[2] ?? 1, factor[3] ?? 1],
      metallicFactor: pbr.metallicFactor ?? 1,
      roughnessFactor: pbr.roughnessFactor ?? 1,
      baseColor: texture(pbr.baseColorTexture),
      metallicRoughness: texture(pbr.metallicRoughnessTexture),
      occlusion: texture(m.occlusionTexture),
      occlusionStrength: m.occlusionTexture?.strength ?? 1,
      normal: texture(m.normalTexture),
      normalScale: m.normalTexture?.scale ?? 1,
      unlit: !!m.extensions?.KHR_materials_unlit,
    };
  });
  const fallback: MeshMaterial = {
    baseColorFactor: [0.8, 0.8, 0.8, 1], metallicFactor: 0, roughnessFactor: 0.7,
    baseColor: null, metallicRoughness: null, occlusion: null, occlusionStrength: 1,
    normal: null, normalScale: 1, unlit: false,
  };

  const primitives: MeshPrimitive[] = [];
  let vertexCount = 0, faceCount = 0;
  const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
  for (const mesh of gltf.meshes ?? []) {
    for (const primitive of mesh.primitives) {
      if ((primitive.mode ?? 4) !== 4 || primitive.attributes.POSITION === undefined) continue;
      const positions = readAccessor(gltf, bin, primitive.attributes.POSITION).data as Float32Array;
      const count = positions.length / 3;
      const normals = primitive.attributes.NORMAL !== undefined ? readAccessor(gltf, bin, primitive.attributes.NORMAL).data as Float32Array : null;
      const uvs = primitive.attributes.TEXCOORD_0 !== undefined ? Float32Array.from(readAccessor(gltf, bin, primitive.attributes.TEXCOORD_0).data) : null;
      let colors: Float32Array | null = null;
      if (primitive.attributes.COLOR_0 !== undefined) {
        const { data, size } = readAccessor(gltf, bin, primitive.attributes.COLOR_0);
        colors = new Float32Array(count * 4);
        for (let i = 0; i < count; i++) {
          colors[i * 4] = data[i * size]; colors[i * 4 + 1] = data[i * size + 1]; colors[i * 4 + 2] = data[i * size + 2];
          colors[i * 4 + 3] = size === 4 ? data[i * size + 3] : 1;
        }
      }
      let indices: Uint32Array;
      if (primitive.indices !== undefined) {
        indices = Uint32Array.from(readAccessor(gltf, bin, primitive.indices).data);
        for (let i = 0; i < indices.length; i++) if (indices[i] >= count) throw new Error("glTF index out of range");
      } else {
        indices = new Uint32Array(count);
        for (let i = 0; i < count; i++) indices[i] = i;
      }
      // Half turn about X: (x, y, z) -> (x, -y, -z).
      for (let i = 0; i < count; i++) {
        positions[i * 3 + 1] = -positions[i * 3 + 1];
        positions[i * 3 + 2] = -positions[i * 3 + 2];
        for (let k = 0; k < 3; k++) {
          const v = positions[i * 3 + k];
          if (v < min[k]) min[k] = v;
          if (v > max[k]) max[k] = v;
        }
        if (normals) { normals[i * 3 + 1] = -normals[i * 3 + 1]; normals[i * 3 + 2] = -normals[i * 3 + 2]; }
      }
      primitives.push({ positions, normals, uvs, colors, indices, material: materials[primitive.material ?? -1] ?? fallback });
      vertexCount += count;
      faceCount += Math.floor(indices.length / 3);
    }
  }
  if (primitives.length === 0) throw new Error("The glTF file has no triangle meshes");
  const center: [number, number, number] = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2];
  const radius = Math.max(1e-3, Math.hypot(max[0] - min[0], max[1] - min[1], max[2] - min[2]) / 2);
  return { kind: "mesh", primitives, vertexCount, faceCount, center, radius };
}

/** Face count from a .glb without decoding textures (for library records). */
export function glbFaceCount(buffer: ArrayBuffer): number {
  try {
    const { json } = readGlbChunks(buffer);
    let faces = 0;
    for (const mesh of json.meshes ?? []) {
      for (const p of mesh.primitives) {
        if ((p.mode ?? 4) !== 4) continue;
        const accessor = json.accessors?.[p.indices ?? p.attributes.POSITION];
        if (accessor) faces += Math.floor(accessor.count / 3);
      }
    }
    return faces;
  } catch { return 0; }
}

/** Free the decoded textures. */
export function disposeMesh(mesh: MeshData) {
  const seen = new Set<ImageBitmap>();
  for (const p of mesh.primitives) {
    for (const t of [p.material.baseColor, p.material.metallicRoughness, p.material.occlusion, p.material.normal]) {
      if (t && !seen.has(t.image)) { seen.add(t.image); t.image.close(); }
    }
  }
}
