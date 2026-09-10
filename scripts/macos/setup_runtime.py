#!/usr/bin/env python3
"""Install and check the same managed ARM runtime as the desktop app.

Run on a Mac with --require-mps for hardware qualification. Hosted CI records
the MPS probe and checks CPU node registration without claiming GPU validation.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import platform
import re
import subprocess
import sys
import urllib.request
import zipfile
from pathlib import Path


def run(*args, **kwargs):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--uv", required=True, type=Path)
    ap.add_argument("--work-dir", required=True, type=Path)
    ap.add_argument("--comfyui-ref")
    ap.add_argument("--require-mps", action="store_true")
    args = ap.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("This check requires native Apple Silicon macOS.")
    root = Path(__file__).resolve().parents[2]
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    runtime = root / "src-tauri/runtime"
    constraints = runtime / "macos-constraints.txt"
    python_version = (runtime / "macos-python.txt").read_text().strip()
    pin = re.search(r'COMFYUI_REF:\s*&str\s*=\s*"([^"]+)"',
                    (root / "src-tauri/src/comfyui_version.rs").read_text()).group(1)
    pin = args.comfyui_ref or pin
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", pin):
        raise SystemExit("Use an explicit ComfyUI release tag.")
    run(args.uv, "python", "install", python_version)
    run(args.uv, "venv", work / "venv", "--python", python_version)
    python = work / "venv/bin/python"
    archive = work / "comfyui.zip"
    urllib.request.urlretrieve(f"https://github.com/Comfy-Org/ComfyUI/archive/refs/tags/{pin}.zip", archive)
    with zipfile.ZipFile(archive) as z:
        for member in z.infolist():
            if not (work / member.filename).resolve().is_relative_to(work):
                raise RuntimeError("Unsafe ComfyUI archive path")
        z.extractall(work)
    comfy = work / f"ComfyUI-{pin.removeprefix('v')}"
    run(args.uv, "pip", "install", "--python", python, "-c", constraints,
        "torch", "torchvision", "torchaudio")
    run(args.uv, "pip", "install", "--python", python, "-c", constraints,
        "-r", comfy / "requirements.txt")
    spec = importlib.util.spec_from_file_location("smoke", root / "scripts/comfyui-compat/smoke_test.py")
    smoke = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(smoke)
    requirements = work / "mooshie-requirements.txt"
    requirements.write_text(smoke.parse_mooshie_requirements(root / smoke.NODES_RS))
    run(args.uv, "pip", "install", "--python", python, "-c", constraints, "-r", requirements)
    # Match the external packs installed by start_comfyui_process, not just
    # bundled nodes. A clean Mac must resolve these native dependencies too.
    nodes_source = (root / smoke.NODES_RS).read_text(encoding="utf-8")
    external_nodes = []
    external_revisions = {}
    for constant in ("STYLE_TRANSFER_PACKAGES", "GGUF_PACKAGES", "REQUIRED_CONTROLNET_PACKAGES"):
        block = re.search(rf"const {constant}:.*?=\s*&\[(.*?)\];", nodes_source, re.S)
        if not block:
            raise RuntimeError(f"Cannot read the app's {constant}")
        for package in re.finditer(r'RequiredCustomNodePackage\s*\{(.*?)\}', block.group(1), re.S):
            fields = package.group(1)
            name = re.search(r'name:\s*"([^"]+)"', fields).group(1)
            url = re.search(r'git_url:\s*"([^"]+)"', fields).group(1)
            req_file = re.search(r'requirements_file:\s*"([^"]+)"', fields).group(1)
            classes = re.search(r'verify_nodes:\s*&\[(.*?)\]', fields, re.S).group(1)
            external_nodes.extend(re.findall(r'"([^"]+)"', classes))
            destination = comfy / "custom_nodes" / name
            run("git", "clone", "--depth=1", url, destination)
            external_revisions[name] = run("git", "-C", destination, "rev-parse", "HEAD", capture_output=True, text=True).stdout.strip()
            if (destination / req_file).exists():
                run(args.uv, "pip", "install", "--python", python, "-c", constraints, "-r", destination / req_file)
    extra_required = work / "external-nodes.json"
    extra_required.write_text(json.dumps(external_nodes))
    run(args.uv, "pip", "check", "--python", python)
    probe = run(python, runtime / "macos_probe.py", capture_output=True, text=True)
    report = json.loads(probe.stdout)
    report["source_sha"] = run("git", "-C", root, "rev-parse", "HEAD", capture_output=True, text=True).stdout.strip()
    report["uv_version"] = run(args.uv, "--version", capture_output=True, text=True).stdout.strip()
    report["comfyui_ref"] = pin
    report["external_node_revisions"] = external_revisions
    report["hardware_qualification"] = args.require_mps
    (work / "runtime-report.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2), flush=True)
    assert report["architecture"] == "arm64", "Python is not ARM-native"
    assert report["python"] == python_version, "Wrong managed Python version"
    for line in constraints.read_text().splitlines():
        if "==" in line and not line.startswith("#"):
            name, version = line.split("==")
            assert report["packages"][name] == version, f"Wrong {name} version"
    if args.require_mps and not report["mps_operation"]:
        raise RuntimeError("MPS operation failed; hardware qualification cannot pass")
    with (work / "requirements-resolved.txt").open("w") as output:
        run(args.uv, "pip", "freeze", "--python", python, stdout=output)
    run(python, root / "scripts/comfyui-compat/test_pause_nodes.py", "--comfyui-dir", comfy)
    run(python, root / "scripts/comfyui-compat/smoke_test.py", "--repo-root", root,
        "--comfyui-dir", comfy, "--boot-timeout", "360",
        "--summary-json", work / "node-summary.json", "--extra-required-file", extra_required)
    print("Runtime and bundled-node checks passed. Full app and model tests remain separate.")


if __name__ == "__main__":
    main()
