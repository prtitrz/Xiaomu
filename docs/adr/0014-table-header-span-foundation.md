# ADR 0014: table identities and checked logical occupancy

Status: accepted safety foundation, 2026-10-04. Logical span commands and
rendering follow this foundation; guards and placeholders are not completion.

## Canonical model

`TableHeader` is a distinct built-in kind with the same block-content shape as
`TableCell`. `is_table_cell()` recognizes both. Raw attrs remain authoritative;
typed readers preserve missing/null/value and never refill defaults. Missing
spans mean one; explicit null, noninteger, nonpositive and unaddressable spans
are refused as geometry. Column widths preserve missing/null or a nonnegative
integer list matching colspan, with zero meaning unspecified. Product colors,
alignment and conflict/repair policies remain outside Core.

`XiaomuDocument::table_grid` exposes a checked logical grid, placements and
unique origins separately. Physical rows may be ragged or empty if prior
rowspans cover every logical slot. Placement stops at the first free slot and
rejects overlap, holes or overrun; it never shifts a colliding cell to repair it.
Nested tables have independent geometry. All arithmetic and allocation bounds
are checked first.

Snapshot-wide aggregate bounds are one million logical slots, one hundred
thousand physical cells and 64 MiB of accounted grid buffers, including nested
and sibling tables. These bound grid work, not all process memory. Very large
older documents may now return `TableResourceLimit`. Ordinary-parent standalone
Cell/Header is also refused; only TableRow admits these built-ins (Custom's
existing extension behavior remains). This closes a pre-existing Cell hole and
does not claim universal zero-impact schema compatibility.

## Safe transition of consumers

The existing rectangular row/column steps keep empty body-cell defaults and
refuse spans before changing data. Runtime cell-range, row/column, navigation
and rich-paste paths similarly refuse still-physical operations on spans.
Inline text/marks and exact Undo inside canonical cells remain possible in
headless Runtime. Header unit-cell reconstruction preserves node kind.

Clipboard v13 is selected for Header/geometric cell semantics; old plain
rectangles retain their previous envelope. Geometry cannot silently travel as
opaque attrs to a reader that only understands rectangular physical rows.
Open table TSV uses logical slots with empty covered fields; closed copy keeps
its existing leaf-text fallback and full structured subtree. Partial span
CellRange copy is not yet implemented and fails explicitly.

The old GPUI flex-row renderer cannot draw spans faithfully. It displays an
explicit placeholder instead of a wrong table. Rendered-only navigation and
focus exclude hidden descendants; retained native paragraph/range handlers
also check their own anchor and current selection, including after host changes
or Undo. A whole-table proxy exception is bound to that exact current node
selection; it cannot write through a later outside caret. Generic Runtime does
not inherit these temporary frontend restrictions.

## Evidence and continuation

### Semantic merge/split foundation

`MergeTableCells` requires a closed logical rectangle and keeps the geometric
top-left cell's identity and kind. Content block identities move in row-major
order; Core retains every block, including empty paragraphs. A product wanting
ProseMirror's blank-cell filtering must perform that policy explicitly and test
its complete command against the original factory. `SplitTableCell` retains the
origin's content and allocates new unit cells/paragraphs after aggregate limits
and the full identity range have been checked. Width entries are sliced by
column, and unrelated attributes are retained.

The opaque inverse records affected payloads, expected absence and maps. Apply
checks exact live payloads and their current membership in the specified table
before a batch exchange; moving an otherwise unchanged row to another valid
table invalidates the old inverse. Whole-tree validation still precedes
publication. Batch store exchange avoids cloning the entire node map per newly
allocated cell; these checks do not constitute a total execution-memory sandbox.

This increment passed 251 Core tests and strict Core all-target Clippy, including
15 new merge/split/budget/mapping and stale-inverse cases. It has no product
toolbar, row/column-span commands, native GUI or visual-parity claim yet.

Core236, Runtime452 and ten new GPUI virtual tests passed; workspace/all-target
tests total1034 and strict Clippy pass. Tests cover Header inverse, aggregate
budgets/overflow, covered empty rows, rejected mutation state, metadata versions,
logical TSV, hidden inputs, stale handler ownership and independent panes.
No native GUI, arbitrary CSS or completed span-layout claim is made.

Next work replaces the guards in coherent slices: unique-origin logical
CellSelection, semantic subtree construction/paste, measured span/column-width
layout and hit/IME geometry, then merge/split and logical row/column operations.
Each operation needs an identity-preserving inverse and one published history
unit. Original product repair-on-Undo behavior must be recorded rather than
assumed to restore raw JSON; Core never repairs persisted input implicitly.
