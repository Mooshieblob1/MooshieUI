# Seedance prompting rules (for agents)

Rules the anime scene pipeline's video prompt builder follows. Code lives in
`src-tauri/src/cloud/scene/` (`prompt.rs` builds the prompt, `timeline.rs`
computes shot and line timings, `audio.rs` builds the voice track); their unit
tests assert these rules. The fal request itself is in
`src-tauri/src/cloud/video/fal.rs`. Background and evidence:
[anime scene pipeline research](research/anime-scene-pipeline.md), sections 3
and 7.

Rules are tagged `[official]` or `[secondary]`. BytePlus's own Seedance pages
are JavaScript-rendered and could not be read by automated tools, so every rule
below is `[secondary]` until a human confirms it in a browser. Primary sources,
checked 2026-10-11:
[fal Seedance 2.5 prompting guide](https://fal.ai/learn/devs/seedance-2-5-prompting-guide),
[fal multi-angle guide](https://fal.ai/learn/tools/how-to-create-multi-angle-video-seedance-2-5),
[fal Seedance 2.5 reference-to-video API](https://fal.ai/models/bytedance/seedance-2.5/reference-to-video/api),
[fal error types](https://fal.ai/docs/model-apis/errors).

## Prompt shape

The builder emits, in this order, dropping sections that would be empty:
Format, Reference roles, Starting state, one timed block per shot, Continuity,
Audio, Ending state, Constraints. `prompt::BUILDER_VERSION` is bumped whenever
the emitted text changes, and the version is shown next to the prompt preview
and stored in the clip's metadata.

## Rules

- **S1** `[secondary]` Use the section order above. Describe the starting state
  before any action. *Builder: always emits Starting state, ending with "Mouth
  closed."*
- **S2** `[secondary]` Timed beats are plain blocks, `0-5 seconds: ...`, each
  starting from the previous beat's result. Timestamps help ordering but are
  approximate, not guaranteed. *Builder: one block per shot with the shot's
  exact span from `timeline.rs`.*
- **S3** `[secondary]` Long clips need enough distinct events; empty time turns
  into waiting, repetition or slow motion. *Builder: every shot needs framing or
  an action; Constraints forbid slow motion and repeated actions.*
- **S4** `[secondary]` A continuous take says "continuous single take" in
  Format and "no cuts" in Constraints. There is no documented hard-cut syntax,
  so cut placement is best-effort.
- **S5** `[secondary]` References are numbered by upload order per modality
  (`@Image1`, `@Audio1`). Each gets one narrow job: what it controls and what it
  must not transfer. *Builder: one role sentence per image, in upload order;
  the voice track is always `@Audio1`.*
- **S6** `[secondary]` Images for appearance and location, video for camera
  motion or performance timing, audio for voice and rhythm.
- **S7** `[official]` Limits for 2.5 on fal: 30 images, 10 videos, 10 audio
  files; each audio file 1.8 to 30.2 s, at most 15 MB, all audio together at
  most 30.2 s; output 4 to 30 s. *Lives in `video::VideoModel::capabilities`
  and is enforced before anything is sent.*
- **S8** `[secondary]` Dialogue goes in straight double quotes with its start
  and end time and the speaker named. A character who is not speaking keeps
  their mouth closed. *Builder: double quotes inside a line become single
  quotes so the quotation cannot end early.*
- **S9** `[secondary]` Say the mouth moves only during the character's own
  lines, and ask for clean lip-synced speech in Audio.
- **S10** `[secondary]` A mouth map from word-level timestamps (ElevenLabs
  Scribe or forced alignment) lists talking and quiet windows. *Not built yet
  (Phase 4); line windows from the measured takes are used today.*
- **S11** `[secondary]` Camera: give screen positions, tie moves to visible
  events, avoid vague words like "dynamic". *Left to the user's framing text.*
- **S12** `[official]` There is no `negative_prompt` field. Constraints go in a
  final section and should match the shot's real risks.
- **S13** `[secondary]` Change one thing at a time between runs. For a
  continuation, pass the previous clip's last frame as `@Image1`.
- **S14** `[official]` `draft: true` returns a 480p clip and a `draft_id`;
  `bytedance/seedance-2.5/draft/complete` renders it at 1080p within 7 days on
  the same account. *The draft id is stored in the clip's metadata; completing
  a draft is Phase 4.*
- **S15** `[secondary]` No anime-specific official advice was found. State the
  anime look in Format and rely on image references for style.

## Safety

- A `content_policy_violation` from fal (HTTP 422, or a failed queue status) is
  shown to the user as final. Nothing retries it, rewords it or resubmits.
- A minor-flagged scene always ends with "age-appropriate everyday scene, fully
  clothed, nothing suggestive", including after the user edits the prompt
  (`job::final_prompt`).
- Reference images are re-encoded before upload, so their embedded prompts and
  settings never leave the machine.

## Unknowns

- Whether `@Audio1` is used verbatim, cloned or only as a style and timing
  hint. The prototype got a re-synthesized, similar voice whose timing did not
  follow the reference exactly.
- Exact cut behaviour, and whether the spoken language must match the audio.

## Refreshing these rules

When a new Seedance version ships, read the official guide first, then update
the tags and dates here, then the builder and its tests, and bump
`BUILDER_VERSION`. Re-check `price.rs` against fal's model page at the same
time.
