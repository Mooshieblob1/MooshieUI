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
import comfy.sd
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
                "max_faces": ("INT", {"default": 0, "min": 0, "max": 100}),
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
        max_faces=0,
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

            # Keep only boxes above threshold, then (when capped) refine the
            # most-confident faces first so max_faces drops the weakest detections.
            boxes = [box for box in detections[0].boxes if box.conf[0].item() >= bbox_threshold]
            if max_faces > 0 and len(boxes) > max_faces:
                boxes = sorted(boxes, key=lambda box: box.conf[0].item(), reverse=True)[:max_faces]

            for box in boxes:

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
    FMT_RAW_RGBA8_WEBP = 5  # 8-bit RGBA raw pixels, encoded to lossless WebP in Rust

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "images": ("IMAGE",),
            },
            "optional": {
                "bit_depth": (["8bit", "16bit"], {"default": "8bit"}),
                "output_format": (["png", "jxl_raw", "webp_raw"], {"default": "png"}),
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
        "in the Tauri backend when output_format=jxl_raw, or lossless WebP "
        "when output_format=webp_raw)."
    )

    def save_images(self, images, bit_depth="8bit", output_format="png", output_role="final"):
        from server import PromptServer

        server = PromptServer.instance
        # WebP is 8-bit only (the container has no 16-bit sample format), so the
        # raw payload is always packed at 8 bits regardless of the bit_depth input.
        want_webp = (output_format == "webp_raw")
        want_raw = want_webp or (output_format == "jxl_raw")
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

            if want_webp:
                _, image_bytes = self._encode_raw(frame, "8bit")
                fmt_tag = self.FMT_RAW_RGBA8_WEBP
            elif want_raw:
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


MOOSHIE_VIDEO_EVENT_TYPE = 102


class MooshieSaveVideo:
    """Save a VIDEO to ComfyUI's output directory and notify the Mooshie
    backend over the client WebSocket (binary event 102) with absolute file
    paths, so Rust can move the mp4 into the gallery without shuttling the
    encoded bytes through the socket. Also writes a poster WebP of frame 0
    next to the mp4 for thumbnail serving.
    """

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "video": ("VIDEO",),
            },
            "optional": {
                "filename_prefix": ("STRING", {"default": "mooshie_video"}),
                # SwarmUI-shaped JSON built by templates/video.rs. Optional so a
                # workflow from an older MooshieUI still validates.
                "metadata_json": ("STRING", {"default": "", "multiline": True}),
            },
        }

    RETURN_TYPES = ()
    OUTPUT_NODE = True
    FUNCTION = "save_video"
    CATEGORY = "mooshie"
    DESCRIPTION = "Saves the video as mp4 with a poster frame and notifies MooshieUI over WebSocket."

    def save_video(self, video, filename_prefix="mooshie_video", metadata_json=""):
        from PIL import Image
        from comfy_api.latest import Types
        from server import PromptServer

        # A raised exception in a node kills the whole prompt, so every step that
        # touches metadata is guarded and degrades to saving without it.
        params = None
        if metadata_json:
            try:
                parsed = json.loads(metadata_json)
                if isinstance(parsed, dict):
                    params = parsed
            except Exception:
                params = None

        width, height = video.get_dimensions()
        full_output_folder, filename, counter, _subfolder, _prefix = (
            folder_paths.get_save_image_path(
                filename_prefix, folder_paths.get_output_directory(), width, height
            )
        )
        video_file = f"{filename}_{counter:05}_.mp4"
        video_path = os.path.join(full_output_folder, video_file)
        # `metadata` values are json.dumps'd by save_to, so this has to be the
        # parsed dict: handing it the original string would double-encode it.
        # The key is `comment` because that is the one mdta key a remux without
        # `-movflags use_metadata_tags` still carries.
        try:
            if params is not None:
                video.save_to(
                    video_path,
                    format=Types.VideoContainer("mp4"),
                    codec="auto",
                    metadata={"comment": params},
                )
            else:
                video.save_to(
                    video_path, format=Types.VideoContainer("mp4"), codec="auto"
                )
        except Exception:
            video.save_to(video_path, format=Types.VideoContainer("mp4"), codec="auto")

        components = video.get_components()
        frames = components.images
        frame_count = int(frames.shape[0])
        fps = float(components.frame_rate)

        poster_path = os.path.splitext(video_path)[0] + "_poster.webp"
        frame0 = (255.0 * frames[0].cpu().numpy()).clip(0, 255).astype(np.uint8)
        poster = Image.fromarray(frame0[:, :, :3], "RGB")
        poster_kwargs = {"format": "WEBP", "quality": 90}
        if params is not None:
            try:
                # UserComment (0x9286) in the Exif sub-IFD (0x8769), the same
                # carrier the Rust WebP writer uses for still images.
                exif = Image.Exif()
                text = json.dumps(params, ensure_ascii=False)
                exif[0x8769] = {
                    0x9286: b"UNICODE\x00" + text.encode("utf-16-be")
                }
                poster_kwargs["exif"] = exif.tobytes()
            except Exception:
                pass
        try:
            poster.save(poster_path, **poster_kwargs)
        except Exception:
            # Same degradation as the mp4 path above: drop the metadata and
            # save the poster anyway rather than killing the prompt.
            poster_kwargs.pop("exif", None)
            poster.save(poster_path, **poster_kwargs)

        payload = json.dumps(
            {
                "video_path": os.path.abspath(video_path),
                "poster_path": os.path.abspath(poster_path),
                "fps": fps,
                "frame_count": frame_count,
                "width": int(width),
                "height": int(height),
            }
        ).encode("utf-8")
        PromptServer.instance.send_sync(MOOSHIE_VIDEO_EVENT_TYPE, payload)
        return {"ui": {"images": []}}

    @classmethod
    def IS_CHANGED(cls, video, filename_prefix="mooshie_video", metadata_json=""):
        # Always re-execute — output nodes should never be cached. Without this,
        # a regenerate with identical inputs (e.g. a pinned seed) cache-hits the
        # whole upstream chain: save_video() never runs, no new file or event is
        # produced, and the UI is left showing the previous video.
        return float("nan")


