# macOS (Apple Silicon)

Native macOS installers are being qualified for the 2.3.1 release. Candidate
builds are experimental until the physical-Mac checks below have passed. Intel
Macs can use MooshieUI in a browser connected to a remote server; the native
local-generation runtime targets Apple Silicon.

## Installation

Use macOS 14 or later and download the `_aarch64.dmg` from the release or candidate
build. Compare its SHA-256 with the accompanying `SHA256SUMS` before installing:

```sh
shasum -a 256 MooshieUI_2.3.1_aarch64.dmg
```

The exact filename/version can differ for a candidate. Copy MooshieUI into
Applications and open it from Finder. This build uses free ad-hoc signing and
is **not notarized by Apple**. If macOS blocks it, try opening once, then use
System Settings > Privacy & Security > Open Anyway. See
[Apple's instructions](https://support.apple.com/en-us/102445).

The setup wizard installs ARM-native Python and ComfyUI. Select Apple Metal for
MPS acceleration, or CPU explicitly if acceleration is unavailable. Setup runs
an actual MPS tensor operation and reports an error if Metal cannot execute it.
Xcode command-line tools may be required for Git-based custom nodes or packages
without prebuilt wheels; a clean-machine test must establish the final installer
prerequisites. Keep model downloads and the runtime outside the `.app` bundle.

The managed runtime pins Python and the torch/torchvision/torchaudio combination
in `src-tauri/runtime/`. Custom-node installs and ComfyUI updates apply the same
torch constraints. Apple memory is shared with macOS, so setup uses ComfyUI's
normal memory management. Keep default attention and precision; CUDA-specific
Sage/Flash attention and FP8 launch overrides are not supported. Individual
models and optional node packs still need MPS testing.

## Candidate validation

The **macOS Native Validation** workflow builds on Apple Silicon, runs Rust tests,
checks bundle architecture and ad-hoc signing, verifies the DMG, installs the
pinned managed runtime, probes MPS, and verifies bundled ComfyUI node registration.
Its `macos-candidate` artifact contains the DMG, hashes, and diagnostic reports.
It runs on relevant PRs and can be dispatched manually without creating a tag.

On an Apple Silicon Mac, the runtime checks can also be run with:

```sh
python3 scripts/macos/setup_runtime.py \
  --uv /absolute/path/to/uv \
  --work-dir "$HOME/mooshie-mac-test" \
  --require-mps
```

Use a fresh directory. The hardware probe is only one part of qualification;
the downloaded app still needs all of these acceptance checks:

- Fresh browser download, checksum, Gatekeeper approval, Finder launch and setup.
- Baseline image and SDXL generation on MPS; record machine/OS, model hash,
  workflow/settings, dtype, elapsed time, output image and logs.
- Img2img, inpainting, LoRA, previews, cancel/retry, repeated generation and
  memory recovery within the machine's capacity.
- JXL gallery save/reopen, PNG metadata export, file dialogs, clipboard and drag/drop.
- Separate results for optional tagging, local prompt assistance, detailers,
  upscalers, video export and interpolation. Do not infer these from a basic image test.
- Quit/relaunch and both keep-alive modes, custom paths with spaces, setup repair,
  ComfyUI update, and a signed application update preserving user data.

Keep evidence tied to the tested source SHA and DMG checksum. A CPU node smoke
test on hosted CI does not qualify Metal generation or Finder behavior. The
current candidate workflow intentionally does not use the production updater
key on PRs, so the application-update test needs a controlled signed candidate.

## Publishing with a normal release

`MACOS_RELEASE_ENABLED` is an opt-in repository Actions variable. Leave it unset
until hardware acceptance evidence has been recorded and reviewed. Once enabled
(`true`), the existing release workflow also runs the Mac build, requires its
checks to pass, publishes the DMG and signed updater archive, and adds
`darwin-aarch64` to `latest.json`. Other platforms can release while Mac hardware
qualification is pending.

Apple signing is separate from Tauri updater signing: retain the existing
`TAURI_SIGNING_PRIVATE_KEY` and password secrets. No Apple developer membership
or Apple credentials are needed. Release assembly requires the expected version,
installer and nonempty updater signature and emits `SHA256SUMS` for all assets.
Manual release dispatch checks out its requested tag. Stage every platform before
publication; an immutable GitHub release may not accept assets added afterward.
