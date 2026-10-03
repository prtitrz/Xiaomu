# Local decoder-only patch

Baseline is the **official Zed Git source**, revision
`16f35a2c881b815a2b6cdfd6687988e84f8447d8`, whose xim-ctext Cargo.toml genuinely
declares version 0.3.0. It is **not byte-identical to crates.io xim-ctext 0.3.0**.
The version and original manifest have not been changed to bypass semver.
The upstream crate files and full MIT license are retained; original/patched
SHA256 values are in PROVENANCE.json. The entire local source delta is
patches/0001-preserve-literal-segments.patch.

Changes:

1. Preserve the initial unescaped chunk literally in the default Latin-1
   charset; do not parse `%@`, `%G` or `$(` as if preceded by ESC
2. Preserve ASCII/default-charset text following an actual UTF-8-end escape
3. Backport the published 0.4.1 ESC `(B`/`(J` branches so valid charset-return
   markers do not leak into mixed CJK/ASCII text. Literal prefixes without
   ESC remain data because of change 1
4. Return InvalidEncoding for high-bit bytes that would overflow the existing
   GB2312/Korean 7-bit-to-8-bit conversion, rather than debug-panic/wrap

No GPUI or XIM transport/input lifecycle changes, no new encoding dependency,
and no claim of full COMPOUND_TEXT conformance or safety for every malformed
input. The stock zed-xim caller still uses expect on decoding errors; this
patch does not change that transport behavior.

The official Git revision and published 0.4.1 both failed the plain-prefix
regressions, so neither is silently substituted for this explicitly patched
copy. Regression vectors live in xiaomu-gpui/tests/compound_text.rs; the
original upstream unit tests remain in src/lib.rs.

The added Cargo.lock exists only to reproduce the excluded package's original
unit tests standalone. Existing encoding_rs replacement/lossy behavior is not
rewritten; checked conversion rejects one specific invalid high-bit case.
The `(J` mapping retains upstream 0.4.1's Latin-1 interpretation and is not a
new claim of complete JIS Roman conformance.
