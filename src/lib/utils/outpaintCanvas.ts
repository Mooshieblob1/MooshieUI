export interface OutpaintPadding {
  top: number;
  bottom: number;
  left: number;
  right: number;
  feather: number;
}

export interface OutpaintGeometry {
  width: number;
  height: number;
  srcX: number;
  srcY: number;
  srcWidth: number;
  srcHeight: number;
  hasPadding: boolean;
}

export interface GeneratedOutpaintAssets {
  paddedBytes: number[];
  maskBytes: number[];
  paddedPreviewUrl: string;
  maskPreviewUrl: string;
  width: number;
  height: number;
}

export const OUTPAINT_MAX_PAD = 2048;
export const OUTPAINT_MAX_FEATHER = 512;

export function clampOutpaintPadding(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(OUTPAINT_MAX_PAD, Math.round(value)));
}

export function clampOutpaintFeather(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(OUTPAINT_MAX_FEATHER, Math.round(value)));
}

export function normalizeOutpaintPadding(pad: OutpaintPadding): OutpaintPadding {
  return {
    top: clampOutpaintPadding(pad.top),
    bottom: clampOutpaintPadding(pad.bottom),
    left: clampOutpaintPadding(pad.left),
    right: clampOutpaintPadding(pad.right),
    feather: clampOutpaintFeather(pad.feather),
  };
}

export function hasOutpaintPadding(pad: OutpaintPadding): boolean {
  const n = normalizeOutpaintPadding(pad);
  return n.top > 0 || n.bottom > 0 || n.left > 0 || n.right > 0;
}

export function computeOutpaintDimensions(
  srcWidth: number,
  srcHeight: number,
  pad: OutpaintPadding,
): OutpaintGeometry {
  const n = normalizeOutpaintPadding(pad);
  const safeSrcW = Math.max(1, Math.round(srcWidth));
  const safeSrcH = Math.max(1, Math.round(srcHeight));
  return {
    width: safeSrcW + n.left + n.right,
    height: safeSrcH + n.top + n.bottom,
    srcX: n.left,
    srcY: n.top,
    srcWidth: safeSrcW,
    srcHeight: safeSrcH,
    hasPadding: n.top > 0 || n.bottom > 0 || n.left > 0 || n.right > 0,
  };
}

/**
 * Compute mask intensity (0 = keep original, 255 = outpaint) at canvas pixel (x, y).
 * Mirrors ComfyUI's `ImagePadForOutpaint` mask feathering inward from any padded edge.
 */
export function computeOutpaintMaskValue(
  x: number,
  y: number,
  geom: OutpaintGeometry,
  pad: OutpaintPadding,
): number {
  const n = normalizeOutpaintPadding(pad);
  const inLeftPad = x < geom.srcX;
  const inRightPad = x >= geom.srcX + geom.srcWidth;
  const inTopPad = y < geom.srcY;
  const inBottomPad = y >= geom.srcY + geom.srcHeight;

  if (inLeftPad || inRightPad || inTopPad || inBottomPad) {
    return 255;
  }

  if (n.feather <= 0) return 0;

  let keep = 1;
  if (n.left > 0) {
    const dist = x - geom.srcX;
    if (dist < n.feather) keep = Math.min(keep, dist / n.feather);
  }
  if (n.right > 0) {
    const dist = geom.srcX + geom.srcWidth - 1 - x;
    if (dist < n.feather) keep = Math.min(keep, dist / n.feather);
  }
  if (n.top > 0) {
    const dist = y - geom.srcY;
    if (dist < n.feather) keep = Math.min(keep, dist / n.feather);
  }
  if (n.bottom > 0) {
    const dist = geom.srcY + geom.srcHeight - 1 - y;
    if (dist < n.feather) keep = Math.min(keep, dist / n.feather);
  }

  return Math.max(0, Math.min(255, Math.round((1 - keep) * 255)));
}

function loadImageElement(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("Failed to load outpaint source image"));
    img.src = url;
  });
}

function canvasToPngBlob(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (!blob) reject(new Error("Failed to encode outpaint canvas"));
      else resolve(blob);
    }, "image/png");
  });
}

/**
 * Automatically builds the edge-extended padded image PNG and feathered mask PNG
 * matching `ImagePadForOutpaint` (used for preview/NovelAI infill parity).
 */
export async function generateOutpaintImageAndMask(
  imageUrl: string,
  rawPad: OutpaintPadding,
): Promise<GeneratedOutpaintAssets> {
  const pad = normalizeOutpaintPadding(rawPad);
  const img = await loadImageElement(imageUrl);
  const geom = computeOutpaintDimensions(img.naturalWidth, img.naturalHeight, pad);

  const imageCanvas = document.createElement("canvas");
  imageCanvas.width = geom.width;
  imageCanvas.height = geom.height;
  const imgCtx = imageCanvas.getContext("2d");
  if (!imgCtx) throw new Error("Failed to create outpaint image context");

  // Neutral grey base, then clamp-extend the 1px edges outward like ImagePadForOutpaint
  imgCtx.fillStyle = "#808080";
  imgCtx.fillRect(0, 0, geom.width, geom.height);
  if (pad.left > 0) {
    imgCtx.drawImage(img, 0, 0, 1, geom.srcHeight, 0, geom.srcY, pad.left, geom.srcHeight);
  }
  if (pad.right > 0) {
    imgCtx.drawImage(
      img,
      geom.srcWidth - 1,
      0,
      1,
      geom.srcHeight,
      geom.srcX + geom.srcWidth,
      geom.srcY,
      pad.right,
      geom.srcHeight,
    );
  }
  if (pad.top > 0) {
    imgCtx.drawImage(img, 0, 0, geom.srcWidth, 1, geom.srcX, 0, geom.srcWidth, pad.top);
  }
  if (pad.bottom > 0) {
    imgCtx.drawImage(
      img,
      0,
      geom.srcHeight - 1,
      geom.srcWidth,
      1,
      geom.srcX,
      geom.srcY + geom.srcHeight,
      geom.srcWidth,
      pad.bottom,
    );
  }
  imgCtx.drawImage(img, geom.srcX, geom.srcY, geom.srcWidth, geom.srcHeight);

  const maskCanvas = document.createElement("canvas");
  maskCanvas.width = geom.width;
  maskCanvas.height = geom.height;
  const maskCtx = maskCanvas.getContext("2d");
  if (!maskCtx) throw new Error("Failed to create outpaint mask context");

  const maskData = maskCtx.createImageData(geom.width, geom.height);
  const pixels = maskData.data;
  for (let y = 0; y < geom.height; y++) {
    for (let x = 0; x < geom.width; x++) {
      const val = computeOutpaintMaskValue(x, y, geom, pad);
      const offset = (y * geom.width + x) * 4;
      pixels[offset] = val;
      pixels[offset + 1] = val;
      pixels[offset + 2] = val;
      pixels[offset + 3] = 255;
    }
  }
  maskCtx.putImageData(maskData, 0, 0);

  const [paddedBlob, maskBlob] = await Promise.all([
    canvasToPngBlob(imageCanvas),
    canvasToPngBlob(maskCanvas),
  ]);

  const [paddedBuf, maskBuf] = await Promise.all([
    paddedBlob.arrayBuffer(),
    maskBlob.arrayBuffer(),
  ]);

  return {
    paddedBytes: Array.from(new Uint8Array(paddedBuf)),
    maskBytes: Array.from(new Uint8Array(maskBuf)),
    paddedPreviewUrl: URL.createObjectURL(paddedBlob),
    maskPreviewUrl: URL.createObjectURL(maskBlob),
    width: geom.width,
    height: geom.height,
  };
}
