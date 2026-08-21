#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
CRATES_DIR = ROOT / "crates"

# Allowed Xiaomu workspace dependencies for each production/support crate.
ALLOWED: dict[str, set[str]] = {
    "xiaomu-core": set(),
    "xiaomu-runtime": {"xiaomu-core"},
    "xiaomu-gpui": {"xiaomu-core", "xiaomu-runtime"},
    "xiaomu-codec-markdown": {"xiaomu-core"},
    "xiaomu-testkit": {"xiaomu-core", "xiaomu-runtime", "xiaomu-gpui"},
}


def dependency_tables(document: dict) -> list[dict]:
    tables: list[dict] = []
    for key in ("dependencies", "dev-dependencies", "build-dependencies"):
        value = document.get(key)
        if isinstance(value, dict):
            tables.append(value)

    targets = document.get("target")
    if isinstance(targets, dict):
        for target in targets.values():
            if not isinstance(target, dict):
                continue
            for key in ("dependencies", "dev-dependencies", "build-dependencies"):
                value = target.get(key)
                if isinstance(value, dict):
                    tables.append(value)
    return tables


def main() -> int:
    manifests: dict[str, Path] = {}
    documents: dict[str, dict] = {}

    for manifest in sorted(CRATES_DIR.glob("*/Cargo.toml")):
        with manifest.open("rb") as handle:
            document = tomllib.load(handle)
        package = document.get("package", {})
        name = package.get("name")
        if isinstance(name, str):
            manifests[name] = manifest
            documents[name] = document

    workspace_crates = set(manifests)
    errors: list[str] = []

    for crate, document in documents.items():
        if crate not in ALLOWED:
            errors.append(
                f"{crate}: new workspace crate has no declared dependency-boundary rule"
            )
            continue

        allowed = ALLOWED[crate]
        seen: set[str] = set()
        for table in dependency_tables(document):
            seen.update(name for name in table if name in workspace_crates)

        forbidden = sorted(seen - allowed)
        if forbidden:
            errors.append(
                f"{crate}: forbidden Xiaomu dependencies: {', '.join(forbidden)}; "
                f"allowed: {', '.join(sorted(allowed)) or '(none)'}"
            )

    production = {"xiaomu-core", "xiaomu-runtime", "xiaomu-gpui", "xiaomu-codec-markdown"}
    for crate in production:
        document = documents.get(crate)
        if document is None:
            continue
        for table in dependency_tables(document):
            if "xiaomu-testkit" in table:
                errors.append(f"{crate}: production crates must not depend on xiaomu-testkit")

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    print("dependency-boundary guard: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
