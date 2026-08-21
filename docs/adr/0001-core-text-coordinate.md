# ADR 0001: Core text coordinates use validated UTF-8 byte offsets

Status: Accepted
Date: 2026-08-22

## Context

Xiaomu needs one Core text coordinate that can be used consistently by document positions, selections, transactions, mapping, and history.

Several coordinate systems are plausible:

- UTF-8 byte offsets;
- Unicode scalar-value indexes;
- UTF-16 code-unit offsets;
- grapheme-cluster indexes.

Rust `str` and `String` are UTF-8. Platform text APIs, especially on Windows, may expose UTF-16 ranges. User-visible caret movement may eventually need grapheme-aware behavior. Those facts describe different boundaries and should not be collapsed into one integer contract.

A raw byte offset would be unsafe as a public semantic coordinate because arbitrary integers can point inside a UTF-8 code point. A permanently trusted offset would also be incorrect because edits can make a coordinate stale even if it was valid in an earlier text revision.

## Decision

`xiaomu-core` uses an opaque `TextOffset` whose numeric representation is a UTF-8 byte index.

Normal external callers cannot construct `TextOffset` from an arbitrary `usize`. They obtain coordinates through `TextBuffer`, which validates bounds and UTF-8 character boundaries.

When an existing offset or range is used with a buffer, the buffer validates it again. A coordinate validated against one revision is not assumed to remain valid against another revision.

`TextRange` is half-open `[start, end)` and must be ordered. Expected invalid coordinates and ranges return typed errors instead of panicking.

UTF-16 conversion belongs to platform/frontend adapters and is not part of the Core coordinate contract.

The Core text boundary guarantees Unicode scalar safety. It does not define grapheme-cluster caret movement. Grapheme-aware navigation and deletion may be implemented at a higher editing layer while still resolving to validated `TextOffset` values for Core operations.

## Alternatives considered

### UTF-16 code-unit offsets

This matches some platform APIs but would make the Core representation platform-shaped and require constant conversion around Rust strings. It is therefore confined to platform adapters.

### Unicode scalar-value indexes

This avoids exposing byte units conceptually, but converting scalar indexes to Rust string byte positions would require scanning or an additional index structure for common operations. It also does not solve grapheme semantics.

### Grapheme-cluster indexes

Grapheme clusters are relevant to user-visible cursor behavior, but they are not a stable low-level storage coordinate. Their segmentation rules add complexity and do not match Rust string slicing boundaries directly.

### Public raw byte offsets

This is simple but allows callers to construct coordinates inside a UTF-8 code point and spreads validation responsibility across the codebase.

## Consequences

Positive:

- Core coordinates align with Rust string storage and slicing boundaries;
- UTF-8 validity checks are centralized;
- UTF-16 remains an adapter concern;
- future storage changes can remain behind `TextBuffer` as long as the semantic coordinate contract is preserved or deliberately revised;
- stale-coordinate validation is explicit rather than relying on accidental trust.

Costs and constraints:

- callers that receive UTF-16 positions must convert them at the frontend boundary;
- grapheme-aware editing requires a higher-level segmentation layer;
- a `TextOffset` may become invalid after an edit and therefore cannot be treated as a permanent anchor;
- future performance work may need indexing support if the underlying text storage changes.

## Revisit when

Revisit this decision if one of the following becomes true:

- profiling shows that preserving UTF-8 byte coordinates prevents an otherwise necessary text-storage architecture;
- collaborative or persistent position identity requires a fundamentally different canonical coordinate model;
- grapheme-level semantics prove impossible to layer cleanly over validated Core offsets;
- a future frontend requires a cross-platform coordinate contract that cannot be isolated in adapters without significant correctness or performance cost.
