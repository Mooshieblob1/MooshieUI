# Reviewing cover timing and sound

Research date: 2026-09-14. The first approach, xAI speech-to-text followed by the
configured LLM, is implemented in the working tree. The source-length default is
also implemented. Direct audio judgment and local forced alignment remain
research options. Experimental audio timing repair was abandoned after the user
rejected both retiming and voice conversion results. These changes are not a
release announcement.

## Implemented first approach

**Review lyric timing** opens a review dialog for a selected recording. It
transcribes the source and cover independently with `/v1/stt`, aligns recognized
words monotonically against the requested lyric order, then sends the measured
report to the existing assistant. The UI offers playable evidence, durable
transcripts/reports and explanation-only retries. Source audio is retained for
the session; a saved source transcript can be used after reload, but source
playback requires reattaching a recording with the same SHA-256 hash.

The implementation bounds uploads to 64 MiB, checks browser duration before
upload (six minutes), limits each STT request to 90 seconds, caps response bytes
and token alignment work, serializes STT calls on the host and rejects invalid
timestamps. Provider errors do not log upstream bodies or audio. Only configured
xAI credentials go to the fixed official endpoint. Desktop and authenticated
browser clients share the command implementation.

Repeated lines are excluded from timing anchors, and an overall estimate needs
three distinct matched lines spanning five seconds. Different lyrics or tempo
disable source word-timing comparisons. This is recognition-based evidence, not
phoneme alignment, musical quality scoring or an automatic repair loop.

A live synthetic spoken clip succeeded through the actual backend after its
normal xAI OAuth refresh: 12 words with timestamps, duration 4.739 seconds. The
configured Grok backend then produced a response accepted by the actual review
JSON/evidence validator. An earlier direct request using the expired stored
token returned 403; refreshed access succeeded. Singing accuracy remains
unqualified. Synthetic analysis tests cover fixed offsets, progressive drift,
missing/repeated/extra words, changed lyrics/tempo, empty recognition, CJK and
invalid timestamps. The source guide contains the user workflow.

The real Svelte UI also passed a headless browser check with synthetic audio and
a mocked service: two-recording transcription, durable measurements after an LLM
failure, explanation retry without re-upload, timestamp seeking, saved-review
reopening without another STT call, and a 390-pixel mobile layout. This is UI
evidence, separate from the live service checks above.

## Recommendation

### Original lyrics at the original sung times

