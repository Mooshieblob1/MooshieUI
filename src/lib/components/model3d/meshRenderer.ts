/**
 * A small WebGL2 textured-mesh renderer for the 3D page (Pixal3D .glb files).
 *
 * It shares the splat viewer's orbit camera and frame (+Y down, see
 * `utils/glb.ts`), and draws glTF metallic-roughness materials with a simple
 * studio light rig: base colour (sRGB), roughness / metal and ambient
 * occlusion from the ORM map, and the normal map through a tangent frame
 * built from screen-space derivatives, so no tangents are needed.
 *
 * Faces are drawn double-sided: generated meshes can have open or flipped
 * patches, and a hole should show the inside, not the background.
 */
import type { MeshData, MeshMaterial, MeshTexture } from "../../utils/glb.js";
import { orbitView, type OrbitCamera } from "./splatRenderer.js";

const VERTEX = `#version 300 es
precision highp float;
uniform mat4 projection, view;
in vec3 position;
in vec3 normal;
in vec2 uv;
in vec4 color;
out vec3 vWorld;
out vec3 vNormal;
out vec2 vUv;
out vec4 vColor;
void main () {
  vWorld = position;
  vNormal = normal;
  vUv = uv;
  vColor = color;
  gl_Position = projection * view * vec4(position, 1.0);
}`;

const FRAGMENT = `#version 300 es
precision highp float;
uniform sampler2D baseMap, ormMap, occlusionMap, normalMap;
uniform vec4 baseFactor;
uniform float metallicFactor, roughnessFactor, occlusionStrength, normalScale;
uniform int flags;
uniform vec3 eye, keyLight, fillLight, rimLight;
in vec3 vWorld;
in vec3 vNormal;
in vec2 vUv;
in vec4 vColor;
out vec4 fragColor;

const int HAS_BASE = 1, HAS_ORM = 2, HAS_OCCLUSION = 4, HAS_NORMAL = 8, UNLIT = 16;

mat3 cotangentFrame(vec3 n, vec3 p, vec2 uv) {
  vec3 dp1 = dFdx(p), dp2 = dFdy(p);
  vec2 duv1 = dFdx(uv), duv2 = dFdy(uv);
  vec3 dp2perp = cross(dp2, n), dp1perp = cross(n, dp1);
  vec3 t = dp2perp * duv1.x + dp1perp * duv2.x;
  vec3 b = dp2perp * duv1.y + dp1perp * duv2.y;
  float scale = inversesqrt(max(max(dot(t, t), dot(b, b)), 1e-20));
  return mat3(t * scale, b * scale, n);
}

vec3 light(vec3 n, vec3 v, vec3 l, vec3 radiance, vec3 diffuse, vec3 f0, float roughness) {
  float ndl = max(dot(n, l), 0.0);
  if (ndl <= 0.0) return vec3(0.0);
  vec3 h = normalize(l + v);
  float a = max(roughness * roughness, 0.002);
  float shininess = 2.0 / (a * a) - 2.0;
  float spec = (shininess + 8.0) / 25.13 * pow(max(dot(n, h), 0.0), shininess);
  vec3 fresnel = f0 + (1.0 - f0) * pow(1.0 - max(dot(h, v), 0.0), 5.0);
  return radiance * ndl * (diffuse + fresnel * spec * 0.25);
}

void main () {
  vec4 base = baseFactor * vColor;
  if ((flags & HAS_BASE) != 0) base *= texture(baseMap, vUv);
  if (base.a < 0.05) discard;
  if ((flags & UNLIT) != 0) {
    fragColor = vec4(pow(clamp(base.rgb, 0.0, 1.0), vec3(1.0 / 2.2)), 1.0);
    return;
  }
  float roughness = roughnessFactor, metallic = metallicFactor;
  if ((flags & HAS_ORM) != 0) {
    vec4 orm = texture(ormMap, vUv);
    roughness *= orm.g;
    metallic *= orm.b;
  }
  roughness = clamp(roughness, 0.04, 1.0);
  metallic = clamp(metallic, 0.0, 1.0);
  float occlusion = 1.0;
  if ((flags & HAS_OCCLUSION) != 0) occlusion = 1.0 + occlusionStrength * (texture(occlusionMap, vUv).r - 1.0);

  vec3 v = normalize(eye - vWorld);
  vec3 n = normalize(vNormal);
  if (dot(n, n) < 0.5) n = normalize(cross(dFdx(vWorld), dFdy(vWorld)));
  if (dot(n, v) < 0.0) n = -n;
  if ((flags & HAS_NORMAL) != 0) {
    // glTF normal maps: +Y points towards decreasing v (up in the image).
    vec3 t = texture(normalMap, vUv).xyz * 2.0 - 1.0;
    t.xy *= vec2(normalScale, -normalScale);
    vec3 mapped = normalize(cotangentFrame(n, vWorld, vUv) * t);
    if (all(equal(mapped, mapped))) n = mapped;
  }

  vec3 diffuse = base.rgb * (1.0 - metallic);
  vec3 f0 = mix(vec3(0.04), base.rgb, metallic);
  // Hemisphere ambient; up is -Y in this frame.
  float up = 0.5 - 0.5 * n.y;
  vec3 ambient = mix(vec3(0.16, 0.15, 0.15), vec3(0.42, 0.43, 0.46), up);
  vec3 color = occlusion * ambient * (diffuse + f0 * 0.5);
  color += light(n, v, keyLight, vec3(1.9, 1.85, 1.75), diffuse, f0, roughness);
  color += light(n, v, fillLight, vec3(0.55, 0.6, 0.7), diffuse, f0, roughness);
  color += light(n, v, rimLight, vec3(0.7), diffuse, f0, roughness);
  // Gentle filmic shoulder, then sRGB-ish encode.
  color = color / (1.0 + 0.25 * color);
  fragColor = vec4(pow(clamp(color, 0.0, 1.0), vec3(1.0 / 2.2)), 1.0);
}`;

