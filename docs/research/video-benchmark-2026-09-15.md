# Vid Gen comparison on RTX 5070

Live local measurements from 15 September 2026. These qualify the tested configurations on one machine and one illustrated source image, not general model quality or support across GPUs.

## Environment and method

- Windows, RTX 5070 with 12 GB VRAM, 32 GB system RAM, driver 616.56.
- ComfyUI 0.35.0, PyTorch 2.12.0+cu130, Comfy Kitchen 0.2.33.
- ComfyUI uses SageAttention and BF16 VAE decoding with its normal dynamic memory management; `--lowvram` is not enabled.
- Pruned NVFP4 MiniMax H3 diffusion models for each task, the NVFP4-AWQ text encoder, FP16 video VAE, and FP32 audio VAE. VDN precision refers to its additional branch, not the base model.
- VDN node revision `3eb63496c24ca70faaf8a14b6c75fcb480e34bf1`. The installed LightX2V and VDN weight files passed their pinned size/hash checks.
- Main comparison: 544×704, 124 native frames at 24 fps (5.17 seconds), seed `20260915`. First/last-frame runs use the same image at both endpoints. Reference-image runs use the separate Ref2VA model and matching reference preset.
- Each method uses the recipe documented in [VIDEO.md](../VIDEO.md). TeaCache and frame interpolation are off. Output uses H.264 with CRF 18.
- Workflows derive from a captured application job, with method-specific wiring checked against the Rust templates, and run directly through the connected ComfyUI API. Native `SaveVideo` keeps benchmark clips separate from the app gallery. This exercises real GPU generation, not the full sequence of UI controls and gallery operations.
- One measured run per configuration. Conditioning can be cached between runs. Total time includes loading, conditioning, sampling, decoding, and saving; the sampling phase is also recorded and includes model preparation performed inside the sampler.
- Peak GPU memory is sampled every five seconds and includes other desktop use. It is not a per-process allocator peak.
- Quality observations use sampled video frames. Audio streams were checked for presence, duration, and nonzero signal; perceptual audio quality has not been graded.

Private workflow JSON, logs, MP4s, frame sheets, measurements, and a playable comparison page are stored in the ignored `error-logs/video-benchmark/` directory. The source image and prompt are not included in this document.

Completed: 18 generation attempts (17 successful clips and one native crash), plus one successful interpolation check. The user's earlier VDN clip is included separately as a reference, not counted as a new test.

## Main comparison

| Method | First/last-frame total | Sampling phase | Reference-image total | Sampling phase |
|---|---:|---:|---:|---:|
| Standard, 20 steps | 2m 17s | 1m 49s | 2m 29s | 1m 44s |
| Larryvrh Turbo, 4 steps | 55s | 29s | Not tested | Not tested |
| Larryvrh Turbo, 6 steps | 1m 07s | 41s | 1m 05s | 38s |
| Larryvrh Turbo, 8 steps | 1m 21s | 53s | Not tested | Not tested |
| LightX2V FL2V v1.2, 4 steps | 1m 09s | 43s | Different task | Different task |
| LightX2V FL2V v1.0, 8 steps | 1m 41s | 1m 16s | Different task | Different task |
| LightX2V Ref2V v1.0, 8 steps | Different task | Different task | 1m 36s | 1m 11s |
| VDN INT8, 8 steps | 3m 40s | 2m 48s | 2m 49s | 2m 18s |
| VDN BF16, 8 steps | Native process crash | No completed step | Not retested | Not retested |

Standard's conditioning was cached in the first/last-frame comparison, while the first VDN run included conditioning. Even when comparing only their sampling phases, INT8 VDN was about 55% slower than Standard for that case. It was also slower in the reference-image comparison.

Larryvrh produced more visible hair motion and clear blinks in the sampled frames. LightX2V and VDN were more restrained for this particular prompt. These observations do not establish a universal quality ranking.

## Eight-second follow-ups

The longer comparison uses 672×864 and 192 native frames at 24 fps (eight seconds). The first four rows use the same first/last image, motion prompt, and seed. Frame interpolation is off.

| Method | Total | Sampling phase | Video VAE decode |
|---|---:|---:|---:|
| Larryvrh Turbo, 4 steps | 2m 37s | 1m 23s | 50s |
| Larryvrh Turbo, 6 steps | 2m 58s | 1m 59s | 51s |
| LightX2V FL2V v1.0, 8 steps | 4m 01s | 2m 55s | 52s |
| Standard, 20 steps | 6m 37s | 5m 41s | 49s |

