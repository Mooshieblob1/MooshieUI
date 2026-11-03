/**
 * A small WebGL2 Gaussian splat renderer for the 3D page.
 *
 * Each Gaussian is an instanced quad. Its centre, 3D covariance and colour
 * live in an RGBA32UI texture (two texels per Gaussian); the vertex shader
 * projects the covariance to screen space (EWA) and stretches the quad along
 * its eigenvectors, and the fragment shader draws the Gaussian falloff.
 * Instances are drawn front to back (order from `splatSort.worker.ts`) with
 * under-blending, so no depth buffer is needed.
 *
 * No three.js and no remote code: everything is bundled, so it runs under the
 * production CSP in the desktop app and in browser mode.
 */
import type { SplatData } from "../../utils/splatPly.js";

const VERTEX = `#version 300 es
precision highp float;
precision highp int;
uniform highp usampler2D u_texture;
uniform mat4 projection, view;
uniform vec2 focal;
uniform vec2 viewport;
in vec2 position;
in int index;
out vec4 vColor;
out vec2 vPosition;
void main () {
  uvec4 cen = texelFetch(u_texture, ivec2((uint(index) & 0x3ffu) << 1, uint(index) >> 10), 0);
  vec4 cam = view * vec4(uintBitsToFloat(cen.xyz), 1.0);
  vec4 pos2d = projection * cam;
  float clip = 1.2 * pos2d.w;
  if (pos2d.z < -clip || pos2d.x < -clip || pos2d.x > clip || pos2d.y < -clip || pos2d.y > clip) {
    gl_Position = vec4(0.0, 0.0, 2.0, 1.0);
    return;
  }
  uvec4 cov = texelFetch(u_texture, ivec2(((uint(index) & 0x3ffu) << 1) | 1u, uint(index) >> 10), 0);
  vec2 u1 = unpackHalf2x16(cov.x), u2 = unpackHalf2x16(cov.y), u3 = unpackHalf2x16(cov.z);
  mat3 Vrk = mat3(u1.x, u1.y, u2.x, u1.y, u2.y, u3.x, u2.x, u3.x, u3.y);
  mat3 J = mat3(
    focal.x / cam.z, 0.0, -(focal.x * cam.x) / (cam.z * cam.z),
    0.0, -focal.y / cam.z, (focal.y * cam.y) / (cam.z * cam.z),
    0.0, 0.0, 0.0
  );
  mat3 T = transpose(mat3(view)) * J;
  mat3 cov2d = transpose(T) * Vrk * T;
  float mid = (cov2d[0][0] + cov2d[1][1]) / 2.0;
  float radius = length(vec2((cov2d[0][0] - cov2d[1][1]) / 2.0, cov2d[0][1]));
  float lambda1 = mid + radius, lambda2 = mid - radius;
  if (lambda2 < 0.0) {
    gl_Position = vec4(0.0, 0.0, 2.0, 1.0);
    return;
  }
  vec2 diagonalVector = normalize(vec2(cov2d[0][1], lambda1 - cov2d[0][0]));
  vec2 majorAxis = min(sqrt(2.0 * lambda1), 1024.0) * diagonalVector;
  vec2 minorAxis = min(sqrt(2.0 * lambda2), 1024.0) * vec2(diagonalVector.y, -diagonalVector.x);
  vColor = clamp(pos2d.z / pos2d.w + 1.0, 0.0, 1.0)
    * vec4((cov.w) & 0xffu, (cov.w >> 8) & 0xffu, (cov.w >> 16) & 0xffu, (cov.w >> 24) & 0xffu) / 255.0;
  vPosition = position;
  vec2 vCenter = vec2(pos2d) / pos2d.w;
  gl_Position = vec4(vCenter + position.x * majorAxis / viewport + position.y * minorAxis / viewport, 0.0, 1.0);
}`;

const FRAGMENT = `#version 300 es
precision highp float;
in vec4 vColor;
in vec2 vPosition;
out vec4 fragColor;
void main () {
  float A = -dot(vPosition, vPosition);
  if (A < -4.0) discard;
  float B = exp(A) * vColor.a;
  fragColor = vec4(B * vColor.rgb, B);
}`;

/** Split an f32 into its IEEE half bits (for packHalf2x16-compatible storage). */
const f32 = new Float32Array(1);
const i32 = new Int32Array(f32.buffer);
function toHalf(value: number): number {
  f32[0] = value;
  const bits = i32[0];
  const sign = (bits >> 31) << 15;
  const exp = (bits >> 23) & 0xff;
  const frac = bits & 0x7fffff;
  if (exp === 0) return sign;
  if (exp === 0xff) return sign | 0x7c00 | (frac ? 0x200 : 0);
  const e = exp - 127 + 15;
  if (e >= 0x1f) return sign | 0x7c00;
  if (e <= 0) {
    if (e < -10) return sign;
    const m = (frac | 0x800000) >> (1 - e);
    return sign | ((m + 0x1000) >> 13);
  }
  return sign | (e << 10) | ((frac + 0x1000) >> 13);
}
const packHalf2x16 = (x: number, y: number) => (toHalf(x) | (toHalf(y) << 16)) >>> 0;

