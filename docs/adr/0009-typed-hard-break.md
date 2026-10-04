# ADR 0009: Typed hard breaks retain identity and marks

Status: Accepted for the canonical foundation; full product editing is not enabled
Date: 2026-10-04

## Evidence and decision

An external structured document can contain both a literal LF inside a text
node and an independently marked hardBreak node. Mapping both to TextRun LF
loses that distinction. This meets ADR0004's reconsideration condition; its
host-neutral default LF command remains compatible, but LF is no longer the
only possible canonical line-break representation.

Reuse the existing inline-atom identity, placement and ordinal coordinate
system. `AtomKind::hard_break()` creates a typed builtin, while
`AtomKind::new("hardBreak")` remains an ordinary, unequal extension. `as_str()`
is a diagnostic/extension label, not a serialization discriminator. The
builtin requires empty NodeAttrs and an exact LF fallback. InlineAtomContent
now carries a real MarkSet; existing constructors keep empty marks. Marks are
not encoded into host attributes or an external sidecar.

Literal TextRun LF retains its bytes and meaning. The builtin consumes no text
bytes and has its own NodeId and seam ordinal. Multiple breaks at one text
offset remain independently addressable and marked.

## Atomic structure and wire

`SplitInlineNode { at: InlinePoint }` splits at an exact seam; legacy SplitNode
still refuses atom-bearing nodes. Mixed JoinNodes retains atom identity and
offset order. New map variants include seam ordinal compensation.
RestoreJoinedNode verifies the exact suffix and restores the old right node's
identity/kind/attrs. Its split map also fixes pure-text join inverse positions.

Clipboard v10 is selected only for a builtin or nonempty atom marks. Typed kind
and marks are explicit mandatory fields; legacy v4–v9 atom bytes stay unchanged.
Same-named extension kinds are never upgraded. Encoding and decoding share
16 MiB metadata, 100,000 JSON values and 128 JSON depth resource limits, plus
duplicate-key rejection on original bytes. These are explicit new resource
limits, not claims that all oversized legacy payloads were always invalid.
Markdown paths that cannot represent an atom reject it rather than flattening;
CodeBlock now has the same guard as Paragraph/Heading.

## GPUI projection and unfinished work

The builtin renderer intrinsically emits display LF and cannot be replaced by
an extension renderer. Canonical atom marks participate in display segments;
the builtin has no chip decoration or host activation. Stock GPUI shape_text
performs logical-line layout. Selected LF receives a visible end-of-line marker,
including consecutive empty rows; soft wraps add no canonical LF marker.

The current tests establish projection, ordinal round trips, line topology and
preedit splice behavior on GPUI's virtual backend, not real-font metrics or OS
IME acceptance. Runtime split/marks/paste integration, host DTO adaptation,
full editing and native geometry validation remain subsequent work. Existing
product profiles stay closed to unsupported hardBreak content until then.

The logical InsertLineBreak intent introduced previously still defaults to an
isolated LF insertion. A product policy must explicitly choose typed insertion;
no old text is silently migrated. Existing fixed GUI candidates are unchanged.

## Verification

Local foundation checkpoint: all 774 workspace tests and strict Clippy pass,
including 10 canonical, 9 mixed-structure, 13 clipboard, 2 Markdown and 4 GPUI
projection tests. Independent read-only review found no confirmed blocker.
This is a local source stage, not a claim of complete native hard-break support
or an upstream publication. Logs: `/workspace/shared/xiaomu-hard-break-*`.
