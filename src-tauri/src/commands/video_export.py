"""Animated AVIF / WebP / GIF export for MooshieUI.

Piped to the ComfyUI venv's python on stdin; the job is one JSON argv.
Decodes with PyAV, reshapes the frame list with numpy for the chosen loop
mode, and encodes with PIL. Every one of those is already in the venv - this
script must never add a dependency.

Protocol: one JSON object per stdout line. Progress lines carry "stage";
the final line carries "result" or "error".
"""

import json
import sys
import traceback

import av
import numpy as np
from PIL import Image


def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def decode(path, width, height):
    """Decode every video frame, scaled to the output size, as RGB ndarrays.

    When height is 0 the Rust side did not have source dimensions available.
    In that case the first frame's aspect ratio is used to compute a
    height that is even and matches the requested width.
    """
    frames = []
    out_h = height  # may be 0 when Rust did not know source dimensions
    with av.open(path) as container:
        stream = container.streams.video[0]
        stream.thread_type = "AUTO"
        src_fps = float(stream.average_rate) if stream.average_rate else 24.0
        total = stream.frames or 0
        for i, frame in enumerate(container.decode(stream)):
            if out_h <= 0:
                # Derive height from the first frame's aspect ratio.
                aspect = frame.height / frame.width if frame.width else 1.0
                out_h = max(2, (round(width * aspect / 2)) * 2)
            rgb = frame.reformat(width=width, height=out_h, format="rgb24")
            frames.append(rgb.to_ndarray())
            if i % 8 == 0:
                emit({"stage": "decode", "done": i, "total": total})
    return frames, src_fps


def resample(frames, src_fps, target_fps):
    """Keep every Nth frame. The caller only ever passes integer divisors, so
    the cadence is even and nothing judders."""
    if target_fps <= 0 or target_fps >= src_fps:
        return frames
    step = max(1, int(round(src_fps / target_fps)))
    return frames[::step]


def seam_delta(frames):
    """Mean absolute difference between the first and last frame at 64x64,
    normalised to 0-100.

    64x64 is deliberate: it measures whether the composition matches, not
    whether individual pixels do, so encoder noise does not drown the signal.
    """
    if len(frames) < 2:
        return 0.0
    a = np.asarray(
        Image.fromarray(frames[0]).resize((64, 64), Image.BILINEAR), dtype=np.float32
    )
    b = np.asarray(
        Image.fromarray(frames[-1]).resize((64, 64), Image.BILINEAR), dtype=np.float32
    )
    return float(np.abs(a - b).mean() / 255.0 * 100.0)


def apply_loop_mode(frames, mode, n):
    """Reshape the frame list. Mirrors output_frame_count() in video_export.rs."""
    f = len(frames)
    if mode == "trim":
        return frames[:-1] if f > 1 else frames
    if mode == "crossfade":
        if n <= 0 or f <= 3 * n:
            return frames
        # out[i] = lerp(src[i], src[F-N+i], 1 - i/N) for i < N, then src[N:F-N].
        # At i=0 that is exactly src[F-N], so the wrap is continuous: the last
        # output frame is src[F-N-1] and the first is src[F-N].
        out = []
        for i in range(n):
            t = 1.0 - (i / n)
            a = frames[i].astype(np.float32)
            b = frames[f - n + i].astype(np.float32)
            out.append(np.clip(a + (b - a) * t, 0, 255).astype(np.uint8))
        return out + frames[n : f - n]
    if mode == "pingpong":
        if f < 3:
            return frames
        # Reversed tail excluding both endpoints, so neither the first nor the
        # last frame plays twice: 2F - 2 frames, seamless by construction.
        return frames + frames[-2:0:-1]
    # "none" and anything unrecognised encode the source verbatim.
    return frames


