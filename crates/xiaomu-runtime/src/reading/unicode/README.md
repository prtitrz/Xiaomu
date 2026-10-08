# Pinned simple Unicode case folding

The reading matcher uses Unicode 17.0.0 default simple folding (CaseFolding.txt
statuses C and S). It excludes full multi-scalar expansions (F) and Turkic
special mappings (T). It does not normalize text. Source UTF-8 is never folded
into a replacement string: streaming KMP compares scalar keys and retains the
original source boundaries in a query-sized ring.

Data source: <https://www.unicode.org/Public/17.0.0/ucd/CaseFolding.txt>

- Version: 17.0.0, source date 2025-07-30
- SHA-256: ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183
- License: `LICENSE-UNICODE`, Unicode License V3, from
  <https://www.unicode.org/license.txt>
- Generated table: `../case_fold_data.rs`, 1,512 mappings
- Regenerate offline with `python crates/xiaomu-runtime/src/reading/unicode/generate.py`

The generator verifies the exact source checksum, sorted unique source scalars,
single-scalar targets and idempotence. The permanent tests compare every valid
Unicode scalar against the complete pinned source, test both directions of every
C/S mapping through the public matcher, and compare the streaming matcher with
an independent naive scalar search over deterministic randomized texts.

A host comparing these results with another platform must record that platform's
Unicode version. In particular, regex-syntax 0.8.11's Unicode 16 tables omit new
Unicode 17 equivalence classes. The public UNICODE_SIMPLE_FOLD_VERSION makes
this version contract explicit; upgrading it requires an intentional data and
compatibility-test update. A locale's language-sensitive casing is not part of
this API.
