"""
MooshieUI custom nodes — lightweight face detection + in-memory image output.
Replaces the heavyweight Impact Pack dependency with a focused implementation.
"""

import io
import json
import struct
import torch
import numpy as np

import comfy.sample
import comfy.samplers
import comfy.utils
import comfy.model_management
import folder_paths
import latent_preview
import os

# Register the "ultralytics" model folder if not already known to ComfyUI.
# Models go into ComfyUI/models/ultralytics/ (e.g. face_yolov8m.pt).
_ultralytics_dir = os.path.join(folder_paths.models_dir, "ultralytics")
os.makedirs(_ultralytics_dir, exist_ok=True)
folder_paths.add_model_folder_path("ultralytics", _ultralytics_dir)


class MooshieFaceDetailer:
    """Detect faces with YOLOv8, crop each to guide_size, re-denoise, composite back."""

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "image": ("IMAGE",),
                "model": ("MODEL",),
                "vae": ("VAE",),
                "positive": ("CONDITIONING",),
                "negative": ("CONDITIONING",),
                "detector_model": (folder_paths.get_filename_list("ultralytics"),),
                "seed": ("INT", {"default": 0, "min": 0, "max": 0xFFFFFFFFFFFFFFFF}),
                "steps": ("INT", {"default": 20, "min": 1, "max": 100}),
                "cfg": ("FLOAT", {"default": 7.0, "min": 0.0, "max": 100.0, "step": 0.1}),
                "sampler_name": (comfy.samplers.KSampler.SAMPLERS,),
                "scheduler": (comfy.samplers.KSampler.SCHEDULERS,),
                "denoise": ("FLOAT", {"default": 0.4, "min": 0.0, "max": 1.0, "step": 0.05}),
                "guide_size": ("INT", {"default": 512, "min": 64, "max": 2048, "step": 64}),
                "bbox_threshold": ("FLOAT", {"default": 0.5, "min": 0.0, "max": 1.0, "step": 0.05}),
                "bbox_padding": ("FLOAT", {"default": 1.5, "min": 1.0, "max": 4.0, "step": 0.1}),
                "feather": ("INT", {"default": 20, "min": 0, "max": 100}),
            }
        }

    RETURN_TYPES = ("IMAGE",)
    FUNCTION = "process"
    CATEGORY = "mooshie"

    def process(
        self,
        image,
        model,
        vae,
        positive,
        negative,
        detector_model,
        seed,
        steps,
        cfg,
        sampler_name,
        scheduler,
        denoise,
        guide_size,
        bbox_threshold,
        bbox_padding,
        feather,
    ):
        from ultralytics import YOLO

        model_path = folder_paths.get_full_path("ultralytics", detector_model)
        if model_path is None:
            print(f"[MooshieFaceDetailer] Model not found: {detector_model}")
            return (image,)

        yolo = YOLO(model_path)

        B, H, W, C = image.shape
        result = image.clone()

        for b in range(B):
            frame = image[b].cpu().numpy()
            if np.isnan(frame).any():
                print(f"[MooshieFaceDetailer] WARNING: NaN values detected in input image batch {b}, replacing with zeros")
                frame = np.nan_to_num(frame, nan=0.0)
            img_np = (frame * 255).astype(np.uint8)

            detections = yolo(img_np, verbose=False)
            if not detections or len(detections[0].boxes) == 0:
                continue

            for box in detections[0].boxes:
                conf = box.conf[0].item()
                if conf < bbox_threshold:
                    continue

                x1, y1, x2, y2 = box.xyxy[0].cpu().int().tolist()

                # Expand bbox with padding factor
                bw, bh = x2 - x1, y2 - y1
                cx, cy = (x1 + x2) / 2, (y1 + y2) / 2
                size = max(bw, bh) * bbox_padding

                cx1 = max(0, int(cx - size / 2))
                cy1 = max(0, int(cy - size / 2))
                cx2 = min(W, int(cx + size / 2))
                cy2 = min(H, int(cy + size / 2))

                crop_h = cy2 - cy1
                crop_w = cx2 - cx1
                if crop_h < 8 or crop_w < 8:
                    continue

                # Crop from current result
                crop = result[b : b + 1, cy1:cy2, cx1:cx2, :].clone()

                # Resize to guide_size (maintain aspect, round to 8 for VAE)
                scale = guide_size / max(crop_h, crop_w)
                new_h = max(8, round(crop_h * scale / 8) * 8)
                new_w = max(8, round(crop_w * scale / 8) * 8)

                resized = torch.nn.functional.interpolate(
                    crop.permute(0, 3, 1, 2),
                    size=(new_h, new_w),
                    mode="bilinear",
                    align_corners=False,
                ).permute(0, 2, 3, 1)

                # Create feathered mask at original crop resolution for pixel-space blending.
                # Use a generous feather proportional to the crop size for seamless edges.
                pixel_feather = max(feather, min(crop_h, crop_w) // 6)
                mask = self._make_feathered_mask(crop_h, crop_w, pixel_feather, image.device)

                # VAE encode
                latent = vae.encode(resized[:, :, :, :3])
                latent = comfy.sample.fix_empty_latent_channels(model, latent)

                # Sample — no noise_mask so the entire crop is denoised uniformly.
                # The pixel-space feathered blend handles the transition to the original.
                noise = comfy.sample.prepare_noise(latent, seed + b)
                callback = latent_preview.prepare_callback(model, steps)
                samples = comfy.sample.sample(
                    model,
                    noise,
                    steps,
                    cfg,
                    sampler_name,
                    scheduler,
                    positive,
                    negative,
                    latent,
                    denoise=denoise,
                    force_full_denoise=True,
                    callback=callback,
                    disable_pbar=False,
                    seed=seed + b,
                )

                # VAE decode
                decoded = vae.decode(samples)
                # Video VAEs (WanVAE etc.) return 5D [B,T,H,W,C] — flatten to 4D
                if decoded.ndim == 5:
                    decoded = decoded.reshape(
                        -1, decoded.shape[-3], decoded.shape[-2], decoded.shape[-1]
                    )

                # Resize back to original crop size
                back = torch.nn.functional.interpolate(
                    decoded.permute(0, 3, 1, 2),
                    size=(crop_h, crop_w),
                    mode="bilinear",
                    align_corners=False,
                ).permute(0, 2, 3, 1)

                # Blend mask is already at original crop resolution
                blend_mask = mask.unsqueeze(0).unsqueeze(-1)  # [1, H, W, 1]

                # Composite: denoised * mask + original * (1 - mask)
                original_crop = result[b : b + 1, cy1:cy2, cx1:cx2, :]
                blended = back * blend_mask + original_crop * (1 - blend_mask)
                result[b : b + 1, cy1:cy2, cx1:cx2, :] = blended.clamp(0, 1)

        return (result,)

    @staticmethod
    def _make_feathered_mask(h, w, feather, device):
        """Create a mask that's 1.0 in the center and smoothly fades to 0.0 at the edges.

        Uses a cosine falloff for each edge, then takes the product of all four
        edges.  This produces smooth, artifact-free transitions — much better
        than a linear ramp whose corners darken non-uniformly.
        """
        if feather <= 0:
            return torch.ones((h, w), dtype=torch.float32, device=device)

        f = min(feather, min(h, w) // 3)
        if f <= 0:
            return torch.ones((h, w), dtype=torch.float32, device=device)

        # Build 1-D cosine ramps: 0 at edge → 1 at f pixels in
        ramp = 0.5 * (1.0 - torch.cos(torch.linspace(0, torch.pi, f, device=device)))

        # Vertical mask: ramp on top/bottom, 1 in the middle
        v = torch.ones(h, dtype=torch.float32, device=device)
        v[:f] = ramp
        v[-f:] = ramp.flip(0)

        # Horizontal mask: ramp on left/right, 1 in the middle
        u = torch.ones(w, dtype=torch.float32, device=device)
        u[:f] = ramp[:min(f, w)]
        u[-f:] = ramp[:min(f, w)].flip(0)

        # Outer product gives smooth 2-D mask (corners blend naturally)
        mask = v.unsqueeze(1) * u.unsqueeze(0)
        return mask


class MooshieSaveImage:
    """Output node that keeps images in RAM and sends them over WebSocket.

    Inspired by SwarmUI's approach — avoids the disk round-trip that ComfyUI's
    built-in SaveImage performs (write → re-read → HTTP serve → delete).
    Benefits: no drive I/O, lower latency, no data-leak from temp files on disk.
    """

    MOOSHIE_EVENT_TYPE = 100  # custom binary WS event type
    # Format sub-types packed into the first 4 bytes after the event type header.
    # The Rust WebSocket handler reads this to tell the frontend what it received.
    FMT_PNG_8 = 1        # 8-bit PNG  (uint8,  standard)
    FMT_PNG_16 = 2       # 16-bit PNG (uint16, higher precision for post-processing)
    FMT_RAW_RGBA8 = 3    # 8-bit RGBA raw pixels  + 8-byte geometry header
    FMT_RAW_RGBA16 = 4   # 16-bit RGBA raw pixels + 8-byte geometry header (native endian)

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "images": ("IMAGE",),
            },
            "optional": {
                "bit_depth": (["8bit", "16bit"], {"default": "8bit"}),
                "output_format": (["png", "jxl_raw"], {"default": "png"}),
            },
        }

    RETURN_TYPES = ()
    OUTPUT_NODE = True
    FUNCTION = "save_images"
    CATEGORY = "mooshie"
    DESCRIPTION = (
        "Sends images directly over WebSocket instead of writing to disk. "
        "Supports 8/16-bit PNG (default) and raw RGBA (encoded to JPEG XL "
        "in the Tauri backend when output_format=jxl_raw)."
    )

    def save_images(self, images, bit_depth="8bit", output_format="png"):
        from server import PromptServer

        server = PromptServer.instance
        want_raw = (output_format == "jxl_raw")

        for i in range(images.shape[0]):
            frame = images[i].cpu().numpy()
            if np.isnan(frame).any():
                print(f"[MooshieSaveImage] WARNING: NaN values in output image {i} — VAE may have failed (VRAM pressure?). Replacing NaN with black.")
                frame = np.nan_to_num(frame, nan=0.0)
                images[i] = torch.from_numpy(frame).to(images.device)

            # Detect all-black output — common after VRAM corruption from rapid
            # interrupts on Blackwell GPUs with cudaMallocAsync.
            if frame.max() < 1e-6:
                print(f"[MooshieSaveImage] WARNING: Output image {i} is all-black (max pixel={frame.max():.2e}). "
                      "This usually means VRAM was corrupted by rapid generation interrupts. "
                      "Try generating again — the models will be reloaded cleanly.")

            if want_raw:
                fmt_tag, image_bytes = self._encode_raw(frame, bit_depth)
            elif bit_depth == "16bit":
                fmt_tag = self.FMT_PNG_16
                image_bytes = self._encode_16bit(images[i])
            else:
                fmt_tag = self.FMT_PNG_8
                image_bytes = self._encode_png_8bit(frame)

            # Payload: format_tag (4 bytes BE) + image data
            payload = struct.pack(">I", fmt_tag) + image_bytes
            server.send_sync(self.MOOSHIE_EVENT_TYPE, payload)

        return {"ui": {"images": []}}

    @staticmethod
    def _encode_png_8bit(frame):
        from PIL import Image

        img_np = (255.0 * frame).clip(0, 255).astype(np.uint8)
        # Output RGBA (alpha=255) so the PNG has an alpha channel.
        h, w, _ = img_np.shape
        rgba = np.full((h, w, 4), 255, dtype=np.uint8)
        rgba[:, :, :3] = img_np[:, :, :3]
        img = Image.fromarray(rgba, "RGBA")
        buf = io.BytesIO()
        img.save(buf, format="PNG")
        return buf.getvalue()

    @classmethod
    def _encode_raw(cls, frame, bit_depth):
        """Pack raw RGBA pixels (no compression) for JXL encoding in Rust.

        Header (8 bytes, big-endian fixed layout):
            width   u16
            height  u16
            channels u8   (always 4 — RGBA)
            depth    u8   (8 or 16)
            reserved u16  (zero)

        Payload: tightly packed RGBA bytes, row-major. 16-bit samples are
        native-endian u16 pairs (matches `zune-jpegxl`'s expected layout).
        """
        h, w, _ = frame.shape
        if w > 0xFFFF or h > 0xFFFF:
            raise ValueError(
                f"MooshieSaveImage raw path only supports <=65535 px per side, got {w}x{h}"
            )

        if bit_depth == "16bit":
            fmt_tag = cls.FMT_RAW_RGBA16
            rgb_u16 = (65535.0 * frame).clip(0, 65535).astype(np.uint16)
            rgba = np.full((h, w, 4), 0xFFFF, dtype=np.uint16)
            rgba[:, :, :3] = rgb_u16[:, :, :3]
            depth = 16
            pixels = rgba.tobytes()  # native endian, matches zune-jpegxl 16-bit input
        else:
            fmt_tag = cls.FMT_RAW_RGBA8
            rgb_u8 = (255.0 * frame).clip(0, 255).astype(np.uint8)
            rgba = np.full((h, w, 4), 255, dtype=np.uint8)
            rgba[:, :, :3] = rgb_u8[:, :, :3]
            depth = 8
            pixels = rgba.tobytes()

        header = struct.pack(">HHBBH", w, h, 4, depth, 0)
        return fmt_tag, header + pixels

    @staticmethod
    def _encode_16bit(image_tensor):
        """Encode a float32 image tensor as a 16-bit RGB PNG.

        Uses OpenCV when available (fast, correct colour order).
        Falls back to a pure-Python PNG writer (zlib + struct) otherwise.
        """
        arr = np.nan_to_num(image_tensor.cpu().numpy(), nan=0.0)
        arr = (65535.0 * arr).clip(0, 65535).astype(np.uint16)

        try:
            import cv2
            # OpenCV expects BGR; our tensor is RGB
            bgr = cv2.cvtColor(arr, cv2.COLOR_RGB2BGR)
            ok, encoded = cv2.imencode(".png", bgr)
            if ok and encoded is not None:
                return encoded.tobytes()
        except ImportError:
            pass

        # Pure-Python fallback: write a valid 16-bit RGB PNG using zlib.
        # PIL cannot write 16-bit RGB, so we build the PNG manually.
        import zlib

        h, w, _ = arr.shape
        # Convert to big-endian (PNG stores 16-bit values as BE)
        arr_be = arr.astype(">u2")

        # Build raw image data: each row = filter_byte(0) + 6 bytes per pixel
        raw_rows = []
        for y in range(h):
            raw_rows.append(b"\x00")  # filter: none
            raw_rows.append(arr_be[y].tobytes())
        raw_data = b"".join(raw_rows)
        compressed = zlib.compress(raw_data)

        def _png_chunk(chunk_type, data):
            chunk = chunk_type + data
            crc = zlib.crc32(chunk) & 0xFFFFFFFF
            return struct.pack(">I", len(data)) + chunk + struct.pack(">I", crc)

        buf = io.BytesIO()
        buf.write(b"\x89PNG\r\n\x1a\n")  # PNG signature
        # IHDR: width, height, bit_depth=16, color_type=2 (RGB)
        ihdr_data = struct.pack(">IIBBBBB", w, h, 16, 2, 0, 0, 0)
        buf.write(_png_chunk(b"IHDR", ihdr_data))
        buf.write(_png_chunk(b"IDAT", compressed))
        buf.write(_png_chunk(b"IEND", b""))
        return buf.getvalue()

    @classmethod
    def IS_CHANGED(cls, images, bit_depth="8bit", output_format="png"):
        # Always re-execute — output nodes should never be cached.
        return float("nan")


NODE_CLASS_MAPPINGS = {
    "MooshieFaceDetailer": MooshieFaceDetailer,
    "MooshieSaveImage": MooshieSaveImage,
}

NODE_DISPLAY_NAME_MAPPINGS = {
    "MooshieFaceDetailer": "Mooshie Face Detailer",
    "MooshieSaveImage": "Mooshie Save Image",
}
