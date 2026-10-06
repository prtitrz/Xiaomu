# Opt-in measured column resize

`xiaomu-gpui` supplies a per-`DocumentView` presentation capability. Core and
Runtime APIs, canonical width representation, persistence, and the default
editor remain unchanged. Install `TableColumnResize` with
`DocumentView::set_table_column_resize(Some(...))` and independently enable
measured table layout. `None` cancels and disables it.

## Host contract

`TableColumnResizeConfig` requires explicit `handle_width` (logical pixels on
either side of an actual cell edge), `min_column_width` (integer logical pixels),
and `last_column_resizable`. Invalid configurations are inert. The explicit
minimum can be 25 even though the native automatic-track minimum remains 40.
This does not claim browser intrinsic/min-content sizing parity.

`TableColumnResizeIntent` contains the table, zero-based logical column,
measured initial width, current requested width and captured document revision.
The read-only guard receives this intent and the current `DocumentSession` at
start, before every preview, on rendering, and before commit. The host must
verify editability, current owner/note/session and its lifecycle generation.
An in-place replacement with identical snapshot data has no observable generic
session generation; document revision and node IDs do not establish a host lease.

The commit callback receives the intent and the actual mutable `DocumentView`,
`Window` and `Context<DocumentView>`. No preview or session borrow survives into
it. The host owns logical-column-to-origin mapping, preservation of other attrs,
policy checks, a single transaction, error handling and storage. In particular,
`apply_edit_transaction` runs final document validation, not typed-intent
preflight. Equal `SetNodeAttrs` steps should be omitted explicitly when the host
wants a no-op; a measured unchanged width may still need to materialize a
missing/null/zero width hint. A callback is never retried.

## Geometry and lifecycle

Only successful current measurements supply handles. Full cell geometry and
Core placements define visible edges, including rowspan-covered rows and the
last logical column of a colspan. Internal logical boundaries inside a merged
cell are not fictitious handles. Parent tables register before descendants;
the innermost containing table owns a hit. The existing seven-pixel cell-range
handle retains priority. Scroll clipping and outside-left boundaries are checked.
A successful drag focuses this editor's existing selection without changing it.

### Measured-table overflow ownership

Every measured table, including a nested table and a view with resize callbacks
disabled, owns its own horizontal viewport. The viewport is constrained by the
same-frame available width; the table keeps its full exact track widths and its
height still comes from real child layout. A 280-pixel inner table in a 250-pixel
parent cell has 226 pixels available after the existing horizontal padding.
Overflow is reachable by native horizontal wheel/trackpad input without a visible
scrollbar, canonical width shrink, document edit, selection change or history unit.

The nearest hovered viewport consumes a horizontal-dominant event only when its
clamped offset changes. Pure vertical events and vertically dominant diagonals
continue to the document; there is no implicit vertical-to-horizontal conversion.
An event that reaches an edge has one owner; a later event with no possible
movement can pass to an outer table/document. Residual deltas are not redistributed.
Offsets are keyed by the mounted table element and owning view, and removed
elements do not retain viewport state. No stock GPUI patch or runtime selector is used.

Actual prepaint offsets drive full cell/text/caret/native-input bounds and resize
measurements. Separate visible clips keep hidden overflow out of pointer hit
candidates without truncating full navigation geometry. A scrolled resize may
change the scroll range: only the deterministic clamp of the original down offset
to the actual device-rounded content/viewport extent is admitted. Grow→shrink→grow
reuses that original offset; unrelated scrolling, viewport-origin changes and the
existing document/owner/composition guards still cancel. Passive horizontal
scrolling does not force IME unmark or commit. These bounds checks are virtual
platform evidence, not a native candidate-placement result or a new horizontal
scroll-to-caret command.

The original editor's per-table `.tableWrapper` uses horizontal overflow, but its
CSS intrinsic table sizing and browser wheel chaining are distinct. This native
viewport contract does not claim pixel-identical browser geometry or scrolling.

Measured starting widths must be positive integers at most 1,000,000; fractional
starts remain unsupported. Native pointer coordinates may be fractional. Widen
the received `f32` coordinates separately to `f64`, subtract the original down
from the current position, add the integral initial width, clamp to the explicit
minimum, then round the positive target to nearest integer (exact halves upward).
Every move and release uses the original down, never accumulated rounded deltas.
Nonfinite coordinates and unrounded targets above 1,000,000 are refused. Widening
does not recover precision already lost by the platform's `f32` representation.

This is an explicit native integer projection policy, not original-editor parity:
ProseMirror tables 1.8.5 `draggedWidth` preserves fractional deltas/results. Neither
canonical fractional widths nor a host's fractional JSON are silently converted.
In stock GPUI 0.2.2 X11, the 16.16 event coordinate decode divides by 65,535;
709→749 physical pixels at scale 1 arrive as approximately 709.0108→749.0114.
The former exact-integrality test cancelled this ordinary 40-pixel gesture.
The native projection yields 280 from initial 240 and that received delta.

Preview overrides only the requested track in a copied layout plan; other auto
tracks retain the native remaining-space policy. Actual child trees remeasure,
wrap, position and publish native caret geometry at the new width, including
nested tables. No canonical mutation, revision, selection, listener or history
operation occurs during preview.

The gesture binds its session pointer, complete immutable snapshot, table key,
available width and measured origin. Guard refusal, changed snapshot/revision,
composition, configuration changes, failed/stale geometry, view replacement or
disposal discard it. Mouse-leave alone does not cancel. Window-wide capture
handles release outside the editor and the next button-free motion. A right-button
release cannot complete the left-button gesture; duplicate downs do not restart
it. Frame handlers retain weak entities and deferred completions carry an exact
gesture token.

## Release ordering and current boundary

When the release width equals the actual successfully measured preview, the
callback runs synchronously during release. If its resulting document differs
only in `colwidth` on that table's direct origin cells, and the new canonical
plan exactly matches the measured tracks and placements, the view transfers
that actual layout's admission to the new key. Every other node, content, kind
and attr must be identical. This makes an immediate native text edit before
repaint retain both the resize and text as separate Undo units. Different
canonical tracks or unrelated changes require a fresh layout.

A release-only coordinate that was not measured by the preceding move must
first complete a real child-layout frame. Its single callback then runs after
that frame, with all guards rechecked. An intervening document edit or owner
change cancels this pending request. This is an observable ordering boundary,
not exact ProseMirror mouseup parity; no input buffering, forced render during
dispatch, custom unmark protocol or GPUI fork is introduced to conceal it.
The normal measured-release path is independently covered without a repaint
between release and native input. Hosts must not describe the unmeasured case
as synchronously committed.

## Evidence boundary

Pure geometry tests and GPUI virtual-platform tests exercise real callback and
layout paths, including exact preview widths, wrapped child/caret geometry,
spans/covered rows/nesting, default-off behavior, pointer noninterference,
stale/lifecycle refusal, outside-editor release, history, and callback order.
These tests are not native desktop GUI, browser CSS parity, or host persistence
acceptance. Product policy, original-editor equivalence, saves/cold reopen and
native interaction evidence remain separate consumer gates.
