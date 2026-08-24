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

### 2.10 Vibe transfer is a two-step flow on V4 and later

V3 accepted a raw base64 PNG inline in `reference_image_multiple`. V4 and
V4.5 do not, and they say so with a bare `500 Error generating image, an
internal error occurred` that names nothing. The image has to be posted to
`/ai/encode-vibe` first, with `{image, model, information_extracted}`; the
reply is raw `.naiv4vibe` bytes whose base64 is the value the generate call
wants.

`encode_pending_vibes()` in `novelai/mod.rs` runs that pass before the payload
is built, which is why `run_inner` constructs the client before calling
`build_request`. A vibe that still has no token by the time `apply_vibes()`
runs is dropped rather than sent raw, so a failed encode costs a missing
reference and not an unexplained 500.

NovelAI bills 2 Anlas per encode, per image and per `information_extracted`
value, so results are cached for the life of the process, keyed on the model,
the extraction level and a SHA-256 of the image.

A restart used to pay again, because the token never left Rust. It does now.
`emit_vibe_encodings()` sends a `novelai:vibes_encoded` event carrying, per
vibe, the token plus the model and the extraction level it was minted for.
The frontend stores all three next to the image in `novelaiSettings`, which
is already persisted, and sends them back on the next generation.
`vibe_needs_encoding()` then re-encodes only when the token is missing, or
when the model or the extraction level no longer matches what the token was
minted for, so a stale token is paid for again rather than sent to a server
that would reject it. The process-lifetime cache still sits in front of all
of this. In browser mode every client sees the event, so the listener applies
it only when the prompt id is one of its own pending prompts, the same filter
previews already use.

The V4 key set also differs: `reference_information_extracted_multiple` is
gone (the value is baked into the token at encode time) and
`normalize_reference_strength_multiple` takes its place.

That last key is a decoy. NovelAI's team confirmed it is frontend logic and
that the backend does nothing with the flag at all, which matches the
measurement that first raised the question: a seed-1 pair on
`nai-diffusion-4-5-full` with two vibes at 1.0 each came back pixel-identical
with the flag true and false. So `normalize_strengths()` in `payload.rs` does
the division here instead, dividing each strength by their sum just before the
request goes out. A sum of zero is left alone rather than divided by, and a
set that already sums to 1 is skipped so two vibes at 0.5 stay a genuine
no-op. The flag itself is still sent, because NovelAI's own client sends it
and request parity is worth more than the byte. The sliders keep showing the
raw values the user set, which is also what the official client does.

### 2.11 Transparent BG is a prompt tag, not a request field

V5's custom VAE is the first NovelAI VAE with a real alpha channel, and their
site exposes it as a "Transparent BG" button beside the prompt box. That button
is a tag shortcut. There is no request field for transparency: the whole feature
is the tag `2.1::transparent background::`, at the weight NovelAI's own release
notes recommend, and the model does the rest.

MooshieUI copies the behaviour but not the placement. The toggle lives in the
NovelAI advanced section and the tag is appended by `payload::with_transparency`
while the request body is built, so it never appears in the user's prompt box.
That is the point of doing it in Rust: the toggle owns the tag, so turning the
toggle off takes the tag away again cleanly, and a saved prompt does not carry a
setting the user cannot see. Both copies of the prompt in the request body (the
`v4_prompt` caption and the top-level `input`) get the same appended text,
because NovelAI reads both.

Three guards sit around the injection:

- **`models.alpha` gates it.** Only the two V5 rows carry the flag. On V4.5 or
  V4 the toggle is hidden by `supportsNovelAiTransparency`, and the tag is
  dropped server side regardless, so a stale persisted setting cannot spend
  Anlas on a picture of a checkerboard.
- **A prompt that already says `transparent background` is left alone**, so a
  user who typed the tag themselves does not get a second copy fighting the
  first one's weight.
- **The free local post-process is skipped while the toggle is on.** That pass
  round-trips the image through ComfyUI's `LoadImage`, whose IMAGE output is RGB
  (alpha goes to the MASK output), so the transparency just paid for would come
  back flattened onto black. `transparency_requested()` in `novelai/mod.rs`
  mirrors the payload's condition exactly, and the panel warns before the
  request is sent rather than after the Anlas is gone.

Nothing else in the output path needed changing. The PNG NovelAI returns is
passed through byte for byte and the gallery encoders are RGBA already.

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

**It is also skipped while Transparent BG is on**, for the reason in section
2.11: the pass would flatten the alpha channel onto a solid background.

## 4. Prompt syntax

Outside NovelAI mode the app translates NovelAI weight syntax (`1.1::tag::`)
into ComfyUI syntax (`(tag:1.1)`). In NovelAI mode that reverses: what the user
types in A1111 or ComfyUI syntax is converted to NovelAI syntax on the way out.
Behaviour outside NovelAI mode is unchanged.

The `@` sigil is also mode-dependent. In ComfyUI mode artist tags are inserted as
`@tag`. In NovelAI mode they are inserted bare, because there `@` is the
prompt-chunk reference sigil (`@[chunk name]`), not an artist marker.

That decision lives in one place: `generation.artistTagPrefix` and
`formatArtistTag()`, with the pure string work in `src/lib/utils/artistTag.ts`.
Matching and removal use the same prefix, and in NovelAI mode, where a bare
artist tag looks like any other danbooru tag, the artist index is what tells
them apart. Saved styles keep a narrower rule (`animaArtistTagPrefix`), because
only Anima-family checkpoints were trained on `@artist`.

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
5. **Whether NovelAI's stealth PNG metadata survives a transparent
   background.** NovelAI hides generation metadata in the low bits of the alpha
   channel of images that have no meaningful alpha. A real cut-out uses that
   channel for picture data, so the stealth payload is either omitted or written
   somewhere the reader does not look. `novelaiPngMetadata.ts` also reads the
   visible `Comment` chunk, which is what the app actually relies on, so a
   missing stealth layer costs nothing here. It is listed because the
   interaction is unverified, not because it is known to break.

`Subscription.extra` captures any key the backend does not name, and
`fetch_subscription` logs them at debug level, so a field NovelAI adds later
shows up rather than being silently dropped.

Confirmed since the first draft, and no longer guesses: the four V5 model ids
and their inpainting variants, the subscription host, the `usage` field names
and the direction of `percent`, the streaming protocol, and that
`normalize_reference_strength_multiple` is inert server side (see section
2.10).

## 7. Manual test checklist

Nothing in this backend is covered by an automated test that touches NovelAI's
servers, so every phase that ships is followed by a hand-test pass recorded
here, newest first. Each entry says plainly whether testing is needed at all.

### 2026-08-25 - Transparent BG for V5

**Requested by:** the user: "add the transparent bg feature that NAI V5 should
have", then "add a button that functionally does the same thing and not showing
it in prompts either".

**What changed.** A "Transparent BG" checkbox in the NovelAI advanced section,
shown only when the selected model's VAE carries an alpha channel, which today
means the two V5 rows. NovelAI ships this as a prompt tag rather than a request
field, so the backend appends `2.1::transparent background::` while the request
body is built and the user's prompt box is never touched. Section 2.11 has the
reasoning and the three guards around the injection. The free local
post-process is suppressed while the toggle is on, with an amber warning in the
panel and a `log::warn!` on the Rust side, because ComfyUI's `LoadImage` would
flatten the alpha onto black.

**Testing needed:** waived by the user: "no need to verify, it's a tag, will
work right out of the box." The checklist below is kept as a regression
reference for anyone who touches the injection later. Steps 4 to 9 spend
Anlas, one image each.

| # | Step | Expected |
|---|------|----------|
| 1 | Select NovelAI V4.5 or V4, open the NovelAI advanced section | No Transparent BG checkbox |
| 2 | Switch to NovelAI V5 Full | Transparent BG appears under Variety+ |
| 3 | Hover its info tip | Explains that V5's VAE is the first with a real alpha channel |
| 4 | Tick it, generate a simple subject (`1girl, standing`) | Result is a cut-out, and the prompt box still shows exactly what you typed |
| 5 | Open the result in something that shows transparency | Genuinely transparent, not white or black |
| 6 | Untick it, generate again on the same seed | Normal background returns, nothing left behind in the prompt |
| 7 | Type `transparent background` yourself, tick the toggle, generate | Only one copy of the tag is sent, output matches step 4 |
| 8 | Tick Transparent BG and Local post-process together | Amber warning appears in the panel before you generate |
| 9 | Generate with both on | Image arrives untouched with alpha intact, Rust log says the local pass was skipped |
| 10 | Tick it on V5, switch to V4.5 without unticking, generate | Normal background, no Anlas spent on a checkerboard |
| 11 | Reload the app | The toggle's state persisted |
| 12 | Import the saved image's metadata back into the app | Transparent BG comes back ticked (`mooshie_novelai_transparent_background`) |
| 13 | Repeat step 4 in browser mode (LAN URL) | Identical behaviour |
| 14 | Switch UI language | Label, info tip and the local-pass warning are translated |

**Do not skip:** 1, 4, 5, 8, 12. Those cover the capability gate, the happy
path, the actual alpha channel, the post-process conflict and the metadata
round trip. **Low-risk, skip if short on time:** 3, 14.

### 2026-08-24 - Director Tools (PR #618)

**Requested by:** the user: "add in the directors tools to this", then "build all".

**What changed.** NovelAI's `/ai/augment-image` endpoint, wired end to end. Six
tools (Background Removal, Line Art, Sketch, Colorize, Change Emotion,
Declutter) run over an image the user already has, from an app-wide modal
reached from three places: the session output context menu, the live preview
context menu, and the gallery hover actions. Results come back through the same
synthetic `nai-` prompt id and `comfyui:*` events a generation uses, so they
land in the session grid and the gallery without any new plumbing.

Only Colorize and Change Emotion read the `defry` slider and the guidance
prompt; the other four ignore both, so the modal hides them. Change Emotion
additionally requires a mood, which the backend joins to the prompt as
`mood;;prompt`. The mood is a free text box, not a dropdown: NovelAI does not
document which mood strings it accepts.

The entry points are hidden, not disabled, unless the NovelAI backend is
selected and an API key is configured. There is nothing the user can do about
either from an image context menu.

**Testing needed:** yes. Every step below needs a NovelAI key and the NovelAI
backend selected. No Anlas is needed: NovelAI does not charge for any Director
Tool, and the modal says so.

| # | Step | Expected |
|---|------|----------|
| 1 | Select the ComfyUI backend, right-click a session output | No Director Tools entry anywhere |
| 2 | Select NovelAI with no API key set, right-click a session output | Still no Director Tools entry |
| 3 | Set a NovelAI key, right-click a session output | "Director Tools" appears below Inpaint |
| 4 | Click it | Modal opens, source thumbnail on the left, six tools, Background Removal preselected |
| 5 | Run Background Removal | Toast "Director Tool started.", modal closes, result arrives in the session grid and the gallery |
| 6 | Open the modal again, pick Colorize | Guidance textarea and Defry slider appear; no mood field |
| 7 | Drag Defry to 5, run | Request is accepted; the backend clamps anything above 5 |
| 8 | Pick Change Emotion with the mood box empty | Run Tool is disabled |
| 9 | Type `happy`, run | Accepted; result shows the emotion change |
| 10 | Switch from Change Emotion to Sketch | Mood, guidance and Defry all clear and hide |
| 11 | Right-click the live preview after a generation finishes | "Director Tools" is in the menu |
| 12 | Right-click the live preview mid-generation, before the output persists | No Director Tools entry (there are no bytes to send yet) |
| 13 | Gallery, details view, hover a row | A yellow "Director Tools" button sits before Copy |
| 14 | Gallery, grid view, hover a tile | A yellow `DT` button sits before the copy icon |
| 15 | Run a tool on a JXL gallery entry | Works: the loader decodes to PNG before sending |
| 16 | Run a tool with an invalid key | Error box inside the modal, modal stays open, nothing enqueued |
| 17 | Press Ctrl+Enter in the guidance box | Submits |
| 18 | Press Escape, or click the backdrop | Modal closes without sending |
| 19 | Repeat 3-5 in browser mode (LAN URL) | Identical behaviour; the command is allowlisted in `webserver.rs` |
| 20 | Switch UI language | Every label in the modal is translated |