_MODEL_EXTENSIONS = (".safetensors", ".sft", ".ckpt", ".pt", ".pth", ".bin")


def _validate_model_path(node_name, path):
    """Resolve and sanity-check an absolute model path supplied as a STRING input.

    The stock loaders take a combo of filenames from one folder, so a model that
    physically sits in the "wrong" folder (a unet in models/checkpoints/, say) is
    rejected at /prompt validation time. MooshieUI detects the real model kind and
    passes an absolute path instead; the Tauri backend has already resolved it
    against ComfyUI's model roots, so here we only guard against typos and
    non-model files.
    """
    if not path or not path.strip():
        raise ValueError(f"{node_name}: empty model path")
    resolved = os.path.abspath(os.path.expanduser(path.strip()))
    if not os.path.isfile(resolved):
        raise ValueError(f"{node_name}: model file not found: {resolved}")
    if not resolved.lower().endswith(_MODEL_EXTENSIONS):
        raise ValueError(f"{node_name}: not a recognized model file: {resolved}")
    return resolved


class MooshieCheckpointLoaderPath:
    """CheckpointLoaderSimple that takes an absolute path instead of a folder combo."""

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "ckpt_path": ("STRING", {"default": "", "multiline": False}),
            }
        }

    RETURN_TYPES = ("MODEL", "CLIP", "VAE")
    FUNCTION = "load_checkpoint"
    CATEGORY = "mooshie"
    DESCRIPTION = (
        "Loads a full checkpoint (baked CLIP + VAE) from an absolute path, so a "
        "checkpoint stored outside models/checkpoints/ still loads."
    )

    def load_checkpoint(self, ckpt_path):
        path = _validate_model_path("MooshieCheckpointLoaderPath", ckpt_path)
        print(f"[MooshieCheckpointLoaderPath] loading {path}")
        out = comfy.sd.load_checkpoint_guess_config(
            path,
            output_vae=True,
            output_clip=True,
            embedding_directory=folder_paths.get_folder_paths("embeddings"),
        )
        return out[:3]


class MooshieDiffusionLoaderPath:
    """UNETLoader that takes an absolute path instead of a folder combo."""

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "unet_path": ("STRING", {"default": "", "multiline": False}),
            },
            "optional": {
                "weight_dtype": (
                    ["default", "fp8_e4m3fn", "fp8_e4m3fn_fast", "fp8_e5m2"],
                    {"default": "default"},
                ),
            },
        }

    RETURN_TYPES = ("MODEL",)
    FUNCTION = "load_unet"
    CATEGORY = "mooshie"
    DESCRIPTION = (
        "Loads a diffusion model (unet/DiT only, no baked CLIP or VAE) from an "
        "absolute path, so a split-file model stored outside "
        "models/diffusion_models/ still loads."
    )

    def load_unet(self, unet_path, weight_dtype="default"):
        path = _validate_model_path("MooshieDiffusionLoaderPath", unet_path)
        # Mirrors core UNETLoader's dtype handling.
        model_options = {}
        if weight_dtype == "fp8_e4m3fn":
            model_options["dtype"] = torch.float8_e4m3fn
        elif weight_dtype == "fp8_e4m3fn_fast":
            model_options["dtype"] = torch.float8_e4m3fn
            model_options["fp8_optimizations"] = True
        elif weight_dtype == "fp8_e5m2":
            model_options["dtype"] = torch.float8_e5m2

        print(f"[MooshieDiffusionLoaderPath] loading {path} (weight_dtype={weight_dtype})")
        model = comfy.sd.load_diffusion_model(path, model_options=model_options)
        return (model,)


