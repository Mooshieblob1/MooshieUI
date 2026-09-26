"""Animated live previews for MiniMax H3 video sampling.

ComfyUI's own video previewer decodes only the first latent frame. In
first/last-frame mode that frame is the user's own keyframe, so the preview
barely changes. This node wraps the sampler's step callback instead: each
step's denoised estimate (video stream only) is decoded with the taeh3 tiny
autoencoder at preview resolution and sent as an animated WebP.

Transport reuses ComfyUI's PREVIEW_IMAGE binary event with image format code 3,
which MooshieUI reads as animated WebP (ComfyUI itself uses 1 for JPEG and
2 for PNG). MooshieUI queues prompts that contain this node with
`preview_method: "none"`, so ComfyUI's single-frame previews do not interleave.
"""
import io
import logging
import math
import struct
import threading

import torch

import comfy.model_management
import comfy.patcher_extension

PREVIEW_IMAGE_EVENT = 1  # protocol.BinaryEventTypes.PREVIEW_IMAGE
WEBP_FORMAT = 3
WRAPPER_KEY = "mooshie_h3_live_preview"
MODEL_FPS = 24.0
LATENT_SCALE = 16  # taeh3 upsamples each latent cell to 16x16 pixels


def video_part(x0):
    """The first batch item of the video stream; H3 carries video and audio nested."""
    if getattr(x0, "is_nested", False):
        x0 = x0.tensors[0]
    return x0[:1]


def preview_latent(video, max_side):
    """Area-shrink the latent so the decoded preview's long side is at most max_side."""
    height, width = video.shape[-2:]
    scale = max_side / (LATENT_SCALE * max(height, width))
    if scale >= 1.0:
        return video
    size = (video.shape[2], max(1, round(height * scale)), max(1, round(width * scale)))
    return torch.nn.functional.interpolate(video.float(), size=size, mode="area").to(video.dtype)


def frame_plan(count, fps, max_frames):
    """Evenly spaced frame indices and a per-frame duration that keeps real clip length."""
    wanted = max(1, min(max_frames, math.ceil(count * fps / MODEL_FPS), count))
    if wanted == 1:
        return [0], int(round(1000 * count / MODEL_FPS))
    indices = [round(i * (count - 1) / (wanted - 1)) for i in range(wanted)]
    return indices, max(1, int(round(1000 * count / MODEL_FPS / wanted)))


def decode_frames(vae, video, max_side, fps, max_frames):
    """Decode on the sampling thread; returns uint8 frames [N, H, W, 3] on CPU."""
    images = vae.decode(preview_latent(video, max_side))
    images = images.reshape(-1, *images.shape[-3:])
    indices, duration_ms = frame_plan(images.shape[0], fps, max_frames)
    frames = (images[indices].clamp(0, 1) * 255).round().to(torch.uint8).cpu().numpy()
    return frames, duration_ms


def encode_webp(frames, duration_ms, quality):
    from PIL import Image

    pictures = [Image.fromarray(frame) for frame in frames]
    buffer = io.BytesIO()
    pictures[0].save(
        buffer,
        format="WEBP",
        save_all=len(pictures) > 1,
        append_images=pictures[1:],
        duration=duration_ms,
        loop=0,
        quality=quality,
        method=0,
    )
    return buffer.getvalue()


def send_preview(webp):
    from server import PromptServer

    server = PromptServer.instance
    server.send_sync(PREVIEW_IMAGE_EVENT, struct.pack(">I", WEBP_FORMAT) + webp, server.client_id)


class LivePreviewSender:
    """Encodes and sends on one worker thread so sampling never waits on WebP.

    `busy` lets the sampling thread skip decoding a step while the previous
    preview is still encoding, so a slow encode costs no GPU time and stale
    previews never queue up.
    """

    def __init__(self, quality, send=send_preview):
        self.quality = quality
        self.send = send
        self._pending = None
        self._closed = False
        self._working = False
        self._cond = threading.Condition()
        self._thread = threading.Thread(target=self._run, name="mooshie-h3-preview", daemon=True)
        self._thread.start()

    @property
    def busy(self):
        with self._cond:
            return self._working or self._pending is not None

    def submit(self, frames, duration_ms):
        with self._cond:
            if self._closed:
                return
            self._pending = (frames, duration_ms)
            self._cond.notify()

    def close(self, timeout=5.0):
        with self._cond:
            self._closed = True
            self._pending = None
            self._cond.notify()
        self._thread.join(timeout)

    def _run(self):
        while True:
            with self._cond:
                while self._pending is None and not self._closed:
                    self._cond.wait()
                if self._closed:
                    return
                frames, duration_ms = self._pending
                self._pending = None
                self._working = True
            try:
                self.send(encode_webp(frames, duration_ms, self.quality))
            except Exception as error:
                logging.warning("[MooshieH3LivePreview] preview not sent: %s", error)
            finally:
                with self._cond:
                    self._working = False


def make_wrapper(vae, max_side, fps, max_frames, quality, send=send_preview):
    def outer_sample(executor, noise, latent_image, sampler, sigmas, denoise_mask=None,
                     callback=None, disable_pbar=False, seed=None, latent_shapes=None):
        sender = LivePreviewSender(quality, send)
        warned = []

        def step(index, x0, x, total_steps):
            # Progress and the x0 hand-off must happen whatever the preview does.
            if callback is not None:
                callback(index, x0, x, total_steps)
            # The finished clip replaces the preview right after the last step,
            # and a skipped step costs nothing while the encoder is busy.
            if index >= total_steps - 1 or sender.busy:
                return
            try:
                frames, duration_ms = decode_frames(vae, video_part(x0), max_side, fps, max_frames)
                sender.submit(frames, duration_ms)
            except Exception as error:
                if not warned:
                    warned.append(True)
                    logging.warning("[MooshieH3LivePreview] preview decode failed: %s", error)

        try:
            return executor(noise, latent_image, sampler, sigmas, denoise_mask, step,
                            disable_pbar, seed, latent_shapes=latent_shapes)
        finally:
            sender.close()
            # The decode grows the allocator pool; hand it back before the
            # model's own decode and the next prompt.
            comfy.model_management.soft_empty_cache()

    return outer_sample


class MooshieH3LivePreview:
    """Attach animated taeh3 live previews to a MiniMax H3 model."""

    @classmethod
    def INPUT_TYPES(cls):
        return {"required": {
            "model": ("MODEL",),
            "vae": ("VAE", {"tooltip": "taeh3 from models/vae_approx."}),
            "max_side": ("INT", {"default": 384, "min": 64, "max": 1024, "step": 16}),
            "fps": ("INT", {"default": 12, "min": 1, "max": 24}),
            "max_frames": ("INT", {"default": 72, "min": 1, "max": 240}),
            "quality": ("INT", {"default": 60, "min": 1, "max": 100}),
        }}

    RETURN_TYPES = ("MODEL",)
    FUNCTION = "apply"
    CATEGORY = "mooshie/video"

    def apply(self, model, vae, max_side, fps, max_frames, quality):
        patched = model.clone()
        patched.add_wrapper_with_key(
            comfy.patcher_extension.WrappersMP.OUTER_SAMPLE,
            WRAPPER_KEY,
            make_wrapper(vae, max_side, fps, max_frames, quality),
        )
        return (patched,)


NODE_CLASS_MAPPINGS = {"MooshieH3LivePreview": MooshieH3LivePreview}
