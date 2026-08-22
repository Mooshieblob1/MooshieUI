# NovelAI backend

MooshieUI can generate through the NovelAI image API as a second backend
alongside ComfyUI. This document records what the integration is and why it is
built the way it is. It is the design rationale, not a task list.

## 1. What the user gets

1. An API key field in Settings, in its own NovelAI section.
2. Four new entries in the model dropdown: NovelAI V5 Full, V5 Curated,
   V4.5 Full, V4 Full.
3. Selecting a NovelAI model adapts the UI. Controls that do not apply to
   NovelAI are hidden, NovelAI-only controls appear.
4. NovelAI inpainting and img2img replace ComfyUI's while in NovelAI mode.
5. A free local upscale, so a paid image can be enlarged without spending
   Anlas. Anima at denoise 0.15 to 0.2 is the suggested refiner.
6. FaceFix, also as a free local pass.
7. Anlas remaining and Opus subscription status, in the Settings NovelAI
   section. An option there also pins a compact version of the same readout to
   the generation page, directly above the generate button. (The Opus
   generation allowance bar is not built: see section 6.)
8. The Anlas cost of the pending request, on the generate button.

NovelAI's recommended sampling defaults are applied when a NovelAI model is
selected: 23 steps, guidance 7.0, Euler Ancestral, Karras, CFG rescale 0.

## 2. Architecture

The client is Rust, not JavaScript. It lives in `src-tauri/src/novelai/`:

| File | Responsibility |
|------|----------------|
| `models.rs` | The `MODELS` table: model ids, inpainting ids, per-model capability flags |
| `params.rs` | `NovelAiParams` and its sub-structs (characters, vibes, director references, coordinates) |
| `payload.rs` | Builds the request body NovelAI expects |
| `response.rs` | ZIP image unpacking, msgpack stream decoding, the subscription shape |
| `client.rs` | The three HTTP calls and their status-code mapping |
| `mod.rs` | Orchestration: prompt ids, the event sink, `run()`, image delivery |

Endpoints:

- `https://image.novelai.net/ai/generate-image`
- `https://image.novelai.net/ai/generate-image-stream`
- `https://image.novelai.net/user/subscription`

All three are on the image host. The subscription record used to be served
from `api.novelai.net`, which now answers with 400 "Please refresh
NovelAI.net. If using a third-party tool, update to the image URL."

Keeping the client in Rust means one code path serves both the desktop app and
browser mode, the API key never leaves the backend, and generation reuses the
existing event, image and queue plumbing rather than duplicating it.

### 2.1 NovelAI reuses the ComfyUI event contract

A NovelAI generation mints a synthetic prompt id `nai-{uuid}` and emits the same
events the frontend already handles:

- `comfyui:progress`
- `comfyui:preview`
- `comfyui:output_image`
- `comfyui:executing` with `node: null`, which means completion
- `comfyui:execution_error`

So `App.svelte` needs no new event handlers. The generate command returns the
prompt id immediately and does the work in a spawned task, exactly like
`queue_prompt`, so the frontend's pending-prompt state is populated before the
first event lands.

### 2.2 Output images reuse the ComfyUI pipeline

`process_output_image` parses a MooshieSaveImage binary frame (a 4-byte
big-endian event id, a 4-byte format tag, then the payload). `deliver_image()`
synthesises such a frame and hands it to that existing function, which buys temp
file handling, JXL encoding, the SSE payload shape and the output cache without
duplicating any of it.

### 2.3 NovelAI is deliberately outside the ComfyUI fair queue

A NovelAI request uses no local GPU, so it does not compete for a worker slot
and does not take one. It still creates a queue entry and can still bind an
alias, which is what lets the optional local post-process report as a single
generation.

### 2.4 Paid work is never silently lost

Anlas is real money, so every failure mode degrades rather than discards:

- `generate_stream` keeps the last preview it saw per sample. If the stream dies
  before its final frames, those previews are delivered instead of nothing.
- Malformed stream frames are skipped, not treated as fatal.
- `normalise_action()` degrades `infill` to `img2img` to `generate` when the
  required image or mask is missing, so a malformed request never burns Anlas on
  a guaranteed 400.
- If the free local post-process cannot start, the NovelAI image is delivered
  unmodified with a warning rather than lost.

### 2.5 Precise Reference and vibe transfer are mutually exclusive

NovelAI rejects a request carrying both. `apply_director_references()` removes
the vibe arrays before writing the director arrays, and the UI mirrors the same
rule. Note that "Precise Reference" and "character reference" are two names for
the same `director_reference_*` system.

### 2.6 Model capabilities are data, not branches

`models.rs` carries per-model booleans (`v4_prompt`, `precise_reference`,
`vibe_transfer`, `character_negatives`, `inpainting_id`). V5's reference
features are wired but set to `false`, because NovelAI has not shipped them.
Turning them on later is a two-boolean change, not a code change.

### 2.7 The generation stream is msgpack, not SSE

