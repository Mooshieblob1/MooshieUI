#!/usr/bin/env python3
"""Collect installers and generate a complete, signed-platform updater manifest."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
from datetime import datetime, timezone
from pathlib import Path


def collect(source: Path, output: Path, tag: str, repo: str, macos: bool = False):
    if not re.fullmatch(r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", tag):
        raise ValueError("Expected a semver release tag beginning with v")
    version = tag[1:]
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError("Release output directory must be empty")
    suffixes = (".deb", ".rpm", ".AppImage", ".AppImage.sig", ".exe", ".exe.sig", ".msi", ".msi.sig")
    if macos:
        suffixes += (".dmg", ".app.tar.gz", ".app.tar.gz.sig")
    for file in source.rglob("*"):
        if not file.is_file() or not file.name.endswith(suffixes):
            continue
        name = file.name
        # Tauri's Mac updater archive omits architecture and version. Rename
        # the file, preserving the signed bytes and matching signature.
        if name.endswith((".app.tar.gz", ".app.tar.gz.sig")):
            name = f"MooshieUI_{version}_aarch64.app.tar.gz" + (".sig" if name.endswith(".sig") else "")
        destination = output / name
        if destination.exists():
            raise ValueError(f"Duplicate release asset: {name}")
        shutil.copyfile(file, destination)

    def one(pattern):
        matches = list(output.glob(pattern))
        if len(matches) != 1:
            raise ValueError(f"Expected exactly one {pattern}, found {len(matches)}")
        return matches[0]

    bundles = {
        "linux-x86_64": one(f"*_{version}_amd64.AppImage"),
        "windows-x86_64": one(f"*_{version}_x64-setup.exe"),
    }
    if macos:
        one(f"*_{version}_aarch64.dmg")
        bundles["darwin-aarch64"] = one(f"*_{version}_aarch64.app.tar.gz")
    platforms = {}
    for platform, bundle in bundles.items():
        signature = Path(str(bundle) + ".sig").read_text().strip()
        if not signature:
            raise ValueError(f"Empty signature for {bundle.name}")
        platforms[platform] = {
            "signature": signature,
            "url": f"https://github.com/{repo}/releases/download/{tag}/{bundle.name}",
        }
    manifest = {
        "version": version,
        "notes": f"MooshieUI {tag}",
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    (output / "latest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    sums = []
    for file in sorted(output.iterdir()):
        with file.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        sums.append(f"{digest}  {file.name}\n")
    (output / "SHA256SUMS").write_text("".join(sums))
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--macos", action="store_true")
    args = parser.parse_args()
    result = collect(args.source, args.output, args.tag, args.repo, args.macos)
    print("Prepared updater platforms:", ", ".join(result["platforms"]))
