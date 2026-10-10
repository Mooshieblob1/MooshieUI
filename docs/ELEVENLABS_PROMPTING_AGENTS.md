# ElevenLabs prompting rules (for agents)

Rules the scene pipeline's ElevenLabs builders follow. Code lives in
`src-tauri/src/cloud/` (`voice_prompt.rs` builds Voice Design descriptions,
`elevenlabs.rs` builds speech requests); both have unit tests that assert these
rules. Background and evidence: [anime scene pipeline research](research/anime-scene-pipeline.md).

Each rule is tagged `[official]` (from ElevenLabs' own docs) or `[secondary]`
(from the prototype run or other sources), with the date it was checked.

Official sources, checked 2026-10-11:
[TTS best practices](https://elevenlabs.io/docs/overview/capabilities/text-to-speech/best-practices.md),
[Eleven v4](https://elevenlabs.io/docs/overview/capabilities/text-to-speech/eleven-v4.md),
[TTS API](https://elevenlabs.io/docs/api-reference/text-to-speech/convert),
[Voice Design](https://elevenlabs.io/docs/eleven-creative/voices/voice-design.md),
[Voice Design API](https://elevenlabs.io/docs/api-reference/text-to-voice/design).

## Rules

- **E1 [official].** Audio tags go in square brackets next to the phrase they
  affect. Three kinds: delivery or emotion, reactions, sound effects. Delivery
  tags can be misread as sounds, so phrase them as voice descriptions
  (`[haughty, scoffing voice]`). The script panel sends a line's delivery note
  as one tag before the line.
- **E2 [official].** Ellipses add pauses and weight; capitals add emphasis. No
  SSML `<break>` on v4.
- **E3 [official].** v4 (`eleven_v4`) takes stability and similarity only;
  style and speed are not available. Lower stability is more expressive. The
  default 0.35 comes from the prototype `[secondary]`.
- **E4 [official].** For Japanese, `apply_language_text_normalization: true`
  improves pronunciation at a large latency cost. Pass `language_code`.
- **E5 [official].** v4 accepts up to 10,000 characters per request.
- **E6 [official].** Voice Design description order: language and region;
  gender, age range and quality; a 2 to 5 word persona with 2 or 3 emotions;
  1 or 2 sentences on timbre, pacing and delivery. Avoid effect words (reverb,
  echo, phone, tape). Description 20 to 1,000 characters; preview text 100 to
  1,000 characters, in the voice's mood. Three previews are billed once.
- **E7 [secondary, prototype].** Voice Design refuses some descriptions with
  `blocked_generation`, including ones describing minors. A refusal is shown
  to the user and never retried automatically. When the character is flagged
  as a minor, it is final: the scene locks Voice Design for that character,
  the minor flag cannot be turned off, and no builder rewrites an age or
  re-describes the character to get past the filter.
- **E8 [official].** Premade voices expire 2026-12-31; designed and cloned
  voices persist. Saving a designed voice uses a voice slot.

## Cost

ElevenLabs bills credits per character at a rate that depends on the plan and
the model, and the v4 rate changed during its launch period. MooshieUI
therefore shows exact character counts and the remaining balance before any
paid call, labels everything as an estimate, and records the real charge from
the `character-cost` response header on each take.

## Refresh procedure

When ElevenLabs ships a new model or changes these endpoints, re-read the
official pages above before changing a builder, update the tags and dates
here, then update the tests in `src-tauri/src/cloud/`.