**Do not skip:** 2, 5, 8, 12, 19. Those cover the availability gate, the happy
path, the only required field, the unpersisted-preview case and browser mode.
**Low-risk, skip if short on time:** 17, 18, 20.

### 2026-08-24 - V5 prompt enhance, and compare stays intact in NAI mode (PR #618)

**Requested by:** the user: "now action
docs/superpowers/specs/2026-08-24-novelai-v5-prompt-enhance-design.md
entirely."

**What changed.** A third enhance path, offered only when the selected
checkpoint is a NovelAI Diffusion V5 model. It rewrites the prompt into the V5
format (scene description, tags, character boxes, undesired content) rather
than the tag soup the ComfyUI enhance produces, in one of six languages, and
stages the result in a review modal instead of overwriting anything. Nothing
reaches the generation store until a row is ticked and Apply is pressed.

The button opens an app wide modal with two stages rather than running the
rewrite against the prompt box. Stage one is a blank input box that takes either
a prompt to rewrite or an instruction ("make it a rainy night scene"), with a
"copy existing prompt" button under it for the tidy-up case and the language
select beside that; stage two is the same review diff as before, with every row
ticked by default. The empty-prompt guard on the button does not apply to the V5
path, because the prompt box is no longer its input.

Also in this pass: the compare grid no longer drives generation while a NovelAI
checkpoint is selected. The compare tab is hidden in NAI mode, and before this
change that trapped a user who had compare on, since the multi-cell generate
path kept firing with no visible way to turn it off. `compare.enabled` is left
alone and a `compare.active` getter (`enabled && !generation.isNovelAi`) feeds
the readers that change behaviour, so switching back to a ComfyUI checkpoint
restores the grid untouched.

**Why it needs hand testing.** `naiParse.ts` and `naiLanguage.ts` are pure
string logic with real edge cases, and there is no frontend test framework to
cover them. Adding one is out of scope and contradicts a deliberate repo
decision, so the mitigation is the fixture set below: it makes a regression
reproducible without adding a dependency.

#### Fixtures

Pick a V5 checkpoint, press Enhance for V5, paste each into the modal's input
box, press Enhance again, and check the stated expectation on the review stage. The model response fixtures
under "malformed responses" cannot be typed in directly; they describe what the
LLM may return, and the check is that the app salvages or flags it rather than
breaking.

**Known good responses.** These should parse cleanly, open the modal with no
amber banner, and populate exactly the rows named.

1. `BASE:` plus `UC:` only, no characters. Two rows, the character list empty.
2. `BASE:`, `UC:`, `CHAR 1:`, `CHAR 2:` against an empty character list. Four
   rows, both character rows carrying the "new" chip.
3. `BASE:`, `UC:`, `CHAR 1:` against one existing character. Three rows, the
   character row showing the existing text on the left and no "new" chip.
4. A `NOTE:` line after the fields. Same rows, plus the note in the amber
   banner at the top. A note is not a problem and must not be listed as one.

**Malformed responses, one per validator rule.** Each should still open the
modal, with the named rule in the amber problem list after the single retry
fails to clear it.

5. Empty `BASE:`. Problem names the empty base field.
6. An em dash or en dash anywhere in any field. Problem names the dash.
7. A markdown fence around the answer. The fence is stripped by the parser; if
   one survives into a field, the problem names it.
8. `CHAR 1:` starting with `1girl`. Problem says counts belong in BASE only.
9. `CHAR 1:` starting with `Character 1`. Problem says start the box with girl,
   boy or other.
10. Quality filler (`masterpiece`, `best quality`) in BASE with the quality
    toggle on. Problem names the filler found.
11. Content after the `Text:` block in BASE. Problem says the Text block must be
    last.
12. Unbalanced `{` or `[` in BASE. Problem names the brackets.
13. An odd number of `::` in a field. This one is repaired silently by the
    normalizer, not reported: check the closing `::` was appended rather than
    looking for a problem line.
14. A chatty preamble before the first label ("Sure, here you go"). Dropped by
    the parser, no problem line.
15. No labels at all, just a bare prompt. Treated as BASE, no problem line.

**Digit suffix artist names.** Prompt:

`artist:as109, artist:92m, artist:k7, 1.4::artist:hito_(nito3), artist:2b_(pixiv)::`

Every token in the weighted span ends in a digit or a digit-bearing
disambiguator. The check: the span survives the rewrite with the names intact
and the weight still attached to the same span, and language detection still
returns English rather than tripping on the digits.

**One prompt per supported language.** Leave the modal's select on Auto and
confirm the review subtitle names the language listed here, then confirm the base prompt comes
back written in it while the tags and the complexity keywords stay English.

| Language | Prompt |
|----------|--------|
| English  | `a girl standing in the rain at night, neon signs` |
| Japanese | `夜の雨の中に立つ少女、ネオンの看板` |
| Chinese  | `雨夜中站立的少女，霓虹灯招牌` |
| German   | `ein Mädchen steht nachts im Regen, über ihr Neonschilder` |
| Spanish  | `una chica con el pelo negro y los ojos verdes bajo la lluvia` |
| Portuguese | `uma menina com cabelo preto e olhos verdes, não sorrindo` |

Japanese and Chinese are settled by script and must be exact. The three Latin
languages are scored on diacritics and stopwords and are genuinely unreliable
on a short prompt; a wrong guess there is a miss, not a bug, and the override
select is the fix. English on a tie is the intended default.

#### Manual checklist

1. Pick a ComfyUI checkpoint. The enhance button reads the normal label, it is
   disabled on an empty prompt box, and it rewrites in place with no modal.
2. Pick a V5 Curated checkpoint. The button reads "Enhance for V5" and stays
   enabled with the prompt box empty. Pressing it opens a modal that spans most
   of the window rather than the bottom panel, on the input stage.
3. On the input stage, the language select lists the four tester languages with
   an asterisk, the Enhance button is disabled until something is typed, and
   Ctrl+Enter in the box runs it.
4. Press "copy existing prompt" with a prompt in the box. The text lands in the
   input. With the prompt box empty, the button is disabled.
5. Pick a V5 Full checkpoint. Same controls, and the review subtitle says V5
   Full rather than V5 Curated. The per-row token bars use the higher budget.
6. With no LLM backend configured, press Enhance for V5. The setup modal opens,
   and finishing setup opens the input stage rather than dropping the action.
7. Run a rewrite. Every row on the review stage arrives ticked, including the
   undesired content row when the rewrite left it empty.
8. Run a rewrite with characters already filled in. Untick every row, press
   Apply. Apply is disabled with nothing ticked, so this cannot be pressed;
   confirm that is what happens.
9. Tick the base row only, Apply. The prompt changes, the undesired content and
   every character box do not.
10. Tick a character row that maps to an existing slot. That slot's prompt
    changes and its position and other fields do not.
11. Tick a character row with the "new" chip. A new slot appears, and at six
    characters no seventh is created and nothing already written is lost.
12. Press Undo V5 rewrite within ten seconds. The prompt, the undesired content
    and every character slot return to what they were, including slots the
    rewrite never touched.
13. Reach the review stage, then switch to a non-V5 checkpoint. The modal closes
    and nothing is applied. Do the same on the input stage: the modal stays open
    and what was typed survives.
14. Press Escape, click the backdrop, and press Cancel on each stage. Each
    closes the modal with nothing applied.
15. Press Enhance, then Cancel while it is still running. The modal stays shut
    when the answer arrives rather than reopening on the review stage.
16. Set the language select to a tester language, reload the app, reopen the
    modal. The select still reads that language.
17. Run a rewrite twice in a row with the same variant. The second is faster,
    because the authored skill note is cached in localStorage rather than
    re-authored.
18. Switch to a NovelAI checkpoint with compare enabled. The compare tab is
    gone, the panel glow is gone, and Generate produces one image rather than
    walking the grid. Switch back to a ComfyUI checkpoint: the grid is still
    there, with the same cells.

**Result:** not yet run. This entry is the checklist, recorded with the change.

### 2026-08-23 - Style and chunk editors are app level modals (PR #618)

**Requested by:** the user: "make the prompt chunk and artist style editors
modals for the whole app not just the bottom panel."

**What changed.** Both editors were already full screen overlays, but they were
mounted inside the Styles tab of the bottom panel, which had two consequences.
They could only be opened from that tab, and on the mobile layout the panel
wrapper carries a CSS transform, which makes it the containing block for any
`position: fixed` descendant, so the overlay was clipped to the panel instead of
the window. Their open state now lives in a small `styleEditors` store and both
editors are mounted once at the App root, outside every transformed wrapper.

**Reachable from the prompt box.** The chunk picker in the prompt toolbar gained
a per chunk edit button and a "+ New chunk" button, so a chunk can be created or
edited without opening the Styles panel. The footer hint now points at the
Styles panel for import and export only, which is what is still exclusive to it.

| # | Step | Expect |
|---|------|--------|
| 1 | Open the bottom panel, Styles tab, Styles list, click Edit on a style | The editor covers the whole window, not just the bottom panel |
| 2 | Close it with the X or Escape | Editor closes, Styles tab is still where you left it |
| 3 | Same tab, Chunks list, click Edit on a chunk | Chunk editor covers the whole window |
| 4 | In the Styles tab click "+ Create" under Styles | A new style is created and its editor opens full screen |
| 5 | In the Styles tab click "+ Create" under Chunks | A new chunk is created and its editor opens full screen |
| 6 | Open the chunk picker from the prompt toolbar, click the pencil on a chunk | The picker closes and the chunk editor opens full screen |
| 7 | Edit that chunk's content, close the editor, reopen the picker | The new content shows in the chunk's preview line |
| 8 | Open the picker and click "+ New chunk" | A chunk named "Chunk N" is created and its editor opens |
| 9 | Close that editor, open the picker | The new chunk is in the list |
| 10 | In the picker, click a chunk name (not the pencil) | The mode picker still opens and stacks above everything |
| 11 | Activate a chunk from the picker, then edit it from the picker | Activation survives the edit, the chunk stays active |
| 12 | Collapse the bottom panel entirely, then use the picker's edit and new buttons | Both still work, the bottom panel is not needed |
| 13 | Check the picker footer text | Reads "Import and export chunks in the Styles panel." |
| 14 | Open a style editor, then press Escape | Closes, no leftover dimmed overlay |

**Result: all 14 pass.** The editors open full screen from both the Styles panel
and the prompt toolbar, and creating or editing a chunk no longer requires the
bottom panel.

### 2026-08-23 - Chunks in the prompt toolbar, NovelAI token bars, no edit/video tabs (PR #618)

**Requested by:** the user: "let's actually move prompt chunks to the prompt box
next to the toggle for uncombining the positive and negative prompt box button,
also hide regional prompting in NAI mode plus max tokens for NAI is 1471, make
it a bar underneath each prompt box (if using more than one, just display the
total of all tokens under each prompt box) also disable the "image edit and
video" tabs from being seen completely in NAI mode since those don't exist."

