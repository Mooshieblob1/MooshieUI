"""One-off merge script.

Combine the previous (Danbooru-derived) `anima-tags.json` with the new
BetaDoggo Gelbooru-derived list. For tags present in the new list, we use the
new entry verbatim (Gelbooru post count). Tags that only exist in the old list
are appended with a `b: 1` flag indicating their count is below the new list's
50-post threshold; the UI renders these as "<50".
"""

import json
import os
import sys

OLD = os.path.join(os.environ["TEMP"], "old-anima-tags.json")
NEW = r"src/lib/assets/anima-tags.json"


def main() -> int:
    with open(OLD, encoding="utf-8") as f:
        old = json.load(f)
    with open(NEW, encoding="utf-8") as f:
        new = json.load(f)

    existing = {e["n"] for e in new}
    merged = list(new)

    added = 0
    for e in old:
        if e["n"] in existing:
            continue
        entry = {"n": e["n"], "c": e["c"], "p": e.get("p", 0), "b": 1}
        if e.get("a"):
            entry["a"] = e["a"]
        merged.append(entry)
        added += 1

    with open(NEW, "w", encoding="utf-8") as f:
        f.write("[\n")
        for i, m in enumerate(merged):
            f.write(json.dumps(m, ensure_ascii=False))
            if i < len(merged) - 1:
                f.write(",")
            f.write("\n")
        f.write("]\n")

    artists = sum(1 for m in merged if m["c"] == 1)
    below = sum(1 for m in merged if m.get("b"))
    print(f"Merged: {len(merged)} entries (new: {len(new)}, added from old: {added})")
    print(f"Artists total: {artists}")
    print(f"Below-threshold flagged: {below}")

    nbs = [m for m in merged if "nekobungi" in m["n"].lower()]
    print(f"nekobungi entries: {len(nbs)}")
    for m in nbs[:5]:
        print(" ", m)
    return 0


if __name__ == "__main__":
    sys.exit(main())
