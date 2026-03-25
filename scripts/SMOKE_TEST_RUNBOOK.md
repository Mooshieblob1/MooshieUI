# Phase 0 — Smoke Test Runbook (DGX Spark)

Run this on the DGX Spark over SSH. Everything below is self-contained.

---

## 1. SSH in

```bash
ssh <user>@<spark-ip>
```

## 2. Pick a working directory

```bash
mkdir -p ~/anima-smoke && cd ~/anima-smoke
```

## 3. Create a venv

```bash
python3 -m venv .venv
source .venv/bin/activate
```

## 4. Install dependencies

```bash
pip install --upgrade pip
pip install torch diffusers transformers accelerate safetensors huggingface_hub Pillow numpy
```

> If the Spark needs a specific PyTorch build (e.g. CUDA 12.x nightly for Blackwell), install that first:
> ```bash
> pip install --pre torch --index-url https://download.pytorch.org/whl/nightly/cu126
> ```

## 5. (Optional) Log in to Hugging Face

Only needed if the repo is gated or you hit rate limits:

```bash
pip install huggingface_hub[cli]
huggingface-cli login
```

## 6. Copy the smoke test script

Paste the entire script below into `smoke_test.py`, or `scp` it from your local machine:

```bash
# Option A — scp from local
scp <local-machine>:~/Repos/DesktopWebUI/comfyui-desktop/scripts/smoke_test.py .

# Option B — paste inline
cat > smoke_test.py << 'PYEOF'
#!/usr/bin/env python3
"""
Phase 0 — 5-image Diffusers smoke test for Anima-preview-2.
"""

import argparse
import hashlib
import json
import logging
import re
import sys
import time
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import torch
from PIL import Image

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

HF_REPO = "circlestone-labs/Anima"

UNET_FILENAME = "split_files/diffusion_models/anima-preview2.safetensors"
CLIP_FILENAME = "split_files/text_encoders/qwen_3_06b_base.safetensors"
VAE_FILENAME = "split_files/vae/qwen_image_vae.safetensors"

WIDTH = 896
HEIGHT = 1152
STEPS = 30
CFG = 4.0
SEED = 42
SAMPLER = "er_sde"
SCHEDULER = "sgm_uniform"

POSITIVE_TEMPLATE = (
    "{artist_tag}, year 2025, newest, masterpiece, best quality, "
    "score_8_up, score_9_up, safe, highres, chen, "
    "1girl, flat chest, smile, waving, upper body"
)
NEGATIVE_PROMPT = (
    "worst quality, low quality, score_1, score_2, score_3, "
    "blurry, jpeg artifacts, sepia"
)

THUMB_WIDTH = 540
THUMB_HEIGHT = 720

TEST_ARTISTS = [
    "@dairi",
    "@ebifurya",
    "@hammer_(sunset_beach)",
    "@ruu_(tksymkw)",
    "@haruyama_kazunori",
]

# ---------------------------------------------------------------------------
# Ledger
# ---------------------------------------------------------------------------

@dataclass
class LedgerEntry:
    artist_tag: str
    slug: str
    status: str = "pending"
    error: str = ""
    prompt: str = ""
    seed: int = SEED
    original_path: str = ""
    thumb_path: str = ""
    thumb_sha256: str = ""
    generation_time_s: float = 0.0
    has_nan: bool = False
    pixel_stats: dict = field(default_factory=dict)
    started_at: str = ""
    completed_at: str = ""


def safe_slug(tag: str) -> str:
    s = tag.lstrip("@").lower()
    s = re.sub(r"[^a-z0-9]+", "_", s)
    return s.strip("_")


# ---------------------------------------------------------------------------
# Scheduler resolution
# ---------------------------------------------------------------------------

def resolve_scheduler(pipe):
    from diffusers import DPMSolverSDEScheduler, EulerDiscreteScheduler

    scheduler_config = dict(pipe.scheduler.config)
    attempted = []

    try:
        scheduler_config["timestep_spacing"] = "trailing"
        sched = EulerDiscreteScheduler.from_config(scheduler_config)
        pipe.scheduler = sched
        attempted.append(("EulerDiscreteScheduler(trailing)", True))
        return pipe, attempted
    except Exception as e:
        attempted.append(("EulerDiscreteScheduler(trailing)", False, str(e)))

    try:
        sched = DPMSolverSDEScheduler.from_config(scheduler_config)
        pipe.scheduler = sched
        attempted.append(("DPMSolverSDEScheduler", True))
        return pipe, attempted
    except Exception as e:
        attempted.append(("DPMSolverSDEScheduler", False, str(e)))

    attempted.append(("default (kept as-is)", True))
    return pipe, attempted


# ---------------------------------------------------------------------------
# Model loading strategies
# ---------------------------------------------------------------------------

log = logging.getLogger("smoke_test")


def strategy_wan_pipeline(hf_repo, device, dtype):
    from diffusers import WanPipeline
    log.info("Trying WanPipeline.from_pretrained(%s)...", hf_repo)
    pipe = WanPipeline.from_pretrained(hf_repo, torch_dtype=dtype)
    pipe = pipe.to(device)
    return pipe


def strategy_auto_pipeline(hf_repo, device, dtype):
    from diffusers import AutoPipelineForText2Image
    log.info("Trying AutoPipelineForText2Image.from_pretrained(%s)...", hf_repo)
    pipe = AutoPipelineForText2Image.from_pretrained(hf_repo, torch_dtype=dtype)
    pipe = pipe.to(device)
    return pipe


def strategy_diffusion_pipeline(hf_repo, device, dtype):
    from diffusers import DiffusionPipeline
    log.info("Trying DiffusionPipeline.from_pretrained(%s)...", hf_repo)
    pipe = DiffusionPipeline.from_pretrained(hf_repo, torch_dtype=dtype)
    pipe = pipe.to(device)
    return pipe


LOAD_STRATEGIES = [
    ("WanPipeline", strategy_wan_pipeline),
    ("AutoPipelineForText2Image", strategy_auto_pipeline),
    ("DiffusionPipeline", strategy_diffusion_pipeline),
]


def load_model(hf_repo, device, dtype):
    attempts = []
    for name, fn in LOAD_STRATEGIES:
        try:
            pipe = fn(hf_repo, device, dtype)
            attempts.append({"strategy": name, "success": True})
            log.info("Loaded with %s", name)
            return pipe, attempts
        except Exception as e:
            msg = f"{type(e).__name__}: {e}"
            attempts.append({"strategy": name, "success": False, "error": msg})
            log.warning("%s failed: %s", name, msg)
    return None, attempts


# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------

def validate_image(img):
    arr = np.array(img, dtype=np.float32)
    has_nan = bool(np.isnan(arr).any())
    has_inf = bool(np.isinf(arr).any())
    stats = {
        "min": float(arr.min()), "max": float(arr.max()),
        "mean": float(arr.mean()), "std": float(arr.std()),
        "has_nan": has_nan, "has_inf": has_inf, "shape": list(arr.shape),
    }
    is_valid = not has_nan and not has_inf and arr.max() > 0
    return is_valid, stats


def downscale_to_thumb(img, w, h):
    return img.resize((w, h), Image.LANCZOS)


def sha256_file(path):
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()


# ---------------------------------------------------------------------------
# Generation
# ---------------------------------------------------------------------------

def generate_one(pipe, artist_tag, output_dir, device):
    slug = safe_slug(artist_tag)
    entry = LedgerEntry(artist_tag=artist_tag, slug=slug)
    entry.prompt = POSITIVE_TEMPLATE.format(artist_tag=artist_tag)
    entry.status = "running"
    entry.started_at = datetime.now(timezone.utc).isoformat()

    log.info("Generating: %s  (slug=%s)", artist_tag, slug)
    log.info("  Prompt: %s", entry.prompt)

    generator = torch.Generator(device=device).manual_seed(SEED)

    try:
        t0 = time.perf_counter()
        result = pipe(
            prompt=entry.prompt,
            negative_prompt=NEGATIVE_PROMPT,
            width=WIDTH, height=HEIGHT,
            num_inference_steps=STEPS,
            guidance_scale=CFG,
            generator=generator,
        )
        entry.generation_time_s = round(time.perf_counter() - t0, 2)
    except Exception as e:
        entry.status = "failed"
        entry.error = f"Generation error: {type(e).__name__}: {e}"
        entry.completed_at = datetime.now(timezone.utc).isoformat()
        log.error("  FAILED generation: %s", entry.error)
        return entry

    img = result.images[0]

    is_valid, stats = validate_image(img)
    entry.pixel_stats = stats
    entry.has_nan = stats["has_nan"]

    if not is_valid:
        entry.status = "failed"
        entry.error = f"Validation failed: NaN={stats['has_nan']}, Inf={stats['has_inf']}, max={stats['max']}"
        entry.completed_at = datetime.now(timezone.utc).isoformat()
        log.error("  FAILED validation: %s", entry.error)
        return entry

    orig_path = output_dir / f"{slug}_896x1152.png"
    img.save(orig_path)
    entry.original_path = str(orig_path)
    log.info("  Saved original: %s", orig_path)

    try:
        thumb = downscale_to_thumb(img, THUMB_WIDTH, THUMB_HEIGHT)
        thumb_path = output_dir / f"{slug}_540x720.webp"
        thumb.save(thumb_path, "WEBP", quality=90)
        entry.thumb_path = str(thumb_path)
        entry.thumb_sha256 = sha256_file(thumb_path)
        log.info("  Saved thumbnail: %s (%s bytes)", thumb_path, thumb_path.stat().st_size)
    except Exception as e:
        entry.status = "failed"
        entry.error = f"Downscale/WebP error: {type(e).__name__}: {e}"
        entry.completed_at = datetime.now(timezone.utc).isoformat()
        log.error("  FAILED downscale: %s", entry.error)
        return entry

    entry.status = "succeeded"
    entry.completed_at = datetime.now(timezone.utc).isoformat()
    log.info("  Succeeded in %.1fs", entry.generation_time_s)
    return entry


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Phase 0 smoke test")
    parser.add_argument("--output-dir", type=Path, default=Path("smoke_output"))
    parser.add_argument("--hf-repo", type=str, default=HF_REPO)
    parser.add_argument("--device", type=str, default=None)
    parser.add_argument("--dtype", type=str, default="bf16", choices=["bf16", "fp16", "fp32"])
    args = parser.parse_args()

    logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s", datefmt="%H:%M:%S")

    if args.device:
        device = args.device
    elif torch.cuda.is_available():
        device = "cuda"
    else:
        device = "cpu"
        log.warning("No CUDA detected — running on CPU (will be very slow)")

    dtype_map = {"bf16": torch.bfloat16, "fp16": torch.float16, "fp32": torch.float32}
    dtype = dtype_map[args.dtype]

    log.info("=" * 60)
    log.info("Phase 0 Smoke Test — Anima-preview-2")
    log.info("=" * 60)
    log.info("Device: %s | Dtype: %s", device, args.dtype)
    log.info("Resolution: %dx%d | Steps: %d | CFG: %.1f", WIDTH, HEIGHT, STEPS, CFG)
    log.info("Sampler: %s | Scheduler: %s", SAMPLER, SCHEDULER)
    log.info("Artists: %s", ", ".join(TEST_ARTISTS))
    log.info("Output: %s", args.output_dir.resolve())
    log.info("-" * 60)

    args.output_dir.mkdir(parents=True, exist_ok=True)

    log.info("Loading model from %s ...", args.hf_repo)
    t_load_start = time.perf_counter()
    pipe, load_attempts = load_model(args.hf_repo, device, dtype)
    t_load = time.perf_counter() - t_load_start

    if pipe is None:
        log.error("=" * 60)
        log.error("SMOKE TEST FAILED — Could not load model with any strategy")
        log.error("=" * 60)
        for a in load_attempts:
            log.error("  %s: %s", a["strategy"], a.get("error", ""))
        log.error("RECOMMENDATION: Fall back to ComfyUI headless API.")
        report = {
            "status": "FAILED", "reason": "model_load_failure",
            "load_attempts": load_attempts,
            "timestamp": datetime.now(timezone.utc).isoformat(),
        }
        (args.output_dir / "smoke_report.json").write_text(json.dumps(report, indent=2))
        sys.exit(1)

    log.info("Model loaded in %.1fs", t_load)

    pipe, sched_attempts = resolve_scheduler(pipe)
    log.info("Scheduler resolution: %s", sched_attempts)

    ledger = []
    total_gen_time = 0.0

    for i, artist in enumerate(TEST_ARTISTS, 1):
        log.info("-" * 60)
        log.info("[%d/%d] %s", i, len(TEST_ARTISTS), artist)
        entry = generate_one(pipe, artist, args.output_dir, device)
        ledger.append(entry)
        if entry.status == "succeeded":
            total_gen_time += entry.generation_time_s

    succeeded = [e for e in ledger if e.status == "succeeded"]
    failed = [e for e in ledger if e.status == "failed"]

    log.info("=" * 60)
    log.info("SMOKE TEST RESULTS")
    log.info("=" * 60)
    log.info("Succeeded: %d / %d", len(succeeded), len(ledger))
    log.info("Failed:    %d / %d", len(failed), len(ledger))

    if succeeded:
        avg_time = total_gen_time / len(succeeded)
        log.info("Avg generation time: %.1fs", avg_time)
        log.info("Total generation time: %.1fs", total_gen_time)

    for e in failed:
        log.error("  FAILED: %s — %s", e.artist_tag, e.error)

    all_passed = len(succeeded) == len(TEST_ARTISTS)

    if all_passed:
        log.info("")
        log.info("SMOKE TEST PASSED")
        log.info("  All 5 images generated, validated, and downscaled successfully.")
        log.info("  Model load time: %.1fs", t_load)
        log.info("  The Diffusers path is viable. Proceed to top-500 pilot.")
    else:
        log.error("")
        log.error("SMOKE TEST FAILED")
        log.error("  %d of %d images failed.", len(failed), len(TEST_ARTISTS))
        if len(failed) == len(TEST_ARTISTS):
            log.error("  RECOMMENDATION: Fall back to ComfyUI headless API.")
        else:
            log.error("  RECOMMENDATION: Investigate per-artist failures before deciding.")

    report = {
        "status": "PASSED" if all_passed else "FAILED",
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "device": device, "dtype": args.dtype,
        "model_repo": args.hf_repo,
        "model_load_time_s": round(t_load, 2),
        "load_attempts": load_attempts,
        "scheduler_attempts": [
            {"name": a[0], "success": a[1]} if len(a) == 2
            else {"name": a[0], "success": a[1], "error": a[2]}
            for a in sched_attempts
        ],
        "parameters": {
            "width": WIDTH, "height": HEIGHT, "steps": STEPS,
            "cfg": CFG, "seed": SEED, "sampler": SAMPLER,
            "scheduler": SCHEDULER, "positive_template": POSITIVE_TEMPLATE,
            "negative_prompt": NEGATIVE_PROMPT,
        },
        "results": [asdict(e) for e in ledger],
        "summary": {
            "total": len(ledger), "succeeded": len(succeeded),
            "failed": len(failed),
            "total_generation_time_s": round(total_gen_time, 2),
            "avg_generation_time_s": round(total_gen_time / len(succeeded), 2) if succeeded else None,
        },
    }

    (args.output_dir / "smoke_report.json").write_text(json.dumps(report, indent=2))
    (args.output_dir / "smoke_ledger.json").write_text(json.dumps([asdict(e) for e in ledger], indent=2))
    log.info("Report: %s/smoke_report.json", args.output_dir)
    log.info("Ledger: %s/smoke_ledger.json", args.output_dir)

    sys.exit(0 if all_passed else 1)


if __name__ == "__main__":
    main()
PYEOF
chmod +x smoke_test.py
```

