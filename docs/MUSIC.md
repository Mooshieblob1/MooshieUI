# Music studio

This guide describes the music tools included in v2.3.6, including reference-song
lookup, temporary song-link imports, audio style analysis and saved style profiles.

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

1. Enter a style, optional sectioned lyrics and maximum recording length. Full planning
   requests melody and harmony; melody planning requests melody alone.
2. Choose **Generate score first** to produce ABC without rendering audio. The
   resulting draft is saved with its style, lyrics, seed and available receipt.
3. Review the draft and choose **Use this score, style and lyrics**. This applies
   all three together. **Undo** restores the previous draft while it is unchanged.
4. Select **Generate song** to render the reviewed score, or edit it first.

You can also import ABC, type in the score editor, or generate directly with an
empty score. Off planning generates without a score; clear ABC before using it.
Saved plans are available from **Saved score drafts**.

Leave **Lyrics** empty for a purely instrumental track. Whitespace-only
lyrics also count as empty. MooshieUI requests instruments only, without vocals,
singing, speech, humming or choir, in both score planning and audio generation.
This also applies to covers and supplied scores. Your saved style and score stay
editable; adding lyrics enables a vocal track again. **Enhance style** and reference
song style drafts respect the instrumental intent. **Generate lyrics** still writes
lyrics when you explicitly choose it. Rendered results depend on YuE2's adherence
to the prompt; this is not a vocal-removal filter.

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

### Auto style from audio

In **Styles**, enable **Auto style from audio**. Choose the current cover source,
upload a separate style reference, or explicitly select one of your generated
recordings. Choose **Analyze audio**, review what the model heard and its
uncertainties, edit the proposed style, then choose **Apply to Styles**. Undo
restores the previous style unless you have edited it since. Playing a song does
not automatically upload it. Existing style text is never replaced automatically.

You can also leave **Styles** empty, enable **Auto style from audio**, select a
reference, and choose **Generate**. The app analyzes that reference, fills the
empty style field and continues generation. A failed or cancelled analysis stops
generation. Changes to the draft or reference while analysis is running stop
automatic submission. If Styles already contains text, Generate uses it unchanged.

In **Settings > Prompt Assistant**, choose **Gemini (Google sign-in)** and sign in
from the desktop app, or configure an audio-input model using **OpenRouter** or a
**Custom** Chat Completions endpoint that accepts MP3 `input_audio`. Gemini uses
Google's official Antigravity companion and accepts the converted audio
directly through its companion protocol. The app checks that the installed
companion advertises audio support. OpenRouter's
model catalog is checked for audio input; a custom endpoint must support this
contract itself. Generic text/image models and speech-transcription endpoints
are insufficient. The panel displays the model and destination before sending
audio; account limits, any API charges and provider data-retention policies apply. Analysis sends
the recording and the requested duration, instrumental/vocal mode and interface
language, not your filename, full lyrics or existing style.

**ChatGPT (subscription)** also supports assistant text and image requests, but
does not provide music audio analysis. Claude remains an API-key provider.
See [account sign-in](PROMPT-ASSISTANT-SIGN-IN.md) for setup and limits.

Enable **Analyze a selected section** to enter start/end seconds. Load the audio
preview, use **Start here** / **End here**, and **Play selection** to audition it.
Only that excerpt is sent to the provider; the result records its exact range.
The file still passes through your app's host in browser/server mode. Input is
bounded to 64 MiB, and the analyzed section to six minutes. Without a section,
recordings over six minutes are rejected instead of silently truncated. Ranges
must fit inside the recording. This uses the startup prerequisite installer on Windows,
macOS and Linux. No local music-analysis model is downloaded. Conversion files
are deleted before the provider request; source uploads and cached previews stay
only in the current panel/session. Replacing the source, changing accounts or
selected songs, disabling the option or leaving the panel clears those
references. Your saved generated recordings remain in the music library.

Cancel stops local processing and drops the active provider request; it cannot
recall audio already received by a provider. Jobs are private to each browser
account and expire when polling stops. Changed inputs invalidate late results.
Repeated analysis of the same source and target within the panel reuses the last
profile without another paid request.

The source description can mention vocals while the proposed output style
remains instrumental when lyrics are blank. Genre, timbre and tempo descriptions
are listening estimates, not exact measurements or song identification. This
creates text guidance for YuE2; use the separate cover transcription workflow
for a reviewed melody score. It does not transfer the original singer or add
native audio conditioning.

