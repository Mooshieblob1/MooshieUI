# Anime scene pipeline: NovelAI character to voiced, animated scene

Research date: 2026-10-11. Status: **proposal, not implemented.** A working prototype of the full pipeline was built and run outside the app (standalone Python scripts against the same APIs) on 2026-10-10/11. Every cost, failure and workaround below comes from that run unless marked otherwise.

Audience: the agents and maintainer who implement MooshieUI. Read `CLAUDE.md`, `AGENTS.md` and `SCOPE.md` first; nothing here overrides them.

---

## 1. Summary and verdict

**Goal:** the user picks a NovelAI character image, writes (or has the prompt assistant write) a short script, and MooshieUI produces a fully animated, voiced anime scene with that character, saved to the gallery like any other video.

**Verdict: feasible, with one scope decision and one new subsystem.** MooshieUI already has nearly every building block: a paid HTTP provider integration with per-account keys (NovelAI), a synthetic-prompt-id job pattern that the gallery, queue and progress UI already understand, Precise Reference and crop-img2img code, a managed ffmpeg, video gallery ingest, interpolation and export. What is new:

1. Two cloud providers that are not NovelAI: **ElevenLabs** (voice design + TTS) and a **cloud video provider** (Seedance 2.5 via fal.ai, with Segmind as a cheaper alternative).
2. A **multi-provider secret store** (today `SecretsFile` holds only the NovelAI key).
3. A **scene job** that orchestrates several paid steps, survives partial failure, and ingests a non-ComfyUI mp4 into the gallery.
4. **Doc-grounded prompt builders** for the video and voice models, following the precedent of `docs/NAI_V5_PROMPTING_AGENTS.md` (section 7).

**Recommended plan:** ship in five phases (section 9). Phase 0 is the scope decision and the secret store; nothing paid ships before the cost-confirmation UX in section 6.6 exists.

---

## 2. Scope (needs a maintainer decision before code)

`SCOPE.md` currently describes MooshieUI as ComfyUI plus "optional NovelAI image generation using the user's own API key", and says MooshieUI is "not a video editor ... does not cut, composite, or grade footage". This feature touches both lines. `CONTRIBUTING.md` and `.github/PULL_REQUEST_TEMPLATE.md` require a feature issue and a scope statement on the PR.

Arguments for fitting the charter, to put in the feature issue:

- **Precedent for user-keyed cloud providers already exists beyond NovelAI**: LLM providers (`prompt_assistant/providers.rs`), the music "Auto style from audio" provider (`commands/music_audio_style.rs`) and xAI speech-to-text (`commands/music_review.rs`) all send user content to paid third-party APIs with the user's own key. The charter text lags the code.
- **Not editing.** The scene is planned before generation (script, shots, timings) and generated in one provider call, exactly like the existing "clips are planned before generation and exported afterwards" model. Joining an audio track to a generated clip is muxing at generation time, the same thing H3's `video_timeline_custom_audio` path already does inside ComfyUI (`templates/video.rs:820-884`). No timeline cutting, compositing or grading UI is proposed.
- **"Video generation on the same footing as images"** is in scope; a cloud video backend is another backend for that, as NovelAI is for images.

Proposed charter amendment (for the maintainer to accept, edit or reject):

> optional NovelAI image generation, and optional cloud voice and video generation (for example ElevenLabs and Seedance), each using the user's own API key and billed by that provider.

If the maintainer rejects cloud video, sections 4.2 and 7.2 still have value: the voice track and keyframes can feed the existing local H3 `ref2va` path, whose Director already treats reference audio as "follow its voice and timbre" (`comfyui-nodes/minimax_director/minimax_plan.py:151-152`). Whether H3 lip-syncs to supplied audio is **unverified** and would need a GPU test.

---

## 3. What the prototype did (evidence)

Prototype character: a gender-swapped, tsundere take on a well-known character, designed as roughly 13 to 14 years old, in an everyday school scene (the age matters for the safety findings below). Master image: a NovelAI V5 image with full metadata.

| Step | What ran | Result | Cost (measured) |
|---|---|---|---|
| Voice design | ElevenLabs `POST /v1/text-to-voice/design`, `eleven_ttv_v3`, Japanese prompt | 3 previews, user picked one, saved via `POST /v1/text-to-voice` | Preview text only (a few hundred characters) |
| Voice lines | `POST /v1/text-to-speech/{voice_id}`, `eleven_v4`, `language_code: "ja"`, stability 0.35 | 4 lines, 2 takes each. **User strongly preferred v4 over v3.** | 32 credits for 8 takes (v4); in a same-line test v4 cost 12 credits vs v3 44 |
| Character sheet | NovelAI V4.5 Full + Precise Reference (`character&style`, strength 1.0, fidelity 1.0), 832x1216, 28 steps | 5 expressions, character fully consistent and matching the V5 master's style | 25 Anlas (5 per generation) |
| Shot keyframes | Same, 1216x832, per-shot tag blocks; establishing shot without reference | 5 keyframes; one regenerated for setting accuracy | 20 Anlas (4 with reference; the no-reference shot was free on Opus) |
| Mouth variants | V4.5 inpainting (`infill`) on small masks | **Failed**: grey, desaturated patches at every strength, RGB or RGBA, full or 1/8 mask | free |
| Mouth variants (retry) | Crop around mouth, upscale to 1024x1024, img2img (strength 0.5 half, 0.65 to 0.72 open), feathered blend back | **Works**, colors match, no seam | free (inside Opus window) |
| Blink variants | Same crop method on eye region | **Failed**: model redraws bangs and fades eyes; dropped | free |
| Still animatic | Loudness-driven mouth flaps over stills + push-in (ffmpeg) | Works, but **user rejected it as too rudimentary**; wanted real animation | free |
| Cloud video | fal `bytedance/seedance-2.5/reference-to-video`, 9 reference images + one 27.98 s voice track, `draft: true` (480p), 28 s, 5 described shots | **Visually strong**: correct shots, on-model throughout, expressive acting, model-added anime extreme close-up. **Audio not used verbatim**: Seedance re-synthesized the speech; the user judged it "close enough". **Timing ignored**: establishing shot ran ~5 s instead of 1.5 s, so the user's exact track muxed over it would be ~4 s out of sync. | **About $6.17** (fal lists $0.2205/s at 480p) |

