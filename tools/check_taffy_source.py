#!/usr/bin/env python3
"""Verify the complete official Taffy 0.9.0 import and its one-line backport."""
from pathlib import Path
import hashlib
import json
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "vendor/taffy"
URL = "https://github.com/DioxusLabs/taffy"
REV = "599b1b7d7410d97e13bafc4c01fbe6d7a9cf060a"
ARCHIVE = "a13e5d13f79d558b5d353a98072ca8ca0e99da429467804de959aa8c83c9a004"
# Canonical JSON of the 62 original path/hash pairs, independently bound to
# the official archive. Updating provenance alone cannot bless source changes.
FILE_MANIFEST = "faf839c1357f45178f7d071811fb6f3060b06ab3e1ffed86ed2e327715320fbf"
PATCH_SHA = "e8b2f8a00fbf5de8c010fc2917c4108e36d3129a38ac5c17060b5ea789f41b34"
FLEXBOX_SHA = "6d75d9176dd3b2fd4dc21ef0f6f842799f83f6b7a7de76f61e052eb47e3aab4d"
LICENSE_SHA = "432807ea9957f2c3fe7ed6590623fd02d3bb86a378e04b9ae3ba28d04e380aa9"
FLEXBOX = "src/compute/flexbox.rs"
PATCH = "patches/0001-lazy-min-content-fallback.patch"
ADDITIONS = {"LICENSE.md", "PROVENANCE.json", "LOCAL_PATCHES.md", PATCH}
ROOT_PATCHES = {
    "xim-ctext": {"path": "vendor/xim-ctext"},
    "taffy": {"path": "vendor/taffy"},
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    provenance = json.loads((VENDOR / "PROVENANCE.json").read_text())
    upstream = provenance["upstream_sha256"]
    vendored = provenance["vendored_sha256"]
    errors = []
    if manifest.get("patch", {}).get("crates-io", {}) != ROOT_PATCHES:
        errors.append("root patch must override exactly the reviewed decoder and Taffy copies")
    if "vendor/taffy" not in manifest.get("workspace", {}).get("exclude", []):
        errors.append("the unmodified upstream manifest must remain outside the workspace")
    entries = [p for p in lock["package"] if p["name"] == "taffy"]
    if len(entries) != 1 or entries[0].get("version") != "0.9.0" or any(
        key in entries[0] for key in ["source", "checksum"]
    ):
        errors.append("expected exactly one path Taffy 0.9.0 without a registry checksum")
    gpui = tomllib.loads((ROOT / "crates/xiaomu-gpui/Cargo.toml").read_text())
    if gpui.get("dev-dependencies", {}).get("taffy") != "=0.9.0":
        errors.append("the permanent regression must exercise exact Taffy 0.9.0")
    expected_identity = {
        "repository": URL,
        "revision": REV,
        "upstream_declared_version": "0.9.0",
        "archive_url": "https://static.crates.io/crates/taffy/taffy-0.9.0.crate",
        "archive_sha256": ARCHIVE,
        "upstream_file_count": 62,
        "upstream_file_manifest_sha256": FILE_MANIFEST,
        "upstream_fix": f"{URL}/pull/1119",
        "upstream_fix_merge": "55cda62a5df9a5d04c0023be6f6dd607b1474fe9",
        "license_file": "LICENSE.md",
        "license_url": f"https://raw.githubusercontent.com/DioxusLabs/taffy/{REV}/LICENSE.md",
        "license_sha256": LICENSE_SHA,
        "license_added_separately": True,
        "local_patch": PATCH,
        "local_patch_sha256": PATCH_SHA,
    }
    for key, expected in expected_identity.items():
        if provenance.get(key) != expected:
            errors.append(f"unreviewed provenance identity: {key}")
    canonical = json.dumps(upstream, sort_keys=True, separators=(",", ":")).encode()
    if len(upstream) != 62 or digest(canonical) != FILE_MANIFEST:
        errors.append("original file manifest does not match the reviewed official archive")
        # Do not use paths from an untrusted replacement manifest.
        print("\n".join(errors), file=sys.stderr)
        return 1
    expected_vendored = dict(upstream)
    expected_vendored[FLEXBOX] = FLEXBOX_SHA
    if vendored != expected_vendored:
        errors.append("vendored hashes must differ only by the reviewed flexbox line")
    paths = list(VENDOR.rglob("*"))
    if any(path.is_symlink() for path in paths):
        errors.append("vendored files and directories must not be symlinks")
    actual = {str(path.relative_to(VENDOR)).replace("\\", "/") for path in paths if path.is_file()}
    expected_files = set(upstream) | ADDITIONS
    if actual != expected_files:
        errors.append(f"missing vendor files: {sorted(expected_files - actual)}")
        errors.append(f"unexpected vendor files: {sorted(actual - expected_files)}")
    for name, expected in expected_vendored.items():
        path = VENDOR / name
        if not path.is_file() or digest(path.read_bytes()) != expected:
            errors.append(f"unrecorded vendor edit or missing file: {name}")
    for name, expected in [(PATCH, PATCH_SHA), ("LICENSE.md", LICENSE_SHA)]:
        path = VENDOR / name
        if not path.is_file() or digest(path.read_bytes()) != expected:
            errors.append(f"unrecorded patch/license edit or missing file: {name}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    # Reverse only in a temporary tree. Every restored official file must match,
    # so a patch cannot silently touch another part of the source import.
    with tempfile.TemporaryDirectory(prefix="xiaomu-taffy-provenance-") as temp:
        temp = Path(temp)
        for name in upstream:
            path = temp / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes((VENDOR / name).read_bytes())
        result = subprocess.run(
            ["git", "apply", "--reverse", str(VENDOR / PATCH)],
            cwd=temp, capture_output=True, text=True,
        )
        if result.returncode or any(digest((temp / name).read_bytes()) != sha for name, sha in upstream.items()):
            errors.append("reversing the patch did not reconstruct all 62 official files")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("Taffy provenance: ok (official 0.9.0; 62 files; one-line lazy fallback; original manifests)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, KeyError, ValueError, TypeError) as error:
        print(f"Taffy provenance: invalid or missing input: {error}", file=sys.stderr)
        raise SystemExit(1)
