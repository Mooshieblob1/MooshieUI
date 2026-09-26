# Video generation methods

MiniMax H3 video generation offers **Standard** and **Turbo** under **Generation method**. These options choose one sampling path. Model quality tiers and first/last-frame or reference-image inputs remain separate settings.

| Method | Sampling | Setup |
|---|---|---|
| Standard | 20 steps; custom stacks can select their sampler and scheduler | Existing H3 model stack |
| Turbo: Larryvrh v4 | 4–8 steps, default 6; MiniMax H3 Turbo sampler, simple scheduler | Turbo node package and the configured Turbo LoRA |
| Turbo: LightX2V FL2V v1.2 | 4 steps, Euler/simple; video/audio shifts 6/3 | Curated FL2V adapter, native ComfyUI nodes |
| Turbo: LightX2V FL2V v1.0 | 8 steps, Euler/simple; video/audio shifts 6/3 | Curated FL2V adapter, native ComfyUI nodes |
| Turbo: LightX2V Ref2V v1.0 | 8 steps, Euler/simple; video/audio shifts 12/3 | Curated reference adapter, native ComfyUI nodes |
| Turbo: PDD FL2VA | 8 steps, Euler/simple; video/audio shifts 12/3 | Alibaba PAI adapter, native ComfyUI nodes, ComfyUI v0.35.0 or newer |
| Turbo: PDD Ref2VA | 8 steps, Euler/simple; video/audio shifts 12/3 | Alibaba PAI reference adapter, native ComfyUI nodes, ComfyUI v0.35.0 or newer |

Older saved Turbo settings migrate to the Turbo option. Switching methods preserves the Turbo step count and custom sampler/scheduler choices. Saved VDN selections migrate to Standard, even if an older Turbo flag is present. VDN is no longer selectable.

## LightX2V presets

Select **Turbo**, then choose a **Turbo preset**. Only presets for the current first/last-frame or reference variant appear. Switching variants maps a LightX2V preset to its matching eight-step preset; Larryvrh remains available for both. Each LightX2V download is about 1.96 GB, revision-pinned and SHA-256 checked. Its step count, Euler sampler, simple schedule and sigma shifts are fixed together. A custom Larryvrh adapter remains a separate saved setting.

LightX2V uses `LoraLoaderModelOnly` and `MiniMaxH3SigmaShift` from ComfyUI, including when the Director timeline drives generation. Install the adapter in `models/loras/` on the connected server; the built-in installer handles managed local installations. The publisher's current settings are in the [FL2V four-step](https://huggingface.co/lightx2v/Minimax-h3-Turbo/discussions/52), [FL2V eight-step](https://huggingface.co/lightx2v/Minimax-h3-Turbo/discussions/48), and [Ref2V eight-step](https://huggingface.co/lightx2v/Minimax-h3-Turbo/discussions/51) release notes.

## PDD presets