Measured Anlas: balance went from 9,780 to 9,735 over the run, exactly 5 per Precise Reference generation (9 of them). This confirms NovelAI's documented Precise Reference surcharge, which `src/lib/utils/novelaiCost.ts` does not model today.

### Lessons that change the design

1. **Never price from third-party tables.** The prototype quoted $3.70 for the draft from an aggregator table that silently included fal's 0.6 multiplier for video inputs; the real charge was $6.17. Price only from the provider's own published formula (section 6.6) and label every figure an estimate.
2. **Safety filters are part of the contract.** ElevenLabs Voice Design rejected a prompt (HTTP 403 `blocked_generation`) when it described a teenage girl. When a character is flagged as a minor, a `blocked_generation` is final: MooshieUI shows it to the user with a clear message and never retries with a reworded age or a re-described character. MooshieUI must not try to get around any provider's safety filter.
3. **Body descriptors must be stripped for young characters.** The source image's metadata contained a body-shape tag. The prompt builder removed it before reuse. For characters flagged as minors, builders should drop body-shape tags and always add `nsfw` to Undesired Content (MooshieUI already prepends `nsfw` for Full models when absent, `payload.rs:265-292`; keep that).
4. **V4.5 small-mask inpainting is unusable for mouth/eye edits through the API.** Use the face detailer's crop-img2img-blend method (`face_pass.rs`) instead. NovelAI's web UI has "Focused Inpainting" (upscales the region to about 1 MP first), which is probably why small edits work there; the API equivalent is doing the crop ourselves.
5. **The audio reference is a voice and style hint, not a soundtrack.** No accessible official doc says otherwise (section 7.2). Design for both outcomes: keep Seedance's audio by default, offer the user's exact track as an alternative, and use the "mouth map" prompting technique (section 7.2, rule S10) to improve alignment.
6. **Draft first.** Seedance 2.5 draft mode (480p, returns `draft_id`) let the user judge a full 28 s scene before paying for resolution. Make draft the default.
7. **Cloudflare blocks default user agents** on `image.novelai.net` (error 1010) from Python's urllib. reqwest's default is probably fine, but set an explicit User-Agent on all new provider clients.
8. **Users care about cost visibility mid-flow.** Two separate cost surprises (draft price, Anlas) were the main friction in the prototype. Section 6.6 is not optional.

---

## 4. Proposed feature

Working name: **Anime Scene** (a mode under Generate > Video, or its own sub-panel; UI placement is a maintainer call). `App.svelte:383-390` currently forces video mode off when a NovelAI model is selected, so this needs its own UI path rather than reusing the H3 panel as-is.

### 4.1 User flow

1. **Character**: pick a gallery image (NovelAI metadata is read automatically, as `NovelAI.md` import already does) or drop a PNG. Optional "character is a minor" toggle, which turns on the safety rules in section 3 lessons 2 and 3. The toggle should default on whenever the prompt contains school-age tags; it costs nothing to be cautious.
2. **Voice**: pick an existing ElevenLabs voice, or design one: the builder drafts a Voice Design prompt from the character (section 7.3), the user edits it, MooshieUI shows the 3 previews, the user saves one (uses an ElevenLabs voice slot; show `voice_slots_used / voice_limit` from `GET /v1/user/subscription` and handle "slots full" by listing voices; deleting one requires explicit confirmation).
3. **Script**: lines with optional per-line delivery notes. The prompt assistant can draft it. Each line becomes a TTS request with audio tags (section 7.3). Show takes, let the user pick per line, regenerate single lines.
4. **Shots**: a short shot list (2 to 6 shots) with framing, action and which line plays in it. The builder computes exact line windows from the rendered audio (section 5.4).
5. **Keyframes (optional but recommended)**: V4.5 Precise Reference keyframes and expression references, with the Anlas estimate including the 5-Anlas reference surcharge.
6. **Generate draft**: itemized cost confirmation (section 6.6), then the cloud video job. Progress, cancel (with the "provider may still bill" warning), result in gallery.
7. **Finish**: keep Seedance audio or swap in the exact voice track; upgrade the draft to 1080p (provider-dependent, priced), or upscale/interpolate locally (GMFSS is already better on anime line art, `templates/rife.rs:27-45`).

### 4.2 Provider choices

| Provider | Model | Why | Status |
|---|---|---|---|
| fal.ai | `bytedance/seedance-2.5/reference-to-video` | Up to 30 s per clip, 30 image refs, 10 audio refs (30.2 s total), draft mode and documented `draft/complete` endpoint. Used in the prototype. | Verified working 2026-10-11 |
| Segmind | `seedance-2.0-fast`, and Seedance 2.5 draft/final models | Same model family at roughly half fal's per-token price per Segmind's own comparison; own upload storage (`/upload-asset`); `reference_images` + `reference_audios` | Schema read, **not yet called**. Response shapes unverified |
| ElevenLabs | `eleven_ttv_v3` (design), `eleven_v4` (TTS) | v4 preferred by the user; designed voices persist (premade voices expire 2026-12-31) | Verified working |