**Chunks moved to where prompts are written.** The chunk list only existed in
the Styles panel at the bottom of the page, so using one meant leaving the
prompt box. A compact picker now sits in the prompt toolbar next to the
combine/uncombine toggle: it lists every chunk, activates or deactivates on
click (opening the existing mode modal), copies the inline `@[Name]` token, and
shows the active count on the trigger. The Styles panel keeps the full editor
(create, edit, duplicate, import, export, delete); the picker is a short path,
not a replacement.

**Regional prompting is gone in NovelAI mode, not just hidden.** Both regional
strategies are ComfyUI graph rewrites, and NovelAI takes a finished prompt over
HTTP, so neither can apply. `supportsRegionalConditioning` and
`supportsRegionalInpaintChain` now return false for NovelAI, and the toolbar
button and its amber "unsupported" hint are dropped rather than shown disabled.

**A token bar under every NovelAI prompt box.** NovelAI's cap is 1471 tokens, a
single ceiling rather than CLIP's 75-token chunk boundary, so it is drawn as a
bar rather than the chunk counter ComfyUI mode keeps. Every box on a side (main
plus extras) is concatenated into one prompt before it is sent, so each bar
shows the whole side's total, with the same number repeated under each box and
a "total" suffix once more than one box is in use. The bar turns amber at 90
percent and red past the limit. The estimator is the existing CLIP heuristic,
so the reading is approximate.

**Image edit and video tabs are hidden in NovelAI mode.** NovelAI has no edit
or video endpoint, so those two tabs are filtered out of the mode list that all
three tab bars render, rather than shown disabled. Switching to a NovelAI model
while sitting on one of them snaps back to txt2img, so nobody is stranded on a
tab that no longer has a way back.

**Testing required: yes.** All four are user-visible UI changes.

| # | Step | Expected |
|---|------|----------|
| 1 | Start the app fresh (`npm run tauri dev`) on a ComfyUI model | Prompt toolbar shows a Chunks button next to the regional and combine/uncombine buttons |
| 2 | Click the Chunks button | A popover lists every chunk with its mode icon and a truncated preview |
| 3 | Click a chunk in the popover | The mode modal opens; picking a mode activates it and the trigger shows "(1)" |
| 4 | Click the same chunk again | It deactivates and the count drops back |
| 5 | Click the `@` button on a chunk row | A toast confirms the copied token; pasting into the prompt gives `@[Name]` |
| 6 | Click outside the popover, then reopen it and press Escape | Both close it |
| 7 | Switch to a NovelAI model | The Chunks and combine/uncombine buttons stay; the regional prompting button is gone |
| 8 | Still in NovelAI mode, check the mode tabs | Only txt2img, img2img and inpainting; no image edit, no video |
| 9 | Switch to ComfyUI, select the image edit tab, then switch back to a NovelAI model | The tab bar drops those two tabs and the view snaps to txt2img |
| 10 | Repeat step 9 with the video tab | Same |
| 11 | Back on ComfyUI, confirm all five tabs and the regional button are present | Nothing was removed for ComfyUI |
| 12 | In NovelAI mode, type into the positive prompt | A thin bar sits under the box reading `N/1471` and filling as you type |
| 13 | Paste enough text to pass 1471 tokens | The bar goes red and the count reads over the limit |
| 14 | Add an extra positive prompt box and put text in both | Both bars show the same combined total with a "total" suffix |
| 15 | Check the negative side the same way | Its bar counts only the negative boxes, independently of the positive side |
| 16 | Switch back to a ComfyUI model | The 1471 bars are gone and the usual 75-token chunk badge is back |

**Do not skip:** 1, 3, 7, 8, 9, 12, 14.

**Follow-up: the list is a modal now.** As first shipped the list was a
dropdown anchored under the button. The prompt toolbar sits in a narrow
column, so a panel wide enough to preview chunk content extended past the left
edge and was clipped at ordinary window widths. It is now a centred modal, the
same shape as the mode picker it opens, which is width independent.

**Result: all pass except 10, which does not apply.** Step 10 has no
traditional model selector to test against (the video tab always uses the
video models), and it is deliberately left unpatched. Steps 1 through 6 were
verified against the original dropdown and still describe the modal, except
that it is now opened and closed as a dialog.

### 2026-08-23 - Typed tokens splice the whole chunk, and a failed Anlas fetch retries (PR #618)

**Reported by:** the user, after rebuilding on the entry below: "@[xenogirl] in
either character and main prompt still doesn't work, only clicking on it does,
I rebuilt, also the anlas counter and opus usage bars are still missing and
anlas cost is still wrong."

**The token cause was in the resolver all along.** `resolveInline` treated any
multi-line chunk as a wildcard: one random line per occurrence. Click
activation in prepend or append mode inserts the whole content. So clicking
always looked right and typing looked broken for any chunk longer than one
line. Proven from the saved generation metadata: the clicked run carried every
line of the chunk, and each typed run carried exactly one, a different one
each time. Typed tokens now splice the whole chunk in at that spot, lines
joined with ", " so per-line trailing commas do not double up. This matches
what the inline help text always promised. Random rolls remain an activation
feature via the wildcard modes, and a pinned ordered-run choice still wins
over the token.

**The Anlas chain itself is correct.** Every link was verified at head: the
config read, the one-way configured flag (fixed in the entry below), the
command, the endpoint host, and the response shape, the last against the live
NovelAI subscription endpoint with the real account. The user's Anlas retest
images predate their rebuild, so that half of the report described the
pre-fix build. Two genuine gaps are fixed anyway: a failed fetch used to
stick until an app restart (it now retries automatically after 30 seconds),
and the failure was swallowed silently (it now lands in the exported log).

**Testing required: yes.** Both halves are user-visible.

| # | Step | Expected |
|---|------|----------|
| 1 | Close the app fully and start `npm run tauri dev` fresh | A clean session, nothing carried over |
| 2 | Keep a chunk with several lines of content, e.g. `Xeno Girl` | - |
| 3 | Type `@[xenogirl]` in the main prompt and generate | The saved image's prompt contains every line of the chunk at that spot, comma separated, same as clicking it |
| 4 | Type `@[xenogirl]` in a character prompt and generate | Same: the whole chunk, in the character prompt |
| 5 | Type `@preset:xeno_girl` in the main prompt and generate | Same as step 3, the old spelling behaves identically |
| 6 | In NovelAI mode, look above the Generate button | The Anlas balance shows a real number and the Opus usage bar is drawn |
| 7 | Check the cost badge on the Generate button at an Opus-included size | Shows the discounted (usually zero) cost |
| 8 | Save anything in Settings, then switch to a ComfyUI model and back | Balance, bar and cost badge all come back unchanged |
| 9 | Only if the readouts are blank: press the refresh arrow in the usage panel, then export logs | The exported log contains a "NovelAI subscription fetch failed" line naming the reason |

**Do not skip:** 3, 4, 5, 6, 7.

**Result: 1 through 8 pass; 9 was not applicable** (the readouts were never
blank, so the failure-log path had nothing to show). Typed tokens now splice
the whole chunk in both the main and character prompts, and the Anlas
balance, Opus bar and cost badge all behave.

### 2026-08-23 - Inline chunk tokens resolve again, and the character prompts are real prompt boxes (PR #618)

**Reported by:** the user, on the entry below: "cost of anlas isn't displaying
properly after switching back to NAI mode and the bar is gone again ...
@preset:xenogirl does not work @[xenogirl] does not work", and "prompt chunks
do not work in the character box".

Four separate causes.

**A shared regex with the `g` flag is stateful.** Moving the token pattern into
one leaf util gave four modules one regex object rather than four copies. A bare
`.test()` leaves `lastIndex` pointing past the first match, and `matchAll`
copies `lastIndex` rather than resetting it, so the very next scan started
halfway through the prompt and found nothing. That is why the highlighter and
the resolver both went quiet. Every scan now takes a fresh matcher from
`presetTokenRegex()`.

**A name typed from memory does not carry its spacing.** `@[xenogirl]` slugs to
`xenogirl`, and a chunk named "Xeno Girl" slugs to `xeno_girl`, so the lookup
missed. The chunk map now carries a second, looser key per chunk with the
underscores dropped, added in its own pass so an exact slug always wins. The
highlighter checks the same loose key, so what lights up is what resolves.

**The NovelAI character prompts never went through chunk resolution.**
`novelAiParams()` spread the settings object straight onto the request, tokens
and all, so a chunk token in a character box travelled to NovelAI as literal
text. `toParams()` now resolves both character fields and counts the chunks it
spliced in, so an inline chunk is not appended a second time by the active-chunk
pass. The two fields also became real `PromptTextarea`s, with the same
highlighting, autocomplete and weight editing as the main box.

**A config read cannot prove a key is gone.** `updateConfig()` writes the
frontend's own config copy into the config cache, and that copy carries a
blanked API key (the backend's `preserve_secrets()` is what keeps the stored
one). Any later unforced read then reported "no key", which turned off the Anlas
readout, took the Opus allowance bar with it and made the cost badge quote the
non-Opus price. `applyConfigured()` is now one-way: only `setApiKey("")` clears
the flag.

**Testing required: yes.** All four are user-visible.

| # | Step | Expected |
|---|------|----------|
| 1 | Create a chunk named `Xeno Girl` with content `green skin, antennae` | The copy button beside the name shows `@[Xeno Girl]` |
| 2 | Type `@[Xeno Girl]` into the positive prompt by hand | It highlights as a chunk token while you type, without clicking anything |
| 3 | Type `@[xenogirl]` (no space, all lowercase) | It highlights too, and is treated as the same chunk |
| 4 | Type `@preset:xeno_girl` | Also highlights, the old form is unchanged |
| 5 | Generate with each of the three forms in turn | All three splice `green skin, antennae` in at that exact spot |
| 6 | Put two tokens in one prompt, e.g. `@[Xeno Girl] and @[Cool Lighting]` | Both resolve. This is the case the stale regex broke |
| 7 | Open the NovelAI character panel and add a character | Its prompt and undesired-content boxes look and behave like the main prompt box: tag autocomplete, weight editing, resizable |
| 8 | Type `@[Xeno Girl]` into a character prompt | It highlights there too |
| 9 | Generate with that character | The character prompt sent to NovelAI has the chunk expanded, not the literal token. The chunk is spliced in once, not appended again at the end |
| 10 | Type into a character box, switch tabs, come back | The text is still there, and no keystroke was lost or reverted mid-typing |
| 11 | Open Settings and save anything at all, then return to the generation page | The NovelAI models are still listed, the Anlas readout is still there, and the Opus bar is still drawn |
| 12 | Switch to a ComfyUI checkpoint and back to a NovelAI model | The balance, the Opus allowance bar and the cost badge all come back with the same numbers |
| 13 | With an Opus subscription, compare the cost badge before and after step 12 | The same figure both times, the Opus discount is not lost |
| 14 | Clear the NovelAI key in Settings | Everything NovelAI disappears, the models included. This is the one path that may turn the flag off |

**Do not skip:** 2, 3, 5, 6, 8, 9, 12, 14.

**Result: steps 7, 10 and 11 stand, but the stale-app diagnosis was wrong.** The user rebuilt and typed tokens still failed the same way, so the hot-swap explanation above does not hold. The real cause was in the resolver itself: `resolveInline` treated a multi-line chunk as a wildcard and spliced one random line per occurrence, while clicking (prepend or append) inserted all of it, which is exactly the "only clicking works" symptom. The Anlas half of the retest was run before the rebuild, so it described the pre-fix build; the fetch chain was separately verified good end to end against the live API. Both are addressed in the entry above; retest there.