/** Pack centres, covariances and colours into the texture layout the shader reads. */
export function packSplatTexture(data: SplatData): { texdata: Uint32Array; width: number; height: number } {
  const width = 1024 * 2;
  const height = Math.max(1, Math.ceil((2 * data.count) / width));
  const texdata = new Uint32Array(width * height * 4);
  const texf = new Float32Array(texdata.buffer);
  const texu = new Uint8Array(texdata.buffer);
  for (let i = 0; i < data.count; i++) {
    texf[8 * i] = data.positions[3 * i];
    texf[8 * i + 1] = data.positions[3 * i + 1];
    texf[8 * i + 2] = data.positions[3 * i + 2];
    texu[4 * (8 * i + 7)] = data.colors[4 * i];
    texu[4 * (8 * i + 7) + 1] = data.colors[4 * i + 1];
    texu[4 * (8 * i + 7) + 2] = data.colors[4 * i + 2];
    texu[4 * (8 * i + 7) + 3] = data.colors[4 * i + 3];

    const sx = data.scales[3 * i], sy = data.scales[3 * i + 1], sz = data.scales[3 * i + 2];
    const w = data.rotations[4 * i], x = data.rotations[4 * i + 1], y = data.rotations[4 * i + 2], z = data.rotations[4 * i + 3];
    // Rotation matrix rows from the quaternion (w x y z).
    const r00 = 1 - 2 * (y * y + z * z), r01 = 2 * (x * y - w * z), r02 = 2 * (x * z + w * y);
    const r10 = 2 * (x * y + w * z), r11 = 1 - 2 * (x * x + z * z), r12 = 2 * (y * z - w * x);
    const r20 = 2 * (x * z - w * y), r21 = 2 * (y * z + w * x), r22 = 1 - 2 * (x * x + y * y);
    // Sigma = R S S R^T.
    const a = sx * sx, b = sy * sy, c = sz * sz;
    const s00 = r00 * r00 * a + r01 * r01 * b + r02 * r02 * c;
    const s01 = r00 * r10 * a + r01 * r11 * b + r02 * r12 * c;
    const s02 = r00 * r20 * a + r01 * r21 * b + r02 * r22 * c;
    const s11 = r10 * r10 * a + r11 * r11 * b + r12 * r12 * c;
    const s12 = r10 * r20 * a + r11 * r21 * b + r12 * r22 * c;
    const s22 = r20 * r20 * a + r21 * r21 * b + r22 * r22 * c;
    texdata[8 * i + 4] = packHalf2x16(4 * s00, 4 * s01);
    texdata[8 * i + 5] = packHalf2x16(4 * s02, 4 * s11);
    texdata[8 * i + 6] = packHalf2x16(4 * s12, 4 * s22);
  }
  return { texdata, width, height };
}

export interface OrbitCamera {
  yaw: number;
  pitch: number;
  distance: number;
  target: [number, number, number];
  fov: number;
}

/**
 * World-to-camera matrix (column-major) for an orbit camera in the splat's
 * frame, where up is -Y and yaw 0 looks along +Z.
 */
export function orbitView(cam: OrbitCamera): Float32Array {
  const cp = Math.cos(cam.pitch), sp = Math.sin(cam.pitch);
  const cy = Math.cos(cam.yaw), sy = Math.sin(cam.yaw);
  const fwd = [-cp * sy, sp, cp * cy];
  const eye = [cam.target[0] - cam.distance * fwd[0], cam.target[1] - cam.distance * fwd[1], cam.target[2] - cam.distance * fwd[2]];
  // right = down x fwd with down = +Y; then down' = fwd x right.
  let right = [fwd[2], 0, -fwd[0]];
  const rn = Math.hypot(right[0], right[1], right[2]) || 1;
  right = right.map((v) => v / rn);
  const down = [
    fwd[1] * right[2] - fwd[2] * right[1],
    fwd[2] * right[0] - fwd[0] * right[2],
    fwd[0] * right[1] - fwd[1] * right[0],
  ];
  const dot = (a: number[], b: number[]) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  return new Float32Array([
    right[0], down[0], fwd[0], 0,
    right[1], down[1], fwd[1], 0,
    right[2], down[2], fwd[2], 0,
    -dot(right, eye), -dot(down, eye), -dot(fwd, eye), 1,
  ]);
}

function projectionMatrix(fx: number, fy: number, width: number, height: number, near: number, far: number): Float32Array {
  return new Float32Array([
    (2 * fx) / width, 0, 0, 0,
    0, -(2 * fy) / height, 0, 0,
    0, 0, far / (far - near), 1,
    0, 0, -(far * near) / (far - near), 0,
  ]);
}

function compile(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type)!;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(shader);
    gl.deleteShader(shader);
    throw new Error(`Splat shader failed to compile: ${log}`);
  }
  return shader;
}

