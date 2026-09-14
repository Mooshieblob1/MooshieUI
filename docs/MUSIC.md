# Music studio

This guide describes the music tools included in v2.3.5.

## Setup

Music uses the configured ComfyUI backend. Select a YuE2 checkpoint under
**Music → Create → Generation settings**. MooshieUI offers the Comfy-Org BF16 and
INT8 ConvRot checkpoint downloads. The managed runtime is pinned to ComfyUI
`c75d8c966c29cb0392259af791f43373315b72db` (v0.35.0 + YuE2).

Restart ComfyUI after updating MooshieUI so its music adapter nodes are loaded.
Remote workers need compatible native YuE2 nodes and the current
`mooshie_nodes.py`. Capability discovery checks each worker; generation stays on
the worker with the selected checkpoint and requested controls. Older native
workers can generate songs and plans with automatic guidance, but cannot report
the new truncation receipts.

**Enhance style**, **Generate lyrics**, and **Edit composition** use your existing
Prompt Assistant configuration. The music guidance adapts the upstream
[yue2-music skill](https://github.com/multimodal-art-projection/YuE/blob/88da114a67df892af0329472073b96a5ef700b93/skills/yue2-music/SKILL.md).
It does not install an agent or Python runtime. Music generation and transcription
do not require that assistant.

## Create and review a score

1. Enter a style, sectioned lyrics and maximum recording length. Full planning
   requests melody and harmony; melody planning requests melody alone.
2. Choose **Generate score first** to produce ABC without rendering audio. The
   resulting draft is saved with its style, lyrics, seed and available receipt.
3. Review the draft and choose **Use this score, style and lyrics**. This applies
   all three together. **Undo** restores the previous draft while it is unchanged.
4. Select **Generate song** to render the reviewed score, or edit it first.

You can also import ABC, type in the score editor, or generate directly with an
empty score. Off planning generates without a score; clear ABC before using it.
Saved plans are available from **Saved score drafts**.

The workbench interprets the native two-voice YuE2 format, checks bar lengths,
ties, supported chords, key and meter changes, and shows the nominal duration.
Unsupported general ABC notation is reported rather than silently rewritten.
Duration comes from the score's quarter-note tempo and bar grid; it is an estimate
of the composition, not a promise of the rendered audio length. A warning appears
when it exceeds the generation ceiling.

**Preview score** shows a piano roll with vocal and instrumental melodies and
optional basic chord voicings. Playback uses a small synthesizer, not a singer or
the generated accompaniment. **Export MIDI** includes the selected melodies and
optional reference chords. **Export printable SVG** saves the displayed page
(up to 32 bars); longer scores have page controls. ABC and MIDI exports contain
the complete score. **Apply tempo** changes `Q:` and checks that both melodies and
their timing in beats stay intact.

## Cover a recording

Choose **Cover a song**. Import a score or expand **Transcribe a source recording**.
For native transcription, select **ComfyUI SheetSage2** and an audio encoder.
An administrator or moderator can download `sheetsage2_bf16.safetensors`
(1.39 GB) into the host's `models/audio_encoders` folder. A remote transcription
worker needs that encoder installed on its own machine.

Native transcription uses `AudioEncoderLoader` and `SheetSage2AudioToABC` through
ComfyUI's model manager, with a bounded temporary audio loader. Upload and job
submission target the same worker. Source files can be WAV, MP3, FLAC, M4A, OGG,
Opus or AIFF, at most 64 MiB and 360 seconds. Oversized decoded recordings are
rejected, not silently cropped.

Importing source audio sets **Maximum length** to the recording's duration,
rounded up to the next whole second. You can extend it afterward; repeated audio
metadata events do not reset your edit. **Use source length** restores that default.
If your browser cannot read the duration, set it manually. The source-length
ceiling limits generation; it does not force lyric timing and can cut off a
performance that runs longer than the source.

Choose **Keep melody, adapt harmony** for the documented YuE2 cover workflow:
melody planning and a score without chord symbols. **Keep melody and harmony**
uses full planning with the supplied chords. Part selection can retain the vocal
melody, instrumental melody, or both; applying the selection replaces unwanted
notes with rests and checks the retained melodies. It cannot recover a discarded
part without Undo or reapplying the transcription draft.

Transcription produces a separate draft. It does not replace the editor when the
job finishes. Review the notes, meter, section order and lyric fit, then apply it
and check the review box before generating. Covers regenerate the recording;
they do not clone the source singer or preserve its waveform.

Native uploads are removed after decoding, including decoder failures. Cancelled
jobs that never reach the loader can leave temporary uploads; a later cover load
cleans up app-owned uploads older than 24 hours. Source recordings are not saved
in the song library or project exports. Active native jobs can be recovered after
a page reload while the host retains their ownership and ComfyUI retains their history.

The existing **Separate Python environment** option remains available for
melody-only transcription. Its environment setup, timeouts and retention differ
from the native route; see [separate-environment setup](research/yue2-integration.md#source-recording-transcription).
The official [cover guide](https://github.com/multimodal-art-projection/YuE/blob/88da114a67df892af0329472073b96a5ef700b93/docs/covers.md)
also explains external transcription and ABC import.

## Review a cover's lyric timing

1. Select the generated recording and choose **Review lyric timing** beside its
   title, or from its library edit menu.
2. Attach the original recording. A source imported in the current session is
   selected automatically. Both recordings must be at most six minutes and 64 MiB.
3. Leave **Source and cover use the same lyrics and tempo** checked only when that
   is true. With rewritten lyrics or an intentional tempo change, uncheck it;
   the review then checks recognition of the target lyrics without comparing
   source entrance times.
4. Choose **Analyze recordings**. MooshieUI uses xAI speech-to-text, then asks your
   configured Prompt Assistant to explain the measured differences. xAI must be
   configured in Assistant settings with a sign-in or API key that permits STT.
   The existing token refresh is used. This is separate from SheetSage2 notation
   transcription and requires no additional local model.
5. Read the review and play its **Cover** and **Source** links to verify issues.
   Expand **Line evidence and transcripts** for the recognized words and times.

The review compares normalized recognized words in lyric order. A positive
entrance difference means the cover starts that line later. The overall offset
is the median entrance difference; change from early to late measures how that
difference develops. These estimates require three distinct, sufficiently matched
lines spanning at least five seconds. Repeated lines and uncertain entrances are
excluded; missing evidence is marked unassessed. Rewritten lyrics and changed
tempos need a different alignment method and are not rated as synchronized.

Speech recognition can mishear singing. A missing word in the transcript does
not prove that the singer omitted it. This text-based review cannot judge melody,
vocal quality, pronunciation or exact syllable-to-note alignment. It does not
repair a recording or change lyrics, scores or manual playback timing.

The action sends the selected recordings to xAI and the timing evidence, style,
score summary and generation receipt to your configured assistant. Reports and
transcripts are saved with the song in the account's device-local library.
Source audio remains session-only; after reloading, attach the exact recording
to enable its saved playback links. MooshieUI checks its audio hash. Completed
transcripts are reused; **Explain saved results** retries only the explanation.
Project exports include saved review data in their manifest, but not source audio.
Closing prevents subsequent steps and ignores late replies; an upload already
sent may still finish. There is no automatic transcription retry.

Validation includes live xAI transcription of a 4.74-second synthetic spoken
clip and a valid Grok explanation through the configured backend. This confirms
endpoint access, token refresh and the request/response flow, not accuracy on a
particular song. See [review methods and limitations](research/music-cover-review.md).

A subsequent test on the user's original Vocaloid recording returned no recognized
words, so the review could not measure its timing drift. Keeping the original
lyrics does not make YuE2 preserve their original sung timestamps. MooshieUI does
not provide automatic correction of sung timing. Experimental retiming and voice
conversion were abandoned after unsuccessful listening tests.

## Edit a composition

**Edit composition** operates on the complete current score, or on a saved song
from its library menu or version comparison. When editing a recording, MooshieUI
saves the original audio before requesting a proposal.

Choose a starting request for reharmonization, tempo, transposition, form or lyric
translation, then describe the intended change. Select which melodies, note
timing, tempo, exact lyrics and section structure to preserve. The assistant
returns complete ABC, style and lyrics. MooshieUI parses the proposal and checks
the selected constraints before displaying it. Invalid replies get at most one
correction attempt, retaining the complete original context.

Review the proposal and choose **Apply to draft**, then generate a new version.
Closing the dialog discards any late reply. If the draft or account changes while
the request is running, the result cannot overwrite it. Requests are bounded to
64,000 characters of combined score/style/lyrics and 16,384 output tokens; large
or incomplete responses can fail validation and leave the draft intact.

These checks verify symbolic score and text properties. They do not prove that
the generated singing follows the score, that a translation fits perfectly, or
that a new arrangement sounds better. Listen to the resulting versions.

## Versions, comparisons and exports

**Create version from this song** copies its score, settings and actual seed into
the editor, retaining a parent relationship. Checked edits also record their
change summary and preservation checks. Recordings remain separate entries in
the device-local library; their original audio is not overwritten.

Open **Versions, compare and export**, select A and B, then play either recording.
Starting one pauses the other. You can retain the playback position when
switching or loop an excerpt. The same seconds may refer to different passages
after a tempo or structure change. The comparison reports differences between
the two symbolic melodies, timing and tempo separately from audio playback.

Export the selected recordings or every version in A's project as a ZIP. It
contains original FLAC bytes, ABC, lyrics, style, request settings, actual seeds,
lineage and available generation receipts. Exports are limited to eight versions
and 256 MiB at once. They do not include uploaded cover sources, model weights,
semantic tokens or acoustic latents. This is a MooshieUI project bundle, not the
YuE2 SDK's complete `save_artifacts` output or a project import format.

## Candidates, sampling and completion

Choose 1, 2, 4 or 8 candidates. Multiple candidates use distinct seeds and run
sequentially. The first uses the entered seed, unless it is random. Each result
retains its settings and candidate group. A failed attempt is recorded and the
batch continues. Cancel stops the current job and pending attempts.

Active job handles, plans and batch attempts persist per account on this device.
Reloading observes an already submitted job; it does not submit it again or
automatically start remaining candidates. **Resume pending candidates** continues
unsubmitted attempts. Backend restarts or lost submission responses may require
checking ComfyUI's queue before starting another batch.

Advanced sampling exposes temperature, top-p, top-k, repetition penalty, ABC
token budget and semantic guidance, with a reset button. Acoustic KSampler CFG
remains 1. Automatic semantic guidance follows the released defaults: 1 with a
score, 1.01 without. Settings are copied into every version.

The adapter reports semantic truncation from the native encoder's actual flag.
Planner truncation follows the pinned generator's end-token contract: exactly
the maximum number of returned non-end tokens means its budget was exhausted.
Warnings distinguish these stages. Legacy results retain **unknown** rather than
guessing from recording duration.

## Validation boundary

The implementation has synthetic tests for native graph wiring, worker routing,
receipts, source decoding bounds, musical invariants, assistant retries, storage,
recovery and exports. Headless Chrome exercises the actual Svelte UI with synthetic
IPC, including planning, checked edits, preview, A/B playback, MIDI/ZIP export and
native transcription submission. These checks do not qualify new live model
inference, installed Tauri/WebView behavior or musical quality on a particular GPU.
See the [integration record](research/yue2-integration.md) for dated evidence.

The ABC interpreter adapts the upstream skill helper under Apache-2.0; the
[license](../third-party/yue2-music-LICENSE.txt) is also included in packaged web
assets at `licenses/yue2-music-LICENSE.txt`.