### 2026-08-23 - Prompt presets are now Prompt Chunks, and @[Name] works inline (PR #618)

Two changes to the same feature.

The user-facing wording moved from "prompt preset" to "prompt chunk" across all
12 locales (16 keys). The code symbols were left alone on purpose, so
`promptPresets.svelte.ts`, `PromptPreset` and the `@preset:` token all keep
their names. Unrelated features that legitimately say "preset" were not touched:
Fooocus style presets, ControlNet presets, video export presets, LoRA presets
and the appearance style presets.

The inline token gained a second, friendlier spelling. `@[Chunk Name]` now
resolves to the same chunk as `@preset:chunk_name`, because the display name is
slugified with the same rule the old form already used. The vocabulary moved
into a leaf util (`src/lib/utils/promptChunkTokens.ts`) so the highlighter, the
inert-range scanner, the scheduler and the store all share one regex instead of
four copies. The copy button in the chunk editor and in the Style Manager now
hands out the `@[Name]` form, falling back to `@preset:slug` when the name is
empty or contains a `]` or a newline.

**Testing required: yes.** New prompt syntax plus a broad string rename.

| # | Step | Expected |
|---|------|----------|
| 1 | Open the Style Manager | The second tab reads "Chunks", its panel title reads "Prompt Chunks", and the empty state says "No chunks yet." |
| 2 | Create a chunk named `Cool Lighting` with content `dramatic rim light` | The copy button beside the name shows `@[Cool Lighting]` and copies exactly that |
| 3 | Paste `@[Cool Lighting]` into the positive prompt | It highlights as a chunk token, the same way `@preset:cool_lighting` does |
| 4 | Generate with that prompt | The final prompt splices `dramatic rim light` in at that exact spot |
| 5 | Replace it with `@preset:cool_lighting` and generate again | Identical result, the old form still works |
| 6 | Click the `@[Cool Lighting]` token in the prompt box | Nothing opens. No tag popup, and the artist detector does not treat it as an artist reference |
| 7 | Name a chunk `Neon, Night` and use `@[Neon, Night]` | Resolves. `@preset:neon_night` resolves to the same chunk |
| 8 | Name a chunk `Weird]Name` | The copy button falls back to `@preset:weird_name` rather than emitting a broken token |
| 9 | Use a chunk token inside a schedule, e.g. `[@[Cool Lighting]:0.5]` | The chunk content expands inside the scheduled segment as before |
| 10 | Save a prompt from the extra prompt box | The button reads "Save as chunk" and the toast reads "Saved '<name>' to chunks" |
| 11 | Hover or tab to an active chunk chip's deactivate control | The accessible label reads "Deactivate chunk <name>" |
| 12 | Switch the UI language to Japanese, then German | The chunk wording is translated in both, no raw keys and no English left in that block |
| 13 | Check the Styles tab, ControlNet, video export and LoRA panels | They all still say "preset", unchanged |

**Do not skip:** 2, 3, 4, 5, 6, 12.

**Result: 1 and 2 pass. 3 fails**, a hand-typed `@[Name]` was not highlighted.
4 passes only by clicking the copy button, which inserts the token for you.
Neither `@preset:<name>` nor `@[<name>]` resolved at generation time, so 5
onwards were blocked. Causes and fixes in the entry above.

### 2026-08-23 - The Anlas balance showed on the ComfyUI backend (PR #618)

**Reported by:** the user, on the entry below: "the anlas counter still exists
even in non NAI mode".

The readout above the generate button was gated on `novelai.apiKeyConfigured`
alone. A key stays configured once it is saved, so anyone who has ever used the
NovelAI backend saw a NovelAI balance sitting over every ComfyUI generation.
The gate is now `generation.isNovelAi && novelai.apiKeyConfigured`, which is
the same rule the cost badge beside it already used.

The subscription fetch moved behind the same condition, so switching to the
ComfyUI backend also stops the component reaching NovelAI's account endpoint.

**Testing required: yes.** A UI visibility change.

| # | Step | Expected |
|---|------|----------|
| 1 | Configure a NovelAI key, then pick a ComfyUI checkpoint | No Anlas readout above the generate button, and no cost badge |
| 2 | Switch to a NovelAI model | The Anlas readout and the cost badge are both back |
| 3 | Refresh the balance with the arrow, then switch back to ComfyUI and back again | The balance is still there and correct, not blanked to `--` |
| 4 | With no NovelAI key configured, in either mode | No readout, as before |

**Do not skip:** 1, 2.

**Result: 1, 2, 3 and 4 pass.**

### 2026-08-23 - The tiling controls are back in NovelAI mode (PR #618)

**Reported by:** the user, on the entry below: "turning off tiling broke it,
turn it back on".

Forcing `upscale_fast_refine = true` for the local pass turned off both
MultiDiffusion and the tiled VAE. That is exactly where the tiled path was
earning its keep: a 4x upscale of a 1024px NovelAI image is around 4096x4096,
and a plain `VAEEncode` / `VAEDecode` at that size is what broke the run. The
derived params no longer touch either flag, so the NovelAI local pass tiles on
the same rules as the ComfyUI backend, and unchecking tiled diffusion by hand
stays available for the runs where it looks cleaner.

The upscale panel shows all four tiling controls again in NovelAI mode: the
tiling checkbox, the Anima forced-tiling notice, the fast refine checkbox and
the tile size slider.

Kept from the reverted change: the denoise soft warning above 0.20, and the
`KSampler`-count assertion in
`the_graph_loads_the_uploaded_image_and_never_samples_it_twice`, which is what
"never samples it twice" actually means. Its companion assertion that the three
tiled node types are absent is gone, since they are expected again.

**Testing required: yes.** A UI change and a change to what the local pass
renders.

| # | Step | Expected |
|---|------|----------|
| 1 | NAI mode, local post-process on, open the upscale panel | Tiling checkbox, tile size slider, fast refine checkbox and the Anima forced-tiling notice are all visible again |
| 2 | Generate with upscale on, tiling left at its default | The run completes, as it did before the tiling change |
| 3 | Uncheck tiled diffusion by hand, generate again | Still works, and this is the state that looked cleanest |
| 4 | Drag the upscale denoise above 0.20 | An amber line appears under the denoise and steps row |
| 5 | Drag it back to 0.20 or below | The line disappears |
| 6 | Switch the UI language and repeat 4 | The line is translated, not English |
| 7 | Switch to the ComfyUI backend and open the upscale panel | Unchanged in every respect |
| 8 | ComfyUI backend, drag upscale denoise above 0.20 | No warning line: it is NovelAI-only |

**Do not skip:** 1, 2, 4, 7.

**Result: 1, 2, 3, 4, 5, 6, 7 and 8 pass.**

### 2026-08-23 - NovelAI mode no longer offers the tiling knobs (PR #618)

**Reported by:** the user, on the entry below, after confirming the CFG fix.
The remaining artifacts went away once tiled diffusion was unchecked by hand,
and that box "should just not be visible in NAI mode anyways". Along with it, a
soft warning when the upscale denoise goes above 0.20.

The local pass is a single refine over a single image, so MultiDiffusion and
the tiled VAE have nothing to spread over and only contribute tile seams. The
derived params now set `upscale_fast_refine = true` (and `upscale_tiling =
false`), which is the one flag that turns both off even for a split-file local
model, where the tiling gate ORs `use_split_model` and would otherwise force
them back on.

The upscale panel hides all three tiling controls in NovelAI mode to match:
the tiling checkbox, the Anima forced-tiling notice, the tile size slider and
the fast refine checkbox. Leaving them visible would be showing knobs that no
longer reach the graph. On the ComfyUI backend nothing changes.

The denoise warning is a plain amber line under the denoise and steps grid,
shown only in NovelAI mode and only above 0.20. It does not clamp anything: the
upscale panel is still where denoise lives, and someone who wants a heavier
re-draw can have one.

`the_graph_loads_the_uploaded_image_and_never_samples_it_twice` also stopped
asserting `VAEEncode == 0`. With tiling off the upscale chain encodes the
upscaled pixels through a plain `VAEEncode`, so that count is one either way
and never proved what it claimed. It now counts `KSampler` nodes, which is what
"never samples it twice" actually means, and asserts the three tiled node types
are absent.

**Testing required: yes.** Both a UI change and a change to what the local pass
renders.

| # | Step | Expected |
|---|------|----------|
| 1 | NAI mode, local post-process on, open the upscale panel | No tiling checkbox, no tile size slider, no fast refine checkbox, and no Anima forced-tiling notice |
| 2 | Generate with upscale on | Clean, with no tile seams, and without having to uncheck anything by hand |
| 3 | Drag the upscale denoise above 0.20 | An amber line appears under the denoise and steps row |
| 4 | Drag it back to 0.20 or below | The line disappears |
| 5 | Switch the UI language and repeat 3 | The line is translated, not English |
| 6 | Switch to the ComfyUI backend and open the upscale panel | All four controls are back, unchanged |
| 7 | ComfyUI backend with Anima, upscale on | Still shows the forced-tiling notice, and still tiles |
| 8 | ComfyUI backend, drag upscale denoise above 0.20 | No warning line: it is NovelAI-only |
| 9 | Turn tiling on in ComfyUI mode, switch to NovelAI, generate, read the log | `tiling=false` on the `local pass` line |

**Do not skip:** 1, 2, 3, 6.

**Result: 2 failed.** Forcing the tiling off broke the local pass outright, so
the change was reverted (see the entry above). The rest was not reached.

### 2026-08-23 - The refine pass was running at half CFG (PR #618)

**Reported by:** the user, on the entry below. Upscaled output "looks quite
noisy and artifacty for both refine and face", plus a direct question: "is it
taking the direct png pixels?"

Yes, and that part is already working as asked. The derived graph's first node
is `LoadImage` on NovelAI's exact returned PNG bytes, uploaded byte for byte,
and the node order after it is the one the user described: pixel upscale, then
the latent refine, then the face segment inpaint, then save. Nothing about the
pipeline shape needed to change.

The noise came from what the refine was given to work with.

`append_upscale_chain` halves CFG for its KSampler, on the reasoning that the
base sampling pass already applied full guidance and the refine only has to
clean up after it. Under `refine_only` there is no base pass, so that KSampler
is the only guidance the image ever receives, and Anima was refining at CFG 2.0
instead of its recommended 4.0. Weak guidance over a GAN upscaler's
high-frequency output leaves that noise in rather than resolving it. The
halving now applies only when a base pass actually ran.

The tiled VAE was a second, smaller contributor. `VAEEncodeTiled` and
`VAEDecodeTiled` both hardcoded `overlap: 64` while the tile size is the
upscale panel's own, typically 1024. ComfyUI's own defaults pair a 512 tile
with a 64 overlap, so this ran at half the overlap ratio it was designed for,
which shows up as seams between tiles. Overlap is now `tile_size / 8` floored
at 64, which restores that ratio and leaves a 512 tile behaving exactly as
before.

Both changes reach the ComfyUI backend too, because `refine_only` is not
NovelAI-only: it is the upscale panel's own "Refine only (skip img2img pass)"
toggle, and the one the gallery preview's "Upscale this image" sets. Those runs
had the same defect and get the same fix.

