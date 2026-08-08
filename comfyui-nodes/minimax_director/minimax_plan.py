"""Timeline -> MiniMax H3 plan, with no pixels touched.

Everything that decides *what* a timeline means — which image is the opening frame,
which becomes a <Picture i> reference, what the storyboard prompt reads like — lives
here and works on metadata alone. Three callers share it:

* the Director node, which then loads exactly the media the plan calls for,
* the /minimax_director/compile_prompt endpoint behind the editor's live prompt
  preview, which loads nothing at all,
* the chain node, which plans one window per shot.

Keeping it in one place is the point: a prompt preview that disagrees with what is
actually sent would be worse than no preview.
"""

# Vendored into MooshieUI from ComfyUI-MiniMaxH3-Director v0.1.5 (GPL-3.0).
# Upstream: https://github.com/seesee75-commits/ComfyUI-MiniMaxH3-Director
# See LICENSE in this directory for the full GPL-3.0 text. Unmodified.

import json
import logging

log = logging.getLogger(__name__)

MODEL_FPS = 24.0

# Limits straight from MiniMax's own model card (huggingface.co/MiniMaxAI/MiniMax-H3),
# not from ComfyUI's node signatures — the two do not agree everywhere.
MAX_REF_IMAGES = 9          # "<= 9 images"
MAX_REF_VIDEOS = 3          # "<= 3 clips"
MAX_REF_AUDIOS = 3          # "<= 3 clips"
MAX_REF_FILES = 12          # "at most 12 files in total across all input types"
REF_VIDEO_MIN_SEC = 2.0     # "each clip must be 2-15 seconds long"
REF_VIDEO_MAX_SEC = 15.0
REF_VIDEO_TOTAL_SEC = 15.0  # "total duration <= 15 seconds"
# Output envelope: "4-15 seconds" at 24 fps
TRAINED_MIN_FRAMES = 96
TRAINED_MAX_FRAMES = 360

ROLE_FIRST = "first"
ROLE_LAST = "last"
ROLE_MIDDLE = "middle"


def align_frame_count(n):
    """H3 only accepts frame counts on the 17k+5 grid."""
    while n % 17 != 5:
        n += 1
    return n


def fmt_seconds(value):
    """0.0 -> '0s', 1.5 -> '1.5s' — the notation used in MiniMax's own templates."""
    rounded = round(float(value), 1)
    if abs(rounded - round(rounded)) < 1e-9:
        return "%ds" % int(round(rounded))
    return "%.1fs" % rounded


def substitute_char_tags(text, replacements):
    """Swap @character1/@char1 .. @character3/@char3 for their resolved text."""
    if not text:
        return text or ""
    for slot in (1, 2, 3):
        value = replacements.get(slot)
        if not value:
            continue
        for tag in ("@character%d" % slot, "@char%d" % slot):
            text = text.replace(tag, value)
    return text


def overlaps(seg, win_start, win_end):
    start = float(seg.get("start", 0))
    length = float(seg.get("length", 1))
    return start < win_end and start + length > win_start


def parse_timeline(timeline_data):
    try:
        return json.loads(timeline_data) if timeline_data else {}
    except Exception as e:
        log.error("[MiniMaxDirector] timeline_data parse error: %s", e)
        return {}


def ref_mode_from(tdata):
    """Is the toolbar on 'Refs ON (ref2va)'?

    Lives here rather than inline in the planner because the Director's lazy-input check
    has to answer the same question *before* the plan is built — the two must not drift,
    or the node would load one checkpoint and condition for the other.
    """
    return str(tdata.get("reference_mode", "OFF")).upper() != "OFF"


def retake_state(tdata):
    """The retake panel's state, or None when retake mode is off / has no base video."""
    if not tdata.get("retakeMode"):
        return None
    video = tdata.get("retakeVideo") or {}
    if not isinstance(video, dict):
        return None
    name = video.get("imageFile") or video.get("fileName")
    if not name:
        return None
    return {
        "video": name,
        "start": int(float(tdata.get("retakeStart", 0) or 0)),
        "length": max(1, int(float(tdata.get("retakeLength", 0) or 1))),
        "base_frames": int(float(video.get("videoDurationFrames", 0) or 0)),
        "prompt": tdata.get("retakePrompt", "") or "",
    }


