#!/usr/bin/env python3
"""Verify the narrow decoder override, its upstream provenance, and stock transport."""
from pathlib import Path
import hashlib
import json
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "vendor/xim-ctext"
URL = "https://github.com/zed-industries/xim-rs.git"
REV = "16f35a2c881b815a2b6cdfd6687988e84f8447d8"
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
FILES = {"Cargo.toml", "README.md", "src/lib.rs", "src/main.rs", "LICENSE"}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    provenance = json.loads((VENDOR / "PROVENANCE.json").read_text())
    vendor_manifest = tomllib.loads((VENDOR / "Cargo.toml").read_text())
    errors = []
    if manifest.get("patch", {}).get("crates-io", {}) != {"xim-ctext": {"path": "vendor/xim-ctext"}}:
        errors.append("root patch must override only the reviewed local decoder")
    for name, version, source in [
        ("gpui", "0.2.2", REGISTRY),
        ("zed-xim", "0.4.0-zed", REGISTRY),
        ("xim-ctext", "0.3.0", None),
    ]:
        found = [(p["version"], p.get("source")) for p in lock["package"] if p["name"] == name]
        if found != [(version, source)]:
            errors.append(f"{name}: expected one {version} from {source}, got {found}")
    if any(p.get("source", "").startswith("git+") for p in lock["package"]):
        errors.append("the decoder vendor must not add Git dependencies")
    test_lock = tomllib.loads((VENDOR / "Cargo.lock").read_text())
    for name in ["encoding_rs", "cfg-if"]:
        production = [p for p in lock["package"] if p["name"] == name]
        standalone = [p for p in test_lock["package"] if p["name"] == name]
        if standalone != production:
            errors.append(f"standalone upstream tests must use the production {name} lock entry")
    if (provenance.get("repository"), provenance.get("revision")) != (URL, REV):
        errors.append("unreviewed upstream identity")
    if provenance.get("not_registry_release") is not True:
        errors.append("Git-source baseline must not be represented as the registry release")
    if vendor_manifest["package"]["version"] != "0.3.0" or provenance.get("upstream_declared_version") != "0.3.0":
        errors.append("upstream package version must not be relabeled")
    for key in ["upstream_sha256", "vendored_sha256"]:
        if set(provenance.get(key, {})) != FILES:
            errors.append(f"unexpected file set in {key}")
    for name in FILES:
        if digest(VENDOR / name) != provenance.get("vendored_sha256", {}).get(name):
            errors.append(f"unrecorded vendor edit: {name}")
        if name != "src/lib.rs" and provenance.get("upstream_sha256", {}).get(name) != digest(VENDOR / name):
            errors.append(f"upstream file changed outside the narrow decoder patch: {name}")
    patch = VENDOR / "patches/0001-preserve-literal-segments.patch"
    if digest(patch) != provenance.get("local_patch_sha256"):
        errors.append("unrecorded local patch edit")
    # Reversing the published patch must reconstruct the recorded original.
    # Work only in a temporary directory; never edit the vendored source.
    with tempfile.TemporaryDirectory(prefix="xiaomu-decoder-provenance-") as temp:
        path = Path(temp) / "src/lib.rs"
        path.parent.mkdir()
        path.write_bytes((VENDOR / "src/lib.rs").read_bytes())
        result = subprocess.run(["git", "apply", "--reverse", str(patch)], cwd=temp, capture_output=True, text=True)
        if result.returncode or digest(path) != provenance["upstream_sha256"]["src/lib.rs"]:
            errors.append("local patch does not reconstruct the recorded upstream decoder")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("XIM decoder provenance: ok (audited local patch; stock GPUI/zed-xim; no Git source exception)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