Step count is still open. `upscale_steps` provably reaches the submitted
KSampler, so a `log::info!` now records every setting the local pass derives
(upscale on/off, method, model, scale, downscale ratio, steps, denoise, CFG,
sampler, scheduler, tiling, tile size, face-fix steps and denoise) so the next
run's log says whether the slider moved the graph or only the perception of it.

**Testing required: yes.** The render changes for NovelAI local passes and for
every ComfyUI upscale that has "Refine only" on.

| # | Step | Expected |
|---|------|----------|
| 1 | NAI mode, Anima, upscale on, face fix off, generate | The upscaled result is noticeably cleaner than before: less grain, fewer crunchy edges |
| 2 | Same again with face fix on | The face is cleaner too, and still matches the rest of the image rather than looking pasted on |
| 3 | Export logs and read the Rust lines for that run | Two `local pass` lines. The second lists steps, denoise, CFG, sampler, tiling and tile size |
| 4 | Compare that line's `steps=` with the upscale panel's Steps slider | They match |
| 5 | Set the upscale panel's Steps to 40, generate, read the log again | `steps=40`, and the run is visibly longer than at the default |
| 6 | Watch the progress bar during the local pass | It counts up to the upscale panel's step count, not to some other number |
| 7 | ComfyUI backend, ordinary generation, upscale on and "Refine only" OFF | Unchanged from before: that refine still runs at half CFG |
| 8 | ComfyUI backend, "Upscale this image" from the gallery preview | Cleaner than before, the same way 1 is: that path sets `refine_only` |
| 9 | Set the upscale tile size to 512 and run | Unchanged from before: overlap stays 64 at that tile size |

**Do not skip:** 1, 3, 5, 7.

**Result: 1, 2, 4, 5, 6, 8 and 9 pass; 3 was not needed.** The refine and the
face fix both came out clean, and the step count now moves the run, so the
`log::info!` never had to be read. The user did have to uncheck tiled diffusion
by hand to get there, which is carried into the entry above.

### 2026-08-23 - The local pass is a straight upscale again (PR #618)

**Reported by:** the user, on the entry below. The low-denoise img2img round
trip "doesn't seem to be working very well", and the "Local refine denoise"
slider should not exist at all: the pass should render with whatever the
upscale (refiner) and face-fix panels are already set to, the same way an
ordinary ComfyUI upscale does.

So the local pass is back to `refine_only = true`: the NovelAI image is loaded
and handed straight to the upscale chain, with no base sampling pass of its own.
`local_denoise` and `local_steps` are gone from `NovelAiParams`, from the
TypeScript settings type, from the store and from all twelve locale files, and
the slider is gone from the NovelAI panel. Denoise and step count now come from
the upscale panel's own controls, and the face-fix panel's from its own.

What did survive from the img2img attempt is the sampler override. NovelAI mode
hides the ComfyUI sampler dropdowns, so `sampler_name` is a stale leftover and
`cfg` is NovelAI's guidance scale. `local_sampler`, `local_scheduler` and
`local_cfg` still come from the picked model's recommendation in
`src/lib/utils/samplingRecommendation.ts`, which the sampler panel's Apply
buttons share.

**Testing required: yes.** This changes what the local pass renders, for every
NovelAI generation that uses it.

| # | Step | Expected |
|---|------|----------|
| 1 | NAI mode, local post-process on, pick Anima, upscale on, generate | The final image is NovelAI's image upscaled. Composition and style are NovelAI's, just at higher resolution and detail |
| 2 | Open the NovelAI local panel | There is no "Local refine denoise" slider anywhere in it |
| 3 | Look at the line under the local model picker after picking Anima | Reads `er_sde / sgm_uniform, CFG 4.0`, with no step count |
| 4 | Change the upscale panel's denoise, then generate | The change is visible in the result: the upscale panel is what drives it now |
| 5 | Change the upscale panel's steps, then generate | The run takes correspondingly longer or shorter |
| 6 | Turn face fix on as well and generate | Face fix runs last, on the upscaled image, at the face-fix panel's own steps |
| 7 | Turn upscale off and face fix on, then generate | Face fix alone still runs, and the pass is not skipped |
| 8 | Pick a model with no known recommendation | The line shows the generic fallback (`euler / normal, CFG 6.0`) and the pass still runs |
| 9 | On the ComfyUI backend, press the Anima "Apply" button in the sampler panel | Unchanged: 30 steps, CFG 4.0, er_sde, sgm_uniform, and the same upscale and facefix step counts as before |
| 10 | Open settings saved while the slider existed | The panel loads with no error and the pass runs; the stale denoise value is simply ignored |
| 11 | Switch the UI language and reopen the NAI local panel | The sampler line is translated and shows no leftover step count |

**Do not skip:** 1, 2, 4, 9.

**Result: 1, 2, 3, 4 and 6 pass. 5 failed and 7 partially failed.** Changing
the upscale panel's steps made no difference to run length and none to the step
count shown on the progress bar. Face fix alone did run, so 7 is not skipped,
but the output of both the refine and the face fix looked noisy and artifacty.
8, 9, 10 and 11 were not exercised. Both failures are carried into the entry
above.

### 2026-08-23 - The local post-process never re-drew the image (PR #618)

**Reported by:** the entry below, once it started delivering. The refined image
arrived and was upscaled correctly, but it still looked like NovelAI had drawn
it. The intended result is an img2img of NovelAI's image through the picked
local model at roughly 0.2 denoise, using that model's own recommended sampling,
and only then the upscale chain and face fix.

Two things were wrong. The derived params set `refine_only = true`, and
`img2img::build` returns immediately after `LoadImage` in that mode, so there
was no img2img sampling pass at all: the NovelAI pixels went straight to the
upscale chain's KSampler and the local model only ever touched the image at
upscale resolution. And the sampling settings were still the NovelAI request's.
NovelAI mode hides the ComfyUI sampler dropdowns and edits `novelaiSettings`
instead, so `sampler_name` was whatever stale value the top level happened to
hold, while cfg was NovelAI's guidance halved by the upscale rule.

The pass is now an ordinary low-denoise img2img: `refine_only = false`, denoise
from a new `local_denoise` (0.2 by default, with a runtime fallback because
`NovelAiParams` derives `Default` and would otherwise hand it 0.0). Sampler,
schedule, steps and cfg come from the picked model's recommendation, which now
lives in `src/lib/utils/samplingRecommendation.ts` and is shared with the
sampler panel's Apply buttons so the two cannot drift. An unrecognised model
falls back to a conservative generic middle rather than to nothing, because the
graph has to name some sampler.

**Testing required: yes.** This changes what the local pass renders, for every
NovelAI generation that uses it.

| # | Step | Expected |
|---|------|----------|
| 1 | NAI mode, local post-process on, pick the Anima model, upscale on, generate | The final image keeps NovelAI's composition but is visibly re-drawn in Anima's style, not NovelAI's |
| 2 | Look at the line under the local model picker after picking Anima | Reads `er_sde / sgm_uniform, 30 steps, CFG 4.0` |
| 3 | Set "Local refine denoise" to 0.05 and generate | The result is nearly identical to the raw NovelAI image |
| 4 | Set it to 0.60 and generate | The result diverges a lot, proving the slider reaches the sampler |
| 5 | Turn face fix on as well and generate | Face fix runs last, on the re-drawn and upscaled image |
| 6 | Pick a model with no known recommendation | The line shows the generic fallback (`euler / normal, 25 steps, CFG 6.0`) and the pass still runs |
| 7 | Pick Juice, then Nanosaur | The line shows each model's own recommended settings |
| 8 | On the ComfyUI backend, press the Anima "Apply" button in the sampler panel | Unchanged: 30 steps, CFG 4.0, er_sde, sgm_uniform, and the same upscale/facefix step counts as before |
| 9 | Open settings saved before this change | Local refine denoise reads 0.20, and the sampler line fills in once the model is re-picked |
| 10 | Switch the UI language and reopen the NAI local panel | The denoise label, its tip and the sampler line are translated |

**Do not skip:** 1, 3, 8.

**Result: reverted.** The img2img pass shipped and made the result worse rather
than better, so it was rolled back to a straight upscale. See the entry above.
Steps 8 and the shared recommendation table survived the revert; the rest of
this table no longer describes what the pass does.

### 2026-08-23 - Desktop events kept ComfyUI's prompt id after the NovelAI handoff (PR #618)

**Reported by:** the entry below, twice. With the local post-process on, the
refined image never arrives and the toast reads "A generation was lost due to a
connection issue". The Rust log disagrees: it shows the output image being
produced (`output_image: format=png ... bytes=5788574`) and the run completing
(`[gen] completed prompt=nai-...`). The backend did all of the work and the
frontend threw it away.

The NovelAI handoff is the only flow where the frontend holds one prompt id for
the whole run while ComfyUI runs the second half under a different one. The
`nai-` id is what `novelai_generate` returned, and `run_local_post_process`
alias-binds ComfyUI's real id back to it rather than replacing it.

Browser mode resolves that alias as it fans events out over SSE, so the browser
sees `nai-...` on every event. `app.emit` has no equivalent layer, so on desktop
the ComfyUI half of the run reached the frontend under an id it had never been
told about. Every per-prompt handler rejects that: the progress events did not
count as activity, `comfyui:output_image` was dropped by the
`pendingPrompts.some(...)` filter in `App.svelte`, and the completion never
matched. Thirty seconds later the reconciler found a prompt with no activity and
no images and called it lost.

The Tauri copy of an outgoing payload now goes through `with_resolved_alias`
before `app.emit`. The SSE copy and the temp event cache keep the real id, which
is what their own alias handling expects.

**Testing required: yes.** This is the fix for the blocker in both entries
below, and it touches every desktop event that carries a prompt id.

| # | Step | Expected |
|---|------|----------|
| 1 | Desktop app. NAI mode, local post-process on, pick the Anima split model, generate | The NovelAI image generates, the local pass runs, and the refined image lands in the gallery. No "generation lost" toast |
| 2 | Watch the progress bar during step 1 after NovelAI finishes | It picks up the ComfyUI pass and shows real step progress, rather than sitting still until the run is declared lost |
| 3 | Repeat step 1 with the model cold, so ComfyUI has to load it | Same result. A long silent model load no longer ends in a lost generation |
| 4 | Generate in NAI mode with the local post-process off | Unchanged: the plain NovelAI image arrives |
| 5 | Generate normally on the ComfyUI backend, desktop | Unchanged. Progress, preview and the final image all behave exactly as before |
| 6 | Generate through the browser UI, both NAI with post-process and plain ComfyUI | Unchanged. Browser mode already resolved the alias and must not have regressed |
| 7 | Cancel a NovelAI generation mid-run, during the local pass | It clears immediately, with no lingering queue entry and no toast afterwards |

**Do not skip:** 1, 5, 6.

**Result: passed.** The refined image arrives, the progress bar follows the
ComfyUI half of the run, and no generation is declared lost. What the pass
actually rendered was wrong, which is the entry above.

### 2026-08-23 - NovelAI prompts declared lost by the desktop reconciler (PR #618)

**Reported by:** step 2 of the entry below. With the local post-process on, the
NovelAI image never arrives and an error toast reads "A generation was lost due
to a connection issue - please try again". Nothing is actually disconnected.

The toast has exactly one source: the 5-second reconciler in `App.svelte`. Its
rule is "if a pending prompt is not in ComfyUI's queue, it finished and we
missed the event", softened by a 30-second grace window off the last progress
event.

