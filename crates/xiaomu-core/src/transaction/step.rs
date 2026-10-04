//! Typed transaction steps.

use crate::document::{
    AtomKind, InlineAtomContent, Mark, MarkKind, MarkSet, Node, NodeAttrs, NodeContent, NodeId,
    TableRect,
};
use crate::selection::InlinePoint;
use crate::text::{TextOffset, TextRange};

/// One typed canonical mutation inside a [`Transaction`](super::Transaction).
///
/// Steps are declarative: they describe *what* changes, and the applying
/// engine decides how to preserve invariants. Structural validity is checked
/// against the target snapshot during application, so a step that is valid
/// for one revision may be rejected for another.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransactionStep {
    /// Replaces `[range.start, range.end)` of one inline node's text with
    /// `replacement`. An empty replacement deletes the span.
    ///
    /// This step keeps the text-only contract: it fails closed when the
    /// closed boundary span touches an inline atom, because a bare
    /// [`TextRange`] cannot distinguish the caret gaps around same-boundary
    /// atoms. Atom-seam edits use [`TransactionStep::ReplaceInlineText`].
    ReplaceText {
        /// Inline-bearing node whose concatenated text is edited.
        node: NodeId,
        /// Half-open byte range into the node's concatenated text.
        range: TextRange,
        /// UTF-8 replacement text; may be empty.
        replacement: String,
    },
    /// Atom-aware mixed-inline text replacement at an exact caret gap.
    ///
    /// Replaces `[at.text_offset(), end)` of `at.node_id()`'s concatenated
    /// text with `replacement`, where `at` addresses the canonical caret gap
    /// that begins the edited span. `at.atom_index()` counts how many
    /// same-boundary atoms stay before the edit; the gap itself is the
    /// boundary between them and the region being replaced:
    ///
    /// ```text
    /// A [atom] B   (atom anchored at text offset 1)
    /// (1, 0) + "X" over [1, 1) → A X [atom] B
    /// (1, 1) + "X" over [1, 1) → A [atom] X B
    /// ```
    ///
    /// Placement rules:
    ///
    /// - atoms anchored before `at.text_offset()`, and seam atoms with
    ///   ordinal `< at.atom_index()`, keep their anchor;
    /// - for an empty range (pure insertion at the seam), seam atoms with
    ///   ordinal `>= at.atom_index()` move after the replacement text;
    /// - atoms anchored at or after `end` shift by the byte-length delta.
    ///
    /// The step fails closed instead of deleting atomic content: a non-empty
    /// replacement whose region contains an atom (strictly inside, or a seam
    /// atom with ordinal `>= at.atom_index()`) is rejected, and callers must
    /// remove such atoms with [`TransactionStep::RemoveInlineAtom`] first.
    /// `at.affinity()` is visual bookkeeping and does not change the
    /// canonical outcome.
    ReplaceInlineText {
        /// Exact mixed-inline caret gap where the edited span starts.
        at: InlinePoint,
        /// Half-open end of the edited span in the node's concatenated text.
        ///
        /// The end boundary stays text-only: atoms anchored exactly at `end`
        /// sit after the replaced region and always survive it, so no
        /// ordinal is needed to address them.
        end: TextOffset,
        /// UTF-8 replacement text; may be empty.
        replacement: String,
    },
    /// Inserts one new canonical inline atom at an exact mixed-inline gap.
    ///
    /// `at.node_id()` is the inline-bearing parent. `at.text_offset()` is a
    /// UTF-8 text boundary and `at.atom_index()` selects one of the `0..=N`
    /// gaps around the atoms already anchored at that boundary. The new atom
    /// receives a fresh stable [`NodeId`].
    InsertInlineAtom {
        /// Exact mixed-inline gap receiving the new atom.
        at: InlinePoint,
        /// Stable semantic atom kind.
        kind: AtomKind,
        /// Extension payload carried by canonical node attributes.
        attrs: NodeAttrs,
        /// Host-neutral canonical atom content.
        content: InlineAtomContent,
    },
    /// Removes one canonical inline atom by stable identity.
    ///
    /// The atom must currently be referenced by exactly one inline parent.
    RemoveInlineAtom {
        /// Inline-atom node to remove.
        atom: NodeId,
    },
    /// Replaces one inline atom's independent canonical marks.
    ///
    /// The target must be an inline-atom node. Its identity, semantic kind,
    /// attributes, fallback text and placement remain unchanged. No positions
    /// move; the inverse restores the exact previous mark set. Core does not
    /// impose host-specific mark exclusion rules.
    SetInlineAtomMarks {
        /// Inline-atom node whose marks are replaced.
        atom: NodeId,
        /// Complete replacement mark set, including exact mark attributes.
        marks: MarkSet,
    },
    /// Restores a previously removed inline atom with its exact identity.
    ///
    /// This inverse-oriented step mirrors [`TransactionStep::RestoreSubtree`]:
    /// callers cannot mint the [`Node`] identity, every restored identity must
    /// currently be absent, and the final snapshot still passes full document
    /// validation. `at` addresses the exact gap in the post-removal parent.
    RestoreInlineAtom {
        /// Exact mixed-inline gap receiving the restored atom.
        at: InlinePoint,
        /// Previously removed canonical atom node payload.
        node: Node,
    },
    /// Inserts a newly allocated node as a child of `parent` at `index`.
    ///
    /// `index` counts existing children before the insertion point. The new
    /// node receives a fresh stable `NodeId` from the document allocator.
    InsertNode {
        /// Existing parent whose child list gains one entry.
        parent: NodeId,
        /// Number of children before the insertion point.
        index: usize,
        /// Kind of the created node.
        kind: crate::document::NodeKind,
        /// Attributes of the created node.
        attrs: NodeAttrs,
        /// Content of the created node; child references must already exist.
        content: NodeContent,
    },
    /// Inserts a whole `rows × columns` table as a child of `parent`.
    ///
    /// Core allocates the table, its rows, its cells, and one empty
    /// paragraph per cell with fresh stable identities; the structural
    /// invariants (uniform column count, non-empty cells) hold in every
    /// snapshot this step can produce. Intermediate table shapes cannot be
    /// expressed through validated `InsertNode` staging, so construction is
    /// a semantic step the way `InsertInlineAtom` is.
    InsertTable {
        /// Existing parent whose child list gains the table.
        parent: NodeId,
        /// Number of children before the insertion point.
        index: usize,
        /// Number of rows; must be at least one.
        rows: usize,
        /// Number of columns; must be at least one and is shared by all rows.
        columns: usize,
    },
    /// Inserts a complete captured table tree with entirely fresh identities.
    ///
    /// Header/body kinds, spans, raw attributes, nested tables, rich blocks,
    /// inline-atom order and independent marks are preserved. Source canonical
    /// IDs never enter the target. Core checks the parent/index, whole fresh-ID
    /// range and combined destination/template table budgets before one batch
    /// insertion. The map names the real new table root. Undo removes it; Redo
    /// restores that same newly allocated tree exactly.
    InsertTableTree {
        /// Existing structural parent receiving the table.
        parent: NodeId,
        /// Number of existing children before the insertion point.
        index: usize,
        /// Bounded immutable template captured from a validated source table.
        tree: super::TableTreeTemplate,
    },
    /// Replaces a closed logical rectangle with a captured table's cell forest.
    ///
    /// The source logical dimensions must equal the rectangle. The target
    /// table, row identities, attributes and row list stay intact; cells outside
    /// the rectangle and their descendants are unchanged. Source outer table
    /// and row wrappers are omitted, while every cell and descendant receives
    /// a fresh identity, including nested table wrappers and inline atoms.
    /// Cell kinds, raw attributes, content and independent marks are preserved.
    ///
    /// Invalid/nonclosed bounds, mismatched dimensions, exhausted identities
    /// and excessive final aggregate grids fail atomically. Maps delete every
    /// removed descendant and record actual physical cell insertion positions.
    /// Undo restores the exact old tree; Redo reuses the first fresh identities.
    ReplaceTableRect {
        /// Existing table whose logical rectangle is replaced.
        table: NodeId,
        /// Nonempty closed rectangle in the target's logical coordinates.
        rect: TableRect,
        /// Complete immutable table template supplying cells and descendants.
        tree: super::TableTreeTemplate,
    },
    /// Inserts one row into `table` at `index`, matching its established
    /// column count.
    ///
    /// Core allocates the cells and their empty paragraphs with fresh
    /// stable identities. The step map names the actual inserted row;
    /// Runtime resolves a caret target inside that subtree when needed.
    /// `index` may be the current row count to append.
    InsertTableRow {
        /// Existing table gaining one row.
        table: NodeId,
        /// Number of existing rows before the insertion point.
        index: usize,
    },
    /// Inserts one column into `table` at `index`.
    ///
    /// Every row gains one cell (with one empty paragraph) at `index`, so
    /// the uniform column count holds in the produced snapshot. Like
    /// [`TransactionStep::InsertTable`], this cannot be expressed through
    /// validated staging — a single row gaining a cell makes the table
    /// ragged — so it is a semantic step. Each row gets a `NodeInserted`
    /// map naming its new cell, so gaps in every row map correctly.
    InsertTableColumn {
        /// Existing table gaining one column.
        table: NodeId,
        /// Number of existing cells (columns) before the insertion point
        /// in every row.
        index: usize,
    },
    /// Inserts a logical row, extending every rowspan crossing its boundary.
    ///
    /// Uncovered columns receive fresh unit cells and empty paragraphs. New
    /// cells have empty attrs; host-specific defaults, type borrowing and width
    /// reconciliation are explicit caller policy. Existing content and cell
    /// identities survive. A fully covered new row may be physically empty.
    InsertTableRowLogical {
        /// Existing checked table, including spanning tables.
        table: NodeId,
        /// Logical row boundary in `0..=row_count`.
        index: usize,
        /// One Cell/Header kind per logical column, including covered slots.
        /// The vector must match logical width; non-cell kinds are rejected.
        cell_kinds: Vec<crate::document::NodeKind>,
    },
    /// Inserts a logical column, extending every colspan crossing its boundary.
    ///
    /// A crossing cell's explicit width list receives zero at the new column;
    /// missing/null widths stay unchanged. Each uncovered row receives a fresh
    /// unit cell with empty attrs and one empty paragraph. Existing cell and
    /// descendant identities survive. Type/default/repair policy is external.
    InsertTableColumnLogical {
        /// Existing checked table, including spanning tables.
        table: NodeId,
        /// Logical column boundary in `0..=column_count`.
        index: usize,
        /// One Cell/Header kind per logical row, including covered slots.
        /// The vector must match logical height; non-cell kinds are rejected.
        cell_kinds: Vec<crate::document::NodeKind>,
    },
    /// Deletes a nonempty proper half-open range of logical rows.
    ///
    /// Intersecting rowspans shrink by the removed overlap. If an origin row
    /// disappears while its cell still covers surviving rows, that original
    /// cell and all content move to the first surviving row below the range.
    /// Fully removed cells lose their whole subtrees, captured in the exact
    /// inverse. Empty or all-row deletion is rejected without changing state;
    /// hosts may expose it as an unhandled/no-op product command.
    DeleteTableRowsLogical {
        /// Existing checked table.
        table: NodeId,
        /// Inclusive first logical row to remove.
        start: usize,
        /// Exclusive last logical row to remove; at most the row count.
        end: usize,
    },
    /// Deletes a nonempty proper half-open range of logical columns.
    ///
    /// Intersecting colspans and explicit width lists lose just the overlap;
    /// surviving cell identities, non-geometric attrs and content remain.
    /// Fully covered cells and their subtrees are removed with an exact inverse.
    /// Empty or all-column deletion is rejected atomically.
    DeleteTableColumnsLogical {
        /// Existing checked table.
        table: NodeId,
        /// Inclusive first logical column to remove.
        start: usize,
        /// Exclusive last logical column to remove; at most the column count.
        end: usize,
    },
    /// Merges a closed logical rectangle containing at least two cell origins.
    ///
    /// The geometric top-left cell keeps its identity, kind and non-geometric
    /// attributes. All blocks move, without copying or filtering, into it in
    /// logical origin order. Spans become the rectangle's dimensions; a width
    /// list retains the survivor's entries and appends unspecified zeros.
    /// Missing/null width attributes remain distinct. Partial intersecting
    /// spans and out-of-grid rectangles fail atomically. Product-specific
    /// empty-block filtering and width reconciliation belong outside Core.
    MergeTableCells {
        /// Existing table containing the entire rectangle.
        table: NodeId,
        /// Nonempty half-open rectangle in logical row/column coordinates.
        rect: TableRect,
    },
    /// Splits one spanning cell into unit cells over its current rectangle.
    ///
    /// The top-left cell keeps its identity and all content. Each other slot
    /// receives a freshly allocated cell and empty paragraph. Every cell keeps
    /// the original kind and non-geometric attrs; width lists are sliced by
    /// column. A unit cell is rejected as a no-op. Core does not repair other
    /// cells' widths or rewrite missing/null attributes into defaults.
    ///
    /// Before copying attrs, a separate expansion budget limits projected
    /// output-cell attribute storage to 64 MiB of accounted key/string/value
    /// payload, one million values and 64 nesting levels. Exceeding it returns
    /// `TableResourceLimit`; it is not a bound on all execution memory.
    SplitTableCell {
        /// Existing table containing the cell origin.
        table: NodeId,
        /// Existing physical cell to split, including Header cells.
        cell: NodeId,
    },
    /// Applies a guarded exact inverse produced by a semantic table-cell edit.
    ///
    /// Stale affected payloads or live identities to be restored cause atomic
    /// rejection. Existing moved block identities and payloads are preserved.
    RestoreTableCells {
        /// Engine-produced inverse payload; callers cannot construct one.
        restore: super::TableCellRestore,
    },
    /// Removes `node` together with its whole subtree from the document.
    ///
    /// The root cannot be removed.
    RemoveNode {
        /// Node to remove.
        node: NodeId,
    },
    /// Replaces all attributes of `node` with `attrs`.
    SetNodeAttrs {
        /// Node whose attributes are replaced.
        node: NodeId,
        /// Complete replacement attribute set.
        attrs: NodeAttrs,
    },
    /// Replaces the semantic kind of `node`, keeping its identity, attributes,
    /// and content.
    ///
    /// The new kind must accept the node's existing content shape, and the
    /// parent must still allow the node as a child. The document root cannot
    /// change kind. Positions do not move.
    SetNodeKind {
        /// Node whose kind is replaced.
        node: NodeId,
        /// Replacement semantic kind.
        kind: crate::document::NodeKind,
    },
    /// Applies `mark` to `[range.start, range.end)` of one inline node.
    ///
    /// A conflicting mark of the same kind inside the range is replaced.
    AddMark {
        /// Inline-bearing node being marked.
        node: NodeId,
        /// Half-open byte range into the node's concatenated text.
        range: TextRange,
        /// Mark to apply.
        mark: Mark,
    },
    /// Re-inserts a previously removed subtree under `parent` at `index`,
    /// keeping every original node identity and payload.
    ///
    /// This step exists so that undo can restore a removed subtree exactly;
    /// its intended producer is [`AppliedTransaction::inverse`](super::AppliedTransaction::inverse).
    /// It is **not** a general-purpose copy or move primitive:
    ///
    /// - every identity in `nodes` must currently be absent from the store,
    ///   so the step can never duplicate or overwrite live nodes;
    /// - node identities cannot be minted by callers, so `nodes` can only
    ///   carry payloads obtained from snapshots of the same document
    ///   lineage — in practice, payloads that this document previously
    ///   removed;
    /// - `root` must be one of `nodes`, and the re-attached subtree must
    ///   pass full-tree validation like any other step.
    ///
    /// Violations fail application atomically with `InvalidTransaction` or a
    /// validation error. Its mapping data is a `NodeInserted` entry carrying
    /// the subtree root.
    RestoreSubtree {
        /// Existing parent whose child list gains the subtree root.
        parent: NodeId,
        /// Number of children before the re-inserted root.
        index: usize,
        /// Identity of the subtree root within `nodes`.
        root: NodeId,
        /// The removed nodes with their original payloads, in deterministic
        /// identity order.
        nodes: Vec<Node>,
    },
    /// Removes every mark of `kind` from `[range.start, range.end)` of one
    /// inline node.
    RemoveMark {
        /// Inline-bearing node being unmarked.
        node: NodeId,
        /// Half-open byte range into the node's concatenated text.
        range: TextRange,
        /// Semantic kind of the marks to remove.
        mark_kind: MarkKind,
    },
    /// Splits one inline-bearing node at `at` in its concatenated text.
    ///
    /// The original node keeps its identity and the text before `at`; a
    /// freshly allocated sibling with the same kind and attributes receives
    /// the text from `at` onward and enters the parent's child list directly
    /// after it. Splitting inside a run gives both halves that run's marks;
    /// splitting exactly at a run boundary leaves each whole run on one
    /// side. Either resulting half may be empty.
    ///
    /// This legacy text-only step rejects any atom-bearing node; use
    /// [`TransactionStep::SplitInlineNode`] to address an exact atom seam.
    SplitNode {
        /// Inline-bearing node being split; must not be the document root.
        node: NodeId,
        /// UTF-8 byte offset of the split point; a validated boundary of
        /// the node's concatenated text (zero through total length
        /// inclusive).
        at: TextOffset,
    },
    /// Splits an inline-bearing node at an exact mixed-inline caret gap.
    ///
    /// Text before `at.text_offset()` and same-boundary atoms before
    /// `at.atom_index()` remain in the original node. The remaining text and
    /// atoms move to a freshly allocated following sibling with the same
    /// kind and attributes. Atom identities and payloads never change;
    /// moved placements subtract the split byte offset. Either side may
    /// contain no text or no atoms. Affinity does not affect the split.
    SplitInlineNode {
        /// Validated UTF-8 boundary and atom ordinal of the split.
        at: InlinePoint,
    },
    /// Restores a sibling absorbed by [`TransactionStep::JoinNodes`].
    ///
    /// This inverse-oriented split restores the exact absent identity,
    /// kind, and attributes of `node`. Its inline content must equal the
    /// suffix currently following `at`, including atom identities and
    /// placements. The suffix atoms stay live and change parent; this step
    /// cannot duplicate, replace, or resurrect an atom payload. As with
    /// [`TransactionStep::RestoreSubtree`], callers cannot mint node IDs.
    /// Invalid identity, content, seam, or parent shape fails atomically.
    RestoreJoinedNode {
        /// Exact gap in the surviving inline node where the sibling began.
        at: InlinePoint,
        /// Previously absorbed inline-bearing sibling from the old snapshot.
        node: Node,
    },
    /// Merges two adjacent inline-bearing siblings into one.
    ///
    /// `second` must be the child immediately following `first`. The merged
    /// node keeps `first`'s identity, kind, and attributes; its inline
    /// content is the normalized concatenation of both contents. Atoms keep
    /// their identities and payloads; `second`'s placements shift by the
    /// byte length of `first` and follow its end-anchored atoms. Only the
    /// absorbed inline node leaves the document. Undo restores its exact
    /// identity, kind, attributes, and content via
    /// [`TransactionStep::RestoreJoinedNode`].
    JoinNodes {
        /// Surviving sibling whose child position stays put.
        first: NodeId,
        /// Sibling immediately after `first` that is absorbed into it.
        second: NodeId,
    },
}
