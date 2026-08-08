# Vendored into MooshieUI from ComfyUI-MiniMaxH3-Director v0.1.5 (GPL-3.0).
# Upstream: https://github.com/seesee75-commits/ComfyUI-MiniMaxH3-Director
# See LICENSE in this directory for the full GPL-3.0 text.
#
# Modifications for MooshieUI, 2026:
#   * node_id renamed to "MooshieH3Director" so this copy cannot collide with the
#     upstream pack if a user also installs it through ComfyUI Manager.
#   * The js/ timeline editor is not vendored. MooshieUI never renders the ComfyUI
#     graph canvas; it builds timeline_data itself and passes it as a widget value.
"""MiniMax H3 Director — a WYSIWYG timeline front-end for MiniMax H3.

The timeline editor (js/minimax_director.js) is a modified version of the LTX Director
editor by WhatDreamsCost (GPL-3.0, see LICENSE), by way of the CS fork by CGlide;
modified in 2026 for MiniMax H3. What changed is everything below the UI, because H3
conditions completely differently from LTX 2.3:

* LTX 2.3 gets per-segment prompts through a Prompt-Relay cross-attention mask.
  H3 is a single-stream packed DiT whose only attention is full self-attention with
  `mask=None` hardcoded, and whose Qwen3-VL text encoder was trained on *storyboard*
  prompts with explicit `[0s-1.5s]` shot markers. So timeline segments are compiled
  into that storyboard form — the model's own native mechanism for timed control,
  and exactly what the official H3 templates do.

* Keyframes: H3's PackedLayout only accepts anchors at frame 0 and frame_count-1, so
  timeline images resolve to first_frame / last_frame. Images in the middle become
  <Picture i> references instead (ref2va), the closest thing H3 offers.

* Audio: H3 generates native stereo audio jointly with the video, so there is no audio
  latent to inpaint. Imported audio becomes an <Audio j> reference for voice/music
  style, and is always also emitted on `combined_audio` for muxing.

* The reference-video track (the old IC-LoRA track) feeds <Video k> references.

Two conditioning paths, each with its own diffusion weights:
  Refs OFF -> t2va / fl2va  (minimax_h3_fl2va_*)
  Refs ON  -> ref2va        (minimax_h3_ref2va_*)

All timeline interpretation lives in minimax_plan.py so the live prompt preview and the
chain node cannot drift from what actually gets encoded.
"""

import json
import logging

import torch

from comfy_api.latest import io

from . import minimax_media as media
from . import minimax_plan as plan
from .minimax_core import core

log = logging.getLogger(__name__)

MODEL_FPS = plan.MODEL_FPS
DEFAULT_W, DEFAULT_H = 1344, 768


# --------------------------------------------------------------------------------------
# helpers
# --------------------------------------------------------------------------------------

def _unpack(out):
    """Normalise an io.NodeOutput / tuple / list into a plain tuple."""
    if out is None:
        return ()
    args = getattr(out, "args", None)
    if isinstance(args, (tuple, list)):
        return tuple(args)
    result = getattr(out, "result", None)
    if isinstance(result, (tuple, list)):
        return tuple(result)
    if isinstance(out, (tuple, list)):
        return tuple(out)
    if isinstance(out, dict) and isinstance(out.get("result"), (tuple, list)):
        return tuple(out["result"])
    return (out,)