PDD (Parallel Decoding Distillation) adapters from [Alibaba PAI](https://huggingface.co/alibaba-pai/MiniMax-H3-Acc-LoRAs) replace the model's output layer with 32 per-interval heads. ComfyUI blends the heads each step spans, using the sampler's sigma schedule. The grid was built on the released 12/3 shifts, and eight Euler steps on the simple schedule land exactly on its block boundaries. The step count, sampler, schedule, shifts and full adapter strength are therefore fixed together. In the [ComfyUI pull request](https://github.com/comfyanonymous/ComfyUI/pull/15908) that added support, testers found eight PDD steps slightly behind 20 Standard steps.

The presets use the pruned conversions from [Kijai's repository](https://huggingface.co/Kijai/MiniMax-H3-experimental/tree/e042fe480f58806578713532b8ae4e3d47d1bd63/loras), which match the pruned DiT in every tier. Each download is about 1.73 GB, revision-pinned and SHA-256 checked. Switching variants maps a PDD preset to its PDD counterpart. The adapters load through `LoraLoaderModelOnly` and `MiniMaxH3SigmaShift`, including when the Director timeline drives generation. They need ComfyUI v0.35.0 or newer on the connected server; generation stops with a message on older servers instead of rendering noise.

TeaCache is skipped while a PDD preset is active, because it reuses the previous step's output and that output came from different heads. The TeaCache setting is kept and applies again with other methods. Upstream testing covered the pruned int8, fp8 and NVFP4 checkpoints. MooshieUI graph tests cover the wiring, not visual quality.

## Live preview

Turn on **Live preview** below TeaCache to watch a rough animated preview of the whole clip while it samples. ComfyUI's own video previewer decodes only the first latent frame, which in first/last-frame mode is the start image you supplied. The `MooshieH3LivePreview` node instead decodes each step's denoised estimate with [madebyollin's taeh3](https://github.com/madebyollin/taehv/tree/62f7591f59dfbb4c3c02b7a621d180a9eeaba26c) tiny autoencoder and sends it as an animated WebP. Previews are capped at 384 px on the long side, 12 fps and 72 frames, and play for the clip's real length.

The first time you turn it on, the managed installation downloads `taeh3.safetensors` (about 23 MB, revision-pinned and SHA-256 checked) into `models/vae_approx/`. No restart is needed. Remote servers need the same file in the same folder. Decoding runs on the sampling thread, and a step is skipped while the previous preview is still encoding, so a slow encode never holds up sampling. The final step is never previewed because the finished clip replaces it. Previews work with every generation method and with the Director timeline; they do not change the output.

Graph and CPU tests cover the wiring, the taeh3 decode and the WebP encoding. The per-step cost on a GPU has not been measured yet.

## Timeline stills in the middle of a clip

With **Use timeline** on in the first/last-frame workflow, a shot whose still starts partway through the clip is pinned at that shot's start frame with ComfyUI's `MiniMaxH3AddGuide` (ComfyUI v0.34.0 or newer). Before, those stills were dropped. Stills at the start and end remain the first and last keyframes, and a clip segment contributes its first frame. The anchor lives in the latent only, so the prompt does not name it as a picture: the text encoder never sees it. In the reference workflow middle stills stay `<Picture>` references, and a retake ignores them as before. On an older ComfyUI they are skipped with a warning in the ComfyUI log.

CPU tests cover the frame mapping and the conditioning with ComfyUI's own node. How closely H3 follows a mid-clip anchor has not been measured on a GPU.

## Concise motion prompts and Live2D

The video prompt enhancer keeps simple single-shot descriptions concise, usually 60–120 words or fewer. It preserves the required H3 fields, reference labels, real endpoint duration and supplied dialogue. Complex requested scenes can still use longer descriptions; short reference prompts are not rejected for their word count.

For an idle animation, type **Live2D this image** with your motion request. The idle guidance targets about 40–90 words: a brief visual anchor, gentle breathing, quick natural blinks where appropriate, a little secondary motion, a fixed camera and quiet ambience. It no longer prescribes slow eyelid cycles, exact blink schedules or independent movement for every visible detail. Explicitly requested closed eyes or different blink behavior are preserved.

Example motion brief:

> Keep the camera fixed on the seated character. Gentle breathing moves her shoulders slightly. She makes quick natural blinks with her eyes open between them, while a soft breeze sways her hair. Preserve the character, pose, framing and illustrated style. Quiet rustling leaves; no music.

A matching last frame is useful when the clip must return to that pose. With only a first frame, the enhancer does not invent a matching endpoint or promise a seamless loop. Review the result: concise guidance improved the measured example but did not fully solve blink instruction following. Previously cached H3 working notes are refreshed for the new guidance.

## Why VDN is no longer offered

The [15 September RTX 5070 comparison](research/video-benchmark-2026-09-15.md) found INT8 VDN slower than Standard and Turbo for the tested clips. It also recorded skipped adapter components on the pruned NVFP4 base and a native ComfyUI crash while streaming BF16 VDN weights. The VDN selector, precision/download controls and automatic readiness polling have been removed. Existing model files and historical metadata are retained; these runtime problems are not claimed fixed.

## Retained drafts and 2× refinement (experimental)

1. Enable **Keep draft for 2× refinement** before generating. Use about **0.5 MP or less**; the exact snapped dimensions must fit the 2.2 MP limit after doubling both axes. Updated MooshieUI nodes must be loaded on the connected ComfyUI server.
2. Generate and save the clip to the gallery. Manual-save mode retains the draft reference when you save the clip.
3. Open the clip's video player and select **Refine 2×**. The action appears only for clips with retained data. It shows the actual output dimensions and retained storage size.
4. Install the **59 MB H3 latent upscaler** if needed, then queue refinement. Defaults are eight steps and strength 0.35; supported controls are 4–20 steps and strength 0.15–0.60.
5. The result arrives as a new gallery clip. You can delete the retained data separately while keeping the original MP4.

This is a second H3 generation pass. It enlarges the clean video latent, resizes visual reference/keyframe conditioning, rebuilds the guider, then adds noise once and refines using an unaccelerated base model with Euler and a linear raw-sigma schedule. Original generated audio, or the Director's supplied audio track, is preserved. Temporal length and any selected interpolation settings are retained. A Turbo draft, or a previously retained VDN draft, can be the source; the second pass does not reapply its acceleration adapter.

The upscaler architecture is vendored from [Mamad8's MIT-licensed implementation](https://github.com/mamad8c/ComfyUI-H3-Latent-Upscaler-Mamad8/tree/e98237773011523528353a8beb4863e65b099a38). Its [pinned film checkpoint](https://huggingface.co/Tridae/H3LatentUpscaler/tree/5c87ab7cf8425a2cfbc3d21da1bffbd686ce67a6) installs to `models/h3_latent_upscalers/h3_clean_latent_upscaler_film_epoch200.safetensors` and is SHA-256 checked. Remote installations need this file and the bundled `mooshie-nodes` package, including `h3_drafts.py`, `h3_upscaler.py` and its license. No external stash/editor package is required.

### Storage and recovery

- Drafts live under the generating ComfyUI server's `output/mooshie_h3_drafts/<id>/`, using a bounded JSON manifest and safetensors. Up to 2 GiB of extra tensor data can be retained per clip. Storage is on that server, separately from gallery MP4 storage and its quota accounting.
- A small `<clip>.mp4.h3draft.json` gallery record associates the clip with that server and worker. Gallery moves and renames preserve the association. Imported MP4 metadata alone does not grant access to retained data.
- Refinement needs the original ComfyUI installation and base model/VAEs. If data or the worker is unavailable, reconnect or restore it. The UI reports missing data; an existing MP4 cannot reconstruct its original draft conditioning automatically.
- Deleting a retained clip also deletes its draft data. A queued refinement prevents deletion, and an unreachable original server must be reconnected before this cleanup can finish. **Delete retained data, keep clip** also removes a stale record when the server reports that its draft is already absent.
- Interrupted file writes use staging directories. Incomplete drafts older than a day are pruned during an availability check, at most hourly; successfully produced clips, including clips awaiting manual save, are preserved.

The full two-pass H3 render has not yet been qualified on a GPU. CPU tests verify the actual learned checkpoint, doubled dimensions, reference/mask handling and exact audio preservation. Quality, peak memory, long clips and live remote-server execution still need paired renders. Keep this opt-in and start with a short draft that comfortably fits your hardware.

## Performance and compatibility

Use the [measured Standard and Turbo results](research/video-benchmark-2026-09-15.md) as a starting point. Generation times and quality depend on the model stack, input, duration, resolution and hardware. The tested four-step and six-step Larryvrh runs were faster than Standard; extra steps did not fix the observed motion errors.

The workflow preserves first/last frames, references, timeline routing, generated audio and interpolation. Graph tests establish the connections; they do not establish visual quality or live compatibility for every model combination. Start with a short text-to-video clip, then compare references and timelines separately.

The advertised SGLang result used eight B200 GPUs and a warmed server. It is not a local generation-time estimate. [SGLang announcement](https://x.com/sgl_project/status/2099536367508402480)

The node implementation and checkpoint originate from [ComfyUI-VDN-H3](https://github.com/Saganaki22/ComfyUI-VDN-H3) and [OpenVDN](https://github.com/OpenVDN/vdn-minimax-h3). Checkpoint weights retain the [MiniMax H3 Community License](https://huggingface.co/OpenVDN/vdn-minimax-h3).

See [video improvement research](research/video-improvements.md) for source revisions, remaining candidates and the GPU validation plan.
