#!/usr/bin/env python3
"""Custom-node compatibility smoke test for MooshieUI against a ComfyUI checkout.

Why this exists
---------------
MooshieUI pins ComfyUI to a release tag (``COMFYUI_REF`` in
``src-tauri/src/setup.rs``). Bumping that pin is usually safe for the app
itself, but ComfyUI's internal refactors can break MooshieUI's *bundled custom
nodes*, which import deep ComfyUI internals (``comfy.sample``,
``comfy.samplers``, ``comfy.model_management``, ``folder_paths``,
``latent_preview``, ...). When such an import breaks, the node silently fails to
register and disappears from ``/object_info`` while ComfyUI itself still starts.

This script reproduces exactly what the app does at runtime
(:func:`ensure_mooshie_nodes` in ``src-tauri/src/comfyui/nodes.rs``): it deploys
the bundled nodes into a ComfyUI checkout, starts ComfyUI in CPU mode, and
asserts every required node class appears in ``/object_info``. A non-zero exit
means a newer ComfyUI release would break MooshieUI's nodes.

Single source of truth
-----------------------
The list of required node classes and the ultralytics pin are parsed from
``src-tauri/src/comfyui/nodes.rs`` so this test never drifts from the app.

Scope
-----
Covers the BUNDLED MooshieUI nodes, plus the input signatures of the CORE
ComfyUI nodes the workflow builder emits by hand. The external ControlNet /
style-transfer packages (comfyui_controlnet_aux, ComfyUi-Untwisting-RoPE, ...)
are third-party git repos cloned at runtime; their compatibility is their own
maintainers' concern and is out of scope here. This limitation is logged, not
silently skipped.

Usage
-----
    python smoke_test.py --comfyui-dir /path/to/ComfyUI [--repo-root .]

Exits 0 if all required bundled node classes register, 1 otherwise.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

# ── Deploy mapping (mirrors ensure_mooshie_nodes in nodes.rs) ────────────────
# (source path relative to repo root, destination path relative to custom_nodes)
NODE_FILE_MAP = [
    ("src-tauri/src/comfyui/mooshie_nodes.py", "mooshie-nodes/__init__.py"),
    ("comfyui-nodes/nodes_tiled_diffusion.py", "nodes_tiled_diffusion.py"),
    ("comfyui-nodes/nodes_guidance.py", "nodes_guidance.py"),
    # Source name has the _combined suffix; deployed name drops it (see nodes.rs).
    ("comfyui-nodes/nodes_sdxl_flux2vae_combined.py", "nodes_sdxl_flux2vae.py"),
    ("comfyui-nodes/nanosaur_support/__init__.py", "nanosaur_support/__init__.py"),
    ("comfyui-nodes/nanosaur_support/nodes.py", "nanosaur_support/nodes.py"),
    ("comfyui-nodes/nanosaur_support/model.py", "nanosaur_support/model.py"),
    ("comfyui-nodes/nanosaur_support/text_encoder.py", "nanosaur_support/text_encoder.py"),
    ("comfyui-nodes/nanosaur_support/vae.py", "nanosaur_support/vae.py"),
]

# A stale package dir from a prior deploy shadows the flat flux2vae file; nodes.rs
# removes it first, so we do too.
STALE_DIRS = ["sdxl-flux2vae-comfyui-node"]

NODES_RS = "src-tauri/src/comfyui/nodes.rs"

# Fallbacks used only if parsing nodes.rs fails (kept in sync with nodes.rs).
DEFAULT_REQUIRED_CLASSES = [
    "MooshieSaveImage",
    "MooshieFaceDetailer",
    "MooshieSegmentDetailer",
    "MooshieSoftGuidance",
    "MooshieSmartGuidance",
    "NanoSaurLoader",
    "ApplyTiledDiffusion",
]
DEFAULT_MOOSHIE_REQUIREMENTS = "ultralytics==8.4.75\n"

# Core ComfyUI node classes the workflow builder emits by hand, with the inputs
# it wires. Registration alone proves nothing here: ComfyUI can keep a class
# name while changing its input shape, and a custom node can claim a name that
# core later takes over (core wins, silently). That is exactly how Anima
# ControlNet broke in #522, where the class existed but wanted `model_patch`
# instead of the third-party node's `lllite_name`. Checking the signature turns
# that into a failed pin bump instead of a failed generation.
REQUIRED_CORE_NODE_INPUTS = {
    "ModelPatchLoader": ["name"],
    "AnimaLLLiteApply": [
        "model",
        "model_patch",
        "image",
        "strength",
        "start_percent",
        "end_percent",
    ],
}


def log(msg: str) -> None:
    print(msg, flush=True)


def parse_required_classes(nodes_rs: Path) -> list[str]:
    """Extract REQUIRED_MOOSHIE_NODE_CLASSES from nodes.rs (single source of truth)."""
    try:
        text = nodes_rs.read_text(encoding="utf-8")
    except OSError as e:
        log(f"WARN: could not read {nodes_rs} ({e}); using built-in class list")
        return list(DEFAULT_REQUIRED_CLASSES)
    m = re.search(
        r"REQUIRED_MOOSHIE_NODE_CLASSES:\s*&\[&str\]\s*=\s*&\[(.*?)\];",
        text,
        re.DOTALL,
    )
    if not m:
        log("WARN: REQUIRED_MOOSHIE_NODE_CLASSES not found in nodes.rs; using built-in list")
        return list(DEFAULT_REQUIRED_CLASSES)
    classes = re.findall(r'"([^"]+)"', m.group(1))
    if not classes:
        log("WARN: REQUIRED_MOOSHIE_NODE_CLASSES parsed empty; using built-in list")
        return list(DEFAULT_REQUIRED_CLASSES)
    return classes


def check_core_node_inputs(object_info: dict) -> list[str]:
    """Report every core node whose input spec no longer matches what we emit."""
    problems = []
    for node_class, expected in REQUIRED_CORE_NODE_INPUTS.items():
        info = object_info.get(node_class)
        if info is None:
            problems.append(f"{node_class}: not registered")
            continue
        spec = info.get("input") or {}
        present = set(spec.get("required") or {}) | set(spec.get("optional") or {})
        missing = [name for name in expected if name not in present]
        if missing:
            problems.append(
                f"{node_class}: missing input(s) {', '.join(missing)} "
                f"(registered inputs: {', '.join(sorted(present)) or 'none'})"
            )
    return problems


def parse_mooshie_requirements(nodes_rs: Path) -> str:
    """Extract the MOOSHIE_NODES_REQUIREMENTS pin (e.g. ultralytics==8.4.75)."""
    try:
        text = nodes_rs.read_text(encoding="utf-8")
    except OSError:
        return DEFAULT_MOOSHIE_REQUIREMENTS
    m = re.search(r'MOOSHIE_NODES_REQUIREMENTS:\s*&str\s*=\s*"([^"]*)"', text)
    if not m:
        return DEFAULT_MOOSHIE_REQUIREMENTS
    # The Rust literal uses an escaped newline; turn it into a real one.
    return m.group(1).encode("utf-8").decode("unicode_escape")


def deploy_nodes(repo_root: Path, comfyui_dir: Path, requirements: str) -> None:
    """Copy bundled MooshieUI nodes into ComfyUI/custom_nodes (mirrors nodes.rs)."""
    custom_nodes = comfyui_dir / "custom_nodes"
    custom_nodes.mkdir(parents=True, exist_ok=True)

    for stale in STALE_DIRS:
        stale_dir = custom_nodes / stale
        if stale_dir.is_dir():
            shutil.rmtree(stale_dir)
            log(f"  removed stale dir custom_nodes/{stale}")

    for src_rel, dst_rel in NODE_FILE_MAP:
        src = repo_root / src_rel
        dst = custom_nodes / dst_rel
        if not src.is_file():
            raise FileNotFoundError(f"bundled node source missing: {src}")
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dst)
        log(f"  deployed {src_rel} -> custom_nodes/{dst_rel}")

    reqs_path = custom_nodes / "mooshie-nodes" / "requirements.txt"
    reqs_path.write_text(requirements, encoding="utf-8")
    log(f"  wrote custom_nodes/mooshie-nodes/requirements.txt ({requirements.strip()})")


def start_comfyui(comfyui_dir: Path, port: int, log_path: Path) -> subprocess.Popen:
    """Launch ComfyUI in CPU mode, streaming its output to log_path."""
    log_file = open(log_path, "w", encoding="utf-8")
    cmd = [
        sys.executable,
        "main.py",
        "--cpu",
        "--port",
        str(port),
        "--listen",
        "127.0.0.1",
        "--disable-auto-launch",
    ]
    log(f"  launching: {' '.join(cmd)} (cwd={comfyui_dir})")
    return subprocess.Popen(
        cmd,
        cwd=str(comfyui_dir),
        stdout=log_file,
        stderr=subprocess.STDOUT,
        env={**os.environ, "PYTHONUNBUFFERED": "1"},
    )


def fetch_object_info(host: str, port: int, timeout_s: float):
    url = f"http://{host}:{port}/object_info"
    req = urllib.request.Request(url)
    with urllib.request.urlopen(req, timeout=timeout_s) as resp:
        return json.loads(resp.read().decode("utf-8"))


def wait_for_object_info(
    proc: subprocess.Popen,
    host: str,
    port: int,
    boot_timeout_s: float,
    log_path: Path,
):
    """Poll /object_info until ComfyUI responds, it exits, or we time out."""
    deadline = time.monotonic() + boot_timeout_s
    last_err = None
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(
                f"ComfyUI exited early with code {proc.returncode} before serving "
                f"/object_info. See {log_path}."
            )
        try:
            return fetch_object_info(host, port, timeout_s=10)
        except (urllib.error.URLError, ConnectionError, OSError, json.JSONDecodeError) as e:
            last_err = e
            time.sleep(2)
    raise TimeoutError(
        f"ComfyUI did not serve /object_info within {boot_timeout_s:.0f}s "
        f"(last error: {last_err}). See {log_path}."
    )


def tail(path: Path, n: int = 60) -> str:
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return "(no log captured)"
    return "\n".join(lines[-n:])


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--comfyui-dir", required=True, help="Path to the ComfyUI checkout to test")
    ap.add_argument(
        "--repo-root",
        default=".",
        help="MooshieUI repo root containing comfyui-nodes/ and src-tauri/ (default: cwd)",
    )
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=8188)
    ap.add_argument(
        "--boot-timeout",
        type=float,
        default=300.0,
        help="Seconds to wait for ComfyUI to serve /object_info (default: 300)",
    )
    ap.add_argument(
        "--summary-json",
        default=None,
        help="Optional path to write a machine-readable result summary",
    )
    args = ap.parse_args()

    repo_root = Path(args.repo_root).resolve()
    comfyui_dir = Path(args.comfyui_dir).resolve()
    nodes_rs = repo_root / NODES_RS
    log_path = comfyui_dir / "comfyui-smoke.log"

    if not (comfyui_dir / "main.py").is_file():
        log(f"ERROR: {comfyui_dir} does not look like a ComfyUI checkout (no main.py)")
        return 1

    required = parse_required_classes(nodes_rs)
    requirements = parse_mooshie_requirements(nodes_rs)

    log("=== MooshieUI custom-node compatibility smoke test ===")
    log(f"repo root      : {repo_root}")
    log(f"ComfyUI dir    : {comfyui_dir}")
    log(f"required nodes : {', '.join(required)}")
    log(f"core signatures: {', '.join(REQUIRED_CORE_NODE_INPUTS)}")
    log(
        "scope note     : bundled MooshieUI nodes and core node signatures only; "
        "external ControlNet / style-transfer git packages are NOT covered by "
        "this test."
    )

    log("\n[1/3] Deploying bundled MooshieUI nodes...")
    try:
        deploy_nodes(repo_root, comfyui_dir, requirements)
    except (OSError, FileNotFoundError) as e:
        log(f"ERROR: deploy failed: {e}")
        return 1

    log("\n[2/3] Starting ComfyUI (CPU) and waiting for /object_info...")
    proc = start_comfyui(comfyui_dir, args.port, log_path)
    object_info = None
    try:
        object_info = wait_for_object_info(
            proc, args.host, args.port, args.boot_timeout, log_path
        )
    except (RuntimeError, TimeoutError) as e:
        log(f"ERROR: {e}")
        log("\n----- ComfyUI log tail -----")
        log(tail(log_path))
        _terminate(proc)
        return 1

    log("\n[3/3] Verifying required node classes and core node signatures...")
    registered = set(object_info.keys())
    missing = [c for c in required if c not in registered]
    signature_problems = check_core_node_inputs(object_info)

    _terminate(proc)

    result = {
        "comfyui_dir": str(comfyui_dir),
        "required": required,
        "missing": missing,
        "core_input_problems": signature_problems,
        "registered_count": len(registered),
        "passed": not missing and not signature_problems,
    }
    if args.summary_json:
        Path(args.summary_json).write_text(json.dumps(result, indent=2), encoding="utf-8")

    if missing:
        log(f"FAIL: {len(missing)} required node class(es) did not register: {', '.join(missing)}")
        log("These nodes likely failed to import against this ComfyUI version.")
    if signature_problems:
        log(f"FAIL: {len(signature_problems)} core node input signature mismatch(es):")
        for problem in signature_problems:
            log(f"  - {problem}")
        log("MooshieUI emits these core nodes directly, so a changed input shape "
            "breaks generation at runtime rather than at startup (see #522).")
    if missing or signature_problems:
        log("\n----- ComfyUI log tail -----")
        log(tail(log_path))
        return 1

    log(f"PASS: all {len(required)} required node classes registered and "
        f"{len(REQUIRED_CORE_NODE_INPUTS)} core node signatures intact "
        f"({len(registered)} total nodes in /object_info).")
    return 0


def _terminate(proc: subprocess.Popen) -> None:
    if proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=15)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


if __name__ == "__main__":
    raise SystemExit(main())