export class SplatRenderer {
  private gl: WebGL2RenderingContext;
  private program: WebGLProgram;
  private texture: WebGLTexture;
  private indexBuffer: WebGLBuffer;
  private vao: WebGLVertexArrayObject;
  private uniforms: Record<string, WebGLUniformLocation | null> = {};
  private worker: Worker;
  private instances = 0;
  /** Bumped per upload so a late order for the previous splat is dropped. */
  private version = 0;
  private near = 0.01;
  private far = 100;

  /** Called when a new draw order arrives, so the owner can redraw. */
  onsorted: (() => void) | null = null;

  constructor(private canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2", { antialias: false, premultipliedAlpha: true, preserveDrawingBuffer: true });
    if (!gl) throw new Error("WebGL2 is not available.");
    this.gl = gl;
    const program = gl.createProgram()!;
    gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, VERTEX));
    gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, FRAGMENT));
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      throw new Error(`Splat shader failed to link: ${gl.getProgramInfoLog(program)}`);
    }
    this.program = program;
    gl.useProgram(program);
    for (const name of ["projection", "view", "focal", "viewport", "u_texture"]) {
      this.uniforms[name] = gl.getUniformLocation(program, name);
    }

    this.vao = gl.createVertexArray()!;
    gl.bindVertexArray(this.vao);
    const quad = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, quad);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-2, -2, 2, -2, 2, 2, -2, 2]), gl.STATIC_DRAW);
    const position = gl.getAttribLocation(program, "position");
    gl.enableVertexAttribArray(position);
    gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);

    this.indexBuffer = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.indexBuffer);
    const index = gl.getAttribLocation(program, "index");
    gl.enableVertexAttribArray(index);
    gl.vertexAttribIPointer(index, 1, gl.INT, 0, 0);
    gl.vertexAttribDivisor(index, 1);

    this.texture = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.uniform1i(this.uniforms.u_texture, 0);

    gl.disable(gl.DEPTH_TEST);
    gl.enable(gl.BLEND);
    // Front-to-back "under" compositing of premultiplied colour.
    gl.blendFuncSeparate(gl.ONE_MINUS_DST_ALPHA, gl.ONE, gl.ONE_MINUS_DST_ALPHA, gl.ONE);
    gl.blendEquationSeparate(gl.FUNC_ADD, gl.FUNC_ADD);

    this.worker = new Worker(new URL("./splatSort.worker.ts", import.meta.url), { type: "module" });
    this.worker.onmessage = (event: MessageEvent<{ order: Int32Array; id: number }>) => {
      const { order, id } = event.data;
      if (id !== this.version) return;
      this.instances = order.length;
      gl.bindBuffer(gl.ARRAY_BUFFER, this.indexBuffer);
      gl.bufferData(gl.ARRAY_BUFFER, order, gl.DYNAMIC_DRAW);
      this.onsorted?.();
    };
  }

  /** Upload a splat. Draws nothing until the first sort comes back. */
  setData(data: SplatData, radius: number) {
    const gl = this.gl;
    const { texdata, width, height } = packSplatTexture(data);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32UI, width, height, 0, gl.RGBA_INTEGER, gl.UNSIGNED_INT, texdata);
    this.instances = 0;
    gl.bindBuffer(gl.ARRAY_BUFFER, this.indexBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Int32Array(0), gl.DYNAMIC_DRAW);
    this.near = Math.max(radius * 0.01, 1e-4);
    this.far = radius * 100;
    this.version++;
    const positions = data.positions.slice();
    this.worker.postMessage({ type: "data", positions, count: data.count }, [positions.buffer]);
  }

  clear() {
    this.version++;
    this.instances = 0;
    this.worker.postMessage({ type: "data", positions: new Float32Array(0), count: 0 });
    this.render(null);
  }

  /** Draw with the given camera; also asks the worker for a new order. */
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
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (!camera) return;
    const view = orbitView(camera);
    this.worker.postMessage({ type: "sort", view, id: this.version });
    if (this.instances === 0) return;
    const focal = height / (2 * Math.tan(camera.fov / 2));
    gl.useProgram(this.program);
    gl.bindVertexArray(this.vao);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.uniformMatrix4fv(this.uniforms.projection, false, projectionMatrix(focal, focal, width, height, this.near, this.far));
    gl.uniformMatrix4fv(this.uniforms.view, false, view);
    gl.uniform2fv(this.uniforms.focal, new Float32Array([focal, focal]));
    gl.uniform2fv(this.uniforms.viewport, new Float32Array([width, height]));
    gl.drawArraysInstanced(gl.TRIANGLE_FAN, 0, 4, this.instances);
  }

  dispose() {
    this.worker.terminate();
    const gl = this.gl;
    gl.deleteTexture(this.texture);
    gl.deleteBuffer(this.indexBuffer);
    gl.deleteVertexArray(this.vao);
    gl.deleteProgram(this.program);
  }
}