function compile(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type)!;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(shader);
    gl.deleteShader(shader);
    throw new Error(`Mesh shader failed to compile: ${log}`);
  }
  return shader;
}

/** Per-vertex normals from face normals, for meshes saved without them. */
export function vertexNormals(positions: Float32Array, indices: Uint32Array): Float32Array {
  const normals = new Float32Array(positions.length);
  for (let f = 0; f + 2 < indices.length; f += 3) {
    const a = indices[f] * 3, b = indices[f + 1] * 3, c = indices[f + 2] * 3;
    const ux = positions[b] - positions[a], uy = positions[b + 1] - positions[a + 1], uz = positions[b + 2] - positions[a + 2];
    const vx = positions[c] - positions[a], vy = positions[c + 1] - positions[a + 1], vz = positions[c + 2] - positions[a + 2];
    const nx = uy * vz - uz * vy, ny = uz * vx - ux * vz, nz = ux * vy - uy * vx;
    for (const i of [a, b, c]) { normals[i] += nx; normals[i + 1] += ny; normals[i + 2] += nz; }
  }
  for (let i = 0; i < normals.length; i += 3) {
    const length = Math.hypot(normals[i], normals[i + 1], normals[i + 2]) || 1;
    normals[i] /= length; normals[i + 1] /= length; normals[i + 2] /= length;
  }
  return normals;
}

interface DrawItem {
  vao: WebGLVertexArrayObject;
  buffers: WebGLBuffer[];
  count: number;
  hasUv: boolean;
  hasColor: boolean;
  material: MeshMaterial;
}

export class MeshRenderer {
  private gl: WebGL2RenderingContext;
  private program: WebGLProgram;
  private uniforms: Record<string, WebGLUniformLocation | null> = {};
  private attributes: Record<string, number> = {};
  private items: DrawItem[] = [];
  private textures = new Map<ImageBitmap, WebGLTexture>();
  private radius = 1;

  /** Kept for parity with SplatRenderer; meshes need no sort. */
  onsorted: (() => void) | null = null;

