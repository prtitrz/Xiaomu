#!/usr/bin/env python3
"""Keep the reviewed git2 advisory exposure outside production dependency graphs.

This supplements cargo-deny; it neither ignores advisories nor accepts new ones.
Run after dependency resolution, with the pinned toolchain. The graph includes
all workspace features and all target platforms, but excludes dev-only edges.
"""
import pathlib
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
packages = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
git2 = [package for package in packages if package["name"] == "git2"]
if not git2:
    print("git2 absent: remove or renew the advisory review record")
    sys.exit(0)
if len(git2) != 1 or git2[0]["version"] != "0.20.4":
    sys.exit("git2 version changed: refresh docs/dependency-advisories.md and this scope guard")
result = subprocess.run(
    ["cargo", "tree", "--locked", "--all-features", "--target", "all",
     "-e", "normal,build", "-i", "git2", "--prefix", "none"],
    cwd=ROOT, text=True, capture_output=True, check=False,
)
if result.returncode:
    sys.exit(result.stderr)
if result.stdout.strip():
    sys.exit("git2 entered the production graph; the test-only advisory review no longer applies:\n"
             + result.stdout)
print("git2 0.20.4 remains dev-only across all features/targets; advisories remain visible")