Considered and deferred, with reasons: **Kling 3.0** (strong anime motion, but voices are prompt-generated; matching the user's voice needs a second lip-sync pass that only re-animates the mouth), **PixVerse C1** (anime-focused and cheap, same two-step lip-sync limitation), **Wan 2.7** (Alibaba documents a `driving_audio` input; worth a later provider adapter), **Veo 3.1** (no user-audio input found). Design the provider layer so these can be added as adapters (section 5.2).

---

## 5. Architecture

All provider traffic stays in Rust (never `fetch` from the webview): it keeps keys out of the frontend and avoids CSP changes (`tauri.conf.json:25`; `connect-src`/`media-src` do not allow fal, Segmind or ElevenLabs CDNs). Results are served through the existing `gallery://` scheme.

### 5.1 Secrets (Phase 0, blocking)

Today: owner keys are plaintext fields in `config.json` (`config.rs:126-129`); named accounts use the encrypted `user_secrets.rs` store, whose `SecretsFile` holds only `novelai_api_key` (`user_secrets.rs:46-54`, with a `version` field explicitly reserved for migrations). There is no generic provider key mechanism (`prompt_assistant/providers.rs` holds one LLM slot only).

Proposal:

- Add `elevenlabs_api_key`, `fal_api_key`, `segmind_api_key` as `Option<SealedValue>` to `SecretsFile`, bump `version`, write a migration test. Mirror `load_nai_key`/`save_nai_key`/`has_nai_key` (`user_secrets.rs:499-533`) generically: `load_key(user, ProviderKey)`, `save_key`, `has_key`.
- Add the same three as owner fields in `AppConfig`, included in `preserve_secrets` (`config.rs:728-742`), `preserve_redacted_secrets` (`:751-760`) and `operator_secrets_mut`/`operator_secrets` (`:353-370`), blanked by `config_to_client_json` (`:388-426`) with `*_configured` flags. Extend the existing tests (`config.rs` ~1000-1215).
- Credential resolution mirrors `novelai::resolve_credential` (`novelai/mod.rs:84-118`): named accounts use only their own key, never the owner's. Newtype each credential with a redacted `Debug` like `NaiCredential` (`mod.rs:50-67`).
- Commands `set_elevenlabs_api_key`, `set_fal_api_key`, `set_segmind_api_key`: desktop handler, `lib.rs` registration, `webserver.rs` dispatch arm (open to authenticated users, since each pays with their own key, like NovelAI), TS wrappers in `api.ts`. Follow the `add-tauri-command` skill.
- Settings: one "Cloud voice and video" category in `SettingsPage.svelte` (`categoryAllowed`, `:1158-1176`) with per-provider key fields, balance/usage readouts where the provider exposes them (ElevenLabs `GET /v1/user/subscription`), and a destination disclosure in the style of `music.audio_style_destination` (`en.ts:105`).

### 5.2 Provider modules

New module tree, modelled on `src-tauri/src/novelai/` (client with mandatory per-call timeouts because the shared client has none, `client.rs:37-50`; status mapping like `check_status`, `:316-345`; no `Debug` on clients):

```
src-tauri/src/cloud/
  mod.rs            // CloudCredential newtypes, resolve_*(), shared EventSink reuse
  elevenlabs.rs     // design_voice, save_voice, list_voices, subscription, tts, (optional) scribe_align
  video/
    mod.rs          // trait VideoProvider { upload, submit, poll, fetch, price, capabilities }
    fal.rs          // queue API: submit -> request_id -> status -> result; storage upload
    segmind.rs      // upload-asset, synchronous or polled generate
  scene/
    mod.rs          // the scene job (section 5.3)
    audio.rs        // build timed voice track, line windows (pure, tested)
    prompt.rs       // Seedance prompt builder (pure, tested; section 7)
    price.rs        // provider price formulas (pure, tested)
```

Use `state.http_client` (never a new client, `CLAUDE.md`), explicit User-Agent, generous generation timeouts (video jobs take minutes; poll, do not hold one request open), and bounded body reads like `bounded_json` (`music_audio_style.rs:115-131`). reqwest has `json` and `multipart` but not `stream`; `res.chunk()` is enough for downloads.

`VideoProvider::capabilities()` returns data (max seconds, max refs per modality, resolutions, draft support, audio-reference support), the same "capabilities are data, not branches" rule as NovelAI models (`docs/NOVELAI.md` 2.6). The prompt builder and UI read capabilities, never provider names.

### 5.3 The scene job

Reuse the NovelAI event contract (`docs/NOVELAI.md` 2.1) so queue, progress and gallery work unchanged:

1. Validate everything before spending (lengths, reference counts and durations from capabilities, ffmpeg present via `state.media_tools.path("ffmpeg")`, keys resolved). Fail inline before minting an id, like `novelai::preflight` (`mod.rs:552`).
2. Mint `scene-{uuid}`, `state.prompt_queue.insert(&id, owner)`, `broadcast_queue_positions()`, spawn, return the id. Do not skip the queue insert: the reconciler declares untracked prompts lost after 30 s (`App.svelte:3354-3395`; `get_queue` injects tracked ids, `commands/api.rs:103-163`).
3. Steps, each emitting `comfyui:progress {prompt_id, value, max, node}` with a locale-keyed step label:
   - TTS for any lines without a cached take (cache by hash of text + voice + model + settings; paid intermediates are never regenerated silently, the vibe-cache precedent in `docs/NOVELAI.md` 2.10).
   - Keyframes via the existing NovelAI path (section 5.5).
   - Build the timed voice track and line windows (section 5.4).
   - Upload refs, submit, poll with periodic progress, honour `prompt_queue.is_cancelled(id)` (`state.rs:328`).
   - Download, write the poster (first keyframe, or ffmpeg frame grab), write metadata, ingest (section 5.6).
4. Emit `comfyui:output_video` and `comfyui:executing {node: null}`, or `comfyui:execution_error`; finish with `cancel_and_remove`.

Paid-work rules (from `docs/NOVELAI.md` 2.4, applied here):

- **Degrade, never discard.** If ingest fails after the provider returned a video, keep the downloaded mp4 in a recoverable location and tell the user where. If a later step fails, keep earlier paid assets (TTS takes, keyframes) in the scene's asset store.
- **Persist the provider job id** (fal `request_id`, Segmind task id, Seedance `draft_id`) to disk as soon as it exists, so a crash or restart can resume polling instead of re-paying. Do not copy `music_audio_style.rs`'s 30 s lease model, which would discard paid video work.
- **Cancellation is local.** Cancelling stops further requests and suppresses output, but a submitted provider job usually keeps running and billing. The cancel UI must say so (new locale key).
- One retry on 429 with backoff, sequential requests (face detailer precedent, `face_detail.rs:365-385`).

### 5.4 Audio assembly (pure, tested)

`scene/audio.rs` builds the single reference track from takes and silences with the managed ffmpeg (both builds), mono 44.1 kHz, and returns exact line windows `[(line_id, start_s, end_s)]`. The prototype's numbers for reference: four lines, 0.5 to 2.0 s gaps, 27.98 s total, windows 1.5 to 9.3, 9.8 to 17.1, 17.6 to 21.5, 22.1 to 26.0 s. Enforce provider limits from capabilities: Seedance 2.5 accepts 1.8 to 30.2 s per audio file and 30.2 s total; 2.0 accepts 15 s total.

Optional precision step: ElevenLabs Scribe word timestamps (`scribe_v2`, forced alignment of known text) give word-level talking/quiet windows for the mouth map (rule S10). Cheap, and it is what fal's own guide recommends.

Muxing the exact voice track over the returned video ("use my exact voice" option) is one ffmpeg call (`-map 0:v -map 1:a -c:v copy`). Offer it, but warn that Seedance does not follow the reference timing exactly, so lips may drift; the prototype measured about 4 s of drift on the first line.

### 5.5 NovelAI steps (reuse, small refactors)

- **Keyframes and expression references**: `GenerationParams` with `novelai.director_references` (`params.rs:84-101`), V4.5 Full only (`models.rs:52-108`). Run through `novelai::build_request` (`mod.rs:427`) and the client's `generate`/`generate_stream`. Reference images are letterboxed by `reference_canvas::letterbox_reference` (`reference_canvas.rs:53`).
- **Mouth/expression variants (optional, mainly for a free "animatic" fallback)**: reuse the face detailer pieces with a caller-supplied rectangle instead of YOLO detection: `plan_crop` (`face_pass.rs:144`), `crop_request` (`face_detail.rs:302`), `generate_with_retry` (`:365`), `composite_face` (`face_pass.rs:404`). Make the private helpers `pub(crate)`. Prototype-tested strengths: 0.5 for parted lips, 0.6 to 0.72 for open mouth, context crop about 4x the mouth box. Do not ship blink variants; they failed.
- **Cost**: add the Precise Reference surcharge (5 Anlas per generation, plus any per-reference charge NovelAI documents) to `novelaiCost.ts`. The current estimator would show these generations as free on Opus, which is wrong (measured, section 3).
- Prompt hygiene for reuse: drop pose and expression tags from the master prompt so each shot sets its own; drop body-shape tags for minors; V4.5 prompts must be ASCII (official: no Japanese in V4.5 prompts).

### 5.6 Gallery ingest of a non-ComfyUI mp4

Today video ingest happens through the `MooshieSaveVideo` ComfyUI node (PyAV writes the comment metadata) and `handle_video_output` (`comfyui/websocket.rs:362-496`). A cloud clip needs a Rust path:

1. Download to a temp dir in the owner's space.
2. Poster: write `{stem}_poster.webp` (thumbnails require it, `api.rs:2573-2579`); use the first keyframe or an ffmpeg frame grab.
3. Metadata: Rust has no udta comment writer (`metadata::mirror_uuid_sidecar` only copies an existing one, `metadata/mod.rs:158`; `isobmff::append_uuid_xmp` is `pub(super)`, `metadata/isobmff.rs:451`). Either add a small udta writer, or set `-metadata comment=<SwarmUI JSON>` in the ffmpeg remux, then call `mirror_uuid_sidecar`. Include the scene recipe (provider, model, seed, prompt, voice id, line texts, reference hashes, draft id) so Remix/Reuse can rebuild the scene.
4. Call `api::save_video_to_gallery` (`commands/api.rs:1863`) into `owner_of(id)`'s gallery (`webserver::user_gallery_dir`, `webserver.rs:7371`), respecting `manual_save_mode` (`config.rs:247-252`), then emit `comfyui:output_video` with the same payload shape as the ComfyUI path.

Interpolation of cloud clips already works through `submit_interpolation` (`commands/video_interpolate.rs:101`) when ComfyUI can read the host path. There is no local video upscaler today (SeedVR2 is image-only, `templates/upscale.rs:6-31`); a local anime upscale (Real-ESRGAN anime video model or a SeedVR2 video graph) is a separate follow-up.

### 5.7 Frontend

- New store `animeScene.svelte.ts` (class singleton with `$state`, `get` accessors, explicit `saveSettings()`, no import cycles with `generation`; cross-store effects in `App.svelte` per `CLAUDE.md`).
- Components under `src/lib/components/video/anime/`: character picker, voice panel (design, previews, slots), script/takes panel (audio playback can reuse `MusicPlayer` waveform code, `MusicPlayer.svelte:24-44`), shot list, cost confirmation dialog, result actions (keep audio / exact voice / upgrade / upscale).
- All IPC through `ipcInvoke`/`ipcListen` (`ipc.ts`), typed wrappers in `api.ts`. Large uploads count against the 256 MiB browser body limit (`webserver.rs:1207`); upload character images and audio by gallery reference, not inline bytes, where possible.
- i18n: every string through `locale.t()`, keys in all 12 locale files; `npm run check:i18n` is a blocking gate.
- Tailwind only, `onclick`, theme accents per `CLAUDE.md`.

---

## 6. Cost and safety UX

### 6.1 to 6.5 Conventions to keep

- Estimates are prefixed `~` and the provider balance is authoritative (`docs/NOVELAI.md` 6.1).
- Disclose destination and "provider charges may apply" before any upload (`en.ts:105` precedent).
- Every non-fatal skip emits a notice with a locale-keyed reason (`face_detail::notify` precedent).
- Tests that would hit paid providers are `#[ignore]` with local mocks (`music_audio_style.rs:928-929` precedent).
- Never log keys, signed URLs or uploaded media URLs at info level.

### 6.6 New convention: itemized confirmation for multi-provider jobs

MooshieUI today shows cost badges but no confirmation dialog (`GenerateButton.svelte:611-681`). A scene can spend on three providers in one click, and the prototype showed users get surprised. Proposal: before any scene job, show an itemized estimate and require one explicit confirm:

| Line item | Source of the estimate |
|---|---|
| ElevenLabs TTS (n lines x takes) | characters x the model's credit rate; show remaining credits from `GET /v1/user/subscription` |
| NovelAI keyframes | `estimateNovelAiCost` plus the Precise Reference surcharge |
| Video | provider formula from `price.rs` using the clip's real resolution, duration and inputs |
| Optional draft upgrade | priced separately, only when the user asks |

Price formulas must come from the provider's own page, implemented as pure Rust with unit tests, and stamped with the date they were checked:

- fal Seedance 2.5 (fal model page, 2026-10-11): tokens = `height x width x (input_seconds + output_seconds) x 24 / 1024`; $0.0214 per 1,000 tokens at 480p/720p, about $0.0234 at 1080p; multiply by 0.6 only when video inputs are attached; image and audio references are not billed. Published examples: about $0.2205/s at 480p, $0.4730/s at 720p, $1.164/s at 1080p. The prototype's 28 s 480p draft matched the $0.2205/s figure.
- Segmind: read from its pricing pages when the adapter is built; do not reuse aggregator tables.

---

## 7. Doc-grounded prompt builders

Prompts in this pipeline are built by code (and optionally refined by the prompt assistant). They must follow each model's documented prompting rules, the same way `docs/NAI_V5_PROMPTING_AGENTS.md` and `src/lib/utils/h3Prompt.ts` ground NovelAI V5 and H3 prompts today. Requirements:

1. **One agent-facing prompting doc per model family**, in `docs/`: `SEEDANCE_PROMPTING_AGENTS.md` and `ELEVENLABS_PROMPTING_AGENTS.md` (NovelAI V4.5 rules can extend the existing NovelAI docs). Each rule is tagged `[official]` or `[secondary]` with its source URL and the date it was checked. Section 7.2 to 7.4 below are the starting content.
2. **Builders are pure Rust functions with unit tests** that assert the rules (section structure present, every reference has a role sentence, every line quoted with a window, constraints last, no automatic retry after a `blocked_generation` for a minor-flagged character, and so on). No frontend test framework exists, so the rules must live in Rust to be testable (`docs/NOVELAI.md` 2.8 precedent).
3. **The prompt assistant gets the same rules as grounding** (`src-tauri/src/prompt_assistant/grounding.rs`), so assistant-written scripts and shot lists respect provider limits and prompting rules.
4. **A refresh procedure**: when a provider ships a new model version, re-read the official guide before changing builders, update the doc's rule tags and dates, then update tests. BytePlus's official Seedance docs are JavaScript-rendered and could not be fetched by automated tools during this research; a human should read them in a browser and confirm or correct the `[secondary]` rules below.
5. **Show the final prompt** to the user before paid submission, editable, with the builder's version.

### 7.1 Seedance prompt template (what the builder emits)

Section order per fal's Seedance 2.5 guide: Format, Reference roles, Starting state, Timeline, Camera, Continuity, Audio, Ending state, Constraints. Drop sections that solve nothing for the shot.

```
Format: {seconds} seconds, {aspect}, 2D Japanese TV anime look, cel shading, clean line art,
hand-drawn character animation at normal speed. {cuts sentence}

Reference roles: @Image1 controls only the character's identity: {invariant traits}; do not
copy its background or pose. @Image2 controls only the location; it contains no people.
@Image{n} controls only the framing and expression of the shot it is named in.
@Audio1 is her voice and contains all of her lines in order.

Starting state: {where she is, what she holds, wind/light direction, mouth closed}.

{a}-{b} seconds: {framing like @ImageN}. {action}. From {start} to {end} seconds she says:
"{exact line}" {delivery}. Her mouth moves only during this line.
... one block per shot ...

Continuity: only she is on screen. Same face, hair, outfit, accessories in every shot.
Her mouth stays closed in every pause.

Audio: clean lip-synced {language} speech in the voice of @Audio1; {ambience}; no music.

Ending state: {final frame}.

Constraints: no other characters, no extra hands, no subtitles or on-screen text, no music,
no slow motion, no repeated actions, no morphing of face or outfit, no 3D rendering.
```

The prototype's first draft used a looser prompt (shots labelled "Shot 1 (0 to 1.5 s)", lines referred to as "her first line" instead of quoted). Its timing drift (establishing shot 5 s instead of 1.5 s) is consistent with the guide's warning that untimed or loosely timed beats get padded. A guide-conformant v2 prompt was written but not yet run; validating it is part of Phase 2.

### 7.2 Seedance rules (starting content for `SEEDANCE_PROMPTING_AGENTS.md`)

All `[secondary]` unless noted: official BytePlus pages were not machine-readable. Primary source: [fal Seedance 2.5 prompting guide](https://fal.ai/learn/devs/seedance-2-5-prompting-guide); multi-angle and audio details from [fal multi-angle guide](https://fal.ai/learn/tools/how-to-create-multi-angle-video-seedance-2-5).

- S1. Use the section order in 7.1. Describe the starting state before any action.
- S2. Timed beats use plain blocks, `0-5 seconds: ...`; each beat starts from the previous beat's result. Timestamps help ordering and reduce skipping but are **approximate, not guaranteed**.
- S3. Long clips need enough distinct events; empty duration becomes waiting, repetition or slow motion.
- S4. Continuous take: say "continuous single take" in Format and "no cuts" in Constraints. **There is no documented hard-cut syntax**; multi-shot results come from describing scene changes. Treat cut placement as best-effort.
- S5. References are numbered by upload order per modality (`@Image1`, `@Video1`, `@Audio1`). Give each one narrow job: what it controls, and what it must not transfer.
- S6. Images for appearance and location; video for camera motion or performance timing; audio for voice/rhythm.
- S7. Limits (2.5): 30 images, 10 videos, 10 audio files, 50 total; each video/audio 1.8 to 30.2 s; per-modality total at most 30.2 s. Output 4 to 30 s. (2.0: 9 images, 3 videos, 3 audio, 12 total, 15 s.)
- S8. Dialogue in straight quotes, with start and end times and the speaker named; for silent characters say they keep their mouth closed.
- S9. Say "mouth moves only during her own lines" and ask for "clean lip-synced speech" in Audio.
- S10. **Mouth map** for external audio: derive talking windows and quiet windows from word-level timestamps (ElevenLabs Scribe or forced alignment), list them in a MOUTH MAP section, and forbid invented lines in quiet windows.
- S11. Camera: give screen position (for example "left third"), when a move starts and stops tied to a visible event, axis rules, and "the camera never moves" for locked shots. Avoid vague words like "dynamic".
- S12. No `negative_prompt` field; constraints go in a final Constraints section and should match the shot's real risks.
- S13. Iterate by changing one thing at a time. For continuation, pass the previous clip's last frame as `@Image1` and describe only what is finished, what stays fixed and the first new action.
- S14. Draft mode (2.5 on fal): `draft: true` returns a 480p clip and a `draft_id`; `bytedance/seedance-2.5/draft/complete` renders it at 1080p within 7 days on the same account. Not documented for 2.0 or confirmed for other providers (Segmind documents its own draft/final pair).
- S15. Anime: no anime-specific official advice found. State the anime look in Format and rely on image references for style; the prototype confirms this works.

Unknowns to keep visible in the doc: whether `@Audio` is used verbatim, cloned, or as a style/timing hint (prototype: re-synthesized, similar voice, timing not followed); exact cut behaviour; whether dialogue language must match the audio reference.

### 7.3 ElevenLabs rules (starting content for `ELEVENLABS_PROMPTING_AGENTS.md`)

Official sources: [TTS best practices](https://elevenlabs.io/docs/overview/capabilities/text-to-speech/best-practices.md), [Eleven v4](https://elevenlabs.io/docs/overview/capabilities/text-to-speech/eleven-v4.md), [TTS API](https://elevenlabs.io/docs/api-reference/text-to-speech/convert), [Voice Design](https://elevenlabs.io/docs/eleven-creative/voices/voice-design.md), [Text to Dialogue](https://elevenlabs.io/docs/overview/capabilities/text-to-dialogue).

- E1 [official]. Audio tags in square brackets, placed next to the phrase they affect. Three kinds: delivery/emotion, reactions, sound effects. Delivery tags can be misread as sounds, so phrase them as voice descriptions (`[haughty, scoffing voice]`, as the prototype did).
- E2 [official]. Ellipses add pauses and weight; capitals add emphasis. No SSML `<break>` on v4.
- E3 [official]. v4 uses stability and similarity; style and speed sliders are not available for v4 in the UI. Lower stability is more expressive; the prototype used 0.35 with good results ([secondary] value).
- E4 [official]. For Japanese, `apply_language_text_normalization: true` improves pronunciation at a large latency cost; pass `language_code: "ja"`.
- E5 [official]. v4 limit 10,000 characters per request; Text to Dialogue 2,000 characters per request, one `voice_id` per turn.
- E6 [official]. Voice Design prompt order: language and region first; gender, age range, quality; persona (2 to 5 words) and 2 to 3 emotions; 1 to 2 sentences on timbre, pacing, delivery. Avoid effect words (reverb, echo, phone, tape). Preview text 100 to 1,000 characters, matching the voice's mood; longer is more stable. Three previews, billed once.
- E7 [prototype, secondary]. Voice Design blocks prompts that describe minors (`blocked_generation`). When the character is flagged as a minor, that block is final and shown to the user; builders never retry it with a reworded age.
- E8 [official]. Premade voices expire 2026-12-31; designed and cloned voices persist on the account. Saving uses a voice slot.

### 7.4 NovelAI V4.5 rules for this pipeline

Official: [Precise Reference](https://docs.novelai.net/en/image/precisereference), [models](https://docs.novelai.net/en/image/models), [inpainting](https://docs.novelai.net/en/image/inpaint), [strength and noise](https://docs.novelai.net/en/image/strengthnoise), [tags](https://docs.novelai.net/en/image/tags).

- N1. Precise Reference is V4.5 only, incompatible with Vibe Transfer, and costs 5 Anlas per generation plus any per-reference charge (measured: 5 per generation with one reference).
- N2. Strength copies visual cues, including unwanted pose and expression; lower it if poses are frozen. Fidelity enforces the reference; high is hard to override.
- N3. Best references: clean, large, plain background, character large in frame; accepted canvases 1024x1536, 1536x1024, 1472x1472.
- N4. V4.5 prompts must be ASCII (no Japanese). Tag order: `1girl`, character tags, series, everything else.
- N5. Small-region edits: NovelAI recommends Focused Inpainting (region upscaled to about 1 MP). Through the API, emulate it with crop, upscale, img2img, blend (prototype-verified); raw `infill` on small masks returned grey patches.

---

## 8. Testing and validation

- `cargo test` for: secret-store migration and redaction; `audio.rs` window maths and limit enforcement; `prompt.rs` rule assertions (7.1 to 7.3); `price.rs` against the provider's published examples (for example 28 s at 480p on fal is about $6.17); capability-driven validation; job id persistence and resume.
- Provider adapters tested against recorded fixtures with a local mock; real calls `#[ignore]`.
- Gates per `CLAUDE.md`: `npm run build`, `cargo check` (desktop and `--no-default-features --features server`), `cargo test`, `cargo fmt`, `npm run check:i18n`.
- Manual qualification (paid, done once per phase by the maintainer): one 8 to 10 s draft per provider, one full scene, cancel mid-job, kill the app mid-poll and confirm resume, browser-mode run with a named account using its own keys.

---

## 9. Phases

| Phase | Scope | Exit criteria |
|---|---|---|
| 0 | Scope issue and decision; multi-provider secret store; settings UI for keys | Keys saved per account, never returned to the client, tests green |
| 1 | ElevenLabs: voice list, design with previews, save (slots), TTS takes with caching, Japanese normalization option | Designed voice and takes stored per user; `blocked_generation` shown clearly and never auto-retried |
| 2 | Cloud video core: `VideoProvider` trait, fal Seedance 2.5 adapter, scene job, itemized confirmation, gallery ingest, Seedance prompt builder and doc | A 28 s draft scene lands in the gallery with poster and metadata; v2 prompt validated against the prototype draft |
| 3 | NovelAI keyframes and expression refs in the scene flow; Precise Reference cost in `novelaiCost.ts`; prompt assistant grounding for scripts and shots | Keyframes improve shot adherence in a side-by-side test; Anlas estimate matches the balance delta |
| 4 | Finishing: exact-voice mux option, mouth map via Scribe, draft completion to 1080p, Segmind adapter | Draft upgrade priced and working; Segmind response shapes verified |
| 5 (optional) | Free local fallback: mouth-flap animatic from keyframes; local anime upscale; Wan 2.7 / Kling adapters | Separate proposals |

---

## 10. Risks and open questions

- **Scope** (section 2): the whole feature depends on the maintainer accepting user-keyed cloud video.
- **Audio reference semantics** are undocumented; users who need their exact voice may get a re-synthesized one. Mitigated by the exact-voice mux option and the mouth map, not solved.
- **Content policy**: young characters trigger provider filters (ElevenLabs Voice Design did; fal did not for this everyday scene). Builders must keep prompts innocent and strip body descriptors; surface provider moderation errors clearly and treat them as final. MooshieUI must not try to bypass any provider filter.
- **Character IP**: users will reference existing franchises; that is the user's responsibility under each provider's terms, but the UI should not ship franchise-specific presets.
- **Price drift**: provider prices and formulas change; `price.rs` constants carry a checked date and the UI labels figures as estimates.
- **Provider churn**: Seedance 2.0 to 2.5 changed limits within months; keep limits in capabilities data.
- **Self-hosted servers**: named accounts must never spend the owner's cloud keys (mirror the NovelAI rule).
- Open: should scenes get their own gallery type (a scene recipe with assets) or stay plain videos with rich metadata? Recommended start: plain video plus metadata, with the recipe JSON stored alongside for Remix.

---

## Appendix A. Provider API reference (as used or read on 2026-10-11)

**fal Seedance 2.5** `bytedance/seedance-2.5/reference-to-video` ([API page](https://fal.ai/models/bytedance/seedance-2.5/reference-to-video/api)): `prompt`, `task` (`reference` | `editing` | `extension`), `image_urls` (30), `video_urls` (10), `audio_urls` (10; MP3/WAV, 1.8 to 30.2 s each, 30.2 s total), `resolution` (480p, 720p, 1080p listed), `draft` (bool, returns `draft_id`), `duration` (`auto` or 4 to 30), `aspect_ratio`, `generate_audio` (default true, same price), `bitrate_mode`, `codec`, `seed`. Output `{video: {url}, seed, draft_id}`. Upload via fal storage; queue API for long jobs.

**fal Seedance 2.0** `bytedance/seedance-2.0/reference-to-video` and `/fast/...`: `image_urls` (9), `video_urls` (3), `audio_urls` (3, 15 s total; needs at least one image or video), `resolution`, `duration` (`auto`, 4 to 15), `aspect_ratio`, `generate_audio`.

**Segmind Seedance 2.0 Fast** `https://api.segmind.com/v1/seedance-2.0-fast`, header `x-api-key` ([API page](https://www.segmind.com/models/seedance-2.0-fast/api)): `prompt` (cite inputs as "image 1", "audio 1"), `reference_images` (9), `reference_audios` (3 MP3, requires an image or video), `reference_videos` (3), `first_frame_url`/`last_frame_url` (not combinable with references), `resolution` (480p, 720p), `duration`, `aspect_ratio`, `generate_audio`, `seed`. Upload: `POST https://workflows-api.segmind.com/upload-asset` with `{data_urls: [...]}` ([docs](https://docs.segmind.com/api-reference/segmind-storage)). Response shapes not yet verified.

**ElevenLabs**: `POST /v1/text-to-voice/design` (`voice_description` 20 to 1,000 chars, `model_id: eleven_ttv_v3`, `text` 100 to 1,000 chars, `guidance_scale`, `seed`), `POST /v1/text-to-voice` (save), `GET /v2/voices`, `DELETE /v1/voices/{id}`, `GET /v1/user/subscription`, `POST /v1/text-to-speech/{voice_id}` (`model_id: eleven_v4`, `language_code`, `voice_settings`, `seed`; header `character-cost` returns the charge). Auth header `xi-api-key`.

**NovelAI** (existing client): host `https://image.novelai.net`, `POST /ai/generate-image` (zip) or `/ai/generate-image-stream` (msgpack frames); Precise Reference fields `director_reference_images`, `director_reference_descriptions` (`base_caption` `character` or `character&style`), `director_reference_information_extracted` (1.0), `director_reference_strength_values`, `director_reference_secondary_strength_values` (1 minus fidelity); all five required.

## Appendix B. Prototype artifacts

The standalone prototype (not app code; for reference only): `make_voice.py` (TTS takes), `nai_tool.py` + `crop_edit.py` (NovelAI sheet, keyframes, crop-img2img mouth variants), `lipflap.py` (loudness mouth flaps), `seedance_scene.py` with `seedance25.json` (first draft prompt) and `seedance25_v2.json` (guide-conformant prompt, unrun). Available from the user on request; they live in a temporary session workspace.

## Sources

Provider and model documentation: [fal Seedance 2.5 API](https://fal.ai/models/bytedance/seedance-2.5/reference-to-video/api), [fal Seedance 2.5 pricing](https://fal.ai/models/bytedance/seedance-2.5/reference-to-video), [fal Seedance 2.5 prompting guide](https://fal.ai/learn/devs/seedance-2-5-prompting-guide), [fal multi-angle Seedance 2.5](https://fal.ai/learn/tools/how-to-create-multi-angle-video-seedance-2-5), [fal Seedance 2.5 vs 2.0](https://fal.ai/learn/devs/seedance-2-5-vs-seedance-2-0), [fal Seedance 2.0 reference-to-video API](https://fal.ai/models/bytedance/seedance-2.0/reference-to-video/api), [Segmind Seedance 2.0 Fast API](https://www.segmind.com/models/seedance-2.0-fast/api), [Segmind pricing](https://www.segmind.com/models/seedance-2.0-fast/pricing), [Segmind storage](https://docs.segmind.com/api-reference/segmind-storage), [Segmind Seedance 2.5 draft/final](https://www.segmind.com/models/seedance-2.5-draft-final/pricing), [BytePlus Seedance 2.0 tutorial](https://docs.byteplus.com/en/docs/modelark/2291680) (not machine-readable), [ElevenLabs TTS best practices](https://elevenlabs.io/docs/overview/capabilities/text-to-speech/best-practices.md), [ElevenLabs Eleven v4](https://elevenlabs.io/docs/overview/capabilities/text-to-speech/eleven-v4.md), [ElevenLabs TTS API](https://elevenlabs.io/docs/api-reference/text-to-speech/convert), [ElevenLabs Voice Design](https://elevenlabs.io/docs/eleven-creative/voices/voice-design.md), [ElevenLabs Text to Dialogue](https://elevenlabs.io/docs/overview/capabilities/text-to-dialogue), [NovelAI Precise Reference](https://docs.novelai.net/en/image/precisereference), [NovelAI models](https://docs.novelai.net/en/image/models), [NovelAI inpainting](https://docs.novelai.net/en/image/inpaint), [NovelAI strength and noise](https://docs.novelai.net/en/image/strengthnoise), [NovelAI tags](https://docs.novelai.net/en/image/tags).

Alternatives considered: [Alibaba Wan 2.7 image-to-video API](https://www.alibabacloud.com/help/en/model-studio/image-to-video-general-api-reference), [Kling lip sync guide](https://kling.ai/quickstart/ai-lip-sync-guide), [PixVerse C1 review](https://pixverse.ai/en/blog/pixverse-c1-cinematic-ai-video-model-review), [Segmind blog: fal vs Segmind per-token pricing](https://blog.segmind.com/seedance-2-5-pricing-fal-charges-95-more-per-token-than-segmind/).
