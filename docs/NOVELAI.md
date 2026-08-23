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

**Result: pending.**

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
