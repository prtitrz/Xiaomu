# Explicit clipboard leaf projection

`SessionPolicy::clipboard_export_spec` can select an LF projection for a finite
set of extension inline atoms. It uses the existing read-only policy callback;
there is no arbitrary serializer, supplied plaintext, or host I/O callback.

```rust
use xiaomu_core::document::AtomKind;
use xiaomu_runtime::clipboard::{ClipboardExportSpec, ClipboardTextProjection};

let projection = ClipboardTextProjection::text_between_lf_with_atoms([
    AtomKind::new("example.reference.v1").unwrap(),
]).unwrap();
let spec = ClipboardExportSpec::new().with_text_projection(projection);
```

A host can retain this immutable spec and return its clone for Copy. The
descriptor accepts 1–32 distinct extension kinds, with at most 128 UTF-8 bytes
per key. It rejects empty lists, duplicate kinds, and typed built-in HardBreak.
Iteration stops on the 33rd entry. Keys are sorted, with no trimming, case
folding, Unicode normalization, prefix matching, or inference from labels.
Extension `AtomKind::new("hardBreak")` is distinct from typed HardBreak and may
be explicitly listed. Clones share the bounded immutable descriptor.

## Projection and authority

`TextBetweenLfWithAtomsV1` retains `TextBetweenLfV1` block/leaf boundaries and
text bytes. Each allowed extension atom emits exactly one LF. Typed HardBreak
already emits LF. Other extension atoms and custom block semantics reject the
whole export. Adjacent atoms, atom-only paragraphs, empty blocks and table cell
forests follow the same deterministic traversal; no visible label, attribute,
URL or renderer fallback is substituted into plaintext.

The structured fragment retains each atom's exact kind, attributes, fallback
label, marks and position. Reading projection, U+FFFC emission, UTF-16 mapping,
document state, selection and history are independent and unchanged. Hosts
choose their own kind keys. No product-specific kinds or attribute rules are
built into Xiaomu.

The descriptor changes only clipboard text. It grants no editing, paste,
default table fitting, schema-admission or asset permission. Cell geometry
builders remain independently composable. Unknown source content cannot be
hidden by a label change. Existing destination policies and source-boundary
checks still apply to decoded rich content.

New descriptor exports reject ordinary `clipboard_slice_for(Cut)` before
source scanning, projection or platform writes. A host needing structural Cut
must supply an explicit [prepared Cut](prepared-table-cut.md) plan. That route
prevalidates the exact deletion candidate and lossless clipboard item before
writing once and publishing the same candidate. Read-only input still permits
Copy and blocks Cut before either policy or platform side effects.

The separate default-off `SessionPolicy::prepare_text_cut` hook also admits
verified nonempty inline-only ranges outside tables through the same prepared
guard. It does not open the public projection-only Cut endpoint; see the
[prepared Cut contract](prepared-table-cut.md#default-off-generic-text-cut).

Historical V1/legacy ordinary Cut behavior is unchanged; its separate fallible Delete
route does not gain prepared-Cut atomicity.

## Conditional framed v15

Only the new descriptor writes `xiaomu.clipboard.v15\n` followed by strict JSON.
Its body has version 15 and this required `text_projection` object:

```json
{"kind":"text_between_lf_with_atoms_v1","atom_kinds":["example.reference.v1"]}
```

The keys in this object always identify extensions. The kind is exactly a
string; externally tagged enum objects are rejected. Array entries must be
nonempty strings, strictly sorted, unique and within the constructor's limits.
Unknown fields, duplicate JSON keys (including escaped equivalents), unknown
descriptors, missing/null values, wrong types, extra roots in a CellRange carrier,
invalid source boundaries, over-budget data, and frame/body version mismatches
reject. The new envelope must be a JSON object, never a positional tuple array.

Decoding rebuilds the validated original structure and recomputes plaintext
using the descriptor; only byte equality with the platform text succeeds.
Encoding performs the same complete roundtrip, including descriptor and
provenance. Descriptor JSON-escaping and framing costs join borrowed source,
clipped-fill and fragment budgets before cloning or text expansion. The bounded
descriptor parser does not first buffer an untagged arbitrary JSON value.

V14 without the new descriptor keeps its previous encoding, including the V1
string and its rejection of extension atoms. V14 rejects v15 descriptors; v15
rejects absent or V1-only descriptors. Readers recognizing the framed family
reject unknown versions, so existing v14 readers reject v15 without falling
back to labels, plain text or images. Historical unframed v4–v13 and foreign
content retain their previous fallback rules. An unframed imitation of v15 is
not a valid native payload and retains that historical fallback boundary.

Framing and recomputation establish consistency, not authenticity. A coherent
third-party rewrite of both rich data and descriptor cannot be identified as a
forgery by this unsigned format. Decoded clipboard content remains untrusted
and must pass the existing destination policy. No signature or permissions are
implied by an allowlist appearing in incoming metadata.

## API migration and verification limits

`ClipboardTextProjection` has a new variant and is now `Clone`, not `Copy`.
Exhaustive matches must handle that variant; use `.clone()` if reusing an owned
projection. `ClipboardExportSpec` was already `Clone`, not `Copy`.
`with_text_projection`, `ClipboardExportSpec::text_projection`, and
`ClipboardSlice::text_projection` are no longer `const`; the getters keep their
owned return type and share descriptor storage. Existing V1 construction and
value comparisons remain source-compatible outside const/Copy-dependent code.

Regression tests cover strict wire/budget boundaries, rich roundtrip, reading
independence, old V1/default behavior, table/cross-block/multiatom projections,
and actual GPUI virtual keyboard Copy/Cut/Paste dispatch. Virtual dispatch and
stock clipboard-writer observations do not establish OS clipboard ownership,
real desktop input acceptance or downstream product acceptance.
