#!/usr/bin/env python3
"""Resolve the managed source shared by desktop, Docker and runtime checks."""
import json
import re
from pathlib import Path


def managed_ref(repo_root: Path) -> str:
    source = (repo_root / 'src-tauri/src/comfyui_version.rs').read_text(encoding='utf-8')
    match = re.search(r'COMFYUI_REF:\s*&str\s*=\s*"([^"]+)"', source)
    if not match:
        raise ValueError('Cannot find the ComfyUI baseline release')
    release = match.group(1)
    override = json.loads((repo_root / 'src-tauri/runtime/comfyui-source.json').read_text())
    if release == override['release']:
        revision = override['revision']
        if not re.fullmatch(r'[0-9a-f]{40}', revision):
            raise ValueError('The managed ComfyUI override must be an immutable commit')
        return revision
    return release


if __name__ == '__main__':
    print(managed_ref(Path(__file__).resolve().parents[2]))