def _snap(value, multiple):
    # Floor, not round — same as the LTX Director's snap() and media.resize_image(), so a
    # derived edge never grows past the box. 16:9 at height 768 lands on 1344, H3's native
    # canvas, instead of overshooting to 1376.
    return max(multiple, (int(value) // multiple) * multiple)


def resolve_canvas(mm, custom_width, custom_height, divisible_by, resize_method, first_image):
    """Pick the output canvas.

    With both dimensions set, the widgets are a *box*, not a verdict: the first timeline
    image is run through the chosen resize_method and the canvas becomes whatever comes
    out. That is what the LTX Director does, and it is why 'maintain aspect ratio' with a
    1024x1024 box gives a 16:9 image 1024x576 instead of a squashed square. Every other
    method returns the full box.
    """
    div = max(1, int(divisible_by))
    if custom_width > 0 and custom_height > 0:
        if first_image is not None:
            fitted = media.resize_image(first_image[:1], custom_width, custom_height,
                                        resize_method, div)
            return int(fitted.shape[2]), int(fitted.shape[1])
        return _snap(custom_width, div), _snap(custom_height, div)

    if first_image is not None:
        src_h, src_w = int(first_image.shape[1]), int(first_image.shape[2])
    else:
        src_w, src_h = DEFAULT_W, DEFAULT_H

    if custom_width > 0:
        w = _snap(custom_width, div)
        return w, _snap(src_h * w / max(1, src_w), div)
    if custom_height > 0:
        h = _snap(custom_height, div)
        return _snap(src_w * h / max(1, src_h), div), h

    # H3's own canvas policy: 768 short edge, 768*1344 area cap, per-axis round to 32
    return mm.adapt_canvas(src_w, src_h)


def resolve_window(tdata, fps, start_frame, duration_frames,
                   start=None, end=None, duration=None):
    """Resolve the render window, honouring automation inputs and retake mode.

    The automation sockets are the only route by which a nonsensical window reaches this
    node: the widgets carry minimums, a connected input carries none. A `duration` of 0 —
    what an upstream node hands over when its own value was never set — used to clamp to
    one timeline frame and then render five, a fifth of a second, without a word. Refuse
    it by name instead. Whatever breaks downstream on a window that short breaks a long
    way from the wire that caused it, which is the expensive kind of bug to report.
    """
    if start is not None:
        if float(start) < 0:
            raise ValueError(
                "MiniMax H3 Director: the connected 'start' is %.3gs. It is a position in "
                "seconds and cannot be negative." % float(start))
        start_frame = int(round(float(start) * fps))
    if end is not None:
        end_frame = int(round(float(end) * fps))
        if duration is None:
            if end_frame <= start_frame:
                raise ValueError(
                    "MiniMax H3 Director: the connected 'end' (%.3gs) is not after the "
                    "window start (%.3gs), so there is nothing to render. Both are in "
                    "seconds." % (float(end), start_frame / fps))
            duration_frames = end_frame - start_frame
    if duration is not None:
        if float(duration) <= 0:
            raise ValueError(
                "MiniMax H3 Director: the connected 'duration' is %.3gs, so there is "
                "nothing to render. It is a length in seconds — H3's trained range is "
                "4-15s. Check the node feeding it; a value that was never set arrives "
                "here as 0." % float(duration))
        duration_frames = max(1, int(round(float(duration) * fps)))

    retake = plan.retake_state(tdata)
    if retake:
        # the marked range replaces the panel window entirely
        return int(retake["start"]), max(1, int(retake["length"]))
    return int(start_frame), max(1, int(duration_frames))


def _load_event_tensor(ev, fps, win_start):
    """Decode the pixels behind one main-track segment, image or video."""
    seg = ev["seg"]
    if ev["kind"] == "video":
        seg_start = float(seg.get("start", 0))
        trim = float(seg.get("trimStart", 0)) + max(0.0, win_start - seg_start)
        return media.load_video_tensor(seg.get("imageFile", ""), trim / fps,
                                       float(seg.get("length", 1)) / fps)
    return media.load_image_tensor(seg)


class _Unconnected:
    """Distinguishes an empty optional socket from a lazy one that is merely unevaluated.

    ComfyUI passes None for both, so a plain `None` default cannot tell them apart.
    """
    def __repr__(self):
        return "<unconnected>"


_UNCONNECTED = _Unconnected()


def pick_model(model_fl2va, model_ref2va, ref_mode_on):
    """Choose the weights the toolbar switch calls for.

    fl2va and ref2va are separate checkpoints, so the switch that changes the conditioning
    path has to change the model too. Connect both and it is automatic; connect one and it
    is used either way, with a warning when that is the wrong one for the current path.
    """
    wanted, other = (model_ref2va, model_fl2va) if ref_mode_on else (model_fl2va, model_ref2va)
    label = "ref2va" if ref_mode_on else "fl2va"
    if wanted is not None:
        return wanted
    if other is not None:
        log.warning("[MiniMaxDirector] The toolbar is on '%s' but no %s model is connected — "
                    "using the other one. Load minimax_h3_%s_* for correct results.",
                    "Refs ON" if ref_mode_on else "Refs OFF", label, label)
        return other
    raise ValueError(
        "MiniMax H3 Director: no model connected. Wire a UNETLoader into 'model (t2v/i2v)' "
        "(minimax_h3_fl2va_*) and/or 'model (ref2v)' (minimax_h3_ref2va_*)."
    )


def _grab_base_frame(video_ref, frame_index, fps):
    """One frame out of the retake base video, by timeline frame index."""
    if frame_index < 0:
        return None
    frames = media.load_video_tensor(video_ref, frame_index / fps, 1.0 / MODEL_FPS)
    if frames is None or frames.shape[0] == 0:
        return None
    return frames[:1]


# --------------------------------------------------------------------------------------
# node
# --------------------------------------------------------------------------------------

class MiniMaxH3Director(io.ComfyNode):
    """Timeline editor -> MiniMax H3 storyboard conditioning + joint AV latent."""

    @classmethod
    def define_schema(cls):
        return io.Schema(
            node_id="MooshieH3Director",
            display_name="MiniMax H3 Director",
            category="MiniMax H3",
            description=(
                "Visual timeline for MiniMax H3. Segments become a storyboard prompt with "
                "[0s-1.5s] shot markers, timeline images become first/last keyframes (fl2va) "
                "or <Picture i> references (ref2va), the reference-video track becomes "
                "<Video k>, and audio clips become <Audio j> plus a muxable audio output. "
                "Retake Mode regenerates a marked range of a base video between its own "
                "surrounding frames."
            ),
            inputs=[
                # lazy: only the checkpoint the toolbar actually calls for gets loaded.
                # See check_lazy_status below — without it ComfyUI resolves both inputs
                # before the node runs and reads ~42 GB of weights to use half of them.
                io.Model.Input("model", display_name="model (t2v/i2v)", optional=True, lazy=True,
                               tooltip="The fl2va weights (minimax_h3_fl2va_*), used when the "
                                       "toolbar is on 'Refs OFF'. Connect both models and the "
                                       "node loads whichever the toolbar switch calls for — "
                                       "the other one is never read from disk."),
                io.Model.Input("model_ref2va", display_name="model (ref2v)", optional=True, lazy=True,
                               tooltip="The ref2va weights (minimax_h3_ref2va_*), used when the "
                                       "toolbar is on 'Refs ON'. Optional — with only one model "
                                       "connected that one is used either way."),
                io.Clip.Input("clip", tooltip="Qwen3-VL-32B MiniMax text encoder (CLIPLoader type 'minimax')."),
                io.Vae.Input("vae", tooltip="minimax_h3_video_vae — encodes keyframes and references."),
                io.Vae.Input("audio_vae", optional=True,
                             tooltip="minimax_h3_audio_vae. Only needed when audio references are used (ref2va)."),
                io.String.Input(
                    "global_prompt", multiline=True, default="", force_input=True, optional=True,
                    tooltip="Conditions the whole video: style, scene, characters. Written above the storyboard.",
                ),
                io.Float.Input("start_second", default=0.0, min=0.0, max=1000.0, step=0.01,
                               tooltip="Start of the render window, in seconds."),
                io.Float.Input("end_second", default=5.0, min=0.0, max=1000.0, step=0.01,
                               tooltip="End of the render window, in seconds."),
                io.Float.Input("duration_seconds", default=5.0, min=0.1, max=1000.0, step=0.01,
                               tooltip="Render length in seconds. Snapped up to H3's 17k+5 frame grid at 24 fps."),
                io.Int.Input("start_frame", default=0, min=0, max=10000, step=1,
                             tooltip="Start of the render window, in timeline frames."),
                io.Int.Input("end_frame", default=120, min=1, max=10000, step=1,
                             tooltip="End of the render window, in timeline frames."),
                io.Int.Input("duration_frames", default=120, min=1, max=10000, step=1,
                             tooltip="Render length in timeline frames (at the timeline's frame_rate)."),
                io.String.Input("timeline_data", default="",
                                tooltip="JSON state of the timeline editor (auto-managed; do not edit by hand)."),
                io.Boolean.Input("use_custom_audio", default=False, optional=True,
                                 tooltip="ON: timeline audio clips are used as <Audio j> references (ref2va). "
                                         "The mixdown is always available on combined_audio regardless."),
                io.Boolean.Input("use_custom_motion", default=True, optional=True,
                                 tooltip="ON: the reference-video track feeds <Video k> references (ref2va)."),
                io.Boolean.Input("inpaint_audio", default=True, optional=True,
                                 tooltip="Unused on H3 — audio is generated jointly with the video and cannot be inpainted."),
                io.String.Input("local_prompts", multiline=True, default="",
                                tooltip="Auto-populated from the timeline editor."),
                io.String.Input("segment_lengths", default="",
                                tooltip="Auto-populated from the timeline editor (pixel-space frame counts)."),
                io.Float.Input("frame_rate", default=24, min=1, max=240, step=1, optional=True,
                               tooltip="Timeline editing rate. Output is always 24 fps; times are converted via seconds."),
                io.Combo.Input("display_mode", options=["frames", "seconds"], default="seconds", optional=True,
                               tooltip="Show the ruler and segment ranges in frames or seconds."),
                io.String.Input("guide_strength", default="",
                                tooltip="Auto-populated from the timeline editor. H3 has no per-keyframe strength, so it is ignored."),
                io.Int.Input("custom_width", default=0, min=0, max=8192, step=1, optional=True,
                             tooltip="Output width. With height set too this is a BOX: 'maintain aspect ratio' "
                                     "keeps the first image's aspect inside it. 0 = derive from the image."),
                io.Int.Input("custom_height", default=0, min=0, max=8192, step=1, optional=True,
                             tooltip="Output height. See custom_width."),
                io.Combo.Input("resize_method",
                               options=["maintain aspect ratio", "stretch to fit", "pad", "pad green", "crop"],
                               default="crop", optional=True,
                               tooltip="How timeline images are fitted to the output canvas."),
                io.Int.Input("divisible_by", default=32, min=1, max=256, step=1, optional=True,
                             tooltip="Snap output dimensions to this multiple. H3 needs 32."),
                io.Int.Input("img_compression", default=0, min=0, max=100, step=1, optional=True,
                             tooltip="H.264 CRF baked into each keyframe. 0 = off (recommended for H3)."),
                io.Boolean.Input("override_audio", default=False, optional=True,
                                 tooltip="Use the reference video's own soundtrack as the timeline audio."),
                io.Combo.Input("ref_image_size", options=["match", "max"], default="match", optional=True,
                               tooltip="ref2va only. 'match' scales references to the output pixel area (fast); "
                                       "'max' keeps a 2048 px short edge for identity, at real speed cost."),
                io.Float.Input("shift_video", default=12.0, min=0.01, max=100.0, step=0.01, optional=True,
                               tooltip="Video flow sigma shift (H3 default 12.0)."),
                io.Float.Input("shift_audio", default=3.0, min=0.01, max=100.0, step=0.01, optional=True,
                               tooltip="Audio flow sigma shift (H3 default 3.0)."),
                io.Image.Input("ref_images", optional=True,
                               tooltip="Extra <Picture i> references (single image or batch), appended after the "
                                       "character slots. ref2va only."),
                io.Float.Input("start", force_input=True, optional=True, default=0.0,
                               tooltip="Automation (connection-only). Window start in SECONDS."),
                io.Float.Input("end", force_input=True, optional=True, default=0.0,
                               tooltip="Automation (connection-only). Window end in SECONDS."),
                io.Float.Input("duration", force_input=True, optional=True, default=0.0,
                               tooltip="Automation (connection-only). Render length in SECONDS."),
            ],
            outputs=[
                io.Model.Output(display_name="model"),
                io.Conditioning.Output(display_name="positive"),
                io.Latent.Output(display_name="latent", tooltip="Joint video+audio latent. Wire to SamplerCustomAdvanced."),
                io.Audio.Output(display_name="combined_audio",
                                tooltip="Timeline audio mixdown. Wire into CreateVideo to replace the generated audio."),
                io.Float.Output(display_name="fps", tooltip="Always 24.0 — H3's native output rate. Wire into CreateVideo."),
                io.Int.Output(display_name="width"),
                io.Int.Output(display_name="height"),
                io.Int.Output(display_name="length", tooltip="Frame count actually generated (snapped to the 17k+5 grid)."),
                io.String.Output(display_name="prompt", tooltip="The compiled storyboard prompt that was encoded."),
                io.String.Output(display_name="retake_info",
                                 tooltip="JSON describing the retake window. Wire into MiniMax H3 Retake Stitch "
                                         "to splice the result back into the base video. Empty when retake is off."),
            ],
        )

    # ------------------------------------------------------------ lazy models

    @classmethod
    def check_lazy_status(cls, timeline_data="", model=_UNCONNECTED,
                          model_ref2va=_UNCONNECTED, **_):
        """Ask for the one checkpoint the toolbar switch calls for, and only that one.

        fl2va and ref2va are ~21 GB each. Without this, ComfyUI resolves both inputs
        before execute() runs, so every render reads both from disk to throw one away —
        which is what pushed a 32 GB machine into a page-file crash (issue #2).

        `None` means *connected but not evaluated yet*, so it cannot be used to detect an
        empty socket; that is what the _UNCONNECTED sentinel is for. Same trick core uses
        in comfy_extras/nodes_logic.py.
        """
        ref_on = plan.ref_mode_from(plan.parse_timeline(timeline_data))
        order = ("model_ref2va", "model") if ref_on else ("model", "model_ref2va")
        have = {"model": model, "model_ref2va": model_ref2va}

        for name in order:                       # preferred first, then the fallback
            if have[name] is _UNCONNECTED:
                continue                         # nothing wired here, try the other
            return [name] if have[name] is None else []
        return []                                # neither connected: execute() raises

    # ---------------------------------------------------------------- execute

    @classmethod
    def execute(cls, clip, vae, start_second, end_second, duration_seconds,
                start_frame, end_frame, duration_frames, timeline_data,
                model=None, model_ref2va=None,
                local_prompts="", segment_lengths="", global_prompt="", guide_strength="",
                frame_rate=24, display_mode="seconds",
                custom_width=0, custom_height=0, resize_method="crop",
                divisible_by=32, img_compression=0, audio_vae=None,
                use_custom_audio=False, inpaint_audio=True, use_custom_motion=True,
                override_audio=False, ref_image_size="match",
                shift_video=12.0, shift_audio=3.0, ref_images=None,
                start=None, end=None, duration=None) -> io.NodeOutput:

        mm = core()
        tdata = plan.parse_timeline(timeline_data)
        fps = float(frame_rate) if frame_rate else 24.0

        win_start, duration_frames = resolve_window(
            tdata, fps, start_frame, duration_frames, start, end, duration)

        extra_refs = 0
        if ref_images is not None:
            try:
                extra_refs = int(ref_images.shape[0])
            except Exception:
                extra_refs = 0

        p = plan.plan_timeline(tdata, win_start, duration_frames, fps,
                               global_prompt=global_prompt,
                               use_custom_motion=use_custom_motion,
                               use_custom_audio=use_custom_audio,
                               override_audio=override_audio,
                               extra_ref_image_count=extra_refs)

        length = p["length"]
        if length > plan.TRAINED_MAX_FRAMES:
            log.warning("[MiniMaxDirector] %d frames (%.1fs) is past H3's trained range of "
                        "~%d-%d frames (~4-15s). Expect drift, looping or a VRAM wall — "
                        "shorten the timeline, or render it as several windows and splice "
                        "them together.",
                        length, p["actual_seconds"], plan.TRAINED_MIN_FRAMES,
                        plan.TRAINED_MAX_FRAMES)
        elif length < plan.TRAINED_MIN_FRAMES:
            log.info("[MiniMaxDirector] %d frames (%.1fs) is below H3's trained range "
                     "(~%d frames / 5s). Fine for tests, weaker motion than a full shot.",
                     length, p["actual_seconds"], plan.TRAINED_MIN_FRAMES)
        if p["prompt_is_fallback"]:
            log.warning("[MiniMaxDirector] No prompt text on the timeline — falling back to 'video'.")
        for warning in p.get("ref_warnings") or []:
            log.warning("[MiniMaxDirector] %s", warning)

        retake = p["retake"]

        # --- load the pixels the plan calls for ------------------------------------
        for ev in p["events"]:
            ev["tensor"] = _load_event_tensor(ev, fps, win_start)

        first_src = last_src = None
        if retake:
            # anchor on the base video's own frames either side of the marked range
            first_src = _grab_base_frame(retake["video"], retake["start"] - 1, fps)
            tail_index = retake["start"] + retake["length"]
            if not retake["base_frames"] or tail_index < retake["base_frames"]:
                last_src = _grab_base_frame(retake["video"], tail_index, fps)
            if first_src is None and last_src is None:
                log.warning("[MiniMaxDirector] Retake: could not read anchor frames from '%s' "
                            "— falling back to a plain text-to-video window.", retake["video"])
        else:
            for ev in p["events"]:
                if ev["role"] == plan.ROLE_FIRST:
                    first_src = ev["tensor"][:1]
                elif ev["role"] == plan.ROLE_LAST:
                    last_src = ev["tensor"][-1:]

        # --- canvas -----------------------------------------------------------------
        canvas_src = first_src
        if canvas_src is None:
            canvas_src = p["events"][0]["tensor"] if p["events"] else None
        width, height = resolve_canvas(mm, int(custom_width), int(custom_height),
                                       int(divisible_by), resize_method, canvas_src)

        def fit(tensor):
            out = media.resize_image(tensor, width, height, resize_method, int(divisible_by))
            if out.shape[1] != height or out.shape[2] != width:
                # A later image with a different aspect than the one that set the canvas.
                # The canvas is already fixed, so cover-crop it to match exactly — otherwise
                # the core node would stretch the keyframe and distort it.
                out = media.resize_image(out, width, height, "crop", int(divisible_by))
            if int(img_compression) > 0:
                out = media.compress_image(out, int(img_compression))
            return out

        first_frame = fit(first_src) if first_src is not None else None
        last_frame = fit(last_src) if last_src is not None else None

        # --- reference payloads ------------------------------------------------------
        ref_image_tensors, ref_videos, ref_video_audios, ref_audios = [], {}, {}, {}
        if p["ref_mode_on"]:
            input_cursor = 0
            for slot in p["ref_image_slots"]:
                src = slot["source"]
                if src == "char":
                    img = slot["image"]
                    ref_image_tensors.append(
                        media.load_image_source(img.get("b64", ""), img.get("name", "")))
                elif src == "input":
                    ref_image_tensors.append(ref_images[input_cursor:input_cursor + 1])
                    input_cursor += 1
                else:
                    ev = slot["event"]
                    tensor = ev["tensor"]
                    if slot.get("keyframe") == plan.ROLE_LAST:
                        ref_image_tensors.append(fit(tensor[-1:]))
                    elif slot.get("keyframe"):
                        ref_image_tensors.append(fit(tensor[:1]))
                    else:
                        ref_image_tensors.append(tensor[:1])

            for seg in p["ref_video_segs"]:
                idx = len(ref_videos)
                seg_start = float(seg.get("start", 0))
                seg_len = float(seg.get("length", 1))
                offset = max(0.0, win_start - seg_start)
                trim = float(seg.get("trimStart", 0)) + offset
                clip_sec = min(plan.REF_VIDEO_MAX_SEC,
                               max(plan.REF_VIDEO_MIN_SEC, (seg_len - offset) / fps))
                frames = media.load_video_tensor(seg["videoFile"], trim / fps, clip_sec)
                if frames.shape[0] < 5:
                    log.warning("[MiniMaxDirector] Reference video '%s' is shorter than 5 "
                                "frames — skipped.", seg.get("fileName", seg["videoFile"]))
                    continue
                ref_videos["ref_video_%d" % idx] = frames
                if override_audio:
                    clip_audio = media.load_audio_segment(
                        {"audioFile": seg["videoFile"], "trimStart": trim,
                         "length": clip_sec * fps}, fps, file_key="audioFile")
                    if clip_audio is not None:
                        ref_video_audios["ref_video_audio_%d" % idx] = clip_audio

            for seg in p["ref_audio_segs"]:
                clip_audio = media.load_audio_segment(seg, fps)
                if clip_audio is not None:
                    ref_audios["ref_audio_%d" % len(ref_audios)] = clip_audio

            if first_frame is not None or last_frame is not None:
                log.info("[MiniMaxDirector] ref2va has no first/last keyframe slot — the "
                         "timeline keyframes were added as <Picture i> references instead.")
                first_frame = last_frame = None

        prompt = p["prompt"]
        log.info("[MiniMaxDirector] %s%s | %dx%d | %d frames (%.2fs @24fps) | %d shots | "
                 "refs: %d img / %d vid / %d audio",
                 p["mode"], " (retake)" if retake else "", width, height, length,
                 p["actual_seconds"], len(p["shots"]),
                 len(ref_image_tensors), len(ref_videos), len(ref_audios))
        # The full storyboard is one to two screens of text; the node's `prompt` output and
        # the COMPILED PROMPT panel both show it, so keep it out of the console by default.
        log.debug("[MiniMaxDirector] prompt:\n%s", prompt)

        # --- conditioning ------------------------------------------------------------
        if p["ref_mode_on"]:
            if (ref_audios or ref_video_audios) and audio_vae is None:
                raise ValueError(
                    "MiniMax H3 Director: audio references need the audio VAE. Connect "
                    "minimax_h3_audio_vae to the Director's 'audio_vae' input (or turn off "
                    "the audio track / Override Audio)."
                )
            out = mm.MiniMaxH3ReferenceToVideo.execute(
                clip=clip, vae=vae, audio_vae=audio_vae, prompt=prompt,
                width=width, height=height, length=length,
                ref_image_size=ref_image_size,
                ref_images={"ref_image_%d" % i: t for i, t in enumerate(ref_image_tensors)} or None,
                ref_videos=ref_videos or None,
                ref_video_audios=ref_video_audios or None,
                ref_audios=ref_audios or None,
            )
        else:
            middles = [e for e in p["events"] if e["role"] == plan.ROLE_MIDDLE]
            if middles:
                log.warning("[MiniMaxDirector] %d timeline image(s) sit in the middle of the "
                            "window. H3 only anchors keyframes at the first and last frame, so "
                            "they were ignored — switch the toolbar to 'Refs ON (ref2va)' to "
                            "use them as <Picture i> references.", len(middles))
            out = mm.MiniMaxH3ImageToVideo.execute(
                clip=clip, vae=vae, prompt=prompt,
                width=width, height=height, length=length,
                first_frame=first_frame, last_frame=last_frame,
            )

        conditioning, latent = _unpack(out)[:2]

        chosen_model = pick_model(model, model_ref2va, p["ref_mode_on"])
        patched_model = _unpack(mm.MiniMaxH3SigmaShift.execute(
            model=chosen_model, shift_video=float(shift_video),
            shift_audio=float(shift_audio)))[0]

        audio_out = media.build_combined_audio(
            timeline_data, win_start,
            max(1, int(round(p["actual_seconds"] * fps))), fps, override_audio=override_audio)

        retake_info = ""
        if retake:
            retake_info = json.dumps({
                "base_video": retake["video"],
                "timeline_fps": fps,
                "start_frame": retake["start"],
                "length_frames": retake["length"],
                "base_frames": retake["base_frames"],
                "generated_frames": length,
                "generated_fps": MODEL_FPS,
                "width": int(width), "height": int(height),
            })

        return io.NodeOutput(patched_model, conditioning, latent, audio_out,
                             MODEL_FPS, int(width), int(height), int(length), prompt,
                             retake_info)


NODE_CLASS_MAPPINGS = {"MiniMaxH3DirectorCS": MiniMaxH3Director}
NODE_DISPLAY_NAME_MAPPINGS = {"MiniMaxH3DirectorCS": "MiniMax H3 Director"}
