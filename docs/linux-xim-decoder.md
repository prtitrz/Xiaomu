# Stock-GPUI Linux COMPOUND_TEXT decoder repair

This decoder-only change is separate from [Linux unmark preservation](linux-unmark-preservation.md). GPUI stays at crates.io `=0.2.2`; `zed-xim` stays at crates.io `0.4.0-zed`. There is no GPUI transport patch, custom input owner, pointer wait gate, or DirectIBus change.

## Why an explicit vendor is necessary

Registry `xim-ctext 0.3.0` rejected valid Chinese COMPOUND_TEXT, causing stock `zed-xim` to panic before the result reached Xiaomu. The official Zed Git revision `16f35a2c881b815a2b6cdfd6687988e84f8447d8` adds CJK decoding and genuinely declares package version 0.3.0, but is **not the crates.io 0.3.0 release**. It initially passed six vectors and a bounded Chinese native test, then independent review found ordinary ASCII data loss. It was rejected as a release candidate.

Reproduced examples without an ESC byte:

- `%@` → empty
- `%Ghello` → `hello`
- `$(` → `InvalidEncoding` (the stock transport's expect can still panic)

Published 0.4.1 reproduced the same defects, additionally `(Bhello` → `hello`; it also does not satisfy the existing transport's `^0.3` constraint. We do not relabel a 0.4 package as 0.3. The explicit vendor retains the genuine 0.3.0 manifest, original files and complete MIT license from [the identified official Git revision](https://github.com/zed-industries/xim-rs/tree/16f35a2c881b815a2b6cdfd6687988e84f8447d8/xim-ctext).

`vendor/xim-ctext/PROVENANCE.json` contains full origin/revision, original and patched file hashes. The complete local source diff is `patches/0001-preserve-literal-segments.patch`. The patch:

1. Handles the first unescaped segment as literal default-charset text
2. Preserves text following an actual UTF-8-end escape
3. Backports the actual ESC `(B`/`(J` branches from published 0.4.1, preventing charset-return marker leakage. The unchanged original long-Japanese test demonstrates the need
4. Returns `InvalidEncoding` instead of arithmetic overflow for high-bit bytes in the existing Chinese/Korean 7-bit conversion

Existing encoding_rs lossy/replacement behavior elsewhere is unchanged. The `(J` interpretation follows upstream 0.4.1's Latin-1 behavior, not a claim of comprehensive JIS Roman semantics. This is not a complete COMPOUND_TEXT rewrite. Stock zed-xim still uses expect on decoding errors; **not every malformed/unsupported native payload is made panic-safe by this decoder repair**.

## Embedding hosts must apply a root override

```toml
[patch.crates-io]
xim-ctext = { path = "vendor/xim-ctext" }
```

Cargo `[patch]` belongs to the top-level workspace/application and is **not inherited from a dependency**. Hosts must include the same audited vendor (preserving its license/provenance/patch) and set their own root path override. A checkout-based host can point at the vendor within that checkout instead. Commit the host's resolved lockfile. Merely adding Xiaomu or copying its library manifest/lockfile is insufficient.

Verify `cargo tree -i xim-ctext` resolves the audited local path, while `gpui` and `zed-xim` remain registry packages. Do not revert to the rejected Git pin solely because its declared version is the same. An eventual official fixed release requires a separately validated compatible update.

## Policy and tests

`deny.toml` keeps strict unknown-Git/registry denial with no new Git allowlist. `tools/check_xim_decoder_source.py` checks the root override, stock transport versions/sources, vendor hashes and declared version, and reverses the local patch in a temporary directory to reconstruct the recorded original hash. CI runs this alongside [cargo-deny's source/license/bans policy](https://embarkstudios.github.io/cargo-deny/checks/sources/cfg.html).

Synthetic regressions cover empty/ASCII; every printable two-byte ASCII prefix; GB2312 Chinese, Japanese and Korean; literal prefixes followed by real escaped segments/reset suffixes; mixed CJK/ASCII; UTF-8 extension boundaries; all high-bit bytes in Chinese/Korean conversions; and selected malformed sequences returning stable errors rather than decoder panics. These are generated vectors, not captured private-session payloads. The original upstream eight tests also run separately because the vendor is excluded from workspace membership. No assertion generalizes this bounded matrix to all encodings or malformed payloads.

## Historical native evidence and pending vendor gate

The earlier **rejected Git-pin** combined candidate, SHA256 `a33c0f7852d10de86d188d83cfe1260260150a0a9f9d01f5b128fa80ddbd6d59`, had bounded Linux/X11 IBus/libpinyin success: left `你好` and right `世界` Space commits once each; unmark before normal focus switching, no wait; Escape cancellation; one-unit Undo; and a Chinese result after 100 ordinary left/right switches, without the old patched-route context budget or the earlier unsupported-Chinese panic. Exit/cleanup was 0. Those observations remain true **but do not establish that decoder as release-safe or substitute for vendor validation**.

Official default `display-style=0` visually showed candidate `你好` as application preedit and retained it on clicking right (left revision 1, right 0). That default-style text observation was not independently byte-inspected. Earlier experimental style 1 supplied literal `ni hao |`, confirmed by copying to a plain-text app. Xiaomu preserves exactly the IME-provided preedit; it does not guess which characters to strip. A small empty candidate/arrow panel remained in one default-style run, a runtime UI limitation rather than canonical text. None of this claims perfect GUI behavior, Windows/macOS/Wayland native acceptance, or full production WorkspaceStore integration.

Audited-vendor validation on Linux: **529 workspace tests**, including **15 decoder regression tests**, plus all **8 original upstream tests** passed. The standalone upstream-test lock pins the same encoding_rs/cfg-if entries as production. Clippy (`-D warnings`), formatting, source-size/dependency-boundary guards, the provenance/reverse-patch guard, and cargo-deny bans/licenses/sources all passed. These are the vendor's own results; the historical six-vector Git-pin result is not reused as its proof. Native execution with the exact vendored candidate has not yet been recorded; keep that separate from the earlier rejected-pin observations.
