# Vendored into MooshieUI from ComfyUI-MiniMaxH3-Director v0.1.5 (GPL-3.0).
# Upstream: https://github.com/seesee75-commits/ComfyUI-MiniMaxH3-Director
# See LICENSE in this directory for the full GPL-3.0 text.
#
# Modifications for MooshieUI, 2026:
#   * node_id renamed to "MooshieH3RetakeStitch" to avoid colliding with the
#     upstream pack.
"""Splice a retake back into its base video.

The Director's Retake Mode regenerates only the marked range, anchored on the base
video's own frames either side of it. This node puts the result back: head from the
base video, the freshly generated middle, then the tail — video and audio both, all
resampled onto H3's 24 fps output grid.

It is driven entirely by the Director's `retake_info` output, so the range can never
fall out of sync with what was actually generated.
"""

import json
import logging

import torch
import torch.nn.functional as F

from comfy_api.latest import io

from . import minimax_media as media

log = logging.getLogger(__name__)

MODEL_FPS = 24.0
AUDIO_SR = media.AUDIO_SR


def _fit(frames, width, height):
    """Cover-crop [N, H, W, 3] onto the generated canvas."""
    if frames is None or frames.shape[0] == 0:
        return None
    if frames.shape[1] == height and frames.shape[2] == width:
        return frames
    return media.resize_image(frames, width, height, "crop", 1)


def _silence(seconds):
    return torch.zeros((2, max(1, int(round(seconds * AUDIO_SR)))), dtype=torch.float32)


def _audio_slice(path, start_sec, dur_sec):
    """Stereo 44.1 kHz slice of a file's soundtrack, silence-padded when short."""
    want = max(1, int(round(dur_sec * AUDIO_SR)))
    try:
        waveform = media._decode_audio_stereo(path)
    except Exception as e:
        log.warning("[MiniMaxRetake] could not read audio from %s: %s", path, e)
        waveform = None
    if waveform is None:
        return _silence(dur_sec)
    a = max(0, int(round(start_sec * AUDIO_SR)))
    clip = waveform[:, a:a + want]
    if clip.shape[1] < want:
        clip = torch.cat([clip, torch.zeros((2, want - clip.shape[1]), dtype=clip.dtype)], dim=1)
    return clip


def _as_stereo_44k(audio):
    """Any ComfyUI AUDIO dict -> [2, N] float32 at 44.1 kHz."""
    if audio is None:
        return None
    waveform = audio.get("waveform")
    if waveform is None:
        return None
    if waveform.ndim == 3:
        waveform = waveform[0]
    waveform = waveform.to(torch.float32).cpu()
    if waveform.shape[0] == 1:
        waveform = waveform.repeat(2, 1)
    elif waveform.shape[0] > 2:
        waveform = waveform[:2]
    sr = int(audio.get("sample_rate", AUDIO_SR))
    if sr != AUDIO_SR:
        try:
            import torchaudio
            waveform = torchaudio.functional.resample(waveform, sr, AUDIO_SR)
        except Exception:
            # linear fallback keeps the splice usable even without torchaudio
            n = int(round(waveform.shape[1] * AUDIO_SR / max(1, sr)))
            waveform = F.interpolate(waveform.unsqueeze(0), size=n,
                                     mode="linear", align_corners=False)[0]
    return waveform