class MooshieLoadVideoPath:
    """Decode an mp4 from an absolute path into frames plus audio.

    ComfyUI's stock loaders read a filename inside the input directory, but the
    gallery lives elsewhere and post-hoc interpolation has to re-open a clip
    that was already saved. The Rust side proves the path sits inside the
    caller's own gallery before it ever reaches here.

    `output_fps` is returned rather than assumed so the caller never has to
    guess the source rate: interpolating an already-interpolated 48 fps clip
    yields 96, not 48.
    """

    @classmethod
    def INPUT_TYPES(cls):
        return {
            "required": {
                "video_path": ("STRING", {"default": "", "multiline": False}),
                "fps_multiplier": ("INT", {"default": 2, "min": 1, "max": 8}),
            }
        }

    RETURN_TYPES = ("IMAGE", "AUDIO", "FLOAT")
    RETURN_NAMES = ("images", "audio", "output_fps")
    FUNCTION = "load_video"
    CATEGORY = "mooshie"
    DESCRIPTION = (
        "Loads an mp4 from an absolute path as frames plus audio, and reports "
        "the playback rate the interpolated result should use."
    )

    def load_video(self, video_path, fps_multiplier=2):
        # Imported lazily so a ComfyUI install without PyAV can still load the
        # rest of this module.
        import av

        path = (video_path or "").strip()
        if not path:
            raise ValueError("MooshieLoadVideoPath: empty video path")
        resolved = os.path.abspath(os.path.expanduser(path))
        if not os.path.isfile(resolved):
            raise ValueError(f"MooshieLoadVideoPath: file not found: {resolved}")

        frames = []
        source_fps = 24.0
        with av.open(resolved) as container:
            if not container.streams.video:
                raise ValueError(f"MooshieLoadVideoPath: no video stream in {resolved}")
            stream = container.streams.video[0]
            stream.thread_type = "AUTO"
            if stream.average_rate:
                source_fps = float(stream.average_rate)
            for frame in container.decode(stream):
                frames.append(frame.to_ndarray(format="rgb24"))

        if not frames:
            raise ValueError(f"MooshieLoadVideoPath: decoded zero frames from {resolved}")

        images = torch.from_numpy(np.stack(frames).astype(np.float32) / 255.0)
        audio = self._load_audio(resolved, len(frames) / source_fps)
        print(
            f"[MooshieLoadVideoPath] {len(frames)} frames at {source_fps:.3f} fps "
            f"from {resolved}"
        )
        return (images, audio, float(source_fps * fps_multiplier))

    @staticmethod
    def _load_audio(path, duration_seconds):
        """Decode the audio track, or synthesise silence of the same length.

        Returning silence rather than None lets the graph wire `audio`
        unconditionally: CreateVideo accepts a silent track, but a missing
        required link fails prompt validation outright.
        """
        import av

        sample_rate = 44100
        chunks = []
        try:
            with av.open(path) as container:
                if container.streams.audio:
                    stream = container.streams.audio[0]
                    sample_rate = int(stream.rate or sample_rate)
                    resampler = av.audio.resampler.AudioResampler(
                        format="fltp", layout="stereo", rate=sample_rate
                    )
                    for frame in container.decode(stream):
                        for resampled in resampler.resample(frame):
                            chunks.append(resampled.to_ndarray())
                    for resampled in resampler.resample(None):
                        chunks.append(resampled.to_ndarray())
        except Exception as exc:
            # A broken audio track must not lose the user's interpolated video.
            print(f"[MooshieLoadVideoPath] audio decode failed ({exc}), using silence")
            chunks = []

        if chunks:
            waveform = torch.from_numpy(np.concatenate(chunks, axis=1)).unsqueeze(0)
        else:
            samples = max(1, int(round(duration_seconds * sample_rate)))
            waveform = torch.zeros((1, 2, samples), dtype=torch.float32)
        return {"waveform": waveform, "sample_rate": sample_rate}

    @classmethod
    def IS_CHANGED(cls, video_path, fps_multiplier=2):
        # Re-run when the file on disk changes, not just when the path string
        # does, so re-interpolating an overwritten clip is not served stale.
        try:
            return os.path.getmtime(os.path.abspath(os.path.expanduser((video_path or "").strip())))
        except OSError:
            return float("nan")


NODE_CLASS_MAPPINGS = {
    "MooshieFaceDetailer": MooshieFaceDetailer,
    "MooshieSegmentDetailer": MooshieSegmentDetailer,
    "MooshieSaveImage": MooshieSaveImage,
    "MooshieCheckpointLoaderPath": MooshieCheckpointLoaderPath,
    "MooshieDiffusionLoaderPath": MooshieDiffusionLoaderPath,
    "MooshieSaveVideo": MooshieSaveVideo,
    "MooshieLoadVideoPath": MooshieLoadVideoPath,
}

NODE_DISPLAY_NAME_MAPPINGS = {
    "MooshieFaceDetailer": "Mooshie Face Detailer",
    "MooshieSegmentDetailer": "Mooshie Segment Detailer",
    "MooshieSaveImage": "Mooshie Save Image",
    "MooshieCheckpointLoaderPath": "Mooshie Checkpoint Loader (path)",
    "MooshieDiffusionLoaderPath": "Mooshie Diffusion Model Loader (path)",
    "MooshieSaveVideo": "Mooshie Save Video",
    "MooshieLoadVideoPath": "Mooshie Load Video (Path)",
}
