"""Probe the actual managed Python process, including a real MPS operation."""
import importlib.metadata
import json
import platform
import sys

import psutil
import torch

report = {
    "architecture": platform.machine(),
    "python": platform.python_version(),
    "executable": sys.executable,
    "macos": platform.mac_ver()[0],
    "packages": {name: importlib.metadata.version(name) for name in ("torch", "torchvision", "torchaudio")},
    "unified_memory_bytes": psutil.virtual_memory().total,
    "mps_built": torch.backends.mps.is_built(),
    "mps_available": torch.backends.mps.is_available(),
    "mps_operation": False,
}
if report["mps_available"]:
    try:
        x = torch.ones((4, 4), device="mps")
        result = x @ x
        torch.mps.synchronize()
        report["mps_operation"] = bool(torch.all(result.cpu() == 4).item())
    except Exception as error:
        report["mps_error"] = str(error)
print(json.dumps(report))
