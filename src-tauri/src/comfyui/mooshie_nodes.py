"""
MooshieUI custom nodes — lightweight face detection + in-memory image output.
Replaces the heavyweight Impact Pack dependency with a focused implementation.
"""

import io
import json
import re
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


class MooshieSegmentDetailer:
    """Detect a region by text (CLIPSeg) or YOLO model, re-denoise it with its
    own conditioning, and composite back using the (grown + blurred) detected
    mask — SwarmUI-style <segment:...> refinement."""

    CLIPSEG_REPO = "CIDAS/clipseg-rd64-refined"

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "image": ("IMAGE",),
                "model": ("MODEL",),
                "vae": ("VAE",),
                "positive": ("CONDITIONING",),
                "negative": ("CONDITIONING",),
                "detection": ("STRING", {"default": ""}),
                "seed": ("INT", {"default": 0, "min": 0, "max": 0xFFFFFFFFFFFFFFFF}),
                "steps": ("INT", {"default": 20, "min": 1, "max": 100}),
                "cfg": ("FLOAT", {"default": 7.0, "min": 0.0, "max": 100.0, "step": 0.1}),
                "sampler_name": (comfy.samplers.KSampler.SAMPLERS,),
                "scheduler": (comfy.samplers.KSampler.SCHEDULERS,),
                "denoise": ("FLOAT", {"default": 0.6, "min": 0.0, "max": 1.0, "step": 0.05}),
                "guide_size": ("INT", {"default": 512, "min": 64, "max": 2048, "step": 64}),
                "threshold": ("FLOAT", {"default": 0.5, "min": 0.0, "max": 1.0, "step": 0.05}),
                "mask_grow": ("INT", {"default": 16, "min": 0, "max": 256}),
                "mask_blur": ("INT", {"default": 8, "min": 0, "max": 64}),
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
        detection,
        seed,
        steps,
        cfg,
        sampler_name,
        scheduler,
        denoise,
        guide_size,
        threshold,
        mask_grow,
        mask_blur,
    ):
        detection = (detection or "").strip()
        if not detection:
            return (image,)

        B, H, W, C = image.shape
        result = image.clone()

        for b in range(B):
            frame = image[b].cpu().numpy()
            if np.isnan(frame).any():
                frame = np.nan_to_num(frame, nan=0.0)
            img_np = (frame * 255).astype(np.uint8)

            if detection.lower().startswith("yolo-"):
                mask = self._yolo_mask(detection[len("yolo-"):], img_np, H, W, threshold)
            else:
                mask = self._clipseg_mask(detection, img_np, H, W, threshold)

            if mask is None or (mask >= threshold).sum().item() < 16:
                print(f"[MooshieSegmentDetailer] No region found for '{detection}' (batch {b})")
                continue

            mask = mask.to(image.device)
            if mask_grow > 0:
                mask = torch.nn.functional.max_pool2d(
                    mask[None, None],
                    kernel_size=mask_grow * 2 + 1,
                    stride=1,
                    padding=mask_grow,
                )[0, 0]
            blurred = self._blur_mask(mask, mask_blur)

            ys, xs = torch.nonzero(blurred > 0.01, as_tuple=True)
            if ys.numel() == 0:
                print(f"[MooshieSegmentDetailer] Mask faded below blend threshold for '{detection}' (batch {b})")
                continue
            pad = 32
            cy1 = max(0, int(ys.min().item()) - pad)
            cy2 = min(H, int(ys.max().item()) + 1 + pad)
            cx1 = max(0, int(xs.min().item()) - pad)
            cx2 = min(W, int(xs.max().item()) + 1 + pad)
            crop_h, crop_w = cy2 - cy1, cx2 - cx1
            if crop_h < 8 or crop_w < 8:
                continue

            crop = result[b : b + 1, cy1:cy2, cx1:cx2, :].clone()

            scale = guide_size / max(crop_h, crop_w)
            new_h = max(8, round(crop_h * scale / 8) * 8)
            new_w = max(8, round(crop_w * scale / 8) * 8)
            resized = torch.nn.functional.interpolate(
                crop.permute(0, 3, 1, 2),
                size=(new_h, new_w),
                mode="bilinear",
                align_corners=False,
            ).permute(0, 2, 3, 1)

            latent = vae.encode(resized[:, :, :, :3])
            latent = comfy.sample.fix_empty_latent_channels(model, latent)

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

            decoded = vae.decode(samples)
            if decoded.ndim == 5:
                decoded = decoded.reshape(
                    -1, decoded.shape[-3], decoded.shape[-2], decoded.shape[-1]
                )

            back = torch.nn.functional.interpolate(
                decoded.permute(0, 3, 1, 2),
                size=(crop_h, crop_w),
                mode="bilinear",
                align_corners=False,
            ).permute(0, 2, 3, 1)

            # Composite with the blurred detected mask so irregular shapes
            # (eyes, hands) blend cleanly — not a rectangular feather.
            blend = blurred[cy1:cy2, cx1:cx2].unsqueeze(0).unsqueeze(-1)
            original_crop = result[b : b + 1, cy1:cy2, cx1:cx2, :]
            result[b : b + 1, cy1:cy2, cx1:cx2, :] = (
                back * blend + original_crop * (1 - blend)
            ).clamp(0, 1)

        return (result,)

    @staticmethod
    def _parse_yolo_name(name):
        """'model.pt-2' -> ('model.pt', 2); 'model.pt' -> ('model.pt', None)."""
        m = re.match(r"^(.+\.(?:pt|onnx))-(\d+)$", name, re.IGNORECASE)
        if m:
            return m.group(1), int(m.group(2))
        return name, None

    def _yolo_mask(self, name, img_np, H, W, threshold):
        """Union mask [H, W] float 0/1 from YOLO detections, or None."""
        from ultralytics import YOLO

        model_name, match_index = self._parse_yolo_name(name.strip())
        model_path = folder_paths.get_full_path("ultralytics", model_name)
        if model_path is None:
            print(f"[MooshieSegmentDetailer] YOLO model not found: {model_name}")
            return None

        yolo = YOLO(model_path)
        detections = yolo(img_np, verbose=False)
        if not detections or len(detections[0].boxes) == 0:
            return None

        boxes = detections[0].boxes
        seg_masks = detections[0].masks.data if detections[0].masks is not None else None

        # Confidence-sorted indices above threshold; -N selects the Nth best match.
        order = sorted(range(len(boxes)), key=lambda i: boxes.conf[i].item(), reverse=True)
        order = [i for i in order if boxes.conf[i].item() >= threshold]
        if not order:
            return None
        if match_index is not None:
            if match_index < 1 or match_index > len(order):
                return None
            order = [order[match_index - 1]]

        mask = torch.zeros((H, W), dtype=torch.float32)
        for i in order:
            if seg_masks is not None:
                m = torch.nn.functional.interpolate(
                    seg_masks[i][None, None].float().cpu(),
                    size=(H, W),
                    mode="bilinear",
                    align_corners=False,
                )[0, 0]
                mask = torch.maximum(mask, (m > 0.5).float())
            else:
                x1, y1, x2, y2 = boxes.xyxy[i].cpu().int().tolist()
                mask[max(0, y1) : min(H, y2), max(0, x1) : min(W, x2)] = 1.0
        return mask

    def _clipseg_mask(self, text, img_np, H, W, threshold):
        """Binary mask [H, W] from CLIPSeg text detection.

        Weights cache under models/clipseg/ (auto-downloaded on first use) and
        are released after each run — no persistent VRAM/RAM residency.
        """
        from transformers import CLIPSegProcessor, CLIPSegForImageSegmentation
        from PIL import Image as PILImage

        cache_dir = os.path.join(folder_paths.models_dir, "clipseg")
        os.makedirs(cache_dir, exist_ok=True)

        processor = CLIPSegProcessor.from_pretrained(self.CLIPSEG_REPO, cache_dir=cache_dir)
        seg_model = CLIPSegForImageSegmentation.from_pretrained(
            self.CLIPSEG_REPO, cache_dir=cache_dir
        )
        try:
            pil = PILImage.fromarray(img_np)
            inputs = processor(text=[text], images=[pil], return_tensors="pt")
            with torch.no_grad():
                logits = seg_model(**inputs).logits
            heat = torch.sigmoid(logits.float())
            if heat.ndim == 3:
                heat = heat[0]
            mask = torch.nn.functional.interpolate(
                heat[None, None], size=(H, W), mode="bilinear", align_corners=False
            )[0, 0]
            max_heat = mask.max().item()
            mean_heat = mask.mean().item()
            pixels_above = int((mask >= threshold).sum().item())
            print(
                f"[MooshieSegmentDetailer] CLIPSeg '{text}': "
                f"max={max_heat:.3f} mean={mean_heat:.3f} threshold={threshold:.3f} "
                f"pixels_above={pixels_above}"
            )
            # Return soft sigmoid values [0,1] — the threshold gates existence only;
            # soft values let both eyes (or any bilateral feature) blend proportionally
            # to confidence rather than the brighter one winning exclusively.
            return mask
        finally:
            del seg_model, processor

    @staticmethod
    def _blur_mask(mask, radius):
        """Approximate gaussian blur with 3 box blurs (replicate-padded avg_pool)."""
        if radius <= 0:
            return mask.clamp(0, 1)
        k = radius * 2 + 1
        m = mask[None, None]
        for _ in range(3):
            m = torch.nn.functional.avg_pool2d(
                torch.nn.functional.pad(m, (radius, radius, radius, radius), mode="replicate"),
                kernel_size=k,
                stride=1,
            )
        return m[0, 0].clamp(0, 1)


