# ADR 0007: Preserve exact typed link attributes

Status: Accepted (local experiment)
Date: 2026-10-03

## Context

Structured documents distinguish missing, explicit null and string values for
link href, target, rel, class and title. A two-field href/title link cannot
represent those states. Sidecars or host-only mappings would split canonical
document truth and lose data through editing, copying, Undo or serialization.

## Decision

- Add frontend-neutral `StringAttribute::{Missing, Null, Value(String)}` and
  `LinkAttributes` with five independent typed fields. Empty strings are values;
  Core does not infer defaults, normalize tokens, validate URI schemes or open URLs.
- `LinkMark::from_attributes` / `attributes` preserve every field. The existing
  `new(href, title)` convenience constructor means a string href, missing
  target/rel/class, and a missing or string title. `href()` now returns
  `Option<&str>` rather than fabricating a string for missing/null. Value-only
  accessors are explicitly documented as projections; exact codecs use attributes.
- `classic_parts()` returns href/title only when that old representation is
  lossless. Markdown and fixture adapters share this check rather than each
  forgetting a null or extra attribute. Unrepresentable values are rejected.
- Full attributes participate in mark equality, duplicate normalization,
  semantic-kind conflict checks and ordinary AddMark/RemoveMark inverses. There
  is no link sidecar and no host-specific Core branch.
- Clipboard uses v8 only when a link cannot be represented by classic href/title.
  The new `link_attributes` mark carries an `attrs` object with all five fields.
  Each field is an explicit tagged `missing`, `null` or `string` value; string
  values carry `value`. Omitted fields, unexpected fields, wrong types and
  duplicate keys reject the whole fragment. Empty struct variants enforce the
  same strictness for missing/null tags as for string tags.
- A fragment containing new link marks uses v8 even when mixed with ordinary
  links, node null attributes, nested quote/list content or table/row attributes.
  Classic links retain the existing v4/v5/v6/v7 choice. Valid v4-v7 payloads keep
  their old interpretation, including the legacy link title's null/missing
  equivalence; v1-v3 remain unsupported. New marks in a pre-v8 envelope fail closed.
- Markdown refuses extended or null link attributes. Classic link round trips
  retain the existing form. The heading serializer cannot reconstruct link marks,
  so Heading+Link is also explicitly rejected rather than silently dropping it.

This is an additive canonical capability with a deliberate 0.x Rust accessor
change. Existing semantic snapshots map losslessly through the old constructor;
Core has no serialized bytes to migrate. `DocumentVersion::CURRENT` remains 1,
while the actual private clipboard representation has its own v8 boundary.

## Alternatives considered

Keeping href mandatory would lose missing/null. Substituting an empty string
would conflate three distinct states. Loose attribute maps would weaken the
known-field type contract. Adding fields to the old clipboard Link DTO would let
some older readers silently truncate data. Emitting v8 for every document would
unnecessarily break structured interoperability for classic links.

## Consequences

Host codecs must explicitly convert and validate their own link fields. Hosts
decide display, navigation and security policy; the engine stores link semantics
without executing them. Older exporters can refuse via `classic_parts()` and
keep the original canonical document untouched. Rust callers of `href()` must
handle absence explicitly; fixtures and frozen binaries are separate artifacts.

## Verification scope

Dedicated source regressions cover the 243 combinations of five tri-state
attributes, empty/Unicode strings, conflicts and inverse/history restoration,
mixed/nested clipboard v8, legacy versions, malformed and duplicate wire data,
Markdown fail-closed behavior and classic round trips. Build and platform results
are recorded separately by the integration coordinator; this ADR does not imply
native link interaction or URL-opening acceptance.

Local integration checkpoint, 2026-10-03:619 workspace tests plus8 vendored
decoder tests, strict Clippy/fmt, source-size/dependency/provenance guards and
cargo-deny bans/licenses/sources pass. Logs are in
`/workspace/shared/xiaomu-links-checks`. This includes GPUI link decoration,
guarded host transactions, Linux unmark-before-form-MouseDown callback ordering,
and the independent horizontal selection-collapse repair. Independent source
review found no remaining blocker. Actual OS Link UI/Chinese URL acceptance is
still owned by the consumer's native GUI checkpoint; no URL is opened by Core.
