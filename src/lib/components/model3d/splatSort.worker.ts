/// <reference lib="webworker" />
/**
 * Depth sort for the splat viewer, off the main thread. Gaussians are drawn
 * front to back, so each new camera needs a fresh order. A 16-bit counting
 * sort keeps a million Gaussians well under a frame's budget.
 */

let positions: Float32Array | null = null;
let count = 0;
let lastKey = "";

type Message =
  | { type: "data"; positions: Float32Array; count: number }
  | { type: "sort"; view: Float32Array; id: number };

self.onmessage = (event: MessageEvent<Message>) => {
  const msg = event.data;
  if (msg.type === "data") {
    positions = msg.positions;
    count = msg.count;
    lastKey = "";
    return;
  }
  if (msg.type !== "sort" || !positions) return;
  const v = msg.view;
  // Only the view direction and translation decide the order.
  const key = [v[2], v[6], v[10], v[14]].map((n) => n.toFixed(4)).join(",");
  if (key === lastKey) return;
  lastKey = key;

  const depths = new Int32Array(count);
  let min = Infinity;
  let max = -Infinity;
  for (let i = 0; i < count; i++) {
    const d = (v[2] * positions[i * 3] + v[6] * positions[i * 3 + 1] + v[10] * positions[i * 3 + 2]) * 4096 | 0;
    depths[i] = d;
    if (d < min) min = d;
    if (d > max) max = d;
  }
  const buckets = 256 * 256;
  const scale = (buckets - 1) / Math.max(1, max - min);
  const counts = new Uint32Array(buckets);
  for (let i = 0; i < count; i++) {
    const b = ((depths[i] - min) * scale) | 0;
    depths[i] = b;
    counts[b]++;
  }
  const starts = new Uint32Array(buckets);
  for (let i = 1; i < buckets; i++) starts[i] = starts[i - 1] + counts[i - 1];
  // Nearest first: smaller camera-space z is closer.
  const order = new Int32Array(count);
  for (let i = 0; i < count; i++) order[starts[depths[i]]++] = i;
  (self as unknown as Worker).postMessage({ order, id: msg.id }, [order.buffer]);
};