### Saved style profiles

After analysis, name the accepted style and choose **Save style profile**. The
saved record contains your edited style, the original source observations and
uncertainties, audio hash/range, analyzer/model, date, and target duration,
vocal mode and language. It never contains the source audio. Up to 100 profiles
are stored in the device-local library, separately for each browser account.
Replacing temporary audio does not delete these deliberately saved text profiles.

Open **Saved style profiles** to reuse, rename or delete one. Reuse with the same
target needs no provider request. If duration, vocal mode or language changes,
**Adapt to this song** sends the saved text to your configured Prompt Assistant,
then presents an editable draft with Apply/Undo. It does not resend or reanalyze
audio, and it keeps the saved source observations unchanged. Saved profiles and
comparison notes are currently device-local and are not included in project ZIPs.

### Draft a style from a reference song

In **Styles**, enable **Use a reference song**, enter a song title and optionally
an artist and version/remix, then choose **Find song**. Select the matching
recording from the results and choose **Find style**. The configured Prompt
Assistant LLM creates a preview; **Apply to Styles** replaces the style field,
and Undo restores it if you have not edited the applied text.

Lookup uses the public US Apple Music/iTunes song catalog. Results show the
artist, release and version so you can distinguish studio recordings, live
performances, remixes and covers. Matching English Wikipedia song information is
included when available; the source links remain visible. No Apple account or
additional search API key is required. Catalog coverage and service availability
can limit results. This lookup sends the search text to Apple and the selected
song title/artist to Wikipedia; it sends the retrieved context and current music
writing context to your configured LLM. It does not download or analyze audio.

Catalog facts and retrieved song information provide context; the style paragraph
is an AI draft, not a measured reconstruction. The preview lists inferred or
suggested musical details. A broad catalog genre alone cannot verify exact BPM,
key, instrumentation or production. If the model cannot describe that recording
reliably, it leaves the style unchanged. Edits, recording changes, account changes
and closing the panel invalidate late responses. Nothing is applied automatically.

### Import a song link

Under **Transcribe a source recording**, paste a link and choose **Import song**.
YouTube, YouTube Music and Dailymotion download the linked recording. Spotify,
Deezer and Tidal look up the public song title and artist, then search YouTube
for an audio version. These three services are **matching inputs**, not direct
downloads from their subscriptions. The source and original-song links are
shown beside the preview. Listen to verify the artist, arrangement and version
before transcribing; an automatic match can select the wrong recording.

**Song and artist** optionally overrides the matching search, including when a
service's public metadata is unavailable. Use a single track/video URL, not an
album or playlist. Recordings are limited to six minutes and 64 MiB. Private,
sign-in-only, unavailable and blocked recordings may fail; manual uploads remain
available. The importer does not use account cookies or bypass DRM.

MooshieUI automatically prepares **yt-dlp, FFmpeg, Deno and Node.js** in the
background whenever it opens, including the headless server. Setup progress
appears beside the song-link input. The rest of the app and manual uploads remain
usable while setup runs; the importer becomes available immediately afterward.
First use needs an internet connection. Connection failures retry automatically,
then offer **Retry setup** without requiring an app restart.

Tools are installed under the host's MooshieUI app-data folder (`bin/media`),
without administrator privileges or system `PATH` changes. Working managed copies
are verified and reused offline on later launches. Compatible existing FFmpeg,
Deno and Node.js installations are reused; missing, outdated or damaged tools are
installed automatically. The app uses a self-contained yt-dlp package with EJS,
and passes both runtime paths explicitly. Browser clients use the host's tools.
Supported automatic packages cover Windows x64, macOS x64/Apple Silicon and Linux
x64/ARM64. Other hosts need compatible executables configured manually.

Package versions, download URLs and published SHA-256 checksums are pinned in
`src-tauri/src/media_tools_manifest.json`; every download is verified before
extraction or execution. Windows FFmpeg comes from Gyan; macOS/Linux FFmpeg comes
from Martin Riedl's builds. Other tools use their upstream release packages.
Downloads respect the app's network proxy setting. Advanced host overrides use
absolute `MOOSHIE_YT_DLP`, `MOOSHIE_FFMPEG`, `MOOSHIE_DENO` and `MOOSHIE_NODE` paths
when no valid managed copy is present. An overridden Python yt-dlp installation
should include `yt-dlp[default]`. Tool versions can require updating when services
change; tool setup cannot guarantee that every public recording is downloadable.

