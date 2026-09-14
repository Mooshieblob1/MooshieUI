"""Executed only by the separately configured SheetSage2 Python environment."""
import json
import sys
from pathlib import Path


def main():
    from transformers import AutoModel

    model_dir, source, output, device = sys.argv[1:]
    model = AutoModel.from_pretrained(model_dir, trust_remote_code=True).eval().to(device)
    result = model.transcribe(source, melody_only=True)
    if not result.get("abc") or result.get("abc_error"):
        raise RuntimeError("Transcription did not produce a usable melody score")
    abc = result["abc"]
    if len(abc.encode("utf-8")) > 131072:
        raise RuntimeError("Melody score exceeds 128 KiB; transcribe a shorter recording")
    warnings = result.get("warnings") or []
    if isinstance(warnings, str):
        warnings = [warnings]
    Path(output).write_text(json.dumps({
        "abc": abc,
        "warnings": [str(warning)[:2000] for warning in warnings[:32]],
    }, ensure_ascii=False), encoding="utf-8")


if __name__ == "__main__":
    main()