  constructor(private canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: true, depth: true, premultipliedAlpha: true, preserveDrawingBuffer: true });
    if (!gl) throw new Error("WebGL2 is not available.");
    this.gl = gl;
    const program = gl.createProgram()!;
    gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, VERTEX));
    gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, FRAGMENT));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      throw new Error(`Mesh shader failed to link: ${gl.getProgramInfoLog(program)}`);
    }
    this.program = program;
    for (const name of [
      "projection", "view", "baseMap", "ormMap", "occlusionMap", "normalMap", "baseFactor", "metallicFactor",
      "roughnessFactor", "occlusionStrength", "normalScale", "flags", "eye", "keyLight", "fillLight", "rimLight",
    ]) {
      this.uniforms[name] = gl.getUniformLocation(program, name);
    }
    for (const name of ["position", "normal", "uv", "color"]) this.attributes[name] = gl.getAttribLocation(program, name);
    gl.useProgram(program);
    gl.uniform1i(this.uniforms.baseMap, 0);
    gl.uniform1i(this.uniforms.ormMap, 1);
    gl.uniform1i(this.uniforms.occlusionMap, 2);
    gl.uniform1i(this.uniforms.normalMap, 3);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LEQUAL);
    gl.disable(gl.CULL_FACE);
    gl.disable(gl.BLEND);
  }

  private texture(source: MeshTexture | null, srgb: boolean): WebGLTexture | null {
    if (!source) return null;
    const cached = this.textures.get(source.image);
    if (cached) return cached;
    const gl = this.gl;
    const texture = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
    gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
    gl.texImage2D(gl.TEXTURE_2D, 0, srgb ? gl.SRGB8_ALPHA8 : gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, source.image);
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
    this.textures.set(source.image, texture);
    return texture;
  }

  private release() {
    const gl = this.gl;
    for (const item of this.items) {
      gl.deleteVertexArray(item.vao);
      for (const buffer of item.buffers) gl.deleteBuffer(buffer);
    }
    for (const texture of this.textures.values()) gl.deleteTexture(texture);
    this.items = [];
    this.textures.clear();
  }

  /** Upload a mesh (positions already in the viewer's frame). */
  setData(mesh: MeshData, radius: number) {
    const gl = this.gl;
    this.release();
    this.radius = radius;
    const attribute = (vao: WebGLVertexArrayObject, buffers: WebGLBuffer[], name: string, data: Float32Array | null, size: number) => {
      const location = this.attributes[name];
      if (location < 0 || !data) return false;
      gl.bindVertexArray(vao);
      const buffer = gl.createBuffer()!;
      buffers.push(buffer);
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
      gl.enableVertexAttribArray(location);
      gl.vertexAttribPointer(location, size, gl.FLOAT, false, 0, 0);
      return true;
    };
    for (const primitive of mesh.primitives) {
      const vao = gl.createVertexArray()!;
      const buffers: WebGLBuffer[] = [];
      attribute(vao, buffers, "position", primitive.positions, 3);
      attribute(vao, buffers, "normal", primitive.normals ?? vertexNormals(primitive.positions, primitive.indices), 3);
      const hasUv = attribute(vao, buffers, "uv", primitive.uvs, 2);
      const hasColor = attribute(vao, buffers, "color", primitive.colors, 4);
      gl.bindVertexArray(vao);
      const indexBuffer = gl.createBuffer()!;
      buffers.push(indexBuffer);
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer);
      gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, primitive.indices, gl.STATIC_DRAW);
      gl.bindVertexArray(null);
      this.items.push({ vao, buffers, count: primitive.indices.length, hasUv, hasColor, material: primitive.material });
      if (hasUv) {
        this.texture(primitive.material.baseColor, true);
        this.texture(primitive.material.metallicRoughness, false);
        this.texture(primitive.material.occlusion, false);
        this.texture(primitive.material.normal, false);
      }
    }
  }

  clear() {
    this.release();
    this.render(null);
  }

  render(camera: OrbitCamera | null) {
    const gl = this.gl;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const width = Math.max(1, Math.round(this.canvas.clientWidth * dpr));
    const height = Math.max(1, Math.round(this.canvas.clientHeight * dpr));
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width;
      this.canvas.height = height;
    }
    gl.viewport(0, 0, width, height);
    gl.clearColor(0, 0, 0, 0);
    gl.clearDepth(1);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    if (!camera || this.items.length === 0) return;

    const view = orbitView(camera);
    const right = [view[0], view[4], view[8]];
    const down = [view[1], view[5], view[9]];
    const fwd = [view[2], view[6], view[10]];
    const eye = camera.target.map((t, k) => t - camera.distance * fwd[k]);
    const direction = (r: number, d: number, f: number) => {
      const v = [0, 1, 2].map((k) => r * right[k] + d * down[k] + f * fwd[k]);
      const length = Math.hypot(v[0], v[1], v[2]) || 1;
      return new Float32Array(v.map((x) => x / length));
    };

    // Depth range around the model, so the 24-bit buffer keeps its precision.
    const near = Math.max(camera.distance - this.radius * 1.5, camera.distance * 0.01, 1e-4);
    const far = camera.distance + this.radius * 1.5;
    const focal = height / (2 * Math.tan(camera.fov / 2));
    const projection = new Float32Array([
      (2 * focal) / width, 0, 0, 0,
      0, -(2 * focal) / height, 0, 0,
      0, 0, (far + near) / (far - near), 1,
      0, 0, (-2 * far * near) / (far - near), 0,
    ]);

    gl.useProgram(this.program);
    gl.enable(gl.DEPTH_TEST);
    gl.uniformMatrix4fv(this.uniforms.projection, false, projection);
    gl.uniformMatrix4fv(this.uniforms.view, false, view);
    gl.uniform3fv(this.uniforms.eye, new Float32Array(eye));
    // Lights follow the camera: key from the upper left front, cool fill
    // from the right, rim from behind.
    gl.uniform3fv(this.uniforms.keyLight, direction(-0.55, -0.75, -0.6));
    gl.uniform3fv(this.uniforms.fillLight, direction(0.8, -0.1, -0.45));
    gl.uniform3fv(this.uniforms.rimLight, direction(0.2, -0.55, 0.8));

    for (const item of this.items) {
      const m = item.material;
      const base = item.hasUv ? this.textures.get(m.baseColor?.image as ImageBitmap) ?? null : null;
      const orm = item.hasUv ? this.textures.get(m.metallicRoughness?.image as ImageBitmap) ?? null : null;
      const occlusion = item.hasUv ? this.textures.get(m.occlusion?.image as ImageBitmap) ?? null : null;
      const normal = item.hasUv ? this.textures.get(m.normal?.image as ImageBitmap) ?? null : null;
      let flags = 0;
      for (const [unit, texture, bit] of [[0, base, 1], [1, orm, 2], [2, occlusion, 4], [3, normal, 8]] as const) {
        gl.activeTexture(gl.TEXTURE0 + unit);
        gl.bindTexture(gl.TEXTURE_2D, texture);
        if (texture) flags |= bit;
      }
      if (m.unlit) flags |= 16;
      gl.uniform1i(this.uniforms.flags, flags);
      gl.uniform4fv(this.uniforms.baseFactor, new Float32Array(m.baseColorFactor));
      gl.uniform1f(this.uniforms.metallicFactor, m.metallicFactor);
      gl.uniform1f(this.uniforms.roughnessFactor, m.roughnessFactor);
      gl.uniform1f(this.uniforms.occlusionStrength, m.occlusionStrength);
      gl.uniform1f(this.uniforms.normalScale, m.normalScale);
      gl.bindVertexArray(item.vao);
      // Constant attribute values (not VAO state) for what the mesh lacks.
      if (!item.hasUv && this.attributes.uv >= 0) gl.vertexAttrib2f(this.attributes.uv, 0, 0);
      if (!item.hasColor && this.attributes.color >= 0) gl.vertexAttrib4f(this.attributes.color, 1, 1, 1, 1);
      gl.drawElements(gl.TRIANGLES, item.count, gl.UNSIGNED_INT, 0);
    }
    gl.bindVertexArray(null);
  }

  dispose() {
    this.release();
    this.gl.deleteProgram(this.program);
  }
}
