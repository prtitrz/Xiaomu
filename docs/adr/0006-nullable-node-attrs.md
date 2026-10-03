# ADR 0006: Preserve explicit null node attributes

Status: Accepted
Date: 2026-10-03

## Context

External structured formats can distinguish an absent attribute from an explicit null (for example, a paragraph's default `textAlign: null`). Dropping that value or encoding it as an extension sentinel breaks lossless document round trips.

## Decision

- Add `AttrValue::Null` to the existing non-exhaustive canonical value enum. It is a value, including in nested lists/objects; `Some(&AttrValue::Null)` differs from a missing key's `None`. Replacing `NodeAttrs` without the key remains the removal operation.
- Keep Core independent of serde and host formats. Do not add floating-point values or relax node-specific typed image rules. Known image string fields now explicitly reject present non-string values (including Null and Bool) instead of the earlier projection treating them as absent; unknown extension keys remain untouched. This prevents Markdown export from silently dropping an invalid title or second source. Existing snapshots need no migration, so `DocumentVersion::CURRENT` remains 1; a Rust enum extension and a clipboard format version are separate compatibility surfaces.
- Encode null as the private tagged wire value `{"type":"null"}`. Only fragments containing null use clipboard v7. Inspect node attrs, inline-atom attrs, table row attrs, and every nested list/object and subtree. Null-free fragments continue to write v4/v5/v6 according to their existing features.
- Continue reading valid v4/v5/v6 payloads and add v7. Reject null carried in pre-v7 envelopes, unknown attribute variants, and malformed values as a whole fragment, preserving the frontend's existing plain-text fallback. The baseline already rejected clipboard v1–v3; this change does not remove a legacy reader or add one.
- Reject unknown structural wire fields at every DTO level, including tagged null values, nested attrs, runs and marks. This deliberately tightens v4–v6 decoding: earlier serde DTOs could silently ignore extra structural fields, whereas such payloads now fail soft. Valid old payloads remain supported. Arbitrary keys in canonical attribute/object maps remain supported and preserved; they are data rather than DTO fields.
- Reject duplicate JSON object keys recursively before DTO deserialization. An allocation-light validation visitor checks keys without constructing a replacement payload; normal parsing then reads the original bytes. This deliberately rejects ambiguous v4–v6 payloads that previously used last-key-wins behavior, including a null overwritten by another value before feature-version checking. Escaped spellings of the same key count as duplicates.

## Alternatives considered

Omitting null, using a string sentinel, or storing it outside the canonical tree loses the null/missing distinction or creates parallel document state. Writing every fragment as v7 needlessly disables structured paste into older readers. Reusing v4–v6 for a new variant relies solely on older serde rejection and weakens the existing feature-version contract; conditional v7 makes that boundary explicit while preserving all null-free interoperability.

## Consequences

Generic transactions, snapshots and history continue to clone/compare canonical values without special null logic. Clipboard round trips preserve null under newly allocated node/atom IDs. Consumers that do not represent null must reject it rather than omit it; the Markdown codec and harness fixture retain their existing fail-closed boundaries. GPUI dependencies and platform input are unaffected.

## Revisit when

A future persistence codec or canonical value extension needs a migration or additional wire capabilities; version its actual representation rather than conflating it with this additive value.

## Verification

2026-10-03 Linux: 544 workspace tests and the original decoder's 8 tests pass;
strict Clippy, formatting, source-size, dependency boundaries, decoder provenance,
and cargo-deny bans/licenses/sources pass. Dedicated regressions cover explicit
null/missing, split/join/Undo/Redo, fresh clipboard IDs, conditional versions,
unknown wire fields, recursive duplicate keys, and typed-image/Markdown rejection
without mutation. This library change has no GPUI input or vendor modifications;
consumer rich-text round trips and native GUI acceptance are separate gates.
