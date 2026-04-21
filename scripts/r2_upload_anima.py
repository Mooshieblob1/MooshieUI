#!/usr/bin/env python3
"""Upload the Anima artist-gallery dataset to Cloudflare R2.

Credentials (env):
  R2_ACCOUNT_ID           Cloudflare account id
  R2_ACCESS_KEY_ID        R2 access key
  R2_SECRET_ACCESS_KEY    R2 secret
  R2_BUCKET               bucket name (e.g. mooshie-anima)
  R2_PUBLIC_BASE_URL      optional; public HTTPS base (e.g. https://cdn.mooshieblob.com)
                          used only for the final report

Usage:
  python scripts/r2_upload_anima.py \
    --release-dir /workspace/MooshieUI/dataset_output/20260325_anima_all_artists \
    --release-prefix 20260325_anima_all_artists \
    [--images-only | --indices-only] \
    [--concurrency 16] [--dry-run] [--limit N]

Idempotent: HEADs each object first and skips when size matches.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Iterable

try:
    import boto3
    from botocore.client import Config
    from botocore.exceptions import ClientError
except ImportError:
    print("ERROR: boto3 is required. Install with: pip install boto3", file=sys.stderr)
    sys.exit(2)


IMAGE_CACHE_CONTROL = "public, max-age=31536000, immutable"
INDEX_CACHE_CONTROL = "public, max-age=300, must-revalidate"
MANIFEST_CACHE_CONTROL = "public, max-age=60, must-revalidate"


@dataclass
class UploadResult:
    uploaded: int = 0
    skipped: int = 0
    failed: int = 0
    bytes_uploaded: int = 0
    errors: list[str] | None = None

    def __post_init__(self) -> None:
        if self.errors is None:
            self.errors = []


def build_client() -> tuple["boto3.client", str]:
    account_id = os.environ.get("R2_ACCOUNT_ID", "").strip()
    access_key = os.environ.get("R2_ACCESS_KEY_ID", "").strip()
    secret_key = os.environ.get("R2_SECRET_ACCESS_KEY", "").strip()
    bucket = os.environ.get("R2_BUCKET", "").strip()
    missing = [
        name
        for name, val in (
            ("R2_ACCOUNT_ID", account_id),
            ("R2_ACCESS_KEY_ID", access_key),
            ("R2_SECRET_ACCESS_KEY", secret_key),
            ("R2_BUCKET", bucket),
        )
        if not val
    ]
    if missing:
        raise SystemExit(f"Missing required env vars: {', '.join(missing)}")

    endpoint = f"https://{account_id}.r2.cloudflarestorage.com"
    client = boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=access_key,
        aws_secret_access_key=secret_key,
        region_name="auto",
        config=Config(
            signature_version="s3v4",
            retries={"max_attempts": 5, "mode": "standard"},
            max_pool_connections=64,
        ),
    )
    return client, bucket


def head_size(client, bucket: str, key: str) -> int | None:
    try:
        resp = client.head_object(Bucket=bucket, Key=key)
        return int(resp.get("ContentLength", 0))
    except ClientError as exc:
        code = exc.response.get("Error", {}).get("Code", "")
        if code in ("404", "NoSuchKey", "NotFound"):
            return None
        raise


def upload_one(
    client,
    bucket: str,
    path: Path,
    key: str,
    content_type: str,
    cache_control: str,
    dry_run: bool,
) -> tuple[str, int]:
    """Return ("uploaded"|"skipped"|"failed", size). Raises on unrecoverable error."""
    size = path.stat().st_size
    existing = head_size(client, bucket, key)
    if existing is not None and existing == size:
        return "skipped", 0
    if dry_run:
        return "uploaded", size
    with path.open("rb") as fh:
        client.put_object(
            Bucket=bucket,
            Key=key,
            Body=fh,
            ContentType=content_type,
            CacheControl=cache_control,
        )
    return "uploaded", size


def iter_images(images_dir: Path, release_prefix: str) -> Iterable[tuple[Path, str]]:
    for p in sorted(images_dir.iterdir()):
        if p.is_file() and p.suffix.lower() == ".webp":
            yield p, f"{release_prefix}/images/{p.name}"


def upload_batch(
    client,
    bucket: str,
    jobs: list[tuple[Path, str, str, str]],
    concurrency: int,
    dry_run: bool,
    label: str,
) -> UploadResult:
    result = UploadResult()
    lock = threading.Lock()
    total = len(jobs)
    started = time.time()
    last_log = started

    def worker(job: tuple[Path, str, str, str]):
        path, key, ctype, cc = job
        try:
            status, size = upload_one(client, bucket, path, key, ctype, cc, dry_run)
            return status, size, key, None
        except Exception as exc:  # noqa: BLE001
            return "failed", 0, key, f"{key}: {exc}"

    with ThreadPoolExecutor(max_workers=concurrency) as pool:
        futures = [pool.submit(worker, j) for j in jobs]
        for i, fut in enumerate(as_completed(futures), 1):
            status, size, key, err = fut.result()
            with lock:
                if status == "uploaded":
                    result.uploaded += 1
                    result.bytes_uploaded += size
                elif status == "skipped":
                    result.skipped += 1
                else:
                    result.failed += 1
                    if err:
                        result.errors.append(err)
            now = time.time()
            if now - last_log >= 5.0 or i == total:
                rate = i / max(now - started, 1e-6)
                eta = (total - i) / rate if rate > 0 else 0
                print(
                    f"[{label}] {i}/{total}  up={result.uploaded} skip={result.skipped} "
                    f"fail={result.failed}  {rate:.1f}/s  eta={eta/60:.1f}m",
                    flush=True,
                )
                last_log = now
    return result


def upload_images(
    client,
    bucket: str,
    release_dir: Path,
    release_prefix: str,
    concurrency: int,
    dry_run: bool,
    limit: int | None,
) -> UploadResult:
    images_dir = release_dir / "images"
    if not images_dir.is_dir():
        raise SystemExit(f"Images dir not found: {images_dir}")
    jobs: list[tuple[Path, str, str, str]] = []
    for path, key in iter_images(images_dir, release_prefix):
        jobs.append((path, key, "image/webp", IMAGE_CACHE_CONTROL))
        if limit is not None and len(jobs) >= limit:
            break
    print(f"[images] {len(jobs)} candidate files under {images_dir}", flush=True)
    return upload_batch(client, bucket, jobs, concurrency, dry_run, "images")


def upload_indices(
    client,
    bucket: str,
    release_dir: Path,
    release_prefix: str,
    concurrency: int,
    dry_run: bool,
) -> UploadResult:
    indices_dir = release_dir / "indices"
    if not indices_dir.is_dir():
        raise SystemExit(
            f"Indices dir not found: {indices_dir}\n"
            f"Run scripts/r2_build_indices.py first."
        )
    jobs: list[tuple[Path, str, str, str]] = []
    for path in sorted(indices_dir.rglob("*.json")):
        rel = path.relative_to(indices_dir).as_posix()
        key = f"{release_prefix}/indices/{rel}"
        cc = MANIFEST_CACHE_CONTROL if rel == "manifest.json" else INDEX_CACHE_CONTROL
        jobs.append((path, key, "application/json", cc))
    print(f"[indices] {len(jobs)} JSON files under {indices_dir}", flush=True)
    return upload_batch(client, bucket, jobs, concurrency, dry_run, "indices")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--release-dir", type=Path, required=True)
    ap.add_argument("--release-prefix", required=True)
    ap.add_argument("--concurrency", type=int, default=16)
    ap.add_argument("--dry-run", action="store_true", help="HEAD-check only; no PUTs")
    ap.add_argument("--limit", type=int, default=None, help="only upload the first N images (debug)")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--images-only", action="store_true")
    mode.add_argument("--indices-only", action="store_true")
    args = ap.parse_args()

    client, bucket = build_client()
    print(f"[r2] bucket={bucket} endpoint=https://{os.environ['R2_ACCOUNT_ID']}.r2.cloudflarestorage.com")
    if args.dry_run:
        print("[r2] DRY RUN — no objects will be written")

    started = time.time()
    image_result = UploadResult()
    index_result = UploadResult()

    if not args.indices_only:
        image_result = upload_images(
            client, bucket, args.release_dir, args.release_prefix,
            args.concurrency, args.dry_run, args.limit,
        )
    if not args.images_only:
        index_result = upload_indices(
            client, bucket, args.release_dir, args.release_prefix,
            args.concurrency, args.dry_run,
        )

    elapsed = time.time() - started
    public_base = os.environ.get("R2_PUBLIC_BASE_URL", "").rstrip("/")
    report = {
        "release_prefix": args.release_prefix,
        "bucket": bucket,
        "public_base_url": public_base,
        "elapsed_s": round(elapsed, 1),
        "dry_run": args.dry_run,
        "images": asdict(image_result),
        "indices": asdict(index_result),
    }
    report_path = args.release_dir / "r2_upload_report.json"
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"\n[r2] report written: {report_path}")
    print(json.dumps(
        {"images": {k: v for k, v in report["images"].items() if k != "errors"},
         "indices": {k: v for k, v in report["indices"].items() if k != "errors"}},
        indent=2,
    ))

    failed = image_result.failed + index_result.failed
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
