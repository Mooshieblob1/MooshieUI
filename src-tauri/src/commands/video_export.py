"""Animated AVIF / WebP / GIF and re-encoded MP4 export for MooshieUI.

Piped to the ComfyUI venv's python on stdin; the job is one JSON argv.
Decodes with PyAV, reshapes the frame list with numpy for the chosen loop
mode, and encodes with PIL - except MP4, which PIL cannot write, so that one
round-trips through PyAV. Every one of those is already in the venv - this
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


def exif_blob(text):
    """A minimal TIFF blob whose only tag is the Exif sub-IFD UserComment.

    Same carrier the Rust WebP writer and MooshieSaveVideo's poster use, so one
    reader handles stills, posters, and animated exports alike.

    Returns None if this Pillow build cannot produce one, so the caller saves
    without metadata rather than failing the export.
    """
    from PIL import Image

    try:
        exif = Image.Exif()
        exif[0x8769] = {0x9286: b"UNICODE\x00" + text.encode("utf-16-be")}
        return exif.tobytes()
    except Exception:  # noqa: BLE001 - metadata is best-effort everywhere here
        return None


def save_best_effort(img, out_path, kwargs, metadata_key):
    """Save, retrying without the metadata kwarg if Pillow rejects it.

    Whether a build's GIF, WebP or AVIF plugin accepts a tag kwarg is a property
    of the build, not of the export, so a rejection must cost the metadata and
    nothing else. Only the metadata key is dropped on the retry, and a failure
    with any other cause still raises.
    """
    try:
        img.save(out_path, **kwargs)
    except Exception:  # noqa: BLE001 - an export without metadata beats no export
        if metadata_key not in kwargs:
            raise
        kwargs.pop(metadata_key, None)
        img.save(out_path, **kwargs)


def encode_gif(frames, out_path, fps, colors, loop_count, metadata_json=""):
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
    kwargs = {
        "save_all": True,
        "append_images": imgs[1:],
        "duration": max(20, round(1000.0 / fps)),
        "loop": loop_count,
        "disposal": 2,
        "optimize": False,
    }
    if metadata_json:
        # GIF's Comment Extension, which Pillow splits into 255-byte sub-blocks.
        kwargs["comment"] = metadata_json.encode("utf-8")
    save_best_effort(imgs[0], out_path, kwargs, "comment")


def encode_webp(frames, out_path, fps, quality, loop_count, metadata_json=""):
    imgs = []
    for i, fr in enumerate(frames):
        imgs.append(Image.fromarray(fr))
        if i % 8 == 0:
            emit({"stage": "encode", "done": i, "total": len(frames)})
    kwargs = {
        "format": "WEBP",
        "save_all": True,
        "append_images": imgs[1:],
        "duration": max(20, round(1000.0 / fps)),
        "loop": loop_count,
        "quality": max(0, min(100, quality)),
        "method": 4,
    }
    if metadata_json:
        blob = exif_blob(metadata_json)
        if blob:
            kwargs["exif"] = blob
    save_best_effort(imgs[0], out_path, kwargs, "exif")


def encode_avif(frames, out_path, fps, quality, loop_count, metadata_json=""):
    imgs = []
    for i, fr in enumerate(frames):
        imgs.append(Image.fromarray(fr))
        if i % 8 == 0:
            emit({"stage": "encode", "done": i, "total": len(frames)})
    kwargs = {
        "format": "AVIF",
        "save_all": True,
        "append_images": imgs[1:],
        "duration": max(20, round(1000.0 / fps)),
        # Accepted but not honoured: animated AVIF loops continuously regardless,
        # and Pillow reads the value back as None. Passed for symmetry with WEBP.
        "loop": loop_count,
        # AV1 quality, not the libwebp scale - the presets send lower numbers.
        "quality": max(0, min(100, quality)),
        # Measured on this project's venv at 640x368 x 124 frames: speed 8 encodes
        # in 1.0 s versus 3.8 s at speed 6, for 0.26 MB versus 0.24 MB. Not worth
        # the wait.
        "speed": 8,
    }
    if metadata_json:
        blob = exif_blob(metadata_json)
        if blob:
            kwargs["exif"] = blob
    save_best_effort(imgs[0], out_path, kwargs, "exif")


def add_audio_copy_stream(out, in_stream):
    """Declare a verbatim copy of the source audio track on the output container.

    `add_stream_from_template` is the PyAV 13+ spelling; older builds say
    `add_stream(template=...)`. Both produce a stream whose codec parameters are
    copied rather than re-derived, which is what makes a copy a copy.
    """
    if hasattr(out, "add_stream_from_template"):
        return out.add_stream_from_template(in_stream)
    return out.add_stream(template=in_stream)


def encode_mp4(frames, out_path, fps, crf, source_path, keep_audio, metadata_json=""):
    """H.264 in mp4, optionally carrying the source clip's audio over untouched.

    Copying the audio is both lossless and far simpler than re-encoding it:
    PyAV's AAC encoder needs an AudioFifo and a resampler to feed it fixed
    1024-sample frames, and none of that buys anything when the source track is
    already AAC in an mp4 - which is exactly what MiniMax H3 writes.

    Returns whether an audio track actually made it into the file, so the UI can
    say so rather than leaving the user to discover a silent export.
    """
    height, width = frames[0].shape[0], frames[0].shape[1]
    # The video's own length, which is what the audio gets truncated to. Every
    # loop mode either keeps the frame count or changes it deliberately, and the
    # audio has to follow the result rather than the source.
    duration = len(frames) / float(fps) if fps > 0 else 0.0

    src = None
    audio_in = None
    audio_out = None
    wrote_audio = False
    # Without `use_metadata_tags` the mov muxer only emits the handful of tags it
    # maps to QuickTime atoms and silently drops everything else, so the mdta
    # keys never reach the file. MooshieSaveVideo's `save_to` sets this flag for
    # itself; here it has to be asked for.
    mux_options = {"movflags": "use_metadata_tags"} if metadata_json else {}
    with av.open(out_path, mode="w", options=mux_options) as out:
        if metadata_json:
            # `comment` rather than a custom key: only standard keys survive a
            # later remux that does not set use_metadata_tags.
            out.metadata["comment"] = metadata_json
        vstream = out.add_stream("libx264", rate=fps, options={"crf": str(crf)})
        vstream.width = width
        vstream.height = height
        # 4:2:0 8-bit is the only combination every browser, phone and chat client
        # decodes. libx264 would otherwise pick 4:4:4 from the rgb24 input and
        # produce a file that plays in ffplay and nowhere else.
        vstream.pix_fmt = "yuv420p"

        # Every stream has to be declared before the first packet is muxed, so the
        # audio is set up here even though it is written last.
        if keep_audio:
            try:
                src = av.open(source_path)
                if src.streams.audio:
                    audio_in = src.streams.audio[0]
                    audio_out = add_audio_copy_stream(out, audio_in)
            except Exception as exc:  # noqa: BLE001 - a silent mp4 beats no mp4
                emit({"stage": "audio_skipped", "reason": str(exc)})
                audio_in = None
                audio_out = None

        try:
            for i, fr in enumerate(frames):
                for packet in vstream.encode(av.VideoFrame.from_ndarray(fr, format="rgb24")):
                    out.mux(packet)
                if i % 8 == 0:
                    emit({"stage": "encode", "done": i, "total": len(frames)})
            # Drain the encoder's lookahead queue; without this the tail of the
            # clip never reaches the file.
            for packet in vstream.encode():
                out.mux(packet)

            if audio_in is not None:
                try:
                    for packet in src.demux(audio_in):
                        # demux yields a final empty flush packet with no dts.
                        if packet.dts is None:
                            continue
                        if (
                            duration > 0
                            and packet.pts is not None
                            and float(packet.pts * audio_in.time_base) >= duration
                        ):
                            break
                        # Reassigning the stream rescales pts/dts into the output
                        # time base; the packet payload is never touched.
                        packet.stream = audio_out
                        out.mux(packet)
                        wrote_audio = True
                except Exception as exc:  # noqa: BLE001
                    emit({"stage": "audio_skipped", "reason": str(exc)})
        finally:
            if src is not None:
                src.close()
    return wrote_audio


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
    metadata_json = job.get("metadata_json", "")
    has_audio = False
    if fmt == "gif":
        encode_gif(
            frames, out_path, job["fps"], job["quality"], job["loop_count"], metadata_json
        )
    elif fmt == "webp":
        encode_webp(
            frames, out_path, job["fps"], job["quality"], job["loop_count"], metadata_json
        )
    elif fmt == "mp4":
        # The Rust side already cleared keep_audio for the formats and loop modes
        # that cannot carry a track; .get() only covers an older caller.
        has_audio = encode_mp4(
            frames,
            out_path,
            job["fps"],
            job.get("crf", 20),
            job["source"],
            job.get("keep_audio", False),
            metadata_json,
        )
    else:
        encode_avif(
            frames, out_path, job["fps"], job["quality"], job["loop_count"], metadata_json
        )

    import os

    emit(
        {
            "result": {
                "path": out_path,
                "size_bytes": os.path.getsize(out_path),
                "frame_count": len(frames),
                "seam_delta": delta,
                "applied_loop_mode": mode,
                "has_audio": has_audio,
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