def build_palette(frames, colors):
    """Global palette from up to 24 evenly spaced frames.

    This is palettegen: quantise a montage of samples once, then map every
    frame onto that fixed palette so colours do not shimmer between frames.
    """
    step = max(1, len(frames) // 24)
    sample = frames[::step][:24]
    montage = np.concatenate(sample, axis=0)
    return Image.fromarray(montage).quantize(
        colors=max(2, min(256, colors)), method=Image.Quantize.MEDIANCUT
    )


def encode_gif(frames, out_path, fps, colors, loop_count):
    palette = build_palette(frames, colors)
    imgs = []
    for i, fr in enumerate(frames):
        # paletteuse: fixed palette + Floyd-Steinberg dithering.
        imgs.append(
            Image.fromarray(fr).quantize(
                palette=palette, dither=Image.Dither.FLOYDSTEINBERG
            )
        )
        if i % 8 == 0:
            emit({"stage": "encode", "done": i, "total": len(frames)})
    imgs[0].save(
        out_path,
        save_all=True,
        append_images=imgs[1:],
        duration=max(20, round(1000.0 / fps)),
        loop=loop_count,
        disposal=2,
        optimize=False,
    )


def encode_webp(frames, out_path, fps, quality, loop_count):
    imgs = []
    for i, fr in enumerate(frames):
        imgs.append(Image.fromarray(fr))
        if i % 8 == 0:
            emit({"stage": "encode", "done": i, "total": len(frames)})
    imgs[0].save(
        out_path,
        format="WEBP",
        save_all=True,
        append_images=imgs[1:],
        duration=max(20, round(1000.0 / fps)),
        loop=loop_count,
        quality=max(0, min(100, quality)),
        method=4,
    )


def encode_avif(frames, out_path, fps, quality, loop_count):
    imgs = []
    for i, fr in enumerate(frames):
        imgs.append(Image.fromarray(fr))
        if i % 8 == 0:
            emit({"stage": "encode", "done": i, "total": len(frames)})
    imgs[0].save(
        out_path,
        format="AVIF",
        save_all=True,
        append_images=imgs[1:],
        duration=max(20, round(1000.0 / fps)),
        # Accepted but not honoured: animated AVIF loops continuously regardless,
        # and Pillow reads the value back as None. Passed for symmetry with WEBP.
        loop=loop_count,
        # AV1 quality, not the libwebp scale - the presets send lower numbers.
        quality=max(0, min(100, quality)),
        # Measured on this project's venv at 640x368 x 124 frames: speed 8 encodes
        # in 1.0 s versus 3.8 s at speed 6, for 0.26 MB versus 0.24 MB. Not worth
        # the wait.
        speed=8,
    )


def main():
    job = json.loads(sys.argv[1])
    frames, src_fps = decode(job["source"], job["width"], job["height"])
    if not frames:
        raise RuntimeError("the source video decoded to zero frames")

    frames = resample(frames, src_fps, job["fps"])
    delta = seam_delta(frames)
    emit({"stage": "seam", "seam_delta": delta})

    mode = job["loop_mode"]
    if mode == "auto":
        # Mirrors resolve_auto() in video_export.rs; the Rust side cross-checks
        # the value we report back.
        mode = "trim" if delta < job["auto_threshold"] else "none"
    frames = apply_loop_mode(frames, mode, job["crossfade_frames"])

    out_path = job["out"]
    fmt = job["format"]
    if fmt == "gif":
        encode_gif(frames, out_path, job["fps"], job["quality"], job["loop_count"])
    elif fmt == "webp":
        encode_webp(frames, out_path, job["fps"], job["quality"], job["loop_count"])
    else:
        encode_avif(frames, out_path, job["fps"], job["quality"], job["loop_count"])

    import os

    emit(
        {
            "result": {
                "path": out_path,
                "size_bytes": os.path.getsize(out_path),
                "frame_count": len(frames),
                "seam_delta": delta,
                "applied_loop_mode": mode,
            }
        }
    )


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:  # noqa: BLE001 - the Rust side needs the message
        emit({"error": f"{type(exc).__name__}: {exc}"})
        traceback.print_exc(file=sys.stderr)
        sys.exit(1)
