# macOS (Apple Silicon)

Starting with v2.3.9, every release ships a native Apple Silicon DMG alongside
the Windows and Linux builds, with automatic updates. The Mac build passes the
automated checks below but has not been through the full physical-Mac
acceptance list, so treat it as user-tested: please report anything that breaks.
Intel Macs can use MooshieUI in a browser connected to a remote server; the
native local-generation runtime targets Apple Silicon.

## Installation

Use macOS 14 or later. Download `MooshieUI_<version>_aarch64.dmg` from the
[latest release](https://github.com/Mooshieblob1/MooshieUI/releases/latest) and
compare its SHA-256 with the release's `SHA256SUMS` before installing:

```sh
shasum -a 256 MooshieUI_2.3.9_aarch64.dmg
```

Copy MooshieUI into
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

The initial ARM compatibility run registered all 26 required node classes and
passed the MPS tensor probe on macOS 15.7.9 with Python 3.11.14 and PyTorch 2.11.0.
[Runtime evidence](https://github.com/Mooshieblob1/MooshieUI/actions/runs/34410176260)
is tied to source `7f5e8ea`. DWPose reported CPU fallback in this run; accelerated
pose preprocessing is not qualified. These results do not establish model
generation, optional-feature or installed-app acceptance.

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

`MACOS_RELEASE_ENABLED` is an opt-in repository Actions variable, set to `true`
since v2.3.9: Mac builds ship as user-tested rather than waiting for the
hardware acceptance list above. With it enabled, the release workflow also runs
the Mac build, requires its checks to pass, publishes the DMG and signed updater
archive, and adds `darwin-aarch64` to `latest.json`. Unset it to release other
platforms without a Mac build.

Apple signing is separate from Tauri updater signing: retain the existing
`TAURI_SIGNING_PRIVATE_KEY` and password secrets. No Apple developer membership
or Apple credentials are needed. Release assembly requires the expected version,
installer and nonempty updater signature and emits `SHA256SUMS` for all assets.
Manual release dispatch checks out its requested tag. Stage every platform before
publication; an immutable GitHub release may not accept assets added afterward.
