# Improving MooshieUI music generation

Research date: 2026-09-20. Primary documentation reviewed; no comparative paid
Suno/Udio/Lyria generation session was performed. Feature documentation describes
behavior, not undisclosed training data, model architecture or measured quality.

## What the products establish

| Product | Documented behavior | Implication for this project |
| --- | --- | --- |
| Suno | Its current [v6 FAQ](https://help.suno.com/en/articles/13924481) describes multiple references, workflow selection from natural language, section edits and a Max Mode intended for harder generation tasks. Variety can change style prompts. | Keep intent separate from execution. Show proposed prompt changes, offer reversible edits, and reserve extra generation effort for a concrete goal. The FAQ does not disclose whether Max Mode samples, ranks or reruns candidates. |
| Suno | [Creative Sliders](https://help.suno.com/en/articles/6141377) expose separate style and uploaded-audio influence. | Audio guidance is distinct from text adherence. An LLM-generated caption alone does not justify adding an Audio Influence slider to YuE2. |
| Udio | [Styles](https://help.udio.com/en/articles/11003046-styles) uses one or two recordings, selectable 32-second reference windows, strength and relative blend controls. Saved styles can be reused. | Let users choose the relevant musical section and save useful profiles. Combining text profiles is feasible; equivalent audio blending requires a suitable generator. |
| Udio | [Remixing](https://help.udio.com/en/articles/10694179-remixing-your-music) retains the source prompt by default, with variance controlling the requested amount of change. [Sessions](https://help.udio.com/en/articles/11649525-sessions-udio-s-timeline-editing-view) provides timeline replacement/extension, alternative takes and Undo. | Preserve the original take, make changes explicit, and compare audible outcomes. A score change followed by full regeneration cannot promise preservation of untouched audio. |
| Lyria 2 | The [public Lyria API](https://docs.cloud.google.com/gemini-enterprise-agent-platform/reference/models/lyria-music-generation) exposes `lyria-002`: English text prompts, optional negative prompts, and either a seed or sample count. It returns 30-second, 48 kHz WAV instrumentals. Its request schema has no source-audio field. | It is a possible separate instrumental backend, not evidence that Lyria 2's public API auto-detects an uploaded song's style. Duration, vocals, exclusions and reference support need explicit capability gates. |

Some Udio help pages were last edited in 2025 and retain expired promotional
copy. This research uses their feature descriptions, not their pricing or
availability as a current guarantee. Lyria 2 is also distinct from Lyria Realtime
and later Lyria versions; capabilities from those products cannot be attributed
to `lyria-002`.

The inference from these workflows is that convenience comes from several layers:
saved metadata, audio representations, prompt preparation, model capabilities and
iteration tools. There is no public basis here for saying Suno or Udio uses a
particular captioner, CLAP, MERT, genre classifier or exact hidden prompt.

## Implemented now: audio to an editable style

The first version listens through the configured provider, with a source selected
explicitly by the user. [OpenRouter's audio contract](https://openrouter.ai/docs/guides/overview/multimodal/audio)
accepts base64 audio through Chat Completions. We check audio input in its model
catalog and send MP3 after bounded host-side conversion. A Custom endpoint can
use the same contract, but automatic capability discovery is not standardized.
Unsupported models fail visibly; they do not silently substitute text knowledge.

The profile requests genre, energy, instrumental roles, groove, vocal qualities,
production texture and arrangement observations, with uncertainties. Source
observations and target instructions remain separate. For blank lyrics, the
proposed target style explicitly excludes vocals even if the source has singing.
The user can edit, apply or discard it; analysis never silently overwrites style.

This is a captioning feature. YuE2 receives style text plus lyrics and optional
ABC, as documented in its [generation interface](https://github.com/multimodal-art-projection/YuE/blob/main/skills/yue2-music/references/generation-and-covers.md).
The cover pipeline supplies reviewed melody ABC separately. Neither step
preserves the complete source recording, singer identity or production.

## Implementation assessment and status

Items 1, 2, 3 and 5 are implemented locally (unreleased). Item 4 is a small
prototype; item 6 remains deferred. These are workflow improvements, not claims
that the underlying YuE2 model now matches a hosted product's audio quality.

| Priority | Improvement | Why and acceptance criterion |
| --- | --- | --- |
| 1 | Select and audition reference sections | **Implemented.** Preview player, start/end selection, selected-window analysis, and stored range. Worth doing because a quiet intro or applause can misrepresent the desired section. Automatic multi-section comparison is deferred. |
| 2 | Reusable profiles with provenance | **Implemented.** Named per-account profiles retain accepted text, original observations, source hash/range, model and target. Same-target reuse is free; changed duration/vocal mode/language uses reviewed text-only adaptation. No source audio is saved. |
| 3 | Better take comparison | **Implemented.** Synchronized A/B playback, local loudness matching by attenuation, differing settings, persistent human votes and notes. This makes listening comparisons fairer without inventing an automated quality score. |
| 4 | Section-aware arrangement | **Prototype.** Editable sections and timing budgets produce reviewed style guidance. Existing ABC is unchanged. Worth testing for adherence before building deeper score automation; no audio replacement or continuation is claimed. |
| 5 | Intent-to-operation assistance | **Implemented.** Explicit style/lyrics/score/all scopes, contextual examples, before/after previews, preservation checks and Undo. Style and lyric edits work without a score. Protected fields cannot be supplied by the model. |
| 6 | Optional additional generators | Evaluate Lyria 2 for short instrumentals only if a hosted generation backend is desired. Implement real capability limits instead of sending unsupported lyric, duration, negative-prompt or audio-reference fields to every model. |

Do not map a Suno/Udio percentage directly onto YuE2 CFG, temperature or planning:
those parameters have different meanings. A text-profile blend can express
instrumental and production preferences, but a “70% source A” label would imply
a calibrated control we have not measured.

## Evaluation needed

Use owned/synthetic fixtures spanning instrumental music, sparse singing, live
recordings, non-English lyrics, abrupt genre changes, quiet openings and similar
instrument timbres. Have listeners mark supported observations and unsupported
claims without seeing the model identity. Track source understanding separately
from how well generated music follows an accepted style.

Measure request latency/cost and cancellation behavior as well as genre, dominant
instrument roles, groove, vocal presence and production descriptions. Exact BPM
and key need appropriate music-analysis tools and evaluation; model confidence
is not a calibrated probability. Instrumental compliance should be checked by
listening to generated audio, not merely by inspecting an empty lyrics field.

Current local validation covers payload/response bounds, private jobs, stale
source/account cancellation, model changes, excerpt extraction, integrated
loudness fixtures, profile privacy/account isolation, scoped edit protection,
duration fitting, preview/Apply/Undo, frontend regressions and conversion to a
local mock provider. Desktop and phone browser checks cover v2 library migration,
profile reuse, A/B playback, listening notes and Apply/Undo. This establishes request plumbing and UI
behavior, not hosted-model listening accuracy or YuE2 output improvement. Native
macOS/Linux app runs and paid-provider music evaluation remain unverified.
