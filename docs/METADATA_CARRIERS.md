# Metadata carriers

Where MooshieUI puts generation parameters in each output format, and what
survives which transport. MooshieUI's own parameter payload is the SwarmUI-shaped
JSON produced by `metadata::format_swarmui_json()`. NovelAI PNGs additionally
carry their original provider metadata, described below.

## NovelAI PNG metadata

NovelAI uses `Software`, `Source`, `Description` and a JSON `Comment` chunk.
MooshieUI reads those through `novelai/metadata.rs` and preserves original PNG
text chunks through gallery saving, clipboard copying and NovelAI face-detail
compositing. Post-processing information can accompany the original metadata;
the original source parameters alone do not describe every later edit.

On desktop, copying a newly generated NovelAI image waits for its gallery save
when needed so the saved PNG can supply the original chunks. Browser clipboard
behavior also depends on the browser and the application receiving the paste.
Importing a V5 image ignores its placeholder zero for undesired-content strength;
a meaningful zero from older models can still be restored.

### MooshieUI marker

A NovelAI PNG generated through MooshieUI is never re-encoded, so it cannot get
the usual `parameters` chunk or stealth payload. Instead every save, export and
clipboard route splices one extra text chunk in just before `IEND`
(`metadata::mark_novelai_png`), without decoding the image. Every byte NovelAI
wrote, its text chunks, the `IDAT` data and the stealth alpha, stays identical,
so novelai.net still recognises the file.

- Keyword `MooshieUI`. Not `parameters`, which this app and A1111-style tools
  read first and which would shadow NovelAI's chunks, and not `Software`, which
  NovelAI owns.
- `tEXt` when the value is Latin-1, otherwise uncompressed `iTXt`, the same rule
  as the `parameters` writer.
- Value: compact JSON with no prompt, for example
  `{"software":"MooshieUI","version":"2.3.49","backend":"novelai","model":"nai-diffusion-4-5-full","generated":"2026-11-17T04:20:00+08:00","source":"ui"}`.
  `model` is left out when the save did not record one; `source` is `mcp` for
  images the MCP server saved and `ui` otherwise.
- Idempotent: a PNG that already has a `MooshieUI` chunk comes back unchanged,
  so repeated exports and copies never add a second one.
- Chunk only, whatever the metadata mode: the marker never touches pixels.
  The invisible watermark skips NovelAI PNGs for the same reason.

On read, NovelAI's chunks (or its stealth payload) still supply every
generation setting. The reader only adds a `source` entry, "MooshieUI
(NovelAI)" when the marker is present and "NovelAI" when it is not, which the
gallery's metadata panel shows as Source. A NovelAI image that was re-encoded
by local post-processing is not affected: it carries the regular `parameters`
payload with `mooshie_extra.software = "MooshieUI"`.

These guarantees concern the preserved PNG path, not arbitrary re-encoding by
external editors or messaging services. See [NovelAI backend](NOVELAI.md) for
the integration and [Gallery and Metadata](https://github.com/Mooshieblob1/MooshieUI/wiki/Gallery-and-Metadata)
for user-facing import/export instructions.

## What is written

| Format | Container-native carrier | Top-level uuid XMP box | Written by |
|--------|--------------------------|------------------------|------------|
| PNG | iTXt / tEXt "parameters", optional stealth alpha (NovelAI PNGs: tEXt / iTXt "MooshieUI" marker only, see above) | n/a | Rust |
| JXL | xml box | n/a | Rust |
| WebP (still and animated) | RIFF EXIF UserComment, optional stealth alpha | n/a | Rust for stills, Pillow for animated |
| MP4 | moov/udta/meta mdta key "comment" | yes | PyAV for the bytes, Rust for the box |
| AVIF | Exif item | yes | Pillow for the bytes, Rust for the box |
| GIF | Comment Extension | n/a | Pillow |

The uuid box uses Adobe's XMP identifier BE7ACFCB-97A9-42E8-9C71-999491E3AFAC
and is appended after every existing top-level box, so it moves no existing
byte and sample offsets stay valid. mp4 and avif are both ISOBMFF, so exports
in either format get the box; webp and gif have no equivalent and keep only
their container-native carrier.

Verified on the export probes: animated webp carries the payload as UTF-16BE
in an EXIF chunk, avif carries it in an Exif item, an exported avif carries
both that Exif item and a top-level uuid box, and gif carries UTF-8 in a
Comment Extension at the head of the file.

## What survives

Measured, not assumed. Test payload was 1331 bytes of UTF-8 containing CJK
characters, quotes, and braces.

| Carrier | ffmpeg -c copy remux | full re-encode | Discord round trip |
|---------|----------------------|----------------|--------------------|
| MP4 moov/udta/meta "comment" | survives | survives | box renamed to skip, payload zeroed |
| MP4 moov/udta (c)cmt | survives | survives | box renamed to skip, payload zeroed |
| MP4 top-level uuid XMP | dropped | dropped | survives byte-identical |
| WebP EXIF chunk (still) | n/a | n/a | chunk deleted |
| PNG iTXt chunk | n/a | n/a | chunk deleted |
| Animated WebP EXIF chunk | n/a | n/a | not measured |
| AVIF Exif item | n/a | n/a | not measured |
| AVIF top-level uuid XMP | n/a | n/a | not measured |
| GIF Comment Extension | n/a | n/a | not measured |

The two mp4 carriers are exactly complementary, which is why both are written.

Discord's mp4 scrub is surgical rather than a transcode: the file size and
every byte offset are preserved, and only the moov/udta subtree changes. The
scrubber walks moov/udta and neutralises every child it finds. It does not walk
the top level, so a uuid box sitting beside moov passes through untouched.

Discord also strips PNG text chunks, so any PNG prompt reader operating on a
Discord re-download gets nothing. Pixels came back byte-identical on both test
images, so stealth-LSB is the only image carrier that still works through
Discord.

## Open measurement

Discord durability for animated WebP, AVIF and GIF is still open. Nothing in
the design depends on the answer, because these are export formats the user
chooses deliberately rather than the default gallery output, but the table
above stays honest about it until someone runs the round trip.

To close it, upload one probe of each format to Discord, download each back,
and check whether the payload marker still appears in the bytes. The still
WebP and PNG rows were measured exactly this way.

What the existing rows suggest, as inference rather than measurement:

- Animated WebP uses the same RIFF EXIF chunk as a still WebP, and Discord
  deletes that chunk on stills, so expect it to be deleted here too.
- AVIF is ISOBMFF like mp4. If Discord runs the same udta-style scrub, the
  Exif item is at risk and the top-level uuid box should survive, which is
  the mp4 result. If Discord instead re-encodes AVIF as it does for some
  image types, both are lost.
- GIF has no precedent in the table at all.

## Reader dispatch order

Container-native first, uuid second. Our writer emits both copies from the same
payload so they always agree. A third-party tool that edits metadata edits the
container-native copy, because that is what exiftool and ffmpeg touch, and
leaves a stale uuid behind. Canonical-first means someone else's edit wins over
our stale sidecar.

## Not covered

- No metadata opt-out exists for video, because none exists for images either.
- Existing gallery videos are not backfilled.
- Stealth-LSB does not apply to animated formats: lossy encoding destroys it.
- Alpha is not preserved. H264 in mp4 has no alpha channel, and while animated
  WebP and AVIF both support one, the video models feed RGB frames in.
- MetadataMode (text_chunk / stealth / both) has no video meaning. All three
  write the same thing.
