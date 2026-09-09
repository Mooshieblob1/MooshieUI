#!/usr/bin/env python3
"""Verify native bundle architecture, ad-hoc signature, DMG and artifact hashes."""
import argparse
import hashlib
import json
import plistlib
import re
import subprocess
from pathlib import Path


def output(*args):
    result = subprocess.run(args, check=True, capture_output=True, text=True)
    return (result.stdout + result.stderr).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    apps = list(args.bundle_dir.glob("macos/*.app"))
    dmgs = list(args.bundle_dir.glob("dmg/*_aarch64.dmg"))
    if len(apps) != 1 or len(dmgs) != 1:
        raise RuntimeError("Expected exactly one app and one ARM DMG")
    app, dmg = apps[0], dmgs[0]
    with (app / "Contents/Info.plist").open("rb") as f:
        info = plistlib.load(f)
    binary = app / "Contents/MacOS" / info["CFBundleExecutable"]
    arch = output("lipo", "-archs", str(binary))
    assert arch == "arm64", f"Unexpected binary architecture: {arch}"
    assert info["LSMinimumSystemVersion"] == "14.0"
    output("codesign", "--verify", "--deep", "--strict", str(app))
    signature = output("codesign", "-dv", "--verbose=4", str(app))
    assert "Signature=adhoc" in signature, "App is not ad-hoc signed"
    output("hdiutil", "verify", str(dmg))
    # Verify the distributed app inside the image as well as the build directory.
    attachment = subprocess.run(
        ["hdiutil", "attach", "-readonly", "-nobrowse", "-noautoopen", "-plist", str(dmg)],
        check=True, capture_output=True,
    )
    mounts = [entry["mount-point"] for entry in plistlib.loads(attachment.stdout)["system-entities"]
              if "mount-point" in entry]
    try:
        assert len(mounts) == 1, "Expected one mounted installer volume"
        installed_app = Path(mounts[0]) / app.name
        assert installed_app.is_dir(), "Installer does not contain the built app"
        output("codesign", "--verify", "--deep", "--strict", str(installed_app))
        installed_signature = output("codesign", "-dv", "--verbose=4", str(installed_app))
        cdhash = re.search(r"^CDHash=(.+)$", signature, re.M).group(1)
        assert re.search(r"^CDHash=(.+)$", installed_signature, re.M).group(1) == cdhash, "Installer app differs from the build"
    finally:
        for mount in mounts:
            output("hdiutil", "detach", mount)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    with dmg.open("rb") as disk_image:
        digest = hashlib.file_digest(disk_image, "sha256").hexdigest()
    report = {
        "source_sha": output("git", "rev-parse", "HEAD"),
        "version": info["CFBundleShortVersionString"],
        "architecture": arch,
        "minimum_macos": info["LSMinimumSystemVersion"],
        "signature": signature,
        "installer_app_verified": True,
        "dmg_sha256": digest,
        "hardware_tested": False,
    }
    (args.output_dir / "bundle-report.json").write_text(json.dumps(report, indent=2))
    (args.output_dir / "SHA256SUMS").write_text(f"{report['dmg_sha256']}  {dmg.name}\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
