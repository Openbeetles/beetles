#!/usr/bin/env python3
"""
Remove standalone size="small" lines that belong to TextField / FormControl blocks.
Keeps size on Button, IconButton, Radio, Checkbox, etc.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "src"


def can_drop_size_small(lines: list[str], i: int) -> bool:
    line = lines[i]
    if not re.match(r"^\s*size=\"small\"\s*$", line):
        return False
    lookback = "".join(lines[max(0, i - 30) : i])
    if "<TextField" not in lookback and "<FormControl" not in lookback:
        return False
    # If the nearest opening tag before this line is Button/IconButton/Fab, keep small
    chunk = "".join(lines[max(0, i - 40) : i])
    last_tf = max(chunk.rfind("<TextField"), chunk.rfind("<FormControl"))
    last_btn = max(
        chunk.rfind("<Button"),
        chunk.rfind("<IconButton"),
        chunk.rfind("<Fab"),
        chunk.rfind("<LoadingButton"),
    )
    if last_btn > last_tf:
        return False
    # Inline Radio/Checkbox in FormControlLabel
    if "control={<Radio" in lookback or "control={<Checkbox" in lookback:
        return False
    return True


def process_file(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    changed = False
    for i, line in enumerate(lines):
        if can_drop_size_small(lines, i):
            changed = True
            continue
        out.append(line)
    if changed:
        path.write_text("".join(out), encoding="utf-8")
    return changed


def main() -> None:
    n = 0
    for path in sorted(ROOT.rglob("*.tsx")):
        if process_file(path):
            print(path.relative_to(ROOT.parent))
            n += 1
    print(f"updated {n} files", file=sys.stderr)


if __name__ == "__main__":
    main()