`/ai/generate-image-stream` does not speak server-sent events. The request opts
in with `parameters.stream = "msgpack"`, and the response is a sequence of
frames, each a big-endian `u32` byte count followed by that many bytes of
msgpack:

| Key | Meaning |
|-----|---------|
| `event_type` | `intermediate`, `final` or `error` |
| `image` | the image bytes, as a msgpack binary |
| `samp_ix` | which sample of the batch this frame belongs to |
| `step_ix` | the zero-based step, on intermediate frames |
| `message` | the error text, on error frames |

`stream` is injected in `client.rs` on a copy of the body rather than in
`payload.rs`, because the non-streaming `/ai/generate-image` endpoint must not
receive it. `StreamDecoder` buffers until a whole frame is present, since chunk
boundaries have nothing to do with frame boundaries, and refuses a length
prefix beyond 64 MiB so a corrupt one fails loudly instead of waiting forever
for bytes that are never coming.

Finals are collected into a map keyed by `samp_ix`, because NovelAI interleaves
the samples of a batch and the images have to come back in order.

### 2.8 The Opus allowance is derived in Rust

`/user/subscription` returns `usage: { percent, isNegative, timeUntilNextPercent }`
for Opus accounts. `percent` is the allowance **remaining**: NovelAI's own web
app labels the same field "% of Opus Generations remaining".

`Usage::allowance()` reduces those three numbers to the `OpusAllowance` the bar
draws, mirroring the website's arithmetic so the two readouts agree:

- displayed percent is `isNegative ? 0 : percent` clamped to 0 through 100
- "low" is `isNegative || percent < 5`
- refill rate is `86400 / timeUntilNextPercent`, rounded to one decimal
- the image estimate is `17.3` images per percent, which is the site's own ratio

Doing this in Rust rather than the component keeps it under test, since the
frontend has no test framework.

### 2.9 The NovelAI panel is one section, not scattered options

Everything NovelAI owns that ComfyUI has no equivalent for lives in a single
collapsible NovelAI section on the generation page, shown only in NovelAI mode:
per-character prompts with a 5x5 placement grid, Precise Reference, Vibe
Transfer, the NovelAI-only sampling options (quality tags, undesired-content
preset, Variety+, dynamic thresholding, guidance rescale, undesired-content
strength), the img2img/inpainting strength and noise, and the local
post-process toggle.

Sampler, steps and guidance are deliberately **not** in it. Those are top-level
generation params shared with ComfyUI, so they stay in the Sampler panel and do
not move when the backend changes. The panel adapts its contents instead: in
NovelAI mode the Sampler panel lists NovelAI sampler names and a noise
schedule, and labels CFG as Guidance.

Going the other way, ComfyUI controls with no NovelAI counterpart are hidden
rather than left to silently do nothing: ControlNet, Style Transfer, LoRAs,
VAE, the denoise slider, Differential Diffusion and the grow-mask slider.
FaceFix and Upscale stay visible on purpose, because in NovelAI mode they drive
the free local post-process pass (section 3).

## 3. The free local post-process

NovelAI has already been paid for the pixels it returns, so upscaling and face
fixing them on the user's own GPU costs nothing extra. That pass is a normal
ComfyUI graph, built by `src-tauri/src/templates/upscale_standalone.rs`.

It does not hand-assemble a graph. It derives a refine-only img2img
`GenerationParams` and calls the ordinary `build_workflow`, because
`img2img::build` already short-circuits after `LoadImage` in refine-only mode.
That keeps the v-prediction, cascade, rectified-flow and smart-guidance
injections in one place.

Two details make it work:

- **The checkpoint has to be swapped.** In NovelAI mode `params.checkpoint`
  names a NovelAI model that ComfyUI cannot load, so the request carries a
  separate `local_checkpoint` (plus its architecture and v-pred flag). Without
  one, the pass is skipped and the image is delivered untouched.
- **The prompt does not need swapping.** The NovelAI syntax rewrite happens in
  Rust while the request body is built and never mutates `params`, so the
  top-level prompt stays in ComfyUI syntax and is what the local pass uses. The
  request also carries optional `local_positive_prompt` and
  `local_negative_prompt` overrides, for a caller that wants the local pass to
  run on different text; when they are null the top-level prompt is used.

Both stages report as one generation: the ComfyUI prompt is alias-bound to the
NovelAI prompt id, so the websocket re-emits its progress, previews, output
image and terminal event under the id the frontend is already tracking.

**Known limitation: the local pass runs on single-image generations only.** One
ComfyUI prompt maps to one alias and one GPU worker, and the first terminal
event finishes the queue entry. A multi-image NovelAI batch has no safe
single-prompt handoff, so batches are delivered untouched and a warning is
logged. Generating one image at a time is the way to get the free upscale today.

## 4. Prompt syntax

Outside NovelAI mode the app translates NovelAI weight syntax (`1.1::tag::`)
into ComfyUI syntax (`(tag:1.1)`). In NovelAI mode that reverses: what the user
types in A1111 or ComfyUI syntax is converted to NovelAI syntax on the way out.
Behaviour outside NovelAI mode is unchanged.