FORMAT_MINIMAX = "minimax"
FORMAT_COMFYUI = "comfyui"


def fmt_timestamp(seconds):
    """MM:SS.mmm — the cut-time format MiniMax's prompt guide uses."""
    seconds = max(0.0, float(seconds))
    minutes = int(seconds // 60)
    rest = seconds - minutes * 60
    return "%02d:%06.3f" % (minutes, rest)


def build_subject_definitions(char_slots, ref_image_slots, ref_video_segs, ref_audio_segs):
    """Bind <Subject N> names to the <Picture i> labels the tokenizer will emit.

    The guide separates <Subject N> (reusable content: a person, a place, a style) from
    <Picture N> (a concrete frame anchor). ComfyUI's tokenizer only ever labels images
    <Picture i>, so a subject has to be *defined* in terms of those labels before shots
    can refer to it — which is exactly what subject_definitions is for.
    """
    lines = []
    subject_of_slot = {}
    for slot_index, _slot in enumerate(char_slots):
        ordinals = [i + 1 for i, s in enumerate(ref_image_slots)
                    if s.get("source") == "char" and s.get("slot") == slot_index]
        if not ordinals:
            continue
        subject = len(subject_of_slot) + 1
        subject_of_slot[slot_index + 1] = subject
        pictures = " and ".join("<Picture %d>" % o for o in ordinals)
        lines.append("<Subject %d> is the character shown in %s." % (subject, pictures))

    for i, _seg in enumerate(ref_video_segs):
        lines.append("<Video %d> is a reference video: follow its motion and camera work."
                     % (i + 1))
    for i, _seg in enumerate(ref_audio_segs):
        lines.append("<Audio %d> is a reference audio clip: follow its voice and timbre."
                     % (i + 1))
    return lines, subject_of_slot


def alignment_instruction(has_first, has_last, shot_count, seconds):
    """The image-alignment line the base guide requires as the very first line.

    Quoted verbatim from VIDEO_PROMPT_WRITING_GUIDE_base_en.md, including its own
    inconsistency: I2VA and L2VA bracket the tokens (`<Picture 1>`, `[Shot 1]`) while
    FL2VA does not. The model was trained on that text, so it is reproduced as written
    rather than tidied up.

    T2VA has no instruction, and the reference guide does not ask for one at all, so this
    only applies to the fl2va path.

    S.SS is the *effective* duration — after snapping to the 17k+5 grid, so a 5s request
    reports 5.16, not 5.00. It is floored to the hundredth rather than rounded, because
    rounding can name a mark that is past the end of the video: 124 frames last
    5.166667s, and "5.17" is outside the clip it is describing. Ten of the twenty-four
    valid frame counts up to 16s round that way (issue #6). The multiply-round-then-floor
    keeps binary noise from stealing a hundredth off an exact value like 12.25.
    """
    n = max(1, int(shot_count))
    s = "%.2f" % (int(round(max(0.0, float(seconds)) * 10000)) // 100 / 100.0)
    if has_first and has_last:
        return ("How the reference pictures align with the target video — Picture 1 "
                "(from Shot 1) aligns with the 0.00-second mark of the target video; "
                "Picture 2 (from Shot %d) aligns with the %s-second mark of the target "
                "video." % (n, s))
    if has_first:
        return ("For the target video, at 0.00 seconds into the target video, "
                "<Picture 1> (from [Shot 1]) is fully referenced.")
    if has_last:
        return ("How the reference pictures align with the target video — <Picture 1> "
                "(from [Shot %d]) aligns with the %s-second mark of the target video."
                % (n, s))
    return ""


def compile_storyboard_minimax(global_prompt, shots, soundscape="", music="",
                               subject_lines=None, retention_lines=None,
                               instruction=""):
    """The notation MiniMax documents in VIDEO_PROMPT_WRITING_GUIDE_*.md.

    `integrated_multimodal_description: [Shot 1] … [Shot 2] At 00:05.000, …`, with the
    first shot carrying no timestamp and every later cut carrying a strictly increasing
    one, followed by the soundscape fields. Sections are only emitted when there is
    something real to put in them — an empty heading is worse than none.
    """
    parts = []

    # the guide: "must be the first line of the final prompt, followed by one blank line
    # before the core fields" — joining parts with a blank line gives exactly that
    if (instruction or "").strip():
        parts.append(instruction.strip())

    if subject_lines:
        parts.append("subject_definitions: " + " ".join(subject_lines))
    if retention_lines:
        parts.append("retention_analysis: " + " ".join(retention_lines))

    written = [s for s in shots if (s["prompt"] or "").strip()]
    body = []
    gp = (global_prompt or "").strip()
    if gp:
        body.append(gp)
    for index, shot in enumerate(written):
        text = shot["prompt"].strip()
        if index == 0:
            body.append("[Shot 1] %s" % text)
        else:
            body.append("[Shot %d] At %s, %s"
                        % (index + 1, fmt_timestamp(shot["start_sec"]), text))
    if body:
        parts.append(("detailed_description: " if subject_lines
                      else "integrated_multimodal_description: ") + " ".join(body))

    if (soundscape or "").strip():
        parts.append("overall_soundscape: " + soundscape.strip())
    if (music or "").strip():
        parts.append("non_diegetic_music: " + music.strip())

    return "\n\n".join(parts).strip()


SOUND_PREFIXES = ("audio:", "sound:", "soundscape:", "sfx:")
MUSIC_PREFIXES = ("music:", "score:", "soundtrack:")


def split_audio_music(text):
    """Lift `Audio: …` / `Music: …` lines out of a prompt into their own fields.

    Everyone writes the soundtrack into the prompt as an "Audio:" line — MiniMax's own
    ComfyUI templates do it too — while the guide wants it in overall_soundscape and
    non_diegetic_music. Only a line that *starts* with one of these labels counts, so
    prose that merely mentions sound is left where it is. Whatever follows such a line
    without its own label belongs to it.
    """
    if not text:
        return "", "", ""
    kept, sound, music = [], [], []
    current = None
    for line in text.splitlines():
        stripped = line.strip()
        low = stripped.lower()
        if any(low.startswith(p) for p in SOUND_PREFIXES):
            current = sound
            stripped = stripped.split(":", 1)[1].strip()
        elif any(low.startswith(p) for p in MUSIC_PREFIXES):
            current = music
            stripped = stripped.split(":", 1)[1].strip()
        elif not stripped:
            current = None            # a blank line ends the block
            kept.append(line)
            continue
        elif current is None:
            kept.append(line)
            continue
        if stripped:
            current.append(stripped)
    return "\n".join(kept).strip(), " ".join(sound).strip(), " ".join(music).strip()


def compile_storyboard(global_prompt, shots, total_seconds):
    """Global block, then timed shot lines in the ComfyUI templates' `[0s-1.5s]` notation."""
    written = [s for s in shots if (s["prompt"] or "").strip()]

    parts = []
    gp = (global_prompt or "").strip()
    if gp:
        parts.append(gp)

    if len(written) == 1 and written[0]["start_sec"] <= 0.01 and \
            written[0]["end_sec"] >= total_seconds - 0.01:
        # One shot covering the whole window is just a prompt — wrapping it in
        # storyboard syntax would imply a cut that isn't there.
        parts.append(written[0]["prompt"].strip())
    elif written:
        parts.append("Timeline:\n" + "\n".join(
            "[%s-%s] %s" % (fmt_seconds(s["start_sec"]), fmt_seconds(s["end_sec"]),
                            s["prompt"].strip())
            for s in written))

    return "\n\n".join(parts).strip()


def classify_events(events, duration_frames, fps):
    """Assign each main-track image/video segment its keyframe role.

    H3's PackedLayout only anchors keyframes at frame 0 and frame_count-1, so a timeline
    image is either the opening frame, the closing frame, or a reference.
    """
    head_window_f = max(1.0, fps * 0.5)
    have_first = have_last = False
    for ev in events:
        starts_at_head = ev["rel_start_f"] <= head_window_f
        reaches_tail = ev["rel_end_f"] >= duration_frames - 0.5

        # An explicit "end frame" flag always wins. Otherwise an image that merely runs to
        # the end of the window is only a closing frame if it did not also start at the
        # head — one clip spanning the whole timeline is the classic i2v case and means
        # "start from this image".
        if ev["is_end"] or (reaches_tail and not starts_at_head):
            if not have_last or ev["is_end"]:
                ev["role"] = ROLE_LAST
                have_last = True
                continue
        if starts_at_head and not have_first:
            ev["role"] = ROLE_FIRST
            have_first = True
            continue
        ev["role"] = ROLE_MIDDLE
    return events


def plan_timeline(tdata, win_start, duration_frames, fps, global_prompt="",
                  use_custom_motion=True, use_custom_audio=False, override_audio=False,
                  extra_ref_image_count=0, soundscape="", music="", prompt_format=None):
    """Work out shots, keyframe roles, reference ordinals and the final prompt.

    Returns a dict; `execute` uses it to decide what media to load, the endpoint just
    reads `prompt`.
    """
    win_end = win_start + duration_frames
    window_seconds = duration_frames / fps
    retake = retake_state(tdata)

    if not global_prompt:
        global_prompt = tdata.get(
            "retake_global_prompt" if retake else "global_prompt", "") or ""

    # The editor's two soundscape boxes live in the timeline, so both consumers read them
    # from the same place and no third copy can go stale. Unlike the global prompt these
    # have no retake twin on purpose: re-rolling part of a shot does not change what the
    # room sounds like. An explicit argument still wins, which is what lets a caller
    # override them without touching the timeline.
    if not (soundscape or "").strip():
        soundscape = tdata.get("overall_soundscape", "") or ""
    if not (music or "").strip():
        music = tdata.get("non_diegetic_music", "") or ""

    ref_mode_on = ref_mode_from(tdata)
    if prompt_format is None:
        prompt_format = str(tdata.get("prompt_format", FORMAT_MINIMAX)).lower()
    if prompt_format not in (FORMAT_MINIMAX, FORMAT_COMFYUI):
        prompt_format = FORMAT_MINIMAX

    # --- character slots (metadata only) ---
    char_slots = []          # [{"count": n, "images": [{"b64","name"}], "description": str}]
    for char_info in tdata.get("characters", []) or []:
        images_list = char_info.get("images", []) or []
        legacy_b64 = char_info.get("imageB64", "")
        if legacy_b64 and not images_list:
            images_list = [{"b64": legacy_b64, "name": char_info.get("fileName", "")}]
        char_slots.append({"images": images_list,
                           "description": char_info.get("description", "") or ""})

    # --- shots + image events ---
    shots, events = [], []
    if retake:
        # Retake replaces the timeline window: one shot over the marked range, anchored on
        # the base video's own frames either side of it.
        text = retake["prompt"].strip()
        if not text:
            for seg in sorted(tdata.get("segments", []) or [],
                              key=lambda s: float(s.get("start", 0))):
                if overlaps(seg, win_start, win_end) and (seg.get("prompt") or "").strip():
                    text = seg["prompt"]
                    break
        shots.append({"prompt": text, "start_sec": 0.0, "end_sec": window_seconds})
    else:
        segments = [s for s in (tdata.get("segments", []) or [])
                    if overlaps(s, win_start, win_end)]
        segments.sort(key=lambda s: float(s.get("start", 0)))
        for seg in segments:
            seg_start = float(seg.get("start", 0))
            seg_len = float(seg.get("length", 1))
            rel_start = max(0.0, seg_start - win_start)
            rel_end = min(float(duration_frames), seg_start + seg_len - win_start)
            shots.append({"prompt": seg.get("prompt", "") or "",
                          "start_sec": rel_start / fps, "end_sec": rel_end / fps})

            kind = seg.get("type", "image")
            if kind not in ("image", "video"):
                continue
            if not (seg.get("imageFile") or seg.get("imageB64")):
                continue
            events.append({"seg": seg, "rel_start_f": rel_start, "rel_end_f": rel_end,
                           "is_end": bool(seg.get("isEndFrame")), "kind": kind,
                           "name": seg.get("fileName", "") or "", "role": ROLE_MIDDLE,
                           # which shot this image belongs to, so a picture note can name
                           # it the way the guide does. Resolved to the *written* shot
                           # numbering below — the body only numbers shots that carry text.
                           "shot_index": len(shots) - 1})
        classify_events(events, duration_frames, fps)

    # --- reference ordinals, in the order the tokenizer will present them ---
    char_tag_values = {}
    ref_notes = []
    ref_image_slots = []     # {"source": "char"/"input"/"timeline", ...} in <Picture i> order

    if ref_mode_on:
        for slot_idx, slot in enumerate(char_slots):
            if not slot["images"] or len(ref_image_slots) >= MAX_REF_IMAGES:
                continue
            char_tag_values[slot_idx + 1] = "<Picture %d>" % (len(ref_image_slots) + 1)
            for img in slot["images"]:
                if len(ref_image_slots) >= MAX_REF_IMAGES:
                    break
                ref_image_slots.append({"source": "char", "slot": slot_idx, "image": img})
                ref_image_slots[-1]["slot"] = slot_idx

        for _ in range(max(0, int(extra_ref_image_count))):
            if len(ref_image_slots) >= MAX_REF_IMAGES:
                break
            ref_image_slots.append({"source": "input"})

        # Timeline images in the order they appear on the timeline. `events` is already
        # chronological, so <Picture n> counts up with time. Character slots and the
        # ref_images input stay ahead of all of them, so a character's number never shifts
        # when an image is dropped on the timeline.
        #
        # The wording is the reference guide's own, verbatim where it gives it:
        # "the shot begins from <Picture 1>", "the shot's keyframe corresponds to
        # <Picture 2>", "the shot ends on <Picture 3>". `<Picture N>` is precisely what
        # that guide asks for when an image "serves as a shot's first frame, keyframe,
        # last frame, edited keyframe, or composition anchor", so ref2va does have frame
        # anchors in its notation — what it does not have is FL2VA's vocabulary. Calling
        # these "opening frame" / "closing frame" borrowed from the wrong guide, and made
        # the phrasing depend on where a segment happened to end: flush with the window it
        # read as a closing frame, three frames shorter as something else entirely
        # (issue #4).
        #
        # Shots are named the way the body names them, which counts only shots carrying
        # text. An image whose own segment has no prompt gets the guide's shot-free
        # phrasing rather than a number the reader cannot find.
        #
        # The role also lives on the slot, where it is not about wording at all: it picks
        # which frame of a *video* segment becomes the reference (the last one for
        # ROLE_LAST) and whether it is fitted to the canvas. See minimax_director.py.
        written_shot_no = {}
        counted = 0
        for shot_i, shot in enumerate(shots):
            if (shot["prompt"] or "").strip():
                counted += 1
                written_shot_no[shot_i] = counted

        for ev in events:
            if len(ref_image_slots) >= MAX_REF_IMAGES:
                break
            ordinal = len(ref_image_slots) + 1
            slot = {"source": "timeline", "event": ev}
            if ev["role"] in (ROLE_FIRST, ROLE_LAST):
                slot["keyframe"] = ev["role"]
            shot_no = written_shot_no.get(ev.get("shot_index"))
            at = fmt_seconds(ev["rel_start_f"] / fps)
            if ev["role"] == ROLE_FIRST:
                ref_notes.append("[Shot %d] begins from <Picture %d>" % (shot_no, ordinal)
                                 if shot_no else
                                 "The video begins from <Picture %d>" % ordinal)
            elif ev["role"] == ROLE_LAST:
                ref_notes.append("[Shot %d] ends on <Picture %d>" % (shot_no, ordinal)
                                 if shot_no else
                                 "The video ends on <Picture %d>" % ordinal)
            else:
                ref_notes.append(
                    "The keyframe of [Shot %d] corresponds to <Picture %d>, at %s"
                    % (shot_no, ordinal, at) if shot_no else
                    "<Picture %d> is a composition anchor at %s" % (ordinal, at))
            ref_image_slots.append(slot)
    else:
        for slot_idx, slot in enumerate(char_slots):
            if slot["description"]:
                char_tag_values[slot_idx + 1] = slot["description"]

    # --- reference video / audio tracks ---
    ref_video_segs, ref_audio_segs = [], []
    if ref_mode_on:
        if use_custom_motion:
            motion = [s for s in (tdata.get("motionSegments", []) or [])
                      if s.get("videoFile") and overlaps(s, win_start, win_end)]
            motion.sort(key=lambda s: float(s.get("start", 0)))
            ref_video_segs = motion[:MAX_REF_VIDEOS]
        if use_custom_audio:
            audio = [s for s in (tdata.get("audioSegments", []) or [])
                     if (s.get("audioFile") or s.get("audioB64"))
                     and overlaps(s, win_start, win_end)]
            audio.sort(key=lambda s: float(s.get("start", 0)))
            ref_audio_segs = audio[:MAX_REF_AUDIOS]

    # --- total file cap ---
    # The per-type caps are not the whole story: H3 also takes at most 12 reference files
    # across all types. Trim from the back so the earlier, more deliberate references win.
    ref_warnings = []
    total_files = len(ref_image_slots) + len(ref_video_segs) + len(ref_audio_segs)
    if total_files > MAX_REF_FILES:
        excess = total_files - MAX_REF_FILES
        for bucket, label in ((ref_audio_segs, "audio"), (ref_video_segs, "video"),
                              (ref_image_slots, "image")):
            while excess > 0 and bucket:
                bucket.pop()
                excess -= 1
        ref_warnings.append(
            "H3 takes at most %d reference files in total; %d were dropped."
            % (MAX_REF_FILES, total_files - MAX_REF_FILES))
        # drop the notes for every picture that was trimmed, not just the first one
        kept_pictures = len(ref_image_slots)
        ref_notes = [n for n in ref_notes
                     if not (n.startswith("<Picture ")
                             and int(n[9:n.index(">")]) > kept_pictures)]

    # reference videos: each 2-15s, and no more than 15s of them together
    if ref_video_segs:
        budget = REF_VIDEO_TOTAL_SEC
        kept = []
        for seg in ref_video_segs:
            seconds = max(0.0, float(seg.get("length", 0)) / fps)
            if seconds < REF_VIDEO_MIN_SEC:
                ref_warnings.append(
                    "Reference video '%s' is %.1fs; H3 wants 2-15s per clip."
                    % (seg.get("fileName") or seg.get("videoFile"), seconds))
            # what this clip would actually contribute, after the per-clip cap
            usable = min(seconds, REF_VIDEO_MAX_SEC)
            # the question is whether it FITS, not whether anything is left at all
            if usable > budget + 1e-6:
                ref_warnings.append(
                    "Reference videos exceed the %.0fs total budget; '%s' was dropped."
                    % (REF_VIDEO_TOTAL_SEC, seg.get("fileName") or seg.get("videoFile")))
                continue
            kept.append(seg)
            budget -= usable
        ref_video_segs = kept

    # --- output length ---
    # Computed before the prompt because the alignment instruction has to name the
    # *effective* duration, i.e. after snapping to the 17k+5 grid, not the window length.
    length = align_frame_count(max(5, int(round(window_seconds * MODEL_FPS))))
    actual_seconds = length / MODEL_FPS

    # --- prompt ---
    subject_lines, subject_of_slot = [], {}
    if ref_mode_on and prompt_format == FORMAT_MINIMAX:
        subject_lines, subject_of_slot = build_subject_definitions(
            char_slots, ref_image_slots, ref_video_segs, ref_audio_segs)
        # a named subject beats a bare picture label: it survives across cuts
        for slot, subject in subject_of_slot.items():
            char_tag_values[slot] = "<Subject %d>" % subject

    global_prompt = substitute_char_tags(global_prompt, char_tag_values)
    for shot in shots:
        shot["prompt"] = substitute_char_tags(shot["prompt"], char_tag_values)

    if prompt_format == FORMAT_MINIMAX:
        # the guide keeps the soundtrack out of the shot description
        global_prompt, found_audio, found_music = split_audio_music(global_prompt)
        # A filled box wins, but the lifted line is then thrown away — and it may be work
        # the Enhance node's vision model just did. Say so rather than swallow it.
        for label, box, found in (("overall_soundscape", soundscape, found_audio),
                                  ("non_diegetic_music", music, found_music)):
            if (box or "").strip() and found:
                log.info("[MiniMaxDirector] %s came from the timeline box, so the %s line "
                         "in the prompt text was dropped.", label,
                         "Audio:" if label == "overall_soundscape" else "Music:")
        soundscape = (soundscape or "").strip() or found_audio
        music = (music or "").strip() or found_music
        retention_lines = []
        if subject_lines:
            named = ", ".join("<Subject %d>" % s for s in sorted(subject_of_slot.values()))
            if named:
                retention_lines.append(
                    "Keep the identity, face and clothing of %s consistent across every shot."
                    % named)
            if ref_video_segs:
                retention_lines.append(
                    "Follow the camera work and motion of %s."
                    % ", ".join("<Video %d>" % (i + 1) for i in range(len(ref_video_segs))))
            if ref_audio_segs:
                retention_lines.append(
                    "Keep the voice and timbre of %s."
                    % ", ".join("<Audio %d>" % (i + 1) for i in range(len(ref_audio_segs))))
        if ref_notes:
            retention_lines.extend(n + "." for n in ref_notes)
        # Only the fl2va path: ref2va has no keyframe slot and its guide asks for no
        # instruction line. `written` mirrors the shot numbering the body will use.
        instruction = ""
        if not ref_mode_on:
            written_shots = len([s for s in shots if (s["prompt"] or "").strip()])
            instruction = alignment_instruction(
                any(e["role"] == ROLE_FIRST for e in events),
                any(e["role"] == ROLE_LAST for e in events),
                written_shots, actual_seconds)
        prompt = compile_storyboard_minimax(global_prompt, shots, soundscape, music,
                                            subject_lines, retention_lines, instruction)
    else:
        prompt = compile_storyboard(global_prompt, shots, window_seconds)
        if ref_notes:
            prompt = (prompt + "\n\nReference notes: " + "; ".join(ref_notes) + ".").strip()
        if (soundscape or "").strip():
            prompt = (prompt + "\n\nAudio: " + soundscape.strip()).strip()
        if (music or "").strip():
            prompt = (prompt + "\n\nMusic: " + music.strip()).strip()

    fallback = not prompt
    if fallback:
        prompt = "video"

    has_keyframe = any(ev["role"] in (ROLE_FIRST, ROLE_LAST) for ev in events) or bool(retake)
    mode = "ref2va" if ref_mode_on else ("fl2va" if has_keyframe else "t2va")

    return {
        "prompt": prompt, "prompt_is_fallback": fallback,
        "shots": shots, "events": events, "retake": retake,
        "ref_mode_on": ref_mode_on, "mode": mode,
        "ref_image_slots": ref_image_slots, "ref_notes": ref_notes,
        "ref_video_segs": ref_video_segs, "ref_audio_segs": ref_audio_segs,
        "ref_warnings": ref_warnings, "prompt_format": prompt_format,
        "char_tag_values": char_tag_values,
        "win_start": win_start, "duration_frames": duration_frames, "fps": fps,
        "window_seconds": window_seconds,
        "length": length, "actual_seconds": actual_seconds,
    }
