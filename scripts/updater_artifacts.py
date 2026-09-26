#!/usr/bin/env python3
"""Updater artifacts for release builds that never hold the signing key.

`tauri build` signs its updater artifacts itself, which puts
TAURI_SIGNING_PRIVATE_KEY in the environment of the whole build: npm and Vite
plugins, cargo build scripts and proc macros. Release builds instead run with
`bundle.createUpdaterArtifacts` off, and a separate job signs the same files
afterwards with `tauri signer sign` (same code path, same `.sig` format).

  macos-archive  make `<App>.app.tar.gz` next to the built app, the one updater
                 artifact tauri-bundler creates rather than signing an installer
  list           print the files `tauri build` would have signed, one per line
  check          fail unless each of those has a well-formed `.sig` beside it
"""
from __future__ import annotations

import argparse
import base64
import binascii
import json
import os
import sys
import tarfile
from pathlib import Path

# What tauri-cli 2.x signs after bundling (`sign_updaters` in its bundle.rs) for
# the formats the release builds: the AppImage, deb, rpm and NSIS installers
# themselves, and the macOS updater archive. Keyed by the bundle subdirectory
# Tauri writes each one to, so stray matches elsewhere (an AppDir) are ignored.
BUNDLES = (("appimage", ".AppImage"), ("deb", ".deb"), ("rpm", ".rpm"), ("nsis", "-setup.exe"))
MACOS_BUNDLE = ("macos", ".app.tar.gz")


def macos_archive(bundle_dir: Path) -> Path:
    """Create `macos/<App>.app.tar.gz` like tauri-bundler's updater_bundle.rs.

    Same layout as its tar-rs `append_dir_all` with `follow_symlinks(false)`: the
    `.app` directory is the single top-level entry (tauri-plugin-updater drops the
    first path component on extract) and symlinks stay links. Hard links are
    stored as plain copies, as tar-rs does: the updater unpacks entry by entry and
    could not resolve a hard-link entry.
    """
    apps = [p for p in sorted(bundle_dir.glob("macos/*.app")) if p.is_dir() and not p.is_symlink()]
    if len(apps) != 1:
        raise SystemExit(f"Expected exactly one macos/*.app under {bundle_dir}, found {len(apps)}")
    app = apps[0]
    archive = app.with_name(app.name + ".tar.gz")
    entries = [(app, app.name)]
    for dirpath, dirnames, filenames in os.walk(app):
        dirnames.sort()
        for name in sorted(dirnames + filenames):
            path = Path(dirpath, name)
            entries.append((path, f"{app.name}/{path.relative_to(app).as_posix()}"))
    with tarfile.open(archive, "w:gz", format=tarfile.GNU_FORMAT) as tar:
        for path, arcname in entries:
            tar.inodes.clear()  # never emit a hard-link entry
            info = tar.gettarinfo(str(path), arcname)
            if info.isreg():
                with path.open("rb") as data:
                    tar.addfile(info, data)
            else:
                tar.addfile(info)
    return archive


def updater_files(source: Path, macos: bool) -> list[Path]:
    wanted = BUNDLES + ((MACOS_BUNDLE,) if macos else ())
    files = []
    for directory, suffix in wanted:
        found = sorted(p for p in source.rglob(f"*{suffix}") if p.is_file() and p.parent.name == directory)
        if len(found) != 1:
            raise SystemExit(f"Expected exactly one {directory}/*{suffix} under {source}, found {len(found)}")
        files.append(found[0])
    return files


def _b64(value: str) -> bytes:
    return base64.b64decode(value.strip(), validate=True)


def pubkey_id(config: Path) -> bytes:
    """Key id of the updater public key in tauri.conf.json."""
    pubkey = json.loads(config.read_text())["plugins"]["updater"]["pubkey"]
    key = _b64(_b64(pubkey).decode().splitlines()[1])
    if len(key) != 42 or key[:2] != b"Ed":
        raise SystemExit(f"Unexpected updater public key format in {config}")
    return key[2:10]


def signature_key_id(file: Path) -> bytes:
    """Validate the minisign signature box in `<file>.sig` and return its key id.

    Tauri's `.sig` is the base64 of: an untrusted comment, the signature (2-byte
    algorithm, 8-byte key id, 64-byte Ed25519 signature), a trusted comment naming
    the file, and the 64-byte global signature.
    """
    sig = Path(f"{file}.sig")
    if not sig.is_file():
        raise ValueError(f"missing signature {sig}")
    try:
        lines = _b64(sig.read_text()).decode().splitlines()
        signature = _b64(lines[1])
        global_signature = _b64(lines[3])
    except (binascii.Error, UnicodeDecodeError, IndexError) as error:
        raise ValueError(f"malformed signature {sig}: {error}") from None
    if (
        not lines[0].startswith("untrusted comment:")
        or not lines[2].startswith("trusted comment:")
        or f"\tfile:{file.name}" not in lines[2]
        or len(signature) != 74
        or signature[:2] not in (b"Ed", b"ED")
        or len(global_signature) != 64
    ):
        raise ValueError(f"malformed signature {sig}")
    return signature[2:10]


def check(source: Path, macos: bool, config: Path) -> int:
    expected_key = pubkey_id(config)
    failures = []
    for file in updater_files(source, macos):
        try:
            key = signature_key_id(file)
        except ValueError as error:
            failures.append(str(error))
            continue
        if key != expected_key:
            # Tauri itself only warns here: a key rotation release is signed with
            # the old key that installed apps still trust.
            print(f"::warning::{file.name}.sig was made with key {key[::-1].hex().upper()}, "
                  f"not the updater pubkey {expected_key[::-1].hex().upper()} in {config}")
        print(f"signed: {file}")
    for failure in failures:
        print(f"::error::{failure}")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    archive = commands.add_parser("macos-archive")
    archive.add_argument("--bundle-dir", type=Path, required=True)
    for name in ("list", "check"):
        command = commands.add_parser(name)
        command.add_argument("--source", type=Path, required=True)
        command.add_argument("--macos", action="store_true")
        if name == "check":
            command.add_argument("--config", type=Path, default=Path("src-tauri/tauri.conf.json"))
    args = parser.parse_args(argv)
    if args.command == "macos-archive":
        print(macos_archive(args.bundle_dir))
        return 0
    if args.command == "list":
        for file in updater_files(args.source, args.macos):
            print(file)
        return 0
    return check(args.source, args.macos, args.config)


if __name__ == "__main__":
    sys.exit(main())