A NovelAI prompt is never in ComfyUI's queue. It runs off-box, and it is only
tracked in MooshieUI's own fair queue. So the 30-second window is the only thing
keeping it alive, and the handoff to the local post-process blows straight
through it: after the last diffusion step the backend uploads the PNG, waits for
a free GPU worker and waits for ComfyUI to load the model, all without emitting
a single event. A cold split-file model takes well over 30 seconds on its own.

Browser mode already had this right. `webserver.rs` injects every prompt the
internal queue is tracking into `queue_pending`, with a comment saying the
reconciler would otherwise clear them the moment the user clicks generate.
Desktop's `get_queue` never got the same treatment: it filled in
`queue_positions`, which the reconciler does not read, and passed ComfyUI's
queue through untouched.

Desktop now injects the same entries, including the same 120-second submission
shield that lets a genuinely hung `gen-` submit still surface as a lost
generation rather than sit on "Preparing" forever. Nothing masks a real failure:
a NovelAI run is removed from the internal queue on every outcome, and a
handed-off one is removed by the websocket when the ComfyUI prompt ends.

**Testing required: yes.** Backend-only, but it changes when the UI gives up on
a prompt.

| # | Step | Expected |
|---|------|----------|
| 1 | NAI mode, local post-process on, pick the Anima split model, generate | The NovelAI image generates, then the local pass runs and the refined image lands in the gallery. No "generation lost" toast at any point |
| 2 | Repeat with a model ComfyUI has not loaded this session, so the load is cold and slow | Same. The progress bar may sit still for a minute while the model loads, and the generation still completes |
| 3 | Generate in NAI mode with the local post-process off | Unchanged: the plain NovelAI image arrives |
| 4 | Open Settings and the Queue section mid-generation | The NovelAI prompt is listed, as before |
| 5 | Generate normally on the ComfyUI backend | Unchanged. No duplicate queue entries and no stuck "Preparing" |
| 6 | Cancel a NovelAI generation mid-run | It clears immediately, no lingering queue entry and no lost-generation toast afterwards |

**Do not skip:** 1, 2, 5.

**Result:** no change, the same toast still appears. The desktop queue readout
was genuinely wrong and the fix stands, but it was not what broke this. The real
cause is the entry above.

### 2026-08-23 - Text encoders filed under `clip/` (PR #618)

**Reported by:** step 3 of the entry below, on a machine where the local
post-process died with a lost connection as soon as a split-file model was
picked.

ComfyUI's `CLIPLoader` offers `models/text_encoders/` and the legacy
`models/clip/` as a single list, and the frontend model picker already merges
the two (`src/lib/stores/models.svelte.ts`). `read_modelspec` did not: it drew
its `recommended_clip_model` from `text_encoders/` alone. On the reporting
machine `text_encoders/` holds one unrelated 15 GB encoder and Anima's
`qwen_3_06b_base.safetensors` sits in `clip/`, so the recommendation came back
empty.

Empty is a deliberate state, not an error. The recommendation is omitted rather
than substituted when nothing installed is compatible, because a mismatched
encoder fails deep inside sampling instead of failing loudly. But nothing
downstream checked for it, so the NovelAI local pass built a `CLIPLoader` with
`clip_name: ""` and handed it to ComfyUI.

Three changes.

- **The recommendation** now reads both folders and de-duplicates, matching what
  `CLIPLoader` actually offers. This also fixes the main model picker on any
  install using the legacy layout.
- **The local pass** refuses to submit a split-file graph whose text encoder or
  VAE is still unresolved, and names the missing half in the log instead of
  letting an empty loader input reach ComfyUI.
- **The picker** replaces the neutral "Split model: using X and Y" hint with an
  amber warning when either companion is unresolved, so the gap is visible
  before generating rather than after.

**Testing required: yes.**

| # | Step | Expected |
|---|------|----------|
| 1 | NAI mode, local post-process on, pick the Anima split model | The hint reads "Split model: using qwen_3_06b_base.safetensors and qwen_image_vae.safetensors" (or whichever encoder is installed), not the amber warning and not a blank name |
| 2 | Generate | The local pass runs to completion and the refined image lands in the gallery. No lost connection |
| 3 | Check the Rust log for `NovelAI <id>: local pass model=` | `split=true`, and `clip=` names a real file rather than `None` |
| 4 | Switch to the main ComfyUI backend and pick the same Anima model | The text encoder and VAE fields fill in the same way |
| 5 | Temporarily move the encoder out of `clip/`, restart ComfyUI, re-pick the model | The amber warning appears under the dropdown, and generating delivers the plain NovelAI image with a log line naming the missing text encoder rather than an error from ComfyUI |
| 6 | Pick a plain checkpoint | No hint, no warning, and the pass runs as before |

**Do not skip:** 1, 2, 5.

**Result:** the encoder is found now, but step 2 still fails with "a generation
was lost due to a connection issue". That turned out to be a second, unrelated
bug in the desktop queue readout, fixed in the entry above.

### 2026-08-23 - Split-file models in the NovelAI local post-process (PR #618)

**Reported by:** step 4 of the entry below. The local post-process model picker
listed only `models/checkpoints`, which on the reporting machine holds one file
and that file is broken, so the pass could not be run at all.

Two layers were hardcoded to the single-file case.

- **The picker.** The dropdown was fed `models.checkpoints` alone, and the store
  always asked the backend for `readModelSpec("checkpoints", filename)`, so a
  split-file model living in `models/diffusion_models` (Anima, Flux, Chroma and
  the rest) was invisible to it. It now renders two option groups, "Checkpoints"
  and "Diffusion models", and each option carries its folder along with the name
  as `"{category}:{filename}"`.
- **The workflow builder.** `upscale_standalone::build_params` hardcoded
  `use_split_model = false` and `vae = None`, so even a hand-set split file would
  have been handed to `CheckpointLoaderSimple` and failed to load. The derived
  params now follow the file that was picked: a split model gets UNETLoader +
  CLIPLoader + VAELoader, a full checkpoint keeps the single loader.

The companion files are chosen for the user rather than asked for. Selecting a
model reads its ModelSpec, which reports `model_kind`, `recommended_clip_model`,
`recommended_clip_type` and `recommended_vae`; the store matches those against
the installed text encoders and VAEs (exact name first, then basename) and fills
them in, and the hint under the dropdown names the two it settled on. A `.gguf`
file skips the tensor-key check and is judged by the folder it sits in.

A file in the wrong folder still loads. `local_model_category` records where the
file physically is, and Rust sets `model_source_category` only when that folder
disagrees with the loader mode, which is what makes `run_local_post_process`
resolve it to an absolute path before the graph is built.

**Testing required: yes.** All of it is new behaviour in the picker.

| # | Step | Expected |
|---|------|----------|
| 1 | NAI mode, open the local post-process section and open the model dropdown | Two groups, "Checkpoints" and "Diffusion models", each listing the installed models for that folder. An empty group is not rendered |
| 2 | Pick a split-file model (Anima) | A hint appears under the dropdown reading "Split model: using <clip> and <vae>", naming a text encoder and a VAE that are actually installed |
| 3 | Generate with the local post-process on | The pass runs. No `CheckpointLoaderSimple` failure, and the refined image lands in the gallery |
| 4 | Pick a plain checkpoint instead | The hint disappears, and generating still runs the pass exactly as before |
| 5 | Set the model back to none | The post-process is skipped and a plain NovelAI image comes back |
| 6 | Take a post-processed image and upload it to novelai.net | The site does not read it, and MooshieUI's lightbox still names NovelAI as the backend. Unchanged from the entry below |
| 7 | Restart the app and reopen the section | The picked model, its folder and the hint all come back |
| 8 | Optional. Pick a model whose recommended VAE is not installed | Generation fails with a ComfyUI error naming the missing file, rather than quietly loading something else |

**Do not skip:** 2, 3, 7.

**Result:** 1 and 2 pass. 3 fails and blocks 4 through 8: the pass errors out
with a lost connection. Cause found and fixed in the entry above.

### 2026-08-23 - App-mode metadata loss, and Anlas-free aspect ratios (PR #618)

**Reported by:** the hand-test pass on the entry below. Steps 6 and 7 passed in
browser mode and failed in the desktop app, and a 2:3 generation at side 1024
was charging Anlas while 3:4 was free.

Two causes, unrelated to each other.

- **Every export re-encoded the PNG.** The gallery write path was already
  correct in both modes: files on disk carry NovelAI's six chunks byte for byte.
  The loss happened on the way out. `saveImageAs` and `saveImageToDir` embed the
  app's own metadata into whatever they are about to hand the user, which
  reaches Rust `embed_image_metadata` -> `embed_png_metadata`, a full decode and
  re-encode that drops the text chunks and overwrites the stealth alpha. Rather
  than guard four frontend call sites, the guard now sits at the one Rust
  function every export and clipboard route passes through: a PNG that still
  carries NovelAI's chunks comes back untouched. Nothing is lost by that, since
  the metadata the call would have written says the same thing and the app reads
  NovelAI's chunks back just as happily as its own. A locally post-processed
  image has already lost the chunks to its own re-encode, so it falls through
  and is embedded as normal.
- **The aspect ratio mapping rounded up past one megapixel.** Opus covers a
  generation only while it stays at or under 1,048,576 pixels. The area-faithful
  formula picks the closest pair on the grid, in either direction, so 2:3, 3:2
  and 21:9 all landed on 1,064,960, over by 16,384, while 3:4 and 16:9 happened
  to round down and stayed free. In NovelAI mode the dimensions are now taken
  from the largest pair on the 64px grid whose area does not exceed the
  requested one. Local backends keep the old mapping.

| Preset | Before, at side 1024 | After | Free on Opus |
|--------|----------------------|-------|--------------|
| 1:1 | 1024x1024 | unchanged | yes, exactly at the cap |
| 4:3 | 1152x896 | unchanged | yes |
| 3:4 | 896x1152 | unchanged | yes |
| 16:9 | 1344x768 | unchanged | yes |
| 9:16 | 768x1344 | unchanged | yes |
| 3:2 | 1280x832 | **1216x832** | now yes |
| 2:3 | 832x1280 | **832x1216** | now yes |
| 21:9 | 1664x640 | **1536x640** | now yes |

832x1216 and 1216x832 are NovelAI's own portrait and landscape presets, so the
new numbers are not an invention of ours.

The highlight was built as well, since it still says something at side lengths
above 1024 where the presets genuinely do cost Anlas: on an Opus account in
NovelAI mode, every preset the plan covers at the current side length and step
count gets a green border, and a green dot with "Free Opus gens" sits at the top
right of the aspect ratio section. The border is computed from the same function
the buttons apply, so it cannot disagree with what clicking one produces.

**Testing required: yes.** 1, 3 and 6 are the ones that were broken.

| # | Step | Expected |
|---|------|----------|
| 1 | Desktop, NAI mode, local post-process **off**. Generate, Save Image As, upload the saved PNG to novelai.net | The site reads its own metadata. This is the one that was broken |
| 2 | Desktop. Same image, save it to a folder instead (the gallery save-to-directory action), upload that file | Same result |
| 3 | Desktop. Copy the image to the clipboard, then paste into an image editor, and separately drag the gallery file onto novelai.net | The editor gets a real PNG, the site accepts the dragged file |
| 4 | Desktop, NAI mode, with a local post-process applied (upscale). Save the result, upload it to novelai.net | The site does **not** read it. MooshieUI's own lightbox still shows NovelAI as the backend |
| 5 | Desktop, ComfyUI backend. Generate, Save Image As, drag the saved file back into the app | Settings restore as before. The guard must not touch local images |
| 6 | NAI mode, Opus account, side 1024, 28 steps. Click each of the eight aspect presets | 1:1 1024x1024, 4:3 1152x896, 3:2 1216x832, 16:9 1344x768, 21:9 1536x640, 3:4 896x1152, 2:3 832x1216, 9:16 768x1344. All eight carry a green border, the "Free Opus gens" dot shows, and the generate button quotes ~0 Anlas |
| 7 | Same, side 1536 | The green borders and the dot disappear, and the cost badge quotes a real Anlas price |
| 8 | Back to side 1024, raise steps to 50 | Same: borders and dot gone, cost badge quotes Anlas |
| 9 | Non-Opus NAI account, or switch to the ComfyUI backend | No green borders and no dot at all |

