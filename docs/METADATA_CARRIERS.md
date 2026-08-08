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
