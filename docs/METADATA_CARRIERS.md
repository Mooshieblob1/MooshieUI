# Metadata carriers

Where MooshieUI puts generation parameters in each output format, and what
survives which transport. The payload is always the same SwarmUI-shaped JSON
that `metadata::format_swarmui_json()` produces, so one reader handles all of
it.

## What is written

| Format | Container-native carrier | Top-level uuid XMP box | Written by |
|--------|--------------------------|------------------------|------------|
| PNG | iTXt / tEXt "parameters", optional stealth alpha | n/a | Rust |
| JXL | xml box | n/a | Rust |
| WebP (still and animated) | RIFF EXIF UserComment, optional stealth alpha | n/a | Rust for stills, Pillow for animated |
| MP4 | moov/udta/meta mdta key "comment" | yes | PyAV for the bytes, Rust for the box |
| AVIF | Exif item | yes | Pillow for the bytes, Rust for the box |
| GIF | Comment Extension | n/a | Pillow |

The uuid box uses Adobe's XMP identifier BE7ACFCB-97A9-42E8-9C71-999491E3AFAC
and is appended after every existing top-level box, so it moves no existing
byte and sample offsets stay valid.

## What survives

Measured, not assumed. Test payload was 1331 bytes of UTF-8 containing CJK
characters, quotes, and braces.

| Carrier | ffmpeg -c copy remux | full re-encode | Discord round trip |
|---------|----------------------|----------------|--------------------|
| MP4 moov/udta/meta "comment" | survives | survives | box renamed to skip, payload zeroed |
| MP4 moov/udta (c)cmt | survives | survives | box renamed to skip, payload zeroed |
| MP4 top-level uuid XMP | dropped | dropped | survives byte-identical |
| WebP EXIF chunk | n/a | n/a | chunk deleted |
| PNG iTXt chunk | n/a | n/a | chunk deleted |

The two mp4 carriers are exactly complementary, which is why both are written.

Discord's mp4 scrub is surgical rather than a transcode: the file size and
every byte offset are preserved, and only the moov/udta subtree changes. The
scrubber walks moov/udta and neutralises every child it finds. It does not walk
the top level, so a uuid box sitting beside moov passes through untouched.

Discord also strips PNG text chunks, so any PNG prompt reader operating on a
Discord re-download gets nothing. Pixels came back byte-identical on both test
images, so stealth-LSB is the only image carrier that still works through
Discord.

Discord durability for animated WebP, animated AVIF, and GIF is not yet
measured.

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
