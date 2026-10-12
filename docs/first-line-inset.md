# Optional first-visual-line inset

`first_line_inset::FirstLineInsetProvider` is an optional host projection on
`TextSizeCapability`. It receives the immutable candidate document, canonical
inline-bearing node, and resolved inherited block font size in logical pixels.
The host can inspect ancestors; eligibility, units, attribute interpretation and
context rules remain entirely outside the engine. No canonical data is changed.

## Shared admission and lifetime

Construct the capability with a fixed `TextSizeStyleProvider` and the window's
native text system, then call `with_first_line_inset_provider`. Share the same
configured capability with the final `SessionPolicy::validate_document` check
and `EditorInstance::with_text_size_capability`. Font-size marks are not needed:
uniform paragraphs can opt into first-line indentation independently of inline
size changes. The fixed style provider is required so policy and rendering
measure exactly the same native font, base size, line height, runs and colors.
The capability does not install policy for the host.

Every initial document and final edit, transaction, import, Undo and Redo candidate
must pass that policy. Preedit is separately admitted against the same resolved
style before display/commit. A rejected overlay cannot silently commit the rejected
replacement. Existing content-unavailable native-input guards remain active.
Provider callbacks must be pure, deterministic and non-reentrant. To replace or
remove a provider or change theme/configuration, rebuild and revalidate the editor
with a matching policy and view; mutating a callback's captured configuration is
unsupported. The crate-internal capability attachment clears layout, bounds, caret
and cache identities. Different editor views own independent geometry caches.

## Geometry

An absent provider or zero (including negative zero) leaves the previous layout
route unchanged. Positive inset uses real first-row wrapping, not a paint offset,
synthetic spaces, extra atoms, or a GPUI fork. `MAX_FIRST_LINE_INSET_PX` is 65,536
logical pixels: negative, non-finite and over-budget values fail admission, without
clamping or rewriting data. The limit bounds presentation arithmetic, not schema.

The first visual row has available wrap width `max(logical_wrap_width - inset, 0)`;
every continuation uses the full logical width received during measurement. A hard break does not restart the inset, and a
leading hard break consumes it. An oversized first indivisible shaped cluster
is placed on the first row and overflows; no automatic empty row is inserted.
Empty blocks retain the existing base strut and a consistent native caret origin.
This is an engine contract, not a claim about browser caret parity.

Each row's final origin is recomputed idempotently as
`inset + alignment.offset(allocated_width - inset, row_width)`, with inset zero after
row zero. Signed center/right overflow behavior matches the existing alignment
capability. There is no justification feature. The shared row origin and actual
row heights drive glyph/decorations paint, selection, caret, hit testing, reading
highlights, Home/End, vertical movement and native UTF-16/IME bounds. Intrinsic
extent includes the first-row inset.

There are deliberately two widths, following stock GPUI 0.2.2: Taffy can round
final allocation after logical-width measurement. Wrapping and its cached row
partition retain the exact request width; alignment uses the final allocated box.
Prepaint never relabels a width-sensitive opt-in shape as measured at a different width,
and never reflows after height has been allocated. It recomputes shared row origins
idempotently in the allocated box on every frame, including cache hits. Different
logical widths remain distinct cache identities even when they round to the same
outer bounds. Intrinsic probes and preedit have no reusable key. Cache identity
also includes exact resolved inset, style and sizes. This is native layout behavior,
not a claim that rounded outer bounds equal the logical shaping width.

## Conservative support and evidence limits

Positive inset forces the existing conservative real-row renderer even for uniform
text. Latin, CJK, kana, Hangul, allowed common/inherited characters and supported
emoji extended grapheme clusters are admitted only when native observable cluster
geometry is reliable. Complex/bidi scripts, unsupported controls, unsafe size
seams, failed shaping and the existing 1 MiB/4096-call per-block work budget fail
closed before publication. Zero-inset uniform Unicode retains stock GPUI behavior.
No unrestricted Unicode first-line layout or browser line-break equivalence is
claimed. Original browser observations inform first-row-only and narrow-overflow
semantics; an observed leading-break widget interaction is not reproduced as an
invented universal blank-row rule.

Deterministic GPUI tests use its virtual text system and cover row shaping requests,
geometry, admission, cache and input contracts. Actual platform fonts, glyph ink,
window interaction and native IME require the separate opt-in `editor_harness
--first-line-inset` checkpoint. Do not treat virtual tests as native acceptance.
