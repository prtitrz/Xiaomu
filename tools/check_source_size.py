#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
SCAN_ROOTS = (ROOT / "crates", ROOT / "examples")
WARN_LINES = 500
FAIL_LINES = 700
SKIP_PARTS = {"target", "tests", "benches", "fixtures", "vendor", "generated"}
GENERATED_MARKERS = ("@generated", "DO NOT EDIT")


def should_skip(path: Path) -> bool:
    relative = path.relative_to(ROOT)
    if any(part in SKIP_PARTS for part in relative.parts):
        return True

    try:
        head = "\n".join(path.read_text(encoding="utf-8").splitlines()[:8])
    except UnicodeDecodeError:
        return False

    return any(marker in head for marker in GENERATED_MARKERS)


def count_lines(path: Path) -> int:
    with path.open("r", encoding="utf-8") as handle:
        return sum(1 for _ in handle)


def main() -> int:
    warnings: list[tuple[Path, int]] = []
    failures: list[tuple[Path, int]] = []

    for scan_root in SCAN_ROOTS:
        if not scan_root.exists():
            continue
        for path in sorted(scan_root.rglob("*.rs")):
            if should_skip(path):
                continue
            lines = count_lines(path)
            if lines > FAIL_LINES:
                failures.append((path, lines))
            elif lines > WARN_LINES:
                warnings.append((path, lines))

    for path, lines in warnings:
        print(
            f"warning: {path.relative_to(ROOT)} has {lines} lines "
            f"(review threshold: {WARN_LINES})"
        )

    for path, lines in failures:
        print(
            f"error: {path.relative_to(ROOT)} has {lines} lines "
            f"(hard limit: {FAIL_LINES})",
            file=sys.stderr,
        )

    if failures:
        print(
            "Split oversized source files by responsibility or move dedicated "
            "tests/fixtures out of production source.",
            file=sys.stderr,
        )
        return 1

    print("source-size guard: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