**Do not skip:** 1, 3, 6.

**Result:** 1, 3, 6, 7, 8 and 9 pass. 2 goes through the same export path as
1, so it is redundant with it. 5 was not applicable. 4 could not be run: the
local post-process model picker offered only `models/checkpoints`, which on the
reporting machine holds a single broken file. That gap is the entry above.

### 2026-08-23 - Clipboard interop follow-up: drop targets, browser-mode save, Anlas estimate (PR #618)

**Reported by:** the hand-test pass on the entry below. Steps 2, 3, 4 and the
browser-mode repeats of them failed, which blocked 6.

Four separate causes, only two of them the same bug wearing different clothes.

- **Browser-mode save was still re-encoding.** `webserver.rs` carries its own
  copy of the gallery save path, `save_to_gallery_in_dir`, and the NovelAI guard
  had only been added to the desktop `save_to_gallery_inner`. The browser copy
  ran `embed_png_metadata()` unconditionally, which is exactly the decode and
  re-encode that drops NovelAI's text chunks and overwrites its stealth alpha.
  Same guard now sits in both.
- **`_embed_temp_metadata` re-encoded too.** That endpoint exists so a browser
  right-click "Copy Image" carries our metadata. On a NovelAI PNG that trade is
  the wrong way round, so it now hands the bytes back untouched.
- **The desktop drop handler was panel-scoped.** `setupTauriDragDrop` looked up
  a section under the cursor, then an image drop zone, and if neither matched it
  did nothing at all. Dropping an image anywhere else in the window now falls
  back to a plain metadata import.
- **Browser mode had no window-level drop handler,** so Firefox followed its own
  default and navigated the tab to the dropped file. A `dragover` and a `drop`
  listener now sit on the window, both skipped when an inner zone has already
  called `preventDefault`, so the sections and the image inputs keep the
  targeted behaviour they had.

Two findings worth writing down because they are not bugs:

- **The desktop save path was already correct when it was tested.** The binary
  in use predated the fix. Gallery files written after it carry the six NovelAI
  chunks (`Title`, `Description`, `Software`, `Source`, `Generation time`,
  `Comment`); files written before it carry a single `parameters` chunk.
- **Step 4 cannot be made to work through Firefox.** On Windows the app puts a
  file drop list on the clipboard, not a bitmap, precisely so the PNG bytes are
  never touched. A web page cannot read a file reference out of a paste. Drag
  the gallery file onto novelai.net, or use its file picker.

The same pass turned up an Anlas readout problem that had nothing to do with
the clipboard. The account record is fetched on demand, and the only thing that
asked for it was the usage readout, which sits behind a display toggle. With the
toggle off nothing ever fetched it, so `isOpus` stayed false and the cost badge
quoted the full price of a generation Opus covers. The generate button now asks
for the record itself, and the readout defaults to on, since a balance you have
to switch on is a balance nobody sees before spending against it.

**Testing required: yes.** Steps 4, 6 and 8 are the ones that were actually
broken.

| # | Step | Expected |
|---|------|----------|
| 1 | Desktop, NAI mode. Drag a NovelAI PNG from Windows Explorer onto the middle of the app window, not onto any panel | Settings restore, same as Ctrl+V |
| 2 | Same drag, this time onto a specific section (Prompts, Sampler, Dimensions) | Only that section restores. Unchanged from before this fix |
| 3 | Same drag onto the img2img image input | The image uploads as the input. No metadata import |
| 4 | Browser mode. Drag a NovelAI PNG from Explorer anywhere onto the page | Settings restore. The tab does **not** navigate away to the image |
| 5 | Browser mode. Drag an image straight out of a Firefox tab onto the page | Either the same restore or nothing at all, but never a navigation away |
| 6 | Browser mode, NAI mode, local post-process **off**. Generate, take the file out of the gallery folder, upload it to novelai.net | The site accepts it and reads its own metadata. This is the one that was broken |
| 7 | Browser mode. Right-click the finished image, Copy Image, paste into an image editor | A PNG arrives, not a blank |
| 8 | Desktop, NAI key configured, usage readout switched **off** in Settings. Opus account, 1024x1024, 28 steps | The badge on the generate button reads ~0 Anlas, not ~28 |
| 9 | Fresh browser-mode client (clear site data first) with a NAI key configured | The Anlas readout and Opus bar appear above the generate button without toggling anything |
| 10 | Switch the readout off in Settings, reload | It stays off |

**Do not skip:** 4, 6, 8.

**Result: 1 pass, 2 pass, 3 pass, 5 pass. 4 passes from Explorer onto a
Firefox tab. 6 and 7 pass in browser mode and fail in the desktop app: both
the saved file and the copied image reached novelai.net with no metadata. 8
passes. 9 and 10 skipped by the tester. The app-mode failure is fixed in the
entry above.**

### 2026-08-23 - NovelAI clipboard interop, both directions (PR #618)

**Asked for:** copy an image off novelai.net, Ctrl+V into the app, and get the
settings back; and the reverse, an image this app generated through NovelAI
staying acceptable to novelai.net, unless a local post-process touched it.

Four pieces, one of which is nothing:

- **Copy out needed no code.** `copy_image_to_clipboard` and
  `copy_gallery_image_to_clipboard` already read PNGs off disk and put them on
  the clipboard unchanged. Only JXL and WebP get decoded and re-encoded, and
  NovelAI output is neither: `deliver_image()` tags it as PNG.
- **Nothing re-encodes a NovelAI PNG any more.** The one place that did was
  `embed_png_metadata()`, called from `save_to_gallery_inner`. It fully decodes
  and re-encodes, which drops NovelAI's own text chunks and, in stealth mode,
  overwrites the alpha bits its hidden copy lives in. A PNG that still carries
  NovelAI chunks now goes to disk byte for byte instead.
- **A reader for NovelAI's metadata**, `novelai::metadata`, plus a browser-mode
  mirror in `novelaiPngMetadata.ts`. NovelAI writes no `parameters` chunk, so
  its images used to read as having no metadata at all.
- **A writer half**, in `buildPngMetadata`, so a post-processed image still says
  it came from NovelAI and restores its NovelAI settings on reimport.

Two things worth stating plainly, because neither matches the literal ask:

- **The reader is not gated on NAI mode.** Mode is a frontend concept and the
  reader is in Rust; it only fires on an image that actually carries NovelAI
  chunks, so gating it would add a switch that never changes an outcome.
- **Byte preservation is decided from the bytes, not from a flag.** A pure
  NovelAI generation still has the chunks and is preserved; the same image after
  a local post-process lost them in the re-encode that post-processing already
  performed, so it takes the normal embed path. No flag needed anywhere.

Restored settings land in the right halves of the panel: NovelAI's sampler and
noise schedule go into `novelaiSettings`, not the top-level `samplerName` and
`scheduler`, which stay ComfyUI values for the local post-process pass. The
quality toggle and UC preset are forced off on import, because the captured
prompt already has their text folded into it and re-enabling them would append
a second copy.

**Testing required: yes.** Steps 3 and 6 are the ones that cannot be inferred
from the code.

| # | Step | Expected |
|---|------|----------|
| 1 | Desktop, NAI mode. Copy an image on novelai.net, focus the app, Ctrl+V | Prompt, UC, seed, steps, sampler, noise schedule, CFG and model all populate. Nothing is saved to the gallery |
| 2 | Drag that same file onto the app window instead | Same restore as step 1 |
| 3 | Generate in NAI mode with local post-process **off**. Take the file out of the gallery folder and upload it to novelai.net (img2img or vibe transfer) | The site accepts it and reads its own metadata back |
| 4 | Copy that same gallery image from inside the app, paste into any image editor | A PNG arrives, not a blank or a re-encode |
| 5 | Ctrl+V that gallery PNG back into the app | Settings restore. Sampler and noise schedule land in the NovelAI panel, and the ComfyUI sampler dropdown is untouched |
| 6 | Generate in NAI mode with local post-process **on**, then upload that file to novelai.net | The site does **not** recognise it as its own. Reimporting it into the app still restores the settings and still reads as NovelAI |
| 7 | Paste a NovelAI image with two or more characters | The character list comes back with each prompt and position |
| 8 | Browser mode (LAN or `--server`). Repeat step 1 | Same restore, via the client-side reader |
| 9 | Regression: paste a normal ComfyUI or SwarmUI PNG in ComfyUI mode | Unchanged from before this change |

**Do not skip:** 3, 6, 8.

**Result: 1, 5, 7, 9 pass. In browser mode 8-1, 8-5 and 8-7 pass. 2, 3, 4, 8-2,
8-3 and 8-4 fail; 6 and 8-6 are blocked behind 3.** The follow-up entry above
records what each failure turned out to be.

### 2026-08-23 - Style fragment weights in NovelAI mode (PR #618)

**Reported by:** Phase F follow-up step 2. An active Artist Style showed up as
`(artist_tag:1)`, which is A1111/ComfyUI weight syntax, not NovelAI's.

The outgoing request was already correct. `novelai::prompt_syntax::to_novelai()`
runs on `positive_prompt` in `build_request()` and rewrites `(tag:w)` into
`w::tag::`, dropping the weight entirely when it is exactly 1. So NovelAI
received the bare `artist_tag`. What was visible was the pre-conversion prompt
that the app stores and displays, not the payload.

The `(tag:1)` wrapper was still worth removing at the source.
`styles.buildPromptFragment()` now writes a weight of exactly 1 as a bare tag.
`(tag:1)` and `tag` render identically on every backend, so nothing changes in
ComfyUI, but the stored prompt stops carrying syntax the user never typed, and
`mergeTagPrompts()` can now dedupe a style's artist against the same tag typed
by hand (it compares whole tag strings, so `(artist:1)` and `artist` used to
both survive).

Rust regression tests were added to `prompt_syntax.rs` pinning the shapes the
style store can emit, including a tag with escaped Danbooru parentheses.

**Testing required: yes,** small.

| # | Step | Expected |
|---|------|----------|
| 1 | NovelAI mode, activate a style whose artist weight is 1, generate | The saved metadata prompt shows the bare `artist_tag`, no `(artist_tag:1)` |
| 2 | Same style but set the artist weight to 1.2, generate | Metadata shows `(artist_tag:1.2)`; the image is visibly weighted |
| 3 | Type the same artist tag by hand in the prompt box with that style active, generate | The tag appears once, not twice |
| 4 | ComfyUI checkpoint, style active at weight 1, generate | Unchanged output from before this fix |

**Do not skip:** 1 and 3.

**Result: pass, all four.** Step 2's `(artist_tag:1.2)` in the lightbox is
correct and not a leak. Image metadata is written from `GenerationParams`, the
app's canonical backend-neutral prompt, which is always ComfyUI syntax. The
NovelAI rewrite happens later, in `build_request()`, and is not written back to
`params`. That is deliberate: loading settings from a gallery image puts the
metadata prompt straight back into the prompt box, which expects ComfyUI
syntax, so a NovelAI-syntax metadata prompt would round-trip through
`translateNaiWeightSyntax()` and come back as `(artist_tag:1.2)` anyway.