## 7. Run the smoke test

```bash
python smoke_test.py --output-dir ./smoke_output --dtype bf16
```

That's it. The script handles everything: model download, loading, generation, validation, downscaling, and reporting.

### Flags

| Flag | Default | Notes |
|------|---------|-------|
| `--output-dir` | `./smoke_output` | Where images + reports go |
| `--hf-repo` | `circlestone-labs/Anima` | HuggingFace repo to pull from |
| `--device` | auto (`cuda` if available) | Force `cpu` if needed |
| `--dtype` | `bf16` | Try `fp16` if bf16 has issues |

## 8. Check results

```bash
# Quick pass/fail
cat smoke_output/smoke_report.json | python3 -m json.tool | head -5

# List generated files
ls -lh smoke_output/

# View per-artist results
cat smoke_output/smoke_ledger.json | python3 -m json.tool
```

### Expected output files (on pass)

```
smoke_output/
  dairi_896x1152.png
  dairi_540x720.webp
  ebifurya_896x1152.png
  ebifurya_540x720.webp
  hammer_sunset_beach_896x1152.png
  hammer_sunset_beach_540x720.webp
  ruu_tksymkw_896x1152.png
  ruu_tksymkw_540x720.webp
  haruyama_kazunori_896x1152.png
  haruyama_kazunori_540x720.webp
  smoke_report.json
  smoke_ledger.json
```

