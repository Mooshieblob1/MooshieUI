# YuE2 integration — September 12, 2026

YuE2 generation uses native ComfyUI models and interfaces, without a separate
YuE2 runtime. MooshieUI's small adapter nodes expose semantic guidance and actual
generation-limit receipts. Covers can use native ComfyUI SheetSage2 or the existing
separate-environment bridge. See the [current music studio guide](../MUSIC.md).

The [September 14 capability assessment](#capability-assessment-2026-09-14)
records the pre-implementation comparison with upstream documentation. The eight
approved feature groups are now implemented in the working tree; deferred stage
archives, alternate decoders, similarity search and automatic quality scoring
remain outside this change. This is not a release announcement.

The subsequent [cover timing review](music-cover-review.md) adds opt-in xAI
speech-to-text and evidence-based explanations through the configured assistant.
It includes source/cover playback links, saved transcripts and source-length
defaults. This is approximate lyric review, separate from quality scoring.

## Upstream status

- The supplied [YuE repository](https://github.com/multimodal-art-projection/YuE)
  now contains YuE2, including lyrics/style conditioning and editable ABC scores.
- ComfyUI [PR #16250](https://github.com/Comfy-Org/ComfyUI/pull/16250) merged at
  `b058ec652802e1de24f1d429b3c7c1fb868f791c` on September 11, 2026, 23:34 UTC.
  Its [maintainer example](https://github.com/user-attachments/files/32133499/yue2_full.json)
  supplies the sampling and tiled-decoding settings used here.
- Plain ComfyUI `v0.35.0` lacks YuE2. MooshieUI now installs the tested source
  `c75d8c966c29cb0392259af791f43373315b72db` automatically, labelled
  **v0.35.0 + YuE2**, using `src-tauri/runtime/comfyui-source.json`.
  The baseline release remained `v0.35.0` for the compatibility bot until a
  newer release passed the gate. The baseline is now `v0.37.0`, which ships
  YuE2 natively (added in `v0.36.0`), so the override has expired and installs
  use the tag.
  Git install/update and the ZIP fallback resolve the same immutable source.
  Docker and the macOS setup script use the same source resolver by default.
- Existing plain `0.35.0` installs are offered the feature update even though
  the source's version string still reads `0.35.0`. Startup auto-update covers
  managed desktop installs; the Music page also offers update/restart directly.
  Remote hosts remain under their operator's control.
- Music capability discovery checks `/object_info` after restart. Healthy
  servers are connected before any checkpoint is installed, so a first-time
  user can use the model download buttons. No manual development checkout is
  required for managed installs.
- Community alternatives include [ScryptHunter/ComfyUI-YuE2](https://github.com/ScryptHunter/ComfyUI-YuE2)
  and [T8mars/Comfyui-YuE2-T8](https://github.com/T8mars/Comfyui-YuE2-T8).
  These expose different node contracts and are not used by this implementation.
  The older `ComfyUI_YuE` pack is not evidence of YuE2 support.

## Models and hardware

The [official Comfy-Org repack](https://huggingface.co/Comfy-Org/YuE2) goes in
`ComfyUI/models/checkpoints`. It supplies the model, text encoder, and VAE through
`CheckpointLoaderSimple`; no original inference wheel is installed.
Downloads are pinned to model revision `8e6fcf0f23252ed188b634bd50d44f4b01fba890`
and checked against the SHA-256 digests below.

| Checkpoint | Bytes | SHA-256 |
| --- | ---: | --- |
| `yue2_3b_bf16.safetensors` | 7,799,983,228 | `33765adbf9813c9a50318218760b2fd819a319862460a04884607581961c6fee` |
| `yue2_3b_int8_convrot.safetensors` | 3,960,938,800 | `96fe199377309001ed8cd26a944baeee8cc31a20ba7c36d1d3c0a7e1f4149db6` |

The model card declares **CC BY-NC 4.0** for the weights. The original inference
package declares Apache-2.0 for its code; these are separate licenses.
The [official inference guide](https://github.com/multimodal-art-projection/YuE/blob/main/docs/generation.md)
starts from a BF16-capable NVIDIA GPU with 24 GB VRAM. INT8 checkpoint size does
not establish peak generation VRAM or prove support for a particular GPU.
No macOS/Metal qualification is claimed.

## Implemented behavior

- Desktop and mobile Music navigation; shared Svelte workspace in browser mode.
- Style, sectioned lyrics, maximum duration (1–360 seconds), seed, and steps.
- **Enhance style** and **Generate lyrics** above their respective text boxes,
  using the existing Prompt Assistant model or API provider. Generate lyrics
  opens a modal for a topic, story, mood, perspective or language. Opening the
  modal does not call the LLM; submitting the topic starts generation.
- Full/melody score planning, direct generation, or a caller-edited ABC score.
- Dedicated **Cover a song** mode with source playback, SheetSage2 transcription,
  editable ABC import, chord-symbol removal, Undo, and a score-review gate.
- Official BF16/INT8 downloads through existing download permissions and hash checks.
- Shared GPU submission, per-user prompt ownership, worker-specific history and
  audio retrieval, cancellation, FLAC playback/download, and score/settings reuse.
- Native `YuE2GenerateMusic` supplies the actual duration to
  `EmptyYuE2LatentAudio`. Sampling uses `dpm_2`, `sgm_uniform`, CFG 1, and
  32 steps by default. Decode tiles/overlap are 1920/128, matching the example.
- Native `SaveAudio` writes `ComfyUI/output/audio/mooshie_yue2_*.flac`, including
  ComfyUI workflow metadata. Audio does not enter the JXL image gallery pipeline.
- Queue broadcasts identify music jobs so image-generation progress does not
  mistakenly adopt them. Music results and settings are held separately.
- Create/Library navigation, optional song titles, editable library titles,
  playlists, search, ordered and shuffled playback, and a global bottom player.
- Audio, titles, playlists and manual lyric timings persist in a per-account
  IndexedDB library on the current device; saved songs work without ComfyUI history.

### YuE2 writing assistant

`src/lib/utils/yue2Skill.ts` supplies a task-specific system prompt through the
existing `call_external_llm` desktop/server route. The backend retains provider
credentials and selects the configured external or local model. This path skips
image-tag grounding. Writing needs a configured Prompt Assistant; it does not
require a connected ComfyUI worker.

The [official generation guide](https://github.com/multimodal-art-projection/YuE/blob/main/docs/generation.md)
and [example request](https://github.com/multimodal-art-projection/YuE/blob/main/examples/song.json)
keep musical direction in `style` and sectioned sung words in `lyrics`.
The injected skill follows that division, retains the user's language and
creative intent, and asks for singable phrases with natural stress. Style
enhancement adds coherent vocal, instrumental, groove, tempo, production and
arrangement details in one paragraph. The style field now shows four rows.

Both writing actions also inject a task-scoped adaptation of the upstream
[yue2-music agent skill](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/SKILL.md)
and its [ABC editing](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/abc-editing.md),
[editing workflow](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/editing-workflows.md)
and [listening/evaluation](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/listening-and-evaluation.md)
references, checked September 14, 2026. The adaptation supplies musical guidance
to the existing text-writing call; it does not install or execute the upstream
Python helpers, generate audio, edit ABC, or create listening comparisons.

The request now carries cover/planning mode and the active ABC score. Scores up
to 8,000 characters are included completely; longer scores supply beginning/end
excerpts, the omitted character count, and a bounded summary of line-level
tempo, meter, key, rhythmic-unit, voice and section markers. Excerpts are explicitly
identified as incomplete, and the full editor score stays intact. Off planning
omits the inactive score. Cover mode always supplies melody-planning context.

The assistant uses this context to preserve existing musical constraints, respect
both vocal and instrumental melodies, and distinguish free accompaniment from
fixed harmony. Lyric adaptation considers phrase order, stress, vowels, pickups,
ties, breathing rests and instrumental passages. When the existing draft is
included, its sections and phrase lengths guide revisions or singable translation.
Explicit changes requested by the user remain creative instructions; changing
text does not silently apply corresponding tempo, harmony or structure edits to
the ABC. The model is instructed to distinguish intended score fit from measured
audio alignment or listening evidence. Output validation rejects leaked ABC,
runtime commands and API fields and permits one corrective retry.

Lyric length uses an application heuristic, not an upstream duration guarantee:
reserve 30% of maximum seconds for breaths, transitions, instruments and the
ending, then allow up to 1.5 sung word equivalents per remaining second. The
draft targets 70–100% of that allowance and can use less for slow or spacious
music. All repeated sections count; CJK characters count as half a word
equivalent. For example, a 120-second limit asks for roughly 88–126 words.
Very short clips request a short hook rather than full song structure. Actual
sung timing still depends on the generated composition and performance.

Malformed or oversized responses get one correction request. A second failure
preserves the current text. Successful changes have per-field Undo; edits made
while a request is pending prevent its result from replacing the new draft.
Changes to the score, planning mode or cover mode also invalidate a pending result.
Leaving the page also prevents late application. Existing ABC scores stay in
place, with a reminder to check lyric compatibility or clear the score.
The modal creates fresh lyrics by default, omitting existing lyrics from the
LLM request and any correction retry. Selecting **Use existing lyrics as context**
includes the current draft and treats the topic field as editing instructions.
The checkbox resets to unchecked whenever the popup opens and is disabled when
there are no existing lyrics. Current style and maximum seconds remain in every
request. Cancel or Escape discards a pending result without changing the draft.

`node scripts/test-music-writing.mjs` covers the real skill and store with a
synthetic LLM, including duration boundaries, multilingual and repeated lyric
counts, retry context, failures and concurrent requests. This does not establish
live provider quality or confirm that YuE2 will finish singing every draft.
It also verifies score/mode context, bounded excerpts, draft opt-in, musical
guidance, field-only correction and stale score/mode protection. The production
build and scoped Svelte type/accessibility checks pass for the skill integration.
The production frontend build, locale parity, and type/accessibility checks for
the changed files pass. The full type check reports three errors outside these
files. No browser was enabled for interactive UI verification in this session.

## Validation and remaining qualification

Rust tests cover parameter limits, exact seeds, graph wiring, automatic/custom/off
planning, missing nodes, output paths, error histories, user ownership, queue
classification, and fetching a finished result from its original worker after
queue cleanup. The HTTP test uses a mock ComfyUI server; it is not model inference.

Validation on this checkout:

- `npm run build` and `npm run check:i18n` pass.
- Desktop and `--no-default-features --features server` Rust compile checks pass.
- Server library suite after the managed-runtime changes: 563 passed,
  0 failed, 1 ignored.
- Desktop library suite after the managed-runtime changes: 577 passed,
  0 failed, 1 ignored. The ordinary test
  executable fails at Windows startup with `STATUS_ENTRYPOINT_NOT_FOUND` because
  it has no Common Controls v6 manifest. A temporary copy with that manifest
  embedded using the Windows SDK `mt.exe` runs successfully. No application
  build configuration or installed runtime was changed for this workaround.
- Svelte checking reports three unrelated errors in `promptSegmentDetail.ts`,
  `ColorTooltip.svelte`, and `UpdateNotification.svelte`; no errors are reported
  for the music implementation.
- Interactive browser verification was unavailable: no browser connection was
  exposed to the automation tools. UI compilation is verified; visual and
  interactive behavior still need a live check.

### Live qualification

Tests used separate ComfyUI checkouts, a separate Python environment, and
separate MooshieUI data/gallery directories under
`error-logs/yue2-validation/`. The installed ComfyUI and its environment were
not updated. The test environment reused the installed GPU libraries and
installed each checkout's exact additional requirements locally.

Hardware/runtime: Windows, NVIDIA RTX 5070 (12 GB), Python 3.11.15,
PyTorch 2.12.0+cu130, torchaudio 2.11.0+cu130, Transformers 5.8.1,
comfy-kitchen 0.2.33, comfy-aimdo 0.5.3. Torch, torchaudio, and CUDA imports
succeeded. These results do not qualify other package combinations or platforms.

**ComfyUI v0.35.0**, commit
`40c4fcdf513a4523e39d54a9d391908af8df8171`:

- The repository's CPU compatibility smoke test passed: all 15 required
  bundled nodes, both checked core input signatures, 939 registered nodes.
- The same node/interface checks passed with CUDA enabled.
- The compiled MooshieUI server connected to the running ComfyUI through its
  normal remote-start IPC, submitted a real image workflow, received the
  WebSocket image, recovered the output, saved it to the isolated gallery,
  and returned a decodable 512 by 512 image through `load_gallery_image_display`.
  The visually inspected result shows the requested red teapot beside a window.
  The local checkpoint required v-prediction with zero-terminal-SNR sampling
  and 0.7 CFG rescaling; the initial EPS test was visually invalid. The corrected
  24-step Euler/normal request, seed 35000, completed in 38.12 seconds including
  polling. This changed the test settings, not the application's sampler code.
- Music capabilities correctly reported the three unavailable YuE2 classes.
- The pin's version/update-state Rust test passed after updating the constant.

**Native YuE2 development build**, commit
`c75d8c966c29cb0392259af791f43373315b72db`:

- All 15 bundled nodes and both core signatures passed; 961 nodes registered.
- MooshieUI discovered all eight required native music classes and the INT8
  checkpoint. The downloaded checkpoint matched the pinned size and SHA-256.
- Requests went through the compiled MooshieUI server's `generate_music`,
  `get_music_status`, and `load_music_audio` IPC routes. This exercised the
  actual Rust workflow, GPU dispatch, completion tracking, and audio retrieval.
- Every retrieved FLAC decoded successfully as non-silent stereo, 48 kHz audio.

| Test | Seed | Result | Wall time including polling/retrieval |
| --- | ---: | --- | ---: |
| Direct generation, no score | 35001 | 8 seconds, 711,712-byte FLAC | 29.91 s |
| Full score planning | 35002 | 16 seconds, 1,626,792-byte FLAC; 859-character ABC score | 49.09 s |
| Cancel a 120-second request after 3 seconds | 35003 | Removed from running/pending queue; interruption reported | 1.03 s to stop |
| New generation after cancellation | 35004 | 4 seconds, 311,571-byte FLAC | 18.70 s |

Audio requests used 32 `dpm_2` steps, `sgm_uniform`, CFG 1, and the existing
1920/128 tiled VAE decode. Synthetic lyrics/style were used. Results and logs:
`error-logs/yue2-validation/compat-035.json`, `live-035.json`, `live-yue2.json`,
and `yue2-full-35002.flac` (with the corresponding `.abc` file).

These are live backend and GPU checks. Browser audio controls, visual/mobile
layout, and listening quality remain unverified because no browser automation
connection was available. Longer songs, BF16, melody/custom-score inference,
multi-GPU hardware, and macOS remain outside this run's qualification. The
sampled GPU memory readings are not a measured peak-memory benchmark.

### Managed install/update verification

The managed source manifest is now consumed by the desktop Git updater, fresh
installation, ZIP fallback, Docker default and macOS runtime setup. The source
override is tied to the baseline release and expires when that baseline changes.
The Music page uses the shared desktop updater, waits for server readiness via
the application event handler, and refreshes capability/model discovery. Remote
connections do not trigger a local startup update.

Verification in `error-logs/yue2-validation/`:

- `managed-provision.json`: upgraded a stock v0.35.0 checkout to the managed
  commit and preserved sentinel files in models, output, input and custom_nodes.
  The downloaded ZIP's runtime sources match Git. ZIP SHA-256:
  `2e8e969f49acdc5c1cac2f54ef26050b6a144c63538f977e04a5308b7c1266d6`.
- The ZIP fallback overlays source files without clearing existing user data.
  A Rust test covers preserved model files and retry after an interrupted copy.
- `managed-compat.json`: 23 required classes and six core input signatures pass.
  `rejected-plain035.json`: the expanded gate correctly rejects the plain
  release's three missing YuE2 nodes, preventing a future downgrade of support.
- `live-managed.json`: actual MooshieUI requests against the newly provisioned
  managed source generated 8-second, 16-second and 4-second stereo FLAC files,
  returned the ABC score, and passed cancellation/recovery. Observed wall times
  were 21.45, 22.84 and 14.67 seconds, with cancellation taking 1.00 second.
  These later runs benefited from the machine's existing kernel caches.
- `live-managed-image.json`: the same runtime passed SDXL v-prediction image
  generation, WebSocket output recovery, gallery save and 512 by 512 readback
  in 22.06 seconds including polling.
- Frontend build and locale parity pass. Svelte checking still reports the
  three pre-existing errors listed above. Python syntax and workflow YAML pass.
  Docker and physical macOS builds were not run; their source selection was
  updated to use the common manifest.

No separate manually selected development checkout is required for managed
users. The already-installed application/runtime were not replaced during these
workspace tests; the changed application provisions this runtime through setup
or its existing update flow.

The device library now persists generated recordings as well as metadata.
Original files also remain in ComfyUI's output directory. At this stage,
cross-device library sync, reference-audio transcription/covers (SheetSage2),
standalone score-only generation, and a piano-roll editor were outside scope.
The later [Song covers](#song-covers) section records the transcription addition.

### Music studio and audio player (2026-09-13)

The Music page now has a responsive song editor and a MooshieUI player that
uses the active theme palette. Controls include play/pause, a waveform seek
bar, ten-second skips, volume/mute, and repeat. The waveform comes from the
recording; browsers without waveform decoding retain a normal seek bar.
Track changes stop the old recording, and the selected recording's lyrics
remain available independently of edits to the next song.

FLAC export opens the native Save dialog on desktop and writes the original
loaded bytes through `save_music_audio`. Cancelling does nothing. Browser
downloads use an attached link and a separate object URL so selecting another
song cannot revoke a download in progress. Saving does not depend on ComfyUI
history remaining available. Save errors and completion feedback appear in
the player; downloads remain available if the browser cannot decode playback.

Validation: frontend build and locale parity pass, both Rust configurations
compile, and the server library suite passes 564 tests (one ignored). Export
logic checks used the real 16-second generated FLAC to confirm byte preservation,
desktop save/cancel/error routing, and browser download URL lifetime. Transport
checks covered seeking bounds, failed play requests, repeated clicks, and stale
promises after changing songs. These JavaScript checks mock the platform/media
APIs; native dialog interaction, browser layout, and audible playback remain
unverified because a browser connection was unavailable. Svelte checking has
the same three pre-existing errors and no diagnostics in the new Music code.
Logs and test harnesses are under `error-logs/music-ui-*`.

### Playback progress, lyric timing and complete endings (2026-09-13)

The vinyl now has a continuous spiral groove, with a played arc driven by the
recording's current time divided by its actual duration. It winds inward and
ends at the run-out outside the label. Seeking and repeat update the arc;
rotation pauses with playback and respects reduced-motion preferences. A small
spindle hole remains at the physical center, separate from the plain record label.

YuE2's [ABC alignment reference](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/abc-editing.md#lyrics-syllables-and-phonemes)
distinguishes intended score/syllable alignment from measured synchronization.
The generation API has no forced phoneme-to-note alignment input, and native
ABC does not carry lyric `w:` fields. The app uses optional manual line-start
marks for highlighting and seeking. Unmarked lines stay readable without guessed
start times. SheetSage2 transcribes musical notes rather than sung word timestamps.
The experimental speech-model alignment path has been removed at the user's request.

The [generation guide](https://github.com/multimodal-art-projection/YuE/blob/main/docs/generation.md#outputs-and-reproducibility)
requires checking truncation, and the
[stage reference](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/generation-and-covers.md)
explicitly says a completed request can still contain truncated audio. The
[sampler](https://github.com/multimodal-art-projection/YuE/blob/main/src/yue2/sampling.py)
stops on `MUSIC_END` or exhausts its token budget. The default semantic budget
is 9,000 tokens, corresponding to 360 seconds at 25 frames per second; the
default 200-token minimum suppresses early stopping for the first eight seconds.
There is no documented guarantee of an exact-length, musically resolved ending.

Our pinned ComfyUI text encoder follows those stopping rules and attaches
`yue2_truncated` to conditioning. `YuE2GenerateMusic` exports conditioning and
actual seconds, but no separate completion flag in its UI/history output. Its
ABC generation path also discards the planner's truncation boolean. A follow-up
completion feature should expose these real flags, distinguish capped excerpts
from completed generation, and support a fresh retry with a larger budget and
the same score. It should not claim that a fade-out or a successful SaveAudio
node establishes musical completion. The Music duration help now explains the
hard cutoff. Existing user duration settings were preserved.

Live check through MooshieUI's server API on the RTX 5070, INT8 checkpoint,
full planning, seed 35002, 32 steps:

- Earlier run: 16-second ceiling, 16-second audio, token-budget warning.
- New run: 90-second ceiling, **47.6-second audio**, no token-budget warning.
  The semantic sampler produced 1,190 music frames within its 2,250-token
  budget. Its saved ABC is byte-identical to the earlier score: 35 bars
  of 2/4 at quarter note = 87, nominally 48.276 seconds.
- Generation and retrieval took 38.03 seconds. The retrieved FLAC is 4,088,834
  bytes and decodes as non-silent 48 kHz stereo. This proves native early stopping
  and use of the returned duration, not a subjective listening-quality judgment.

Evidence: `error-logs/yue2-validation/live-managed-ending.json`,
`comfy-gpu-managed-ending.log`, `ending_test.py`, and
`managed-ending-yue2-full-35002.flac`. The isolated test servers were stopped
afterward. The existing 16-second artifacts were preserved.

### Compact layout and live generation feedback (2026-09-13)

The editor now puts style and lyrics first, with model, seed, sampling and score
settings in one expandable section. Flat surfaces, small corner radii and a
compact player replace the nested bordered cards, gradient, status badge and
decorative M logo. Empty recordings do not show disabled transport controls or
an empty session list. Playback controls, waveform seeking and original FLAC
export remain available for generated songs.

A further design pass retains this layout while strengthening the hierarchy:
an accent edge and larger record distinguish the player, form labels have more
contrast, and Play and Download FLAC have distinct button treatments. Selected
tracks use both an accent edge and a checkmark. Verse and Chorus buttons insert
section headings at a line boundary, preserve existing lyrics and return focus
to the new section. The lyrics editor uses the app's normal reading font.
Generation settings stay collapsed unless needed, and invalid required settings
open the section so the offending input can receive focus. That design pass added
no generation parameters or runtime dependencies.

Design-pass validation: build and locale parity pass, with no new Music type
diagnostics. Lyric insertion checks cover existing text, cursor placement,
repeated sections, Unicode and CRLF. Existing transport, export, progress-event
replay and record geometry checks also pass. Browser inspection remains
unavailable, so these checks do not establish visual or usability qualification.
Evidence is under `error-logs/music-design-*`.

Music consumes the existing ComfyUI execution and progress events through the
shared IPC listeners, separately from image progress. The UI reports queue state,
elapsed time, score writing, composition, rendering, decoding, saving and audio
retrieval. Counters reflect native stage budgets, not estimated overall completion
or remaining time. Composition uses the native 25 frames/second rate; score and
composition may end before their limits. Cancellation, errors and completion
remain visible. Custom scores and off mode skip the score-writing stage.

Validation: a fresh isolated GPU run through the browser/server API produced the
same 47.6-second FLAC in 45.03 seconds including retrieval. Its SSE stream
contained 325 music events, including 147 progress updates and all execution
stages. Replaying those real events through the compiled Music store reached
every stage in order. Additional checks covered unrelated prompts, cached nodes,
late events, cancellation, execution errors and failed audio retrieval. Record
SVG checks at eight playback positions verified the continuous groove stays
outside the label. Existing transport and original-byte FLAC export checks pass.
Build and locale parity pass; Svelte checking retains the three unrelated errors
listed above with no Music diagnostics. Browser layout and audible playback
remain unverified because no browser was available. Test servers were stopped.

Evidence: `error-logs/music-refine-*`,
`error-logs/yue2-validation/progress_test.py`, `live-managed-progress.json` and
`events-managed-progress.json`. No model or runtime changes were needed.

### Device library, playlists and shared playback (2026-09-13)

Music mode has **Create** and **Library** views. The library has searchable track
rows, title or date/playlist-order sorting, duration and date columns, and an
individual song menu for title editing and playlist membership. Users can create,
rename and delete playlists. Playlist deletion leaves recordings intact. Play all
uses the displayed order; Shuffle creates a shuffled queue. Tracks advance when
they end, and previous/next controls operate on the shared queue.

The generation form accepts an optional title. Titles are display metadata and
also supply the default FLAC download filename; they do not alter the audio.
Untitled tracks fall back to their first lyric line. A completed generation saves
to the library without interrupting a song that is already playing.

`musicLibrary.ts` stores metadata, audio blobs and playlists in separate IndexedDB
stores, with a separate database for each account on this device/browser origin.
Listing songs only reads metadata. Audio stays playable after a reload or after
the originating ComfyUI history is cleared. Storage errors are visible and do not
silently report a successful edit; an unsaved recording can be retried or exported.
The library is local to the device. Download FLAC files for backups, since clearing
site/WebView data removes the local library. It does not scan or import historic
files from ComfyUI's output directory.

One store-owned audio element drives both the generation screen's main player
and the bottom bar. Switching pages, switching between Create and Library, or
switching desktop/mobile layouts does not recreate or pause the recording. Both
controls share play/pause, seek, repeat and volume. The bottom bar provides queue
navigation; clicking the song title opens the main player. Replaced audio elements
and object URLs are released, and late fetches cannot replace a newer selection.

### Manual lyric synchronization (2026-09-13)

The speech recognition worker, Transformers.js dependency, WASM assets and model
download path have been removed. **Sync manually** edits the selected recording's
line start times. Users can play/pause, mark the next line at the current playback
time, set any individual line to now, enter start times in seconds, clear marks,
or cancel. Save validates increasing times within the actual audio duration and
persists the marks with the song. A marked line stays active until the next marked
line or the end of the recording; unmarked lines have no inferred start time.
Optional follow scrolling and clicking a timed line to seek remain available.

The earlier speech-model trials are historical only. They measured high renderer
memory use (up to 3,684.7 MiB in the initial CLI adaptation); no such model is now
loaded by the app. Their diagnostic artifacts remain under `error-logs/lyric-sync-*`.

Validation: `node scripts/test-music-library.mjs` covers manual timing boundaries,
shared controls, queue advancement, stale fetches, title and playlist edits,
serialized writes, storage failure handling and account isolation with synthetic
media/storage boundaries. `node scripts/test-music-writing.mjs` covers the retained
writing assistant. Locale parity and the production build pass. Scoped Svelte
checking has no blocking errors; two existing App lightbox label warnings remain.

A production-build headless Chrome test with synthetic WAV recordings also passed
real IndexedDB save/reload and per-account isolation, title and playlist dialogs,
manual timing save, and playback continuity while navigating away from Music.
Desktop (1440×1000) and mobile-width (390×844) screenshots were reviewed; the
bottom player stays in view and the mobile page does not overflow horizontally.
There were no uncaught browser exceptions. This is component/browser validation,
not an installed Tauri WebView, native Save-dialog or live YuE2 generation test.
Evidence: `error-logs/music-browser-run.log`, `music-browser-exceptions.json`,
`music-library-desktop.png`, `music-library-mobile.png`, and
`music-generation-desktop.png`.

## Song covers

This section records the original melody-only bridge implementation. The current
workbench also supports native SheetSage2, full-score fidelity, part selection and
native-dialect validation; use the [current cover instructions](../MUSIC.md#cover-a-recording).

In **Music → Create → Cover a song**, import a melody-only `.abc` file or paste
the score into the editor. **Try an original melody** supplies a short original
example without requiring transcription. Enter a target style and sectioned lyrics
whose phrasing and section order fit the melody, review the score, and select
**Generate cover**. Changing the score invalidates its review. Review is also
required again after loading settings or reusing a saved cover.

This follows the [YuE2 cover guide](https://github.com/multimodal-art-projection/YuE/blob/main/docs/covers.md):
the native ComfyUI node receives `mode="melody"` (YuE2's `cot="melody"`) and the
supplied ABC directly; it does not generate a replacement score. Both the frontend
and backend reject empty scores, missing basic ABC headers/notes, and harmony
symbols in cover mode. **Remove chord symbols** removes quoted harmony labels
while preserving notes, voice headers, inline fields, comments and positioned
annotations. It is an undoable edit. These checks do not replace musical review
or implement a complete ABC parser.

### Source recording transcription

Expand **Transcribe a source recording**, select a WAV, MP3, FLAC, M4A, OGG, Opus
or AIFF file (up to 64 MiB), and listen to it using the source player. **Transcribe
melody** sends the recording to the MooshieUI host, which invokes SheetSage2 with
`melody_only=True`. SheetSage2 automatically loads its MERT2 encoder. The result
and any transcription warnings appear as a separate draft. **Use this score in
the editor** applies it with Undo; it never replaces edits just because a job
finished. Check the notes, meter, sections and lyrics before generating the cover.

SheetSage2 requires a separate Python 3.10/3.11 environment, its model files,
FFmpeg 6.1 with shared libraries, and access to its MERT-v2 parent model. Follow
the [official setup](https://huggingface.co/m-a-p/SheetSage2#quick-start), including
the pinned PyTorch and requirements installation. Keep these dependencies out of
ComfyUI's environment. The host operator then sets these environment variables
before launching MooshieUI:

```powershell
# Windows: absolute paths to the separately installed environment and model.
$env:MOOSHIE_SHEETSAGE2_PYTHON = 'D:\YuE\.venv-sheetsage2\Scripts\python.exe'
$env:MOOSHIE_SHEETSAGE2_MODEL_DIR = 'D:\YuE\models\SheetSage2'
# Launch MooshieUI from this shell so it inherits these settings.
```

```bash
# Linux: set these in the shell or service/container environment running MooshieUI.
export MOOSHIE_SHEETSAGE2_PYTHON=/opt/yue/.venv-sheetsage2/bin/python
export MOOSHIE_SHEETSAGE2_MODEL_DIR=/opt/yue/models/SheetSage2
```

These paths are on the **MooshieUI host**, including when a browser connects to a
remote host. The model directory must contain the downloaded configuration and
Python implementation; loading uses `trust_remote_code=True` as documented by
SheetSage2. Clients cannot supply executable paths or shell commands. The setup
indicator confirms the configured files exist; dependency/model loading is checked
when transcription runs. This feature does not install the environment itself.

The default device is **CPU**, which avoids competing with ComfyUI's GPU jobs.
CPU transcription can be slow. A host with a dedicated GPU can set
`MOOSHIE_SHEETSAGE2_DEVICE=cuda` or `cuda:N`. When sharing a GPU, the host must
stop other inference and release its models before transcription. MooshieUI waits
for transcription to finish before enabling cover generation in this editor.

Jobs run in a separate, cancellable process with a 30-minute limit. The host
allows one transcription at a time. Uploaded recordings and subprocess outputs
live in a unique temporary directory and are removed after success, failure or
cancellation. Job results are private to the submitting account, available for
up to one hour in a bounded memory cache (at most 16 results), and can be recovered on page reload or
with **Refresh**. Scores in the editor persist with the user's music settings;
source recordings do not persist in the music library. Model downloads and their
cache belong to the separate environment.

You can also run the official command externally:

```bash
python models/SheetSage2/infer.py source.wav --output cover-score --melody-only
```

Run it from the separate environment, check its exit status and warnings, and
import `cover-score/score.abc`. Only the ABC, lyrics and style go to YuE2 generation.
Cover mode changes the arrangement around the melody; it does not copy the source
recording's vocal timbre. Existing playback, FLAC export, library and manual lyric
timing work with generated covers.

Cover validation: production frontend build, desktop/server compilation, all 12
locale key/placeholder checks, and scoped Svelte type/accessibility checks pass.
`node scripts/test-music-cover.mjs` checks score validation/removal, review gates,
submission, asynchronous drafts, cancellation and account changes.
`python scripts/test-music-cover-python.py` checks the bridge's real call contract
and invalid transcription results with a synthetic model. All 12 Rust music
tests pass. Headless Chrome checks the real built Svelte UI at desktop and mobile
sizes, including file input, Undo, source upload, stale drafts and submission,
using synthetic IPC. This does not establish live SheetSage2 transcription,
live cover quality or installed Tauri WebView behavior.

The desktop suite passed 587 tests with one ignored; its one process-control test
failed under the sandbox and passed when rerun directly outside it. The full
server suite encountered a failure and a hang in NovelAI refinement tests outside
the cover code and was stopped. These full-suite results include concurrent
workspace changes. Logs and screenshots are under `error-logs/song-cover-*`.

### v2.3.4 release validation (2026-09-14)

Production frontend build, desktop/server Rust checks, Rust formatting, all 12
locale parity checks, diff-scoped Svelte checking, both focused music scripts and
the release-artifact unit tests pass. The full Rust suite reports 578 passed,
zero failed and one existing ignored test. Clippy completes with warnings outside
the changed logic.

The Windows test executables initially failed to load with
`STATUS_ENTRYPOINT_NOT_FOUND`: unlike the app executable, they lacked the Common
Controls v6 manifest needed by `TaskDialogIndirect`. A temporary local Cargo
runner embedded the app's existing manifest into the test executables before
running the full suite. No test assertions were bypassed. The packaged app already
contains that manifest. Logs are under `error-logs/release-2.3.4-*` and the runner
is `error-logs/run-release-tests.ps1`.

## Capability assessment (2026-09-14)

### Approved expansion implemented in the working tree

The user approved the eight summary groups, excluding the separately deferred
projects. Implemented paths now cover native SheetSage2 with fidelity/voice
selection, score-only jobs, native ABC inspection and duration, song versions and
A/B/project export, planner/semantic receipts, checked assistant edits, MIDI and
piano-roll previews, and sequential candidates with advanced sampling.
The [music studio guide](../MUSIC.md) describes the actual controls and limits.

Validation for this expansion:

- Desktop and server compilation passed. The full server-feature Rust suite
  passed **580 tests**, with **0 failures** and **1 existing ignored utility**.
  Two pre-existing local-refiner test fixtures were corrected to set both their
  mock URL and port, matching the current worker endpoint contract.
- The 18 music Rust tests cover native/extended graphs, score-only terminals,
  receipts, export checks and exact-worker routing. Python tests exercise the real
  adapter methods with synthetic CLIP/audio boundaries, including bounded decoding
  and upload cleanup; the separate transcription bridge tests also pass.
- JavaScript tests cover score semantics, preservation checks, MIDI/SVG/ZIP,
  review-first plans, distinct sequential seeds, failures, cancellation, recovery,
  existing writing assistance and library behavior. Both upstream score fixtures
  match the official helper's sounding pitch/onset/duration output.
- Headless Chrome exercises actual Svelte components with synthetic IPC: planning,
  checked editing over SSE, Undo, WebAudio preview, MIDI bytes, IndexedDB versions
  and v1 library migration, A/B and score playback exclusion, project ZIP bytes,
  native transcription submission and a 390-pixel layout without document
  overflow or uncaught exceptions.
- Production frontend build and 12-locale parity passed. Full Svelte checking has
  the same three unrelated baseline errors in `promptSegmentDetail.ts`,
  `ColorTooltip.svelte` and `UpdateNotification.svelte`; none are in music files.

No new live YuE2/SheetSage2 inference, installed Tauri/WebView test or audio-quality
qualification was performed for this expansion. Original source recordings and
SDK stage artifacts are not included in project ZIPs. Logs and browser fixtures
are under `error-logs/music-studio/`. Matching wiki edits are prepared in the
isolated `error-logs/music-studio/wiki/` checkout, without publishing.

### Pre-implementation assessment

This assessment describes the **working tree before the music studio expansion**,
including the original cover and writing-assistant changes. Its recommendations
and implementation notes below are a historical snapshot, not a current backlog.
The approved implementation is documented in [Music studio](../MUSIC.md).
Recommendations and effort estimates below are engineering judgments from source
inspection; no new inference or audio-quality experiment was run for this audit.

Reviewed the upstream README, generation, cover, editing, benchmark and source
notes; the music skill and its five references; the released pipeline/request
interfaces; and the SheetSage2, MERT2, VAE and Comfy-Org model cards. The YuE
repository head inspected was
[`88da114`](https://github.com/multimodal-art-projection/YuE/tree/88da114a67df892af0329472073b96a5ef700b93).
ComfyUI source was inspected at MooshieUI's actual pinned revision,
[`c75d8c9`](https://github.com/Comfy-Org/ComfyUI/tree/c75d8c966c29cb0392259af791f43373315b72db),
not inferred from a release version or model filename.

### What is already covered

The [music graph](../../src-tauri/src/templates/music.rs) already implements
full/melody/off generation and supplied ABC, with seed, duration ceiling and
synthesis steps. The [music store](../../src/lib/stores/music.svelte.ts) and
[library](../../src/lib/utils/musicLibrary.ts) retain recordings, scores and
request settings, support score reuse, and provide playback, playlists and FLAC
export. Lyric timing is manual.

The [cover bridge](../../src-tauri/src/commands/music_cover.py) obtains melody ABC
and warnings through the separately configured SheetSage2 environment. The editor
supports import, source playback, chord removal, Undo and explicit score review.
The uploaded source and full transcription artifacts are not archived in the
library. The [writing skill](../../src/lib/utils/yue2Skill.ts) supplies score-aware
musical guidance to the configured LLM, but does not execute score editing,
symbolic comparisons or audio evaluation. These are useful foundations for the
upstream [agent workflow](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/SKILL.md).

### Recommended additions

Small means mostly existing UI/storage work; medium spans job handling and UI;
large adds a parser, persistent artifact service or checked editing workflow.
These are relative estimates, not delivery dates. Priority 1 establishes the
workflow; priority 2 expands it; priority 3 is optional research work.

| Priority | Addition | MooshieUI implementation path | Effort |
| --- | --- | --- | --- |
| 1 | Native cover transcription | Route uploaded audio through the pinned ComfyUI SheetSage2 nodes, then reuse the existing review editor. Add model discovery/download and worker-owned transcription jobs. | Medium |
| 1 | Generate and review the score first | Submit a graph ending at `YuE2GenerateABC` and `PreviewAny`; return an editable plan before semantic generation or audio synthesis. | Medium |
| 1 | Score inspection and duration estimate | Interpret native ABC into notes, bars, chords and tempo; identify unsupported notation and compare nominal score length with the selected audio ceiling. | Medium–large |
| 1 | Song versions, A/B playback and project export | Add version relationships and change descriptions to the library; export audio, ABC, lyrics and settings together. Preserve the original when creating a variation. | Small–medium |
| 1 | Truncation status | Expose actual generation-limit flags and persist them with each result; retain an unknown state for backends that cannot report them. | Medium–large |
| 2 | Checked AI score edits | Offer reharmonization, tempo/key changes, section edits and singable translation through a bounded edit request, score comparison and review step. | Large |
| 2 | Cover fidelity and melody-part choices | Offer free accompaniment or preserved harmony, and explicit vocal/both-melodies selection. Keep the selected parts and lyrics consistent. | Medium |
| 2 | Score preview and MIDI/sheet export | Display notation or a piano roll; play a synthesized preview of the current score; export MIDI and printable notation. | Medium–large |
| 2 | Several candidate versions | Queue separate seeded requests sequentially, group the results, and let the user compare and select a favorite. | Medium |
| 2 | Advanced generation settings | Expose native semantic sampling controls with reset-to-default and per-version recording; add guidance separately if its backend contract is extended. | Small–medium |
| 3 | Stage archives and alternate decoding | Persist semantic tokens/latents and their identities; qualify separately loaded decoders or an optional SDK runner. | Large |
| 3 | Music analysis and automatic evaluation | Add optional transcription diagnostics, retrieval embeddings or separately installed lyric/quality evaluators. | Large |

### Native transcription is the most useful newly identified backend path

The pinned
[audio encoder nodes](https://github.com/Comfy-Org/ComfyUI/blob/c75d8c966c29cb0392259af791f43373315b72db/comfy_extras/nodes_audio_encoder.py)
include `AudioEncoderLoader` and `SheetSage2AudioToABC`, with `melody` and `full`
modes. A transcription graph can use `LoadAudio` plus these nodes and terminate
at `PreviewAny`. The [Comfy-Org distribution](https://huggingface.co/Comfy-Org/YuE2)
provides `sheetsage2_bf16.safetensors` for `models/audio_encoders`.

The native [encoder adapter](https://github.com/Comfy-Org/ComfyUI/blob/c75d8c966c29cb0392259af791f43373315b72db/comfy/audio_encoders/audio_encoders.py)
loads through ComfyUI's model manager. This offers a path through our existing GPU
queue without installing the Hugging Face SheetSage2 dependencies into ComfyUI.
The upstream instruction to use separate environments still applies to the
[Transformers route](https://github.com/multimodal-art-projection/YuE/blob/main/docs/covers.md)
that the current bridge implements.

Implementation needs capability discovery on the executing worker, a pinned
encoder download, an audio upload contract with unique names and retention rules,
and a transcription job type that accepts ABC without expecting a FLAC result.
Upload and execution must target the same worker. Generation should follow score
review as a separate job. Existing image upload wrappers declare `image/png` and
allow overwrite, so they should not be reused unchanged for source audio.

The native node returns a list of ABC strings; it does not expose the official
interface's structured warning/event/MIDI bundle. Preserve that distinction in
the UI and qualification work. Test real short and full-song recordings, imported
formats, CPU/GPU memory, cancellation, remote workers and page/account changes
before making native transcription the default. The current bridge remains a
possible fallback for hosts without these nodes or for richer artifact exports.

### Planning, validation and musical editing should be one connected workflow

The upstream [generation guide](https://github.com/multimodal-art-projection/YuE/blob/main/docs/generation.md)
supports planning separately from synthesis. The native
[YuE2 nodes](https://github.com/Comfy-Org/ComfyUI/blob/c75d8c966c29cb0392259af791f43373315b72db/comfy_extras/nodes_yue2.py)
already allow a graph that stops after ABC generation. MooshieUI's completion
handler currently requires audio, so this needs an explicit score-only job/result
rather than treating an audio-less history as a generation error. Rendering the
reviewed ABC should bypass automatic replanning. Reusing ABC text is distinct
from the SDK's exact saved-token plan restoration.

The skill's [ABC helper](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/abc-editing.md)
can inspect a bounded native dialect, remove supported chords, compare sounding
notes and compute nominal duration without model inference. Our current header
and chord checks are not equivalent. Integrate a pinned helper in a known Python
environment, or port it with parity fixtures; basic inspection should not require
loading SheetSage2. Preserve ties, key-dependent accidentals, resting bars and both
melodic voices. Unsupported notation should receive a specific explanation.

The [editing guide](https://github.com/multimodal-art-projection/YuE/blob/main/docs/editing.md)
provides the pattern for a dedicated **Edit song** action: copy the baseline,
request a bounded change, validate the new score, then render another recording.
The configured LLM can propose the edit; deterministic code must check requested
pitch/rhythm constraints. Section order and lyric changes need additional checks.
For long scores, supply complete relevant passages and check the entire resulting
score; the Enhance prompt's current head/tail excerpts are insufficient for an
exact whole-song edit.

The [editing workflows](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/editing-workflows.md)
also support instrumental themes, solos and lyric adaptation. Tempo/key controls
should update the actual notation and compatible text. Structural changes must
update corresponding lyric sections. Start with a reviewed single edit and retain
each attempt before considering an autonomous revision loop.

The [cover guide](https://github.com/multimodal-art-projection/YuE/blob/main/docs/covers.md)
distinguishes melody-only adaptation from regeneration with fixed harmony.
Our dedicated cover mode currently forces melody and rejects chords; preserving
harmony would require an explicit mode choice and coordinated UI/Rust validation,
although supplied full scores already work in the original-song editor.
Vocal-only selection must deliberately preserve the remaining part's timing.

### Completion, comparisons and exports need accurate records

The pinned [text encoder](https://github.com/Comfy-Org/ComfyUI/blob/c75d8c966c29cb0392259af791f43373315b72db/comfy/text_encoders/yue2.py)
retains semantic truncation as `yue2_truncated` in conditioning, but its ABC
generation method discards the corresponding boolean. A metadata output node can
surface the semantic flag; accurate planner status needs a backend change.
Do not infer either flag solely from audio length or a successful job. Show planned
duration, maximum allowed duration and actual audio duration as separate values.

The native music node exposes temperature, top-p, top-k and repetition penalty,
which our graph fixes at defaults. Its planner exposes `max_abc_tokens`.
Semantic guidance exists in the encoder but is not a node input. It is a different
stage from the acoustic `KSampler` CFG; increasing sampler CFG while supplying
identical positive/negative conditioning would not implement the SDK guidance
control. The current native music node caps generation at 360 seconds.

Extend [MusicResult](../../src/lib/types/music.ts) with project/parent/version
identities and structured changes. Save the effective settings and available
checkpoint/runtime identities with each version; do not overwrite the baseline
when applying a later edit. Full-audio comparison and optional passage loops can
reuse the player, but matching times are only an approximation after tempo/form
changes. Export requests and scores alongside the original FLAC.

The upstream [listening reference](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/listening-and-evaluation.md)
separates symbolic checks from listening and measured audio alignment. A/B controls
should make that distinction visible. Basic app project export is achievable with
existing data; it should not be labelled an SDK `save_artifacts()` archive.

The released [pipeline](https://github.com/multimodal-art-projection/YuE/blob/main/src/yue2/pipeline.py)
also saves exact plan/semantic tokens, latents, effective configuration, timings
and integrity records. Our ComfyUI workflow does not currently persist those
stage artifacts. Full parity needs new outputs/storage or a separately managed
SDK runner, including artifact ownership and cleanup.

### Useful follow-ons with additional dependencies

The [SheetSage2 interface](https://huggingface.co/m-a-p/SheetSage2)
can provide timed musical events, separate MIDI parts, piano previews and rendered
notation. The current bridge retains only ABC and warnings. A richer transcription
archive could support a beat/section timeline, MIDI export and source/score review.
Rendering has optional dependencies. After editing ABC, regenerate matching MIDI
before using piano audio to review the edit; an old MIDI still plays old notes.

[YuE2-Vae](https://huggingface.co/m-a-p/YuE2-Vae) is the listening decoder; upstream
uses YuE2-Vae-legacy for its recorded benchmark protocol. Identify which decoder
weights the pinned Comfy-Org checkpoint actually contains before promising parity
or adding a decoder selector. Its model card does not establish that identity.
The [stage reference](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/generation-and-covers.md)
permits decoding cached latents again; changing music or lyrics needs generation
again. SDK FP8/vLLM/offload options are not interchangeable with our existing
ComfyUI INT8 path and would need separate qualification.

Sequential candidates and manual preference selection fit the current queue.
The published [best-of-8 protocol](https://github.com/multimodal-art-projection/YuE/blob/main/docs/benchmarks.md)
includes automatic evaluators and candidate selection; eight random seeds alone
do not reproduce it. Even the reported standard YuE2 result selects from two
candidates. The public runtime/skill does not include the complete benchmark
scoring package. Offer **Generate variations** before automatic quality ranking.

[MERT2](https://huggingface.co/m-a-p/MERT-v2-FullSong) provides music representations
that could underpin a separate similarity-search experiment. Genre/mood labels
need appropriate trained predictors and validation. These embeddings are not
YuE2's semantic codec tokens, and a separate feature-extraction stage is not needed
for covers. This is lower priority than completing score-based creation/editing.

### Boundaries and suggested delivery sequence

The released [request interface](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/generation-and-covers.md)
does not expose reference-singer cloning, hard phoneme alignment, negative prompts
or local audio inpainting. Score editing renders a new whole recording. Separate
MIDI parts are not isolated audio stems. CLI `--resume` verifies a completed result;
it does not resume an interrupted render. Lyric recognition/alignment would be a
separate service, not a SheetSage2 option. Retain manual timing while qualifying
any future automatic alignment outside the previously problematic browser worker.

Recommended sequence:

1. Qualify native SheetSage2; add model setup, worker-aware source upload and review.
2. Add score-only jobs, native-dialect inspection and duration feedback. Surface
   semantic truncation; track planner truncation separately until supported.
3. Add version relationships, A/B playback and a basic audio/score/request bundle.
4. Add checked score-edit actions, starting with harmony edits that preserve both
   melodies, then tempo/form changes and singable translation.
5. Add notation/MIDI preview and candidate groups. Evaluate SDK stage archives,
   alternate decoders and analysis models only when those workflows need them.

The target acceptance example is: create an English piano-pop baseline, retain
its audio and score, request jazz harmony while preserving melody and lyric order,
verify the symbolic constraints, and present both complete recordings for listening.
Real model tests must still establish transcription quality, audible adherence,
endings, peak memory and runtime on the hardware/backend combinations we support.
