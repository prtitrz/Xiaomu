# ADR 0009: Typed hard breaks retain identity and marks

Status: Accepted; canonical and native editor primitives implemented, product acceptance pending
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

## Editing and mark inheritance

Runtime SplitBlock and joins now retain complete InlinePoint coordinates and
map both selection endpoints, structural gaps and cell ranges. Range formatting
includes each selected atom's own marks as well as text runs; SetInlineAtomMarks
is one reversible Core step with no positional movement. These are host-neutral
rules: product-specific Code exclusion and mark ordering remain in the host.

`XiaomuDocument::inherited_inline_marks` uses the left contributing inline child
at an exact gap, or the right child when there is no left child. A typed break
with empty marks is a real contributor. Marked extensions also contribute;
legacy unmarked extensions stay transparent. The separate range query takes the
child after its start, and may return None at the end of content. Explicit Runtime
stored marks, including an empty set, override inheritance. Mixed typed input and
composition replacement use the range rule; plain paste uses the insertion-gap
rule. Text-only and legacy-unmarked-extension ranges keep their previous behavior.
Core transaction insertion and inverse restoration use the same exact-gap rules,
so Undo cannot acquire marks from a newly adjacent break.

The Runtime exposes exact input and composition queries for host toolbars and
preedit styling. Byte-only queries cannot invent an atom ordinal: hosts with an
InlinePoint should use the exact API. GPUI preedit uses the complete composition
range and agrees with the committed text at every tested break gap.

## GPUI projection and unfinished work

The builtin renderer intrinsically emits display LF and cannot be replaced by
an extension renderer. Canonical atom marks participate in display segments;
the builtin has no chip decoration or host activation. Stock GPUI shape_text
performs logical-line layout. Selected LF receives a visible end-of-line marker,
including consecutive empty rows; soft wraps add no canonical LF marker.

The current tests establish projection, ordinal round trips, line topology and
preedit splice behavior on GPUI's virtual backend, not real-font metrics or OS
IME acceptance. Native Ctrl+A includes trailing inline atoms, including a block
containing only breaks. Actual virtual-key Copy/Cut/Undo tests retain those atoms'
identity, marks and selection. This does not implement full-document selection
of leading/trailing atomic block images. Host DTO/policy integration and native
real-font/IME acceptance remain separate gates; old frozen candidates stay closed.

The logical InsertLineBreak intent introduced previously still defaults to an
isolated LF insertion. A product policy must explicitly choose typed insertion;
no old text is silently migrated. Existing fixed GUI candidates are unchanged.

## Verification

Local foundation checkpoint: all 774 workspace tests and strict Clippy pass,
including 10 canonical, 9 mixed-structure, 13 clipboard, 2 Markdown and 4 GPUI
projection tests. Independent read-only review found no confirmed blocker.
This is a local source stage, not a claim of complete native hard-break support
or an upstream publication. Logs: `/workspace/shared/xiaomu-hard-break-*`.

The subsequent editor checkpoint passes 829 workspace/all-targets tests. It adds
mixed structure/session mapping, mark transactions, plain paste, Core/Runtime
input inheritance and actual GPUI virtual-key selection/clipboard coverage.
Product command semantics are independently compared against the real consuming
application's editor factory; this library count does not claim product parity
or replace the pending real X11 GUI acceptance.