The one thing this costs is that the metadata prompt cannot be pasted into
NovelAI's own site and keep its weights. That applies to every weighted tag,
not just style tags, and predates this change.

### 2026-08-23 - Phase F follow-up: styles leaked `@` into NovelAI prompts (PR #618)

**Found by:** Phase F step 6. The style editor still showed `@artist` while in
NovelAI mode. Chasing it turned up two bugs, one cosmetic and one that changed
the generated image.

- The artist list in the style editor hardcoded the `@` in its template, so it
  rendered `@name` no matter what was stored. Cosmetic only. It now uses
  `animaArtistTagPrefix`, so it shows the form that will actually be sent.
- `styles.buildPromptFragment()` emitted the stored tag verbatim. A style built
  while an Anima checkpoint was selected stores `@artist`, so activating that
  same style in NovelAI mode put `@artist` straight into the NovelAI prompt,
  where `@` is the prompt-chunk reference sigil. It now takes a `stripSigil`
  flag that `toParams()` sets from `isNovelAi`. The flag is passed in rather
  than read from the store, because `generation` already imports `styles` and
  the reverse would be a cycle.

`animaArtistTagPrefix` now excludes NovelAI explicitly instead of relying on
the family test. A NovelAI model id is not a safetensors file, so no family
metadata is ever resolved for it, and leaning on `isAnima` alone would have
depended on that clearing having happened.

**Testing required: yes,** but short.

| # | Step | Expected |
|---|------|----------|
| 1 | Anima checkpoint, create a style, add an artist. Switch to NovelAI mode and reopen that style | The artist row shows the bare name, no `@` |
| 2 | With that style active in NovelAI mode, generate | The artist tag reaches NovelAI bare. No `@artist` in the request or the saved metadata |
| 3 | Switch back to the Anima checkpoint with the same style still active, generate | `@artist` is back in the prompt, unchanged from before this fix |
| 4 | Non-Anima ComfyUI checkpoint with a style whose artists are stored bare, generate | Still bare. No `@` is added |

**Do not skip:** 2 and 3. Step 2 is the leak itself, step 3 proves the fix did
not break Anima.

### 2026-08-23 - Phase F: artist tag sigil centralised (PR #618)

**Changed:** The `@` on an artist tag is now decided in one place. The new
`src/lib/utils/artistTag.ts` holds the pure string work, and
`generation.artistTagPrefix` / `formatArtistTag()` decide the sigil for the
current mode: `@` in ComfyUI mode, bare in NovelAI mode. Insertion, clipboard
copy, duplicate detection, toggle-off and replace now all read the same rule.
The saved-style form keeps its own narrower rule as
`generation.animaArtistTagPrefix`, because only Anima checkpoints were trained
on `@artist`.

**Fixed:** In NovelAI mode artist insert could only ever add. Duplicate
detection, toggle-off and replace all tested `startsWith("@")`, which is never
true when there is no sigil. Without a sigil an artist tag is indistinguishable
from any other danbooru tag, so the artist index is what identifies one now. If
the index has not loaded the code adds rather than guessing, so nothing gets
silently eaten.

**Testing required: yes.** ComfyUI behaviour is meant to be byte-identical, so
half of this pass is a no-regression check.

| # | Step | Expected |
|---|------|----------|
| 1 | NovelAI mode, empty prompt, click an artist in the artist gallery | Tag inserted bare, no `@` |
| 2 | NovelAI mode, click that same artist again | Tag is removed (toggle off). Before this change it was added a second time |
| 3 | NovelAI mode, with one artist in the prompt, click a different artist | The replace/add modal appears. Replace swaps the artist, Add keeps both |
| 4 | NovelAI mode, use the copy-tag button on a gallery card and in the lightbox | The clipboard holds the bare tag |
| 5 | ComfyUI mode on any non-Anima checkpoint, repeat steps 1 to 4 | `@tag` everywhere, exactly as before this change |
| 6 | Anima checkpoint, Style editor, add an artist. Then switch to a non-Anima checkpoint and add one | Anima stores `@tag`, non-Anima stores the bare name |
| 7 | NovelAI mode, prompt containing an artist tag, run Interrogate and choose Replace | The artist tag survives the replace |
| 8 | NovelAI mode, restart the app and click an artist before the index finishes loading | The tag is added, nothing is removed or replaced |

**Do not skip:** 2 and 5. Step 2 is the bug this phase existed to fix, and
step 5 is the regression guard for every existing ComfyUI user.

**Result: steps 1 to 5 and 7 pass.** Step 6 found a real bug, fixed in the
entry above. Step 8 behaves as designed: clicks that land before the artist
index has loaded each add a copy, then the first click after it loads
removes every copy at once. That is the toggle path doing its job once it
can identify the tag, and it self-heals, so it is accepted rather than
fixed.

### 2026-08-23 - Normalize strengths moves client side (PR #618)

**Fixed:** Normalize strengths did nothing. The checkbox drove
`normalize_reference_strength_multiple`, which NovelAI's team confirmed is
frontend logic that the backend ignores. `normalize_strengths()` in
`payload.rs` now scales the strengths here, just before the request is built.
The flag is still sent for parity with NovelAI's own client. See section 2.10.

**Testing required: yes.** Small, but this is the third attempt at this
checkbox and the first two looked fine from the outside.

| # | Step | Expected |
|---|------|----------|
| 1 | Fix the seed, add two vibes at Strength 1.0 each, generate, tick Normalize strengths, generate again | The two images differ. The log reads `strengths [1.0,1.0]` then `strengths [0.5,0.5]` |
| 2 | Untick it and generate a third time | The image matches the first one exactly |
| 3 | With the box ticked, set both vibes to 0.5 and generate, then set them to 1.0 and generate | Both give the same image, because both normalise to 0.5 and 0.5 |
| 4 | With the box ticked, drag a Strength slider | The slider still shows the raw value; only the log line shows the scaled one |
| 5 | One vibe at 0.6, box ticked, generate | The log reads `strengths [1.0]` |
| 6 | Both vibes at 0.0, box ticked, generate | No crash and no `null` in the request; the log reads `strengths [0.0,0.0]` |

**Do not skip:** 1 and 2. That pair is the whole fix, and step 2 is what
proves the change is the normalisation rather than drift.

**Result: pass.** Steps 1 and 2 confirmed. Same seed, two vibes, images
visibly distinct, and the log reads `strengths [1.0,1.0], normalize false`
then `strengths [0.5,0.5], normalize true`. The backend agrees with the UI.
Remaining steps not run and not needed.

### 2026-08-23 - Vibe tokens persist, normalize strengths (PR #618)

**Shipped:** two follow-ups to the vibe transfer fix.

- **The `.naiv4vibe` token now survives a restart.** The backend emits
  `novelai:vibes_encoded` after an encode pass, the frontend stores the token
  next to the image it came from, and the pair it was minted for (model and
  Information extracted) travels with it. A green **Encoded** badge marks a
  vibe that will cost nothing, and it clears the moment the model or the
  extraction level moves, because at that point the token is stale and the
  next generation pays 2 Anlas for a fresh one. See section 2.10.
- **Normalize strengths.** A checkbox under the vibe list, shown once there is
  at least one vibe. It drives `normalize_reference_strength_multiple`, which
  was wired end to end in Rust but had no way to be turned on. NovelAI's own
  client offers the same option.

**Testing required: yes.**

| # | Step | Expected |
|---|------|----------|
| 1 | Add a vibe image, note the Anlas balance, generate | The vibe gets a green Encoded badge; balance drops by the generation cost plus 2 |
| 2 | Generate again without touching anything | Still badged, no extra 2 Anlas |
| 3 | Restart the app | The vibe is still there and still badged |
| 4 | Generate after that restart | Works, and **no** 2 Anlas is charged this time |
| 5 | Move that vibe's Information extracted slider | The badge disappears immediately |
| 6 | Generate | 2 Anlas is charged and the badge comes back |
| 7 | Switch the model from V4.5 Full to V4 Full | The badge disappears without touching the sliders |
| 8 | Switch back to V4.5 Full | The badge returns, because that token was never thrown away |
| 9 | Move only the Strength slider | The badge stays on, and the next generation charges no encode |
| 10 | Fix the seed, add two vibes at 1.0 and 1.0, generate, then tick Normalize strengths and generate again | Superseded: see the entry above. As shipped here the two images matched, because the flag was inert |
| 11 | Restart with the checkbox ticked | It is still ticked |
| 12 | Remove the last vibe | The Normalize strengths checkbox disappears |
| 13 | Switch UI language | The Encoded badge and the Normalize strengths label and tooltip are translated |

Normalising happens on NovelAI's side, so the Strength sliders keep their
raw values and nothing moves in the UI. `log_vibe_summary()` in
`novelai/mod.rs` logs one line per vibe-carrying request giving the reference
count, the strengths and the flag, and that is the only direct confirmation
the request carried it. It reads the built request body rather than the
params on purpose: the pre-flight validation build in `commands::novelai`
would otherwise double every line, and the tokens themselves are never
logged.

Step 10 could not pass as written, which is what the next entry fixes. Note
also that with two vibes the obvious A/B is a trap, because normalising 0.5
and 0.5 is a no-op and normalising 0.1 and 0.1 lands on exactly the 0.5 and
0.5 case, so a working normalisation and a dropped one both predict identical
images. Only strengths that do not sum to 1 and do not halve into another
tested pair, such as 1.0 and 1.0, can tell the two apart.

**Do not skip:** 3, 4, 5, 7. Those are the persistence itself and the two ways
a token goes stale, which is the only way the badge can lie.
**Low-risk, skip if short on time:** 9, 12, 13.

**Result, 2026-08-23:** 1 to 9 and 11 to 13 pass. Step 10 was a null result,
traced to NovelAI ignoring the flag and fixed in the entry above.

### 2026-08-23 - V4 vibe transfer fix (PR #618)

**Fixed:** vibe transfer sent the raw reference image, which is the V3 request
shape. Every V4 and V4.5 generation with a vibe attached failed with an opaque
500. Reference images now go through `/ai/encode-vibe` first.

**Testing required: yes. Done 2026-08-23: all nine steps pass on both V4.5
Full and V4 Full.** This also unblocked the Phase E checklist below, which
could not be run past step 7.

| # | Step | Expected |
|---|------|----------|
| 1 | Note the Anlas balance, then generate on V4.5 Full with one Vibe Transfer image | An image comes back instead of an error |
| 2 | Check the balance again | Down by the generation cost plus 2 Anlas for the encode |
| 3 | Generate again without touching the vibe | Works, and this time no extra 2 Anlas is spent |
| 4 | Move the vibe's Information extracted slider, generate | Another 2 Anlas is spent, and the result visibly changes |
| 5 | Move only the Strength slider, generate | No extra 2 Anlas |
| 6 | Add a second vibe image, generate | Both influence the result; 2 Anlas charged for the new one only |
| 7 | Restart the app, generate with the same vibe | Works; 2 Anlas is charged again (superseded: tokens now persist, see the entry above) |
| 8 | Generate on V4 Full with a vibe | Works the same way |
| 9 | Remove all vibes, generate | Normal generation, no encode charge |

**Do not skip:** 1, 3. Those are the fix itself and the cache that keeps it
from re-billing.
**Low-risk, skip if short on time:** 5, 7, 9.

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