The importer converts audio to an MP3 preview and deletes its download and
conversion files **before** delivering that preview. The preview stays in memory
and is discarded when a different source is imported or manually uploaded, a
different library song is selected, **Clear source** is used, this panel is left,
or the app closes. It is not saved in settings, the song library or project files.
Cancellation and normal desktop/server shutdown stop active imports and remove
their working files. Disconnected browser imports expire after 30 seconds without
polling (metadata requests have a 20-second timeout); unclaimed results also
expire. An abrupt OS/process crash cannot run normal shutdown cleanup.

After **Transcribe melody**, the transcription backend's separate temporary-upload
lifecycle below applies, including queued native jobs that are cancelled before
decoding. Converting/importing a link does not start transcription automatically.

### Transcribe and review

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

**Edit composition** operates on the current draft, or on a saved song
from its library menu or version comparison. When editing a recording, MooshieUI
saves the original audio before requesting a proposal.

Choose **Style and production only**, **Lyrics only**, **Notes and chords only**,
or **Style, lyrics and score**. Style and lyric edits work without a score; note
edits require a complete supported ABC score. The production, harmony, tempo,
key, form and translation examples select a suitable starting scope. You can
adjust the request and choose which melodies, note timing, tempo, exact lyrics
and section structure to preserve. The assistant returns only selected fields;
protected fields are copied unchanged by the app. MooshieUI checks
the selected constraints before displaying it. Invalid replies get at most one
correction attempt, retaining the complete original context.

Review the **Before** and **After** fields and choose **Apply to draft**, then
generate a new version. Undo restores the previous draft if it has not changed.
Closing the dialog discards any late reply. If the draft or account changes while
the request is running, the result cannot overwrite it. Requests are bounded to
64,000 characters of supplied context and 16,384 output tokens for note edits
(4,096 tokens for style/lyric edits); large
or incomplete responses can fail validation and leave the draft intact.

These checks verify symbolic score and text properties. They do not prove that
the generated singing follows the score, that a translation fits perfectly, or
that a new arrangement sounds better. Listen to the resulting versions.

### Arrangement planner prototype

Open **Arrangement planner · Prototype**, suggest sections or add your own, then
reorder them, assign whole seconds and add optional musical directions. Suggestions
use existing lyric section markers when present, or instrumental themes when
lyrics are blank. **Fit to duration** adjusts section lengths to the current
maximum length. A valid plan has 1–12 sections within that duration.

**Preview style guidance** produces editable timing instructions; **Apply to
Styles** and Undo work without a provider call. Section timing is approximate.
This prototype changes style guidance for a new generation: it does not rewrite
existing ABC, splice audio, continue a recording or preserve untouched audio.
For a changed score structure, generate and review a new score. Planner rows are
session-only; applied style text saves with the normal draft and generated song.

## Versions, comparisons and exports

**Create version from this song** copies its score, settings and actual seed into
the editor, retaining a parent relationship. Checked edits also record their
change summary and preservation checks. Recordings remain separate entries in
the device-local library; their original audio is not overwritten.

Open **Versions, compare and export**, select A and B, then play either recording.
Starting one pauses the other. You can retain the playback position when
switching with **Play A** / **Play B**, including after pausing, or loop an excerpt
within both recordings. Switching clamps the position to their shared duration.
The same seconds may refer to different passages
after a tempo or structure change. The comparison reports differences between
the two symbolic melodies, timing and tempo separately from audio playback, and
lists differing generation settings including actual seeds and sampling options.

**Match listening volume** uses local FFmpeg to estimate integrated loudness
from each converted recording, then attenuates the louder take during playback.
It never boosts audio, changes the saved file or sends audio to an AI provider.
The measurement accepts recordings up to six minutes / 64 MiB; silence and very
short or unsupported files may not provide a usable measurement. Unchecking
restores the previous player volumes; manual volume adjustment exits matching.

Save your own votes for style match, sound clarity, arrangement and ending, plus
listening notes. They persist for the recording pair on this device/account;
swapping A and B preserves which recording received each vote. These are human
listening judgments, not automatic quality scores.

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
