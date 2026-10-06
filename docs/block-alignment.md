# Optional block alignment presentation

`xiaomu-gpui::block_alignment` supplies a presentation-only
`BlockAlignmentProvider::alignment(&Node) -> BlockAlignment` seam.
`BlockAlignment` contains `Left`, `Center` and `Right`. Hosts install a provider
with `EditorInstance::with_block_alignment_provider` or replace/remove it through
`DocumentView::set_block_alignment_provider`. The callback receives the current
canonical inline-bearing node during child synchronization. It must be pure and
non-reentrant. Attribute names, applicable node kinds, inherited/default values,
admission, editing commands and persistence remain host concerns. No Core or
Runtime model, attribute, transaction or schema changes are introduced.

Changing a mounted provider, or state consulted by that provider, requires the
host to notify its GPUI context. Child synchronization resolves effective values
again; changed values invalidate that child's layout. Separate instances and
views retain independent providers. Frontend-only range input proxies do not
invoke the provider.

## Geometry and compatibility

No provider preserves the original left-aligned rendering and coordinate path.
An explicit provider, including one returning `Left`, enables the shared visual
row geometry. This opt-in also fixes exact hard-line-top hit testing and uses
the selected head's soft-wrap affinity for collapsed native bounds queries.
Those existing legacy edge behaviors are not silently changed for other hosts.

Each original GPUI `WrappedLineLayout` supplies logical ranges, soft-wrap
boundaries, glyph positions and visual-row advances. Row width is the next wrap
glyph's x minus the current row's source x, or the original unwrapped width minus
that source x for the final row. Trailing whitespace is retained. Empty lines
have zero width. Center/right offsets are `(container - row width) / 2` and
`container - row width`, including signed offsets for overwide rows.

The stock GPUI 0.2.2 glyph painter receives the same alignment and full text-box
width. Caret, empty and hard-break rows, both soft-wrap affinities, pointer hits,
selection rectangles, vertical motion and native UTF-16 bounds use the cached
rows. Nonempty native ranges enclose the selected visual rectangles, avoiding
unselected adjacent rows at wrap/LF endpoints; collapsed queries preserve the
focused head's affinity. Alignment, exact opt-in width, epoch and the complete resolved text/font/
paint style participate in cache identity. Final prepaint bounds refresh row
offsets even after an earlier measurement pass. Default legacy width quantization
is unchanged.

Stock GPUI's wrapped underline/strikethrough origins do not consistently follow
center/right row offsets. Only for those alignments, Xiaomu shapes a second
decoration-free carrier and replaces its public layout Arc with the untouched
original glyph/cluster/wrap Arc. Clearing decoration flags alone would change
GPUI FontRun segmentation and is insufficient. Cached source-cluster decoration
spans follow the original forward decoration state rather than individual
positioned-glyph advances, so negative offsets inside a cluster cannot extend
or overlap strokes. They use original colors, thickness, wavy underline flags and stock baseline
metrics, then paint through public GPUI primitives at each row offset. The
second shape and span construction happen on a cache miss, not on every paint.
There is no GPUI fork, unsafe code or mutation of a shared layout. Existing
inline-code background behavior is unchanged; this feature does not add a
background renderer.

## Tables and native input boundaries

Alignment is relative to a paragraph's full measured content box, never a
viewport's visible fragment. Existing table-cell padding, column resize reflow,
nested local horizontal scrolling, inherited paint masks and visible pointer
clips remain authoritative. Absolute native coordinates move with the painted
block. The existing post-paint coordinate notification observes alignment and
width changes through the resulting bounds.

This capability does not change composition cancellation/commit or pointer and
wheel ownership. It does not add a native candidate-window clipping contract for
hidden or partly visible text. Stock X11's synchronous queries may still observe
the previously painted layout; platform-specific candidate placement and real
font/IME behavior require independent native acceptance.

`Justify` is not supported or mapped to another alignment. Full justification
remains a required future feature with its own glyph-spacing, geometry and
native acceptance work; hosts must explicitly reject it until implemented.

## Regression scope

Deterministic virtual-GPUI tests cover unequal visual-row widths, empty and
consecutive/trailing LF rows, typed HardBreak seams, CJK/emoji/combining text,
synthetic nonzero glyph origins and cluster-internal marks, trailing whitespace,
overwide rows, changed widths/fonts/alignment, original/carrier Arc identity,
wrapped wavy/strike colors and baselines, native soft-wrap affinity, preedit
cancel/commit/undo, reversed selections, nested table scrolling and resize
preview cancellation. Virtual-font metrics and input callbacks do not establish
real-font shaping or OS candidate-window placement parity.