## 9. Interpret the result

### PASSED

```
SMOKE TEST PASSED
  All 5 images generated, validated, and downscaled successfully.
```

**Next step**: proceed to top-500 pilot benchmark.

### FAILED — model load

```
SMOKE TEST FAILED — Could not load model with any strategy
  RECOMMENDATION: Fall back to ComfyUI headless API.
```

**Next step**: install ComfyUI on the Spark and test with the headless API instead.

### FAILED — some images

```
SMOKE TEST FAILED
  2 of 5 images failed.
  RECOMMENDATION: Investigate per-artist failures before deciding.
```

**Next step**: check `smoke_ledger.json` for the `error` field on failed entries. Common causes:
- NaN pixels → dtype issue (try `--dtype fp16` or `fp32`)
- OOM → unlikely on 128 GB unified memory, but check `nvidia-smi`
- Scheduler mismatch → the script logs which scheduler it actually used

---

## Quick-reference (copy-paste block)

```bash
# One-shot: SSH in and run everything
ssh <user>@<spark-ip>
mkdir -p ~/anima-smoke && cd ~/anima-smoke
python3 -m venv .venv && source .venv/bin/activate
pip install --upgrade pip
pip install torch diffusers transformers accelerate safetensors huggingface_hub Pillow numpy
# paste or scp smoke_test.py here
python smoke_test.py --output-dir ./smoke_output --dtype bf16
cat smoke_output/smoke_report.json | python3 -m json.tool | head -5
```