The original eight-second INT8 VDN job took 8m 53s, including RIFE 2× interpolation. Its console records approximately 7m 33s in the sampling progress bar. That original job is useful context, but its interpolation and timing capture differ from the controlled comparison above.

Turbo's four-step and six-step total times were approximately 2.5× and 2.2× faster than Standard. Reducing steps does not reduce VAE decode time: this stage still costs about 50 seconds at the larger size.

### Motion and prompt follow-ups

The longer prompt requested slow blinking and strongly constrained the pose. Standard, Larryvrh and LightX2V all showed extended closed-eye intervals in sampled frames. Increasing Larryvrh from four to six steps did not correct this behavior.

| Larryvrh six-step input | Total | Sampling phase | Observation |
|---|---:|---:|---|
| Same first and last image, longer prompt | 2m 58s | 1m 59s | Prolonged closed eyes; returns toward the source pose |
| First image only, longer prompt | 2m 59s | 1m 50s | Larger hair movement and final-pose drift; prolonged eye closure remains |
| First image only, shorter prompt | 2m 55s | 1m 47s | Shorter eye closure and more head movement; still misses the requested quick-blink sequence |

Removing the last-frame constraint allowed more motion but did not fix the eye behavior. Simplifying the prompt and explicitly requesting open eyes and quick natural blinks reduced the closed-eye interval from roughly five seconds to roughly two seconds in quarter-second frame samples. It still did not produce the requested two quick blinks. This is partial improvement, not a solved quality issue. An open endpoint also no longer guarantees a loop matching the source pose.

### Frame interpolation

A separate RIFE 2× pass on the shorter-prompt clip took **28.47 seconds**, including video loading and saving. It produced 383 frames at 48 fps (7.979 seconds), with the eight-second audio stream retained and nonzero. The sampled output frames preserve the source clip's composition; playback smoothness and perceptual audio quality were not graded.

This pass loads and re-encodes an already saved MP4. Its duration is a postprocessing measurement, not a full generation time or an exact measure of the application's integrated interpolation overhead. It creates intermediate frames; it does not fix incorrect generated actions.

## Confirmed VDN limitations

### Pruned base adapter handling

The console reports that 51 AdaLN adapter components are skipped when VDN uses `lora_mode=merge` on this pruned NVFP4 base. The [current upstream application code](https://github.com/Saganaki22/ComfyUI-VDN-H3/blob/3eb63496c24ca70faaf8a14b6c75fcb480e34bf1/vdn_h3/apply.py) explicitly excludes these components. This confirms incomplete adapter application; it does not isolate how much that contributes to the observed quality.

Changing all adapters to bypass is not an established fix: the [port's documentation](https://github.com/Saganaki22/ComfyUI-VDN-H3#nodes) reports degraded eight-step DMD output with that mode. No production adapter-mode change was made during this comparison.

### BF16 streaming crash

The BF16 test terminated ComfyUI with `Windows fatal exception: access violation` during model initialization, before the first sampling step completed. The stack reaches `torch.storage.__getitem__` through `LazyBranchTensor.resolve()` in `vdn_h3/spec.py` while fetching streamed branch weights. The checkpoint had passed its checksum. This is a recorded failure in this environment; the underlying native-memory fault has not been isolated.

ComfyUI was restarted through the application's lifecycle command, and the remaining tests continued successfully. The failed BF16 run is not assigned a render time or a quality score.

## Practical interpretation

VDN is not a speed upgrade for the tested workloads on this machine. Its live logs select streamed branch weights and transient buffers, and its compatibility wrapper disables the ComfyUI model compiler while it samples. Its window attention also follows a different path from the accelerated attention used by Standard/Turbo. The [upstream performance discussion](https://github.com/Saganaki22/ComfyUI-VDN-H3#vram-and-performance) distinguishes this consumer ComfyUI implementation from the multi-GPU datacenter results.

For this scene, use Larryvrh at four steps for quick previews and six steps as a practical starting point for a longer render. The six-step setting is not a proven quality winner over four steps on this seed. Extra sampling steps alone did not fix motion instruction following. Use a first-frame-only setup when a matching endpoint is unnecessary, keep the motion prompt concise, and review the result before adding interpolation.

These tests cover first/last-frame and reference-image generation, plus prompt variants. They do not qualify text-only generation, retained-draft 2× refinement, every resolution or seed, or other hardware. No production generation defaults or VDN adapter code were changed as a result of this benchmark.