The `@` sigil is also mode-dependent. In ComfyUI mode artist tags are inserted as
`@tag`. In NovelAI mode they are inserted bare, because there `@` is the
prompt-chunk reference sigil (`@[chunk name]`), not an artist marker.

## 5. Security

The NovelAI API key is a user secret and is handled like the existing Civitai
key:

- Stored in the app config, carried forward by `preserve_secrets()` so a config
  write that omits it cannot blank it. Clearing is explicit, through
  `set_novelai_api_key("")`.
- Redacted to null in the config JSON sent to browser clients, alongside a
  `novelai_api_key_configured` boolean so the UI can show "key set" without ever
  receiving the value.
- Never logged. `NovelAiClient` deliberately does not derive `Debug`.
- The NovelAI commands are moderator-gated in browser mode. Every NovelAI
  generation spends the host's real Anlas, so a LAN guest must not be able to
  bill the person running the instance.

## 6. Known unknowns

These are inferences, not confirmed facts. They are listed so the next person
does not mistake them for verified behaviour.

1. **The Anlas cost badge is an estimate, not NovelAI's number.** The formula
   in `src/lib/utils/novelaiCost.ts` was fitted from observed prices; NovelAI
   publishes no formula and its web bundle carries no cost function to read one
   from. The badge is prefixed with `~` and the readout above the generate
   button remains the authority on what was actually spent.
2. **`n_samples` is clamped to 1 through 8** in `payload.rs`. NovelAI's real cap
   is unconfirmed.
3. **The image estimate on the Opus bar is NovelAI's own approximation.** The
   allowance is a percentage, not an image count; `17.3` images per percent is
   the ratio the website applies, and it is presented as approximate there too.
4. **The Opus generation allowance bar is not built.** The field names that
   carry the remaining allowance are still unconfirmed, and a bar fed by a
   guessed field would read wrong rather than read empty. Anlas remaining and
   Opus status are shown instead.

`Subscription.extra` captures any key the backend does not name, and
`fetch_subscription` logs them at debug level, so a field NovelAI adds later
shows up rather than being silently dropped.

Confirmed since the first draft, and no longer guesses: the four V5 model ids
and their inpainting variants, the subscription host, the `usage` field names
and the direction of `percent`, and the streaming protocol.

## 7. Manual test checklist

Nothing in this backend is covered by an automated test that touches NovelAI's
servers, so every phase that ships is followed by a hand-test pass recorded
here, newest first. Each entry says plainly whether testing is needed at all.

### 2026-08-23 - Phase E: the NovelAI panel (PR #618)

**Testing required: yes.**

| # | Step | Expected |
|---|------|----------|
| 1 | Select a NovelAI model | A "NovelAI" section appears on the right; ControlNet and Style Transfer sections are gone |
| 2 | Switch back to a local model | The NovelAI section disappears, ControlNet and Style Transfer are back |
| 3 | In NovelAI mode, open the model/LoRA area | No LoRA list, no VAE picker |
| 4 | Add a character, type a prompt, generate | The character prompt visibly affects the result |
| 5 | Turn on "Place characters on a grid", pick a corner cell, generate | The character lands in roughly that part of the frame |
| 6 | Add two characters with clearly different descriptions | They stay distinct rather than blending |
| 7 | Add a Vibe Transfer image, then add a Precise Reference image | The vibe list empties and dims; only the reference survives |
| 8 | Do the reverse (reference first, then vibe) | The reference list empties and dims |
| 9 | Select V5 (or whichever model lacks them) | Both reference panels show the "not supported" note instead of controls |
| 10 | Set undesired-content strength off 1.00 | The amber 30-percent warning appears |
| 11 | Look at the generate button in NovelAI mode | An Anlas estimate badge is shown; it moves when you change size or steps |
| 12 | With Opus, at 1024x1024 and 28 steps or fewer | The badge reflects the first sample being free |
| 13 | Switch to img2img or inpainting in NovelAI mode | Strength and Noise appear in the NovelAI panel; the ComfyUI denoise slider is gone |
| 14 | Switch to inpainting | "Keep the area outside the mask" appears; Differential Diffusion and the grow-mask slider are gone |
| 15 | Turn on Local post-processing without picking a checkpoint | Amber "pick a checkpoint" warning |
| 16 | Pick a checkpoint but leave Upscale and Face Fix off | Amber "nothing to do" warning |
| 17 | Enable Face Fix, generate on NovelAI | The image comes back face-fixed, and no extra Anlas is spent |
| 18 | Collapse the NovelAI section, restart the app | It is still collapsed |
| 19 | Switch UI language | Every label in the NovelAI panel is translated |

**Do not skip:** 1, 7, 11, 17. Those cover the section wiring, the exclusivity
rule, the cost estimate and the free local pass.
**Low-risk, skip if short on time:** 6, 18, 19.