class MiniMaxH3RetakeStitch(io.ComfyNode):
    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id="MooshieH3RetakeStitch",
            display_name="MiniMax H3 Retake Stitch",
            category="MiniMax H3",
            description=(
                "Splices a Retake Mode result back into its base video: base head + the "
                "generated range + base tail, video and audio, all on H3's 24 fps grid. "
                "Wire the Director's retake_info in; it carries the range that was rendered."
            ),
            inputs=[
                io.String.Input("retake_info", force_input=True,
                                tooltip="From the Director's retake_info output."),
                io.Image.Input("images", tooltip="The decoded retake (VAEDecode output)."),
                io.Audio.Input("audio", optional=True,
                               tooltip="The retake's audio (VAEDecodeAudio output). "
                                       "Left out, the base video's own audio is kept throughout."),
                io.Boolean.Input("keep_base_audio", default=False, optional=True,
                                 tooltip="Ignore the generated audio and keep the base video's "
                                         "soundtrack across the whole result."),
            ],
            outputs=[
                io.Image.Output(display_name="images", tooltip="Full-length video frames at 24 fps."),
                io.Audio.Output(display_name="audio"),
                io.Float.Output(display_name="fps"),
            ],
        )

    @classmethod
    def execute(cls, retake_info, images, audio=None, keep_base_audio=False) -> io.NodeOutput:
        try:
            info = json.loads(retake_info) if retake_info else {}
        except Exception as e:
            raise ValueError("MiniMax H3 Retake Stitch: retake_info is not valid JSON (%s). "
                             "Wire it from the Director's retake_info output." % e)
        if not info.get("base_video"):
            # Pass through instead of raising. This node sits at the END of the graph, so
            # raising here throws away a finished render — several minutes of sampling —
            # for a node that simply had nothing to do. Leaving it wired while you work
            # normally has to be harmless; it only splices when the Director actually ran
            # a retake.
            log.info("[MiniMaxRetake] no retake in this run (the Director was not in Retake "
                     "Mode, or no base video was marked) — passing the %d frame(s) through "
                     "unchanged.", int(images.shape[0]))
            passthrough_audio = audio or {
                "waveform": _silence(images.shape[0] / MODEL_FPS).unsqueeze(0),
                "sample_rate": AUDIO_SR,
            }
            return io.NodeOutput(images, passthrough_audio, MODEL_FPS)

        base = info["base_video"]
        tl_fps = float(info.get("timeline_fps") or 24.0) or 24.0
        start_f = int(info.get("start_frame") or 0)
        length_f = int(info.get("length_frames") or 0)
        base_frames = int(info.get("base_frames") or 0)
        height, width = int(images.shape[1]), int(images.shape[2])

        head_sec = max(0.0, start_f / tl_fps)
        gap_start_sec = (start_f + length_f) / tl_fps
        base_sec = base_frames / tl_fps if base_frames else 0.0
        tail_sec = max(0.0, base_sec - gap_start_sec)

        parts, audio_parts = [], []

        if head_sec > 1e-3:
            head = _fit(media.load_video_tensor(base, 0.0, head_sec, out_fps=MODEL_FPS,
                                                max_short_edge=max(width, height),
                                                max_pixels=width * height * 4), width, height)
            if head is not None:
                parts.append(head)
                audio_parts.append(_audio_slice(base, 0.0, head.shape[0] / MODEL_FPS))

        gen = images.to(torch.float32)
        parts.append(gen)
        gen_sec = gen.shape[0] / MODEL_FPS
        gen_audio = None if keep_base_audio else _as_stereo_44k(audio)
        if gen_audio is None:
            gen_audio = _audio_slice(base, head_sec, gen_sec)
        audio_parts.append(gen_audio)

        if tail_sec > 1e-3:
            tail = _fit(media.load_video_tensor(base, gap_start_sec, tail_sec, out_fps=MODEL_FPS,
                                                max_short_edge=max(width, height),
                                                max_pixels=width * height * 4), width, height)
            if tail is not None:
                parts.append(tail)
                audio_parts.append(_audio_slice(base, gap_start_sec, tail.shape[0] / MODEL_FPS))

        out_images = torch.cat(parts, dim=0)

        # pad each audio piece to its own video piece so a short soundtrack cannot shift
        # everything after it out of sync
        fixed = []
        for part, chunk in zip(parts, audio_parts):
            want = max(1, int(round(part.shape[0] / MODEL_FPS * AUDIO_SR)))
            if chunk.shape[1] < want:
                chunk = torch.cat(
                    [chunk, torch.zeros((2, want - chunk.shape[1]), dtype=chunk.dtype)], dim=1)
            fixed.append(chunk[:, :want])
        out_audio = torch.cat(fixed, dim=1)

        log.info("[MiniMaxRetake] stitched %d + %d + %d frames (%.2fs) around the retake at %.2fs",
                 parts[0].shape[0] if head_sec > 1e-3 else 0, gen.shape[0],
                 parts[-1].shape[0] if tail_sec > 1e-3 else 0,
                 out_images.shape[0] / MODEL_FPS, head_sec)

        return io.NodeOutput(out_images,
                             {"waveform": out_audio.unsqueeze(0), "sample_rate": AUDIO_SR},
                             MODEL_FPS)


NODE_CLASS_MAPPINGS = {"MiniMaxH3RetakeStitchCS": MiniMaxH3RetakeStitch}
NODE_DISPLAY_NAME_MAPPINGS = {"MiniMaxH3RetakeStitchCS": "MiniMax H3 Retake Stitch"}
