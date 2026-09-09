# macOS release readiness for 2.3.1

Status: implementation in progress, 2026-09-10. Native CI, managed runtime pins,
MPS setup verification, packaging, updater collection and publication gating are
implemented. Local build gates pass; the first native CI run is pending;
physical-Mac qualification remains outstanding. See [MACOS.md](MACOS.md) for the
current candidate and release procedure. Versions remain 2.3.0 until the normal
2.3.1 release process.

## Proposed support scope

Ship a native Apple Silicon (`aarch64-apple-darwin`) DMG, with free ad-hoc code
signing and no Apple notarization. Users must explicitly allow the app through
Gatekeeper. Tauri supports this through `bundle.macOS.signingIdentity: "-"`;
ad-hoc signing does not provide an Apple-authenticated developer identity.
See [Tauri's signing documentation](https://v2.tauri.app/distribute/sign/macos/).

Use macOS 14 as the provisional minimum, then confirm the minimum against every
pinned runtime and the oldest OS actually tested. Apple's current PyTorch guide
requires Apple Silicon and macOS 14 or later for PyTorch 2.11.0. A 16 GB Mac is a
useful initial test target; this is a proposed test baseline, not a measured
minimum memory requirement for all models.
See [Apple's PyTorch requirements](https://developer.apple.com/metal/pytorch/).

Keep Intel desktop support outside the initial local-generation support claim.
PyTorch stopped publishing macOS x86_64 binaries starting with 2.3.0. An Intel
client for remote ComfyUI is a separate possible deliverable; a universal app
alone would not solve its Python/PyTorch dependency constraints.
See [PyTorch's deprecation announcement](https://dev-discuss.pytorch.org/t/pytorch-macos-x86-builds-deprecation-starting-january-2024/1690).

## Initial audit (before implementation)

This table records the starting point. The implementation described above and in
[MACOS.md](MACOS.md) addresses the build, runtime and packaging work; hardware
qualification is tracked separately below.

| Area | Observed in the repository | Required work |
| --- | --- | --- |
| Desktop release | [release.yml](../.github/workflows/release.yml) builds Windows and Linux only. Linux bundle steps use `runner.os != 'Windows'`. | Add a native ARM macOS build and make Linux conditions explicitly Linux. Produce a DMG and updater archive. |
| Release upload | Asset collection excludes DMGs and macOS updater archives; `latest.json` contains only Windows and Linux. | Collect `.dmg`, `.app.tar.gz`, and `.app.tar.gz.sig`; add `darwin-aarch64`; fail if required assets or signatures are missing. Preserve architecture in asset names, especially if Intel is added later. |
| Release ref | Manual dispatch accepts a tag, but checkout does not explicitly use that input. | Build and collect notes from the requested release ref; verify all three version files and record the exact source SHA. |
| App bundle | [tauri.conf.json](../src-tauri/tauri.conf.json) includes an ICNS icon and macOS configuration, with minimum OS 10.15 and no explicit signing identity. | Configure ad-hoc signing, reconcile the OS minimum with runtime requirements, and validate the installed bundle and its permissions. |
| Bootstrap | [setup.rs](../src-tauri/src/setup.rs) already selects ARM macOS uv, installs managed Python, uses Unix venv paths, and installs torch from PyPI for MPS. | Assert the app, uv, Python, and native wheels are ARM-native. Pin a tested Python/torch/torchvision/torchaudio combination and constrain subsequent dependency installs so they preserve it. |
| GPU detection | Setup currently returns `mps` for every macOS host. [process.rs](../src-tauri/src/comfyui/process.rs) checks accelerator availability and can fall back to CPU. | Distinguish platform eligibility from actual MPS availability; report CPU fallback clearly. A successful CPU generation does not satisfy the Metal release gate. |
| Memory | Setup parses display-profiler output as dedicated VRAM and uses shared VRAM thresholds. | Report Apple unified memory appropriately and keep conservative ComfyUI defaults. Do not treat total system memory as fully available GPU memory. |
| Attention and models | Setup UI restricts optional attention choices, but the backend installation path accepts non-default choices without a GPU check. Custom loaders also expose FP8 options. | Enforce backend compatibility in Rust as well as the UI. Use default attention initially; validate precision, quantization, and optional node compatibility individually on MPS. |
| Optional native tools | [interrogator.rs](../src-tauri/src/interrogator.rs) selects macOS ARM ONNX Runtime; [prompt_assistant/server.rs](../src-tauri/src/prompt_assistant/server.rs) selects macOS ARM llama.cpp and Metal. | Test actual downloads, executable modes, dynamic library loading, tagging, prompt assistance, and video codecs in the installed app. |
| ComfyUI compatibility | [comfyui-compat.yml](../.github/workflows/comfyui-compat.yml) tests node registration on Linux CPU with Python 3.12. Setup normally installs Python 3.11. | Add macOS ARM installation/import coverage using the production runtime versions. Extend compatibility validation to preserve the Mac support claim when ComfyUI is updated. |

The current ComfyUI pin is `v0.34.0` in
[comfyui_version.rs](../src-tauri/src/comfyui_version.rs). Its
[device management](https://github.com/Comfy-Org/ComfyUI/blob/v0.34.0/comfy/model_management.py)
already selects `torch.device("mps")` and shared-memory handling when available.
MooshieUI should use this existing backend and validate the resulting behavior.

## Build and validation gates

Run candidate builds before tagging 2.3.1. A dedicated macOS validation workflow
can produce downloadable test artifacts without publishing a release. An explicit
ARM runner such as `macos-15` is available; standard hosted runners are free for
public repositories under
[GitHub's runner policy](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

- [ ] Run the frontend build, i18n parity check, and direct Svelte check; retain
  diagnostics from any existing type errors. The current `check:types` script
  masks its exit status and cannot establish a passing gate by itself.
- [ ] Run Rust formatting, compile/lint checks, and the existing Rust tests on
  macOS. Preserve the existing Windows/Linux desktop and Linux server gates.
- [ ] Build the release app and DMG for `aarch64-apple-darwin`; verify architecture,
  bundle metadata, ad-hoc signature integrity, and the DMG's integrity. Gatekeeper
  rejection of an unnotarized candidate is expected and is separate from a broken
  code signature.
- [ ] Exercise production-equivalent setup in a clean directory: managed Python,
  locked torch versions, pinned ComfyUI, bundled nodes, imports, `/system_stats`,
  and all required `/object_info` classes. Archive the resolved dependency list.
- [ ] Retain build logs, source SHA, architecture, OS, dependency versions, and
  SHA-256 hashes with the candidate artifacts.

Hosted CI is useful for these checks, but its result is not sufficient evidence
of working local generation. A
[runner issue](https://github.com/actions/runner-images/issues/11899)
documents MPS allocation failures on hosted Macs; this is historical evidence,
not a claim that every current runner lacks Metal. Probe the selected runner,
and require an end-user Mac test regardless.

The real-Mac acceptance gate needs an Apple Silicon tester; availability is still
to be confirmed. Test the downloaded candidate DMG, rather than only a developer
build or an app launched from a shell:

- [ ] Download through a browser, verify its published hash, install in
  `/Applications`, handle Gatekeeper, and launch from Finder.
- [ ] Complete first-run setup in a clean user environment. Identify any required
  Xcode command-line tools explicitly; check that a developer's Homebrew, Git,
  Python, or shell PATH has not hidden missing prerequisites. Test paths with
  spaces, custom data/model directories, and macOS file permission prompts.
- [ ] Confirm ARM-native Python and `torch.backends.mps.is_available()`, then
  execute a tensor operation on MPS and verify ComfyUI reports MPS during real
  generation. Capture torch version, dtype, device, and logs.
- [ ] Generate with an agreed small baseline checkpoint and SDXL, within the
  machine's memory capacity. Record model hashes, workflow/settings, elapsed
  time, and outputs; inspect outputs for blank images and numerical failures.
- [ ] Exercise img2img, inpainting, a LoRA, preview streaming, queue cancellation,
  repeated generation, and memory recovery. Measure memory pressure before
  recommending model sizes or memory limits.
- [ ] Save and reopen JXL gallery images, export PNG with metadata, and check
  file dialogs, clipboard, and drag/drop. Check browser mode if advertised.
- [ ] Test optional upscaling/detailing, tagging, prompt assistance, video export,
  and interpolation before claiming each is supported. Record limitations or
  gate unsupported paths instead of promising all model/node combinations.
- [ ] Quit and relaunch; exercise both keep-alive settings. Test setup repair and
  a controlled ComfyUI update without replacing the validated dependency stack.
- [ ] Perform a signed Tauri update from an earlier candidate version to the
  final candidate; confirm relaunch, app signature integrity, and preserved
  settings/gallery/ComfyUI data.

Use safe precision defaults supported by the tested model and torch/OS pair.
CPU fallback for an unsupported operation can be considered after reproducing
the issue; do not enable it as a substitute for confirming GPU execution. Keep
MPS allocator protections at their defaults during initial qualification.

## Distribution and release decision

Publish a DMG, the Tauri updater `.app.tar.gz` and `.sig`, and SHA-256 checksums.
Tauri's updater signing key is independent of Apple signing and remains required;
the existing key can authenticate Mac updates too. Use `darwin-aarch64` for the
manifest entry and test the update end to end. See
[Tauri's updater documentation](https://v2.tauri.app/plugin/updater/).

Document first launch as: verify the downloaded file, copy the app to
Applications, try opening it, then use System Settings > Privacy & Security >
Open Anyway when macOS blocks the unnotarized app. Link
[Apple's instructions](https://support.apple.com/en-us/102445).
Explain that checksum verification checks the downloaded bytes against the
published release; it is not Apple notarization.

Update the README and generated release download table to state architecture,
minimum macOS version, first-run requirements, and tested feature limitations.
Publish Mac assets with 2.3.1 only after the gates above pass. If hardware testing
is still unavailable, distribute candidate artifacts with an explicit
experimental status and keep validated Mac support pending.

Keep evidence tied to the final release source and artifacts; revalidate changes
made after testing. Stage all validated platform assets before release publication
where possible. The existing workflow anticipates immutable releases, so adding
Mac assets after publication must not be assumed to work.
