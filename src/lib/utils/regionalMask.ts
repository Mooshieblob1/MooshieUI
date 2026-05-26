import type { RegionalPromptSelection } from "../types/index.js";

/** Render a single region as a white-on-black PNG mask (ComfyUI red channel). */
export async function renderRegionMaskPngBytes(
  region: RegionalPromptSelection,
  width: number,
  height: number,
): Promise<number[]> {
  const w = Math.max(1, Math.round(width));
  const h = Math.max(1, Math.round(height));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Could not create mask canvas context");

  ctx.fillStyle = "#000000";
  ctx.fillRect(0, 0, w, h);
  ctx.fillStyle = "#ffffff";

  const x = region.x * w;
  const y = region.y * h;
  const rw = region.width * w;
  const rh = region.height * h;

  if (region.shape === "box") {
    ctx.fillRect(x, y, rw, rh);
  } else if (region.shape === "circle") {
    ctx.beginPath();
    ctx.ellipse(x + rw / 2, y + rh / 2, Math.max(1, rw / 2), Math.max(1, rh / 2), 0, 0, Math.PI * 2);
    ctx.fill();
  } else if (region.points && region.points.length >= 3) {
    ctx.beginPath();
    ctx.moveTo(region.points[0].x * w, region.points[0].y * h);
    for (let i = 1; i < region.points.length; i++) {
      ctx.lineTo(region.points[i].x * w, region.points[i].y * h);
    }
    ctx.closePath();
    ctx.fill();
  } else if (rw > 0 && rh > 0) {
    ctx.fillRect(x, y, rw, rh);
  }

  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob((b) => resolve(b), "image/png");
  });
  if (!blob) throw new Error("Failed to encode regional mask PNG");
  return Array.from(new Uint8Array(await blob.arrayBuffer()));
}

/** Map regional strength (0–2) to inpaint denoise. */
export function regionStrengthToDenoise(strength: number): number {
  const s = Number.isFinite(strength) ? Math.max(0, Math.min(2, strength)) : 0.75;
  return Math.min(0.92, Math.max(0.38, 0.38 + s * 0.27));
}