class MooshieSaveImage:
    """Output node that keeps images in RAM and sends them over WebSocket.

    Inspired by SwarmUI's approach — avoids the disk round-trip that ComfyUI's
    built-in SaveImage performs (write → re-read → HTTP serve → delete).
    Benefits: no drive I/O, lower latency, no data-leak from temp files on disk.
    """

    MOOSHIE_EVENT_TYPE = 100  # custom binary WS event type
    MOOSHIE_CONTROLNET_PREPROCESSOR_EVENT_TYPE = 101
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
                "output_role": (["final", "controlnet_preprocessor"], {"default": "final"}),
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

    def save_images(self, images, bit_depth="8bit", output_format="png", output_role="final"):
        from server import PromptServer

        server = PromptServer.instance
        want_raw = (output_format == "jxl_raw")
        event_type = self.MOOSHIE_CONTROLNET_PREPROCESSOR_EVENT_TYPE if output_role == "controlnet_preprocessor" else self.MOOSHIE_EVENT_TYPE

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
            server.send_sync(event_type, payload)

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
    def IS_CHANGED(cls, images, bit_depth="8bit", output_format="png", output_role="final"):
        # Always re-execute — output nodes should never be cached.
        return float("nan")


NODE_CLASS_MAPPINGS = {
    "MooshieFaceDetailer": MooshieFaceDetailer,
    "MooshieSegmentDetailer": MooshieSegmentDetailer,
    "MooshieSaveImage": MooshieSaveImage,
}

NODE_DISPLAY_NAME_MAPPINGS = {
    "MooshieFaceDetailer": "Mooshie Face Detailer",
    "MooshieSegmentDetailer": "Mooshie Segment Detailer",
    "MooshieSaveImage": "Mooshie Save Image",
}
