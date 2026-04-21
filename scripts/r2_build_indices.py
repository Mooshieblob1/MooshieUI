#!/usr/bin/env python3
"""Build sharded artist-gallery index JSON files from the dataset output.

Reads:
  <release-dir>/artist_tag_index.json       artist tag -> image metadata
  src/lib/assets/anima-tags.json            tag source (aliases, post count)
  <release-dir>/images/                     filesystem reality check

Writes:
  <release-dir>/indices/manifest.json
  <release-dir>/indices/shards/<bucket>.json
  <release-dir>/indices/search.json

The shard bucket is the first character of the slug when alnum, else '_'.
Each entry is annotated with hasImage based on whether the WEBP is present on disk.
Upload the resulting directory via scripts/r2_upload_anima.py --indices-only.
"""
from __future__ import annotations

import argparse
import json
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path


SHARD_SCHEME = "first-char-slug-v1"
INDEX_VERSION = 1


def bucket_for(slug: str) -> str:
    if not slug:
        return "_"
    ch = slug[0].lower()
    if ch.isalnum():
        return ch
    return "_"


def build(release_dir: Path, release_prefix: str, public_base_url: str, anima_tags_path: Path) -> None:
    artist_index_path = release_dir / "artist_tag_index.json"
    if not artist_index_path.is_file():
        raise SystemExit(f"Missing {artist_index_path}")
    if not anima_tags_path.is_file():
        raise SystemExit(f"Missing {anima_tags_path}")

    images_dir = release_dir / "images"
    on_disk: set[str] = {
        p.name for p in images_dir.iterdir() if p.is_file() and p.suffix.lower() == ".webp"
    } if images_dir.is_dir() else set()
    print(f"[indices] on-disk webp files: {len(on_disk)}", flush=True)

    artist_index: dict[str, dict] = json.loads(artist_index_path.read_text(encoding="utf-8"))
    anima_tags = json.loads(anima_tags_path.read_text(encoding="utf-8"))
    tag_meta: dict[str, dict] = {
        t["n"]: {"postCount": t.get("p", 0), "aliases": t.get("a", []) or []}
        for t in anima_tags
        if t.get("c") == 1
    }
    print(f"[indices] artists in source: {len(tag_meta)}; in dataset: {len(artist_index)}", flush=True)

    base = public_base_url.rstrip("/")

    shards: dict[str, dict[str, dict]] = {}
    search_flat: list[dict] = []
    with_image = 0

    for tag, meta in artist_index.items():
        slug = meta.get("slug") or ""
        image_id = meta.get("image_id") or ""
        filename = meta.get("filename_webp") or ""
        object_key = meta.get("object_key") or f"{release_prefix}/images/{filename}"
        image_url = f"{base}/{object_key}" if base else ""

        src = tag_meta.get(tag, {})
        has_image = filename in on_disk
        if has_image:
            with_image += 1

        entry = {
            "tag": tag,
            "slug": slug,
            "imageId": image_id,
            "imageUrl": image_url,
            "objectKey": object_key,
            "postCount": int(src.get("postCount", 0)),
            "aliases": list(src.get("aliases", [])),
            "hasImage": has_image,
        }

        bucket = bucket_for(slug)
        shards.setdefault(bucket, {})[slug] = entry
        search_flat.append(
            {
                "slug": slug,
                "tag": tag,
                "imageId": image_id,
                "postCount": entry["postCount"],
                "shard": bucket,
                "hasImage": has_image,
            }
        )

    # Sort search by post count descending (typeahead ranking hint).
    search_flat.sort(key=lambda e: (-e["postCount"], e["slug"]))

    # Reset output dir.
    out_dir = release_dir / "indices"
    if out_dir.exists():
        shutil.rmtree(out_dir)
    (out_dir / "shards").mkdir(parents=True, exist_ok=True)

    manifest_shards: list[dict] = []
    for bucket, entries in sorted(shards.items()):
        shard_path = out_dir / "shards" / f"{bucket}.json"
        shard_path.write_text(
            json.dumps({"bucket": bucket, "entries": entries}, ensure_ascii=False, separators=(",", ":")),
            encoding="utf-8",
        )
        manifest_shards.append(
            {"bucket": bucket, "count": len(entries), "path": f"shards/{bucket}.json"}
        )

    search_path = out_dir / "search.json"
    search_path.write_text(
        json.dumps(search_flat, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )

    manifest = {
        "version": INDEX_VERSION,
        "releasePrefix": release_prefix,
        "imageBaseUrl": base,
        "shardScheme": SHARD_SCHEME,
        "artistCount": len(artist_index),
        "artistsWithImage": with_image,
        "shards": manifest_shards,
        "searchIndex": {
            "path": "search.json",
            "entries": len(search_flat),
        },
        "generatedAt": datetime.now(timezone.utc).isoformat(),
    }
    manifest_path = out_dir / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")

    # Size summary.
    total_bytes = sum(p.stat().st_size for p in out_dir.rglob("*.json"))
    print(
        f"[indices] wrote {len(manifest_shards)} shards + search.json + manifest.json "
        f"({total_bytes/1024/1024:.2f} MiB) under {out_dir}",
        flush=True,
    )
    print(f"[indices] artists with image on disk: {with_image}/{len(artist_index)}", flush=True)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--release-dir", type=Path, required=True)
    ap.add_argument("--release-prefix", required=True)
    ap.add_argument(
        "--public-base-url",
        required=True,
        help="e.g. https://cdn.mooshieblob.com  (images URL: <base>/<object_key>)",
    )
    ap.add_argument(
        "--anima-tags",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "src" / "lib" / "assets" / "anima-tags.json",
    )
    args = ap.parse_args()

    if not args.public_base_url.startswith(("http://", "https://")):
        print("WARNING: --public-base-url should start with http(s)://", file=sys.stderr)

    build(args.release_dir.resolve(), args.release_prefix, args.public_base_url, args.anima_tags.resolve())
    return 0


if __name__ == "__main__":
    sys.exit(main())
