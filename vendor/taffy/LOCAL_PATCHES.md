# Taffy 0.9.0 lazy min-content backport

This directory contains all 62 files from the official crates.io `taffy` 0.9.0
archive. `PROVENANCE.json` records the archive checksum, packaged Git revision,
every original and vendored file hash, and the local patch hash. The original
`Cargo.toml`, `Cargo.toml.orig`, `Cargo.lock`, package version, features, and public
API are unchanged. No Cargo cache marker or invented registry checksum is added.

The archive omitted its MIT license file. `LICENSE.md` is copied verbatim from
the same official Git revision; its source URL and SHA-256 are recorded separately.

## Only production change

`patches/0001-lazy-min-content-fallback.patch` changes one line in
`src/compute/flexbox.rs`: `unwrap_or({ ... })` becomes `unwrap_or_else(|| { ... })`.
The existing min-content computation is deferred until the main-axis minimum is
unresolved. Explicit minimums, and overflow modes that resolve an automatic
minimum to zero, skip that unused computation. The fallback body is unchanged.

This is the minimal backport of [upstream PR #1119](https://github.com/DioxusLabs/taffy/pull/1119),
merged as `55cda62a5df9a5d04c0023be6f6dd607b1474fe9`. GPUI remains exactly 0.2.2
and Taffy remains exactly 0.9.0; this is not a Taffy upgrade.

## Verification and boundaries

`python tools/check_taffy_source.py` verifies the complete file set, pinned
original hashes, one-file/one-line change, license, root patch and path lock
identity. It reverses the patch in a temporary tree to reconstruct all 62
official files. Run it from the Xiaomu repository, alongside the XIM source guard.

The permanent regression is outside this vendor directory because the published
manifest has `autotests = false`:

```sh
cargo test --locked -p xiaomu-gpui --test taffy_min_content
cargo test --locked --manifest-path vendor/taffy/Cargo.toml --target-dir target
```

The first command checks actual 0.9.0 `TaffyTree` callbacks, row/column minimum
and overflow controls, expected rounded/unrounded geometry, and warm-cache and
dirty-recompute behavior. The second runs the inline tests and doctests shipped
in the published crate, using its original standalone lock. The published
archive omits the full upstream integration/conformance suite; these commands
do not run that omitted suite. Keep build output outside the vendor directory.

Removing redundant library measurement does not establish a native application's
speedup. Host build, consumed-source verification and native acceptance are
separate gates; no shaping, layout geometry, application batching or input
protocol is intentionally changed by this backport.