The user clarified that the required outcome is the generated singer delivering
the original words at the original times. Review and playback highlighting do
not implement that behavior. Source-duration defaults also do not constrain
individual sung words. The released YuE2 interface accepts lyrics and ABC but no
forced phoneme-to-note or word-timestamp channel. MooshieUI currently forwards
those same inputs without a source vocal timing condition.
[YuE2 alignment boundary](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/abc-editing.md#lyrics-syllables-and-phonemes)

The confirmed `mooshie_yue2_00016.flac` cover has a 257-second ceiling and an
actual 249.16-second output with `semantic_truncated=false`. The original Melt
WAV is 256.022 seconds. Its score has a nominal duration of 257.143 seconds,
preserves the opening instrumental bars, and places the first vocal note at
21.429 seconds. The score's two sung pre-chorus sections are not separately
labelled in the supplied lyrics. That is an input-structure discrepancy to
review, not proof of the reported audible error or of a successful remedy.

Live xAI STT on the full original returned empty text and omitted `words`.
A 37-second source excerpt with Japanese language and a reduced VAD threshold
of 0.05 also returned no words. The cover returned some Japanese recognition,
but only about 24% normalized target-text coverage. Consequently the original
word timing and source/cover drift are unassessed. Missing recognition must not
be treated as missing singing. The IPC parser now accepts omitted word arrays
as an empty, inconclusive result. Synthetic speech success did not qualify this
service for the user's Vocaloid song.

The separate audio timing repair experiment was abandoned after unsuccessful
listening tests. No automatic repair is implemented or claimed by the review
feature. Stretching a whole mixed track to the source duration would also move
the accompaniment and would not establish correct syllable placement.

### Review as evidence

Use audio analysis to produce timing evidence, then use the existing configured
LLM to explain it. Add an optional audio-capable reviewer for audible phrasing,
arrangement and pronunciation observations. These should be separate results:
recognizing the words, measuring their timing and judging musical quality answer
different questions. This distinction also appears in the
[YuE2 evaluation guide](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/listening-and-evaluation.md).

The current `call_external_llm` implementation accepts text and optional images.
It has no audio transport. The checked configuration uses Grok 4.5; its documented
API inputs are text and images. This is an interface limitation, not a reason to
abandon the existing backend as the reviewer of analysis results.
[Grok 4.5 model documentation](https://docs.x.ai/developers/models/grok-4.5)

## Available methods

| Method | Documented capability | Proposed MooshieUI role | Unresolved qualification |
|---|---|---|---|
| xAI speech-to-text, then existing LLM | `POST /v1/stt` accepts audio and returns text plus word start/end times | Implemented: compare recordings before asking the configured assistant to explain discrepancies | Refreshed OAuth and synthetic speech verified; accuracy on singing remains untested |
| Gemini audio understanding | Accepts recordings, answers questions about audio and can analyze timestamped passages | Direct listening review of source and cover with a structured issue list | Additional audio-capable model configuration; generated timestamps are not a calibrated forced alignment |
| stable-ts or WhisperX on the host | Word timing/forced alignment; stable-ts can align supplied text, WhisperX combines transcription with language-specific alignment | Local analysis feeding the existing backend LLM, with no second hosted audio account | Singing accuracy, model language coverage and GPU/runtime compatibility need a pilot |
| SOFA | Singing-oriented forced alignment with phoneme dictionaries and checkpoint-based inference | Candidate for more precise sung-phoneme timing | Correct language checkpoint/dictionary, model terms and packaging need verification |

Primary sources: [xAI STT](https://docs.x.ai/developers/model-capabilities/audio/speech-to-text),
[Gemini audio](https://ai.google.dev/gemini-api/docs/audio),
[stable-ts](https://github.com/jianfch/stable-ts#alignment),
[WhisperX](https://github.com/m-bain/whisperX),
[SOFA](https://github.com/qiuqiao/SOFA).

xAI's batch endpoint uses multipart form data, with the audio file last. Its
response contains `text`, `duration` and `words` with `text`, `start` and `end`.
Use the default unformatted words for matching lyrics; do not bias an independent
accuracy check with the entire expected lyric as key terms. The API documents
speech recognition, not validated song scoring. Its sample response has no
per-word confidence field; missing confidence must stay unknown rather than be
invented. API-key examples do not establish the scope of the user's OAuth token.
[xAI request and response reference](https://docs.x.ai/developers/model-capabilities/audio/speech-to-text)

For local processing, stable-ts supports keeping supplied lyric line grouping
and aligning words within supplied segment boundaries. WhisperX documents
language-specific alignment models, CPU execution and GPU memory controls.
Overlapping voices and unalignable words remain limitations. Run either in a
separate host process/environment, sequentially with generation; do not restore
the old browser ASR worker. These are candidates to benchmark, not claims that
speech models reliably align every sung melisma.
[stable-ts alignment](https://github.com/jianfch/stable-ts#alignment),
[WhisperX usage and limitations](https://github.com/m-bain/whisperX)

SOFA is explicitly designed for singing. It supports `.wav`/transcript pairs,
phoneme dictionaries, confidence output and ONNX inference. The default example
uses an OpenCpop dictionary; an English cover needs a compatible checkpoint and
pronunciation mapping. Its repository license alone does not establish every
community checkpoint's terms.
[SOFA inference instructions](https://github.com/qiuqiao/SOFA)

Optional vocal separation can be tested if accompaniment masks words. Demucs
provides vocal separation, but its original repository is archived; selecting a
maintained implementation and checking artifact quality are additional work.
Compare separated and mixed-audio results instead of assuming separation always
helps. Research incorporating pitch into lyric alignment also supports evaluating
music-specific timing cues alongside speech recognition.
[Demucs](https://github.com/facebookresearch/demucs),
[Improving Lyrics Alignment through Joint Pitch Detection](https://arxiv.org/abs/2202.01646)

## Broader research flow

1. Retain the source duration, selected source excerpt, score, intended lyrics and
   edit contract. For unchanged lyrics, obtain a reviewed source word/line timing
   reference once. Keep repeated lines identified by occurrence and section.
2. Independently transcribe the generated recording. Compare recognized words
   against the requested lyric sequence to find omissions, substitutions and
   repetitions. Preserve the raw transcript and identify unsupported languages.
3. Align matching phrases to refine timestamps. Do not force the complete expected
   lyrics into an audio file and then claim every word was actually sung: an
   aligner can assign intervals even when the intended text is wrong.
4. For the same lyrics and intended tempo, compare matched source/cover phrase
   entrances, endings and pauses. Separate a constant entrance offset from
   accumulating drift. For an intentional tempo change, compare in score beats
   using the declared tempo change, and retain both raw and normalized differences.
   An unconstrained time warp would conceal the error being measured.
5. For rewritten lyrics, compare phrase boundaries, vocal rests and a reviewed
   syllable-to-note plan. Original-word matching no longer supplies a valid target.
   Treat missing mappings as unassessed, not as passing synchronization.
6. Send the existing LLM the intended lyrics, ABC, observed words/times, numerical
   differences, uncertain spans and truncation receipts. Ask for an explanation
   grounded in those records, with evidence IDs and suggested next actions.
7. Show issues at clickable playback timestamps alongside A/B playback. Keep
   measured discrepancies separate from an optional audio model's listening
   observations. Do not label the text-only LLM as having listened.

A useful result might describe a late first entrance, a repeated chorus line or
an ending cut off at the ceiling, with links to the corresponding audio passages.
It should not emit an unexplained universal quality score. Review remains useful
even when the result is inconclusive.

## Generation repair and duration

A reviewer can guide a bounded candidate retry while keeping the correct melody
and original recording intact. Candidate selection should compare the same input
contract and retain unsuccessful attempts. Editing the lyric layout or correcting
an erroneous score rest is a distinct, reviewable action. Retiming a mixed final
recording is not a simple lyric-text edit.

The released YuE2 interface has no forced phoneme-to-note input. A designed
alignment can guide preparation and evaluation, but does not force synthesis to
follow every syllable.
[YuE2 lyric alignment limits](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/abc-editing.md#lyrics-syllables-and-phonemes)

Source duration is a useful default ceiling. It cannot make an already drifting
performance fit; the generation may instead stop before the last words. A
different tempo, arrangement or ending may legitimately need a longer ceiling.

## Abandoned audio timing repair experiment

On 2026-09-14, two separate offline approaches were tried on the user's recording:

- Retiming the generated vocal moved its notes with the syllables and left it
  out of sync with the melody and accompaniment.
- Singing voice conversion used the original performance with YuE2's generated
  singer as a voice reference. The user rejected its audible quality.

The user requested that this feature be dropped. The experimental renderers,
listening harness and their tests have been removed, and the preview server has
been stopped. No integration or further automatic repair work is planned.
Passing DSP, pitch or browser checks did not establish acceptable musical quality.
Standard YuE2 covers, source-length defaults, review reports and manual playback
lyric timing remain separate from this abandoned experiment.

## First qualification pass

Use short, manually reviewed singing examples with known lyric/phrase timings.
Include silence before the vocal, repeated choruses, a deliberately omitted line,
a fixed timing offset, progressive drift and one intentional tempo change. Assess
the source and generated recordings, not only synthetic speech. Record unaligned
coverage, word errors and phrase timing error separately from subjective listening.

For a hosted prototype, verify endpoint access with the user's chosen audio
service before promising reuse of existing credentials. The review action must
identify which recordings it sends. For a local prototype, verify Windows host
execution and memory isolation before adding automatic retries. Current sources
are temporary and absent from project exports; retaining a source for later
reviews would require an explicit product/storage change.

## Implemented source-length default

Importing source audio now initializes Maximum length from browser audio metadata,
rounded up to a whole second within the existing 1–360 second generation range.
The field remains editable, with a Use source length button. Late or repeated
metadata does not overwrite a manual edit, a replaced file, a different account
or a running generation. Unsupported browser metadata leaves a manual fallback;
recordings detected over 360 seconds require trimming before transcription.

Validation: actual handler tests, 12-locale parity, production build and a headless
Svelte UI check with a real 3.25-second synthetic WAV passed. That file sets the
ceiling to 4 seconds; extending to 60 seconds survives repeated metadata, and the
reset button restores 4. Full Svelte checking retains the same three unrelated
baseline errors. The live speech-service and LLM qualification added afterward
is described above; local aligners and direct audio reviewers remain untested.
