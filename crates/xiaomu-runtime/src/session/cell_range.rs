//! Rectangular cell geometry shared by commands, clipboard and frontends.
use super::SessionError;
use xiaomu_core::document::{NodeId, NodeKind, TableGrid, TableRect, XiaomuDocument};

/// A rectangular cell selection inside one table (P5.5).
///
/// Endpoints are cell identities, so row/column insertions retain the gesture
/// targets while logical coordinates are rebuilt from the current snapshot.
/// Canonical merge mapping moves absorbed endpoints to their survivor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellRange {
    anchor: NodeId,
    focus: NodeId,
}

impl CellRange {
    pub(crate) const fn new(anchor: NodeId, focus: NodeId) -> Self {
        Self { anchor, focus }
    }

    /// Validates both endpoint cells against `document`.
    ///
    /// Each endpoint must exist with cell content, and both must belong to
    /// the same table (their parent rows share the table parent).
    pub(crate) fn validate(&self, document: &XiaomuDocument) -> Result<(), SessionError> {
        let table_of = |cell: NodeId| -> Result<NodeId, SessionError> {
            if !matches!(
                document.node(cell),
                Some(node) if node.kind().is_table_cell()
            ) {
                return Err(SessionError::SelectionInvalid);
            }
            let row = document
                .parent_of(cell)
                .ok_or(SessionError::SelectionInvalid)?;
            if !document
                .node(row)
                .is_some_and(|node| matches!(node.kind(), NodeKind::TableRow))
            {
                return Err(SessionError::SelectionInvalid);
            }
            let table = document
                .parent_of(row)
                .ok_or(SessionError::SelectionInvalid)?;
            if !document
                .node(table)
                .is_some_and(|node| matches!(node.kind(), NodeKind::Table))
            {
                return Err(SessionError::SelectionInvalid);
            }
            Ok(table)
        };
        if table_of(self.anchor)? != table_of(self.focus)? {
            return Err(SessionError::SelectionInvalid);
        }
        Ok(())
    }

    /// The cell where the range gesture started.
    #[must_use]
    pub const fn anchor(self) -> NodeId {
        self.anchor
    }

    /// The cell where the range gesture currently ends.
    #[must_use]
    pub const fn focus(self) -> NodeId {
        self.focus
    }
}

impl CellRange {
    /// Returns the logical bounding rectangle of the complete endpoint cells.
    ///
    /// Coordinates are half-open and belong to the endpoints' common table.
    /// Reversing the endpoints leaves the rectangle unchanged. Other cells
    /// crossing its boundary do not expand it: callers requiring complete
    /// coverage must separately check [`Self::is_closed_rect`].
    pub fn logical_rect(self, document: &XiaomuDocument) -> Result<TableRect, SessionError> {
        self.checked_grid(document)?
            .rect_between(self.anchor, self.focus)
            .map_err(SessionError::Core)
    }

    /// Returns selected cell origins once each, in logical row-major order.
    ///
    /// This follows ProseMirror's `TableMap.cellsInRect`: a cell is selected
    /// exactly when its top-left origin lies inside [`Self::logical_rect`].
    /// Cells crossing into the rectangle from above or the left are excluded;
    /// cells originating inside and extending below or right are included.
    /// This differs from Core's `TableGrid::unique_cells_in`, which returns
    /// every intersecting cell, and from [`Self::cells`]' unit-cell matrix.
    /// Covered slots and entirely covered physical rows add no duplicates.
    /// Nested tables remain content of their outer cells, not range targets.
    pub fn unique_origins(self, document: &XiaomuDocument) -> Result<Vec<NodeId>, SessionError> {
        let grid = self.checked_grid(document)?;
        let rect = grid
            .rect_between(self.anchor, self.focus)
            .map_err(SessionError::Core)?;
        Ok(grid
            .origins()
            .filter(|cell| {
                (rect.top()..rect.bottom()).contains(&cell.row())
                    && (rect.left()..rect.right()).contains(&cell.column())
            })
            .map(|cell| cell.cell())
            .collect())
    }

    /// Whether every cell intersecting the logical rectangle is fully inside.
    ///
    /// A valid CellRange may be non-closed. This predicate does not normalize
    /// the rectangle or change either endpoint; geometry-dependent commands
    /// such as merge or partial structured copy must choose their own policy.
    pub fn is_closed_rect(self, document: &XiaomuDocument) -> Result<bool, SessionError> {
        let grid = self.checked_grid(document)?;
        let rect = grid
            .rect_between(self.anchor, self.focus)
            .map_err(SessionError::Core)?;
        Ok(grid.is_closed_rect(rect))
    }

    fn checked_grid(self, document: &XiaomuDocument) -> Result<TableGrid, SessionError> {
        self.validate(document)?;
        let table = document
            .parent_of(self.anchor)
            .and_then(|row| document.parent_of(row))
            .ok_or(SessionError::SelectionInvalid)?;
        document.table_grid(table).map_err(SessionError::Core)
    }

    /// Returns unit-cell rows of unique canonical cell identities.
    ///
    /// This is not a logical-slot API: merged cells can occupy several slots
    /// with the same identity. A table containing spans continues to return
    /// `UnsupportedTableOperation` rather than repeating identities or
    /// interpreting physical indexes. Use [`Self::unique_origins`] for
    /// origin-aware editing; this legacy matrix contract stays unchanged.
    ///
    /// Both endpoints must belong to one table. Descendant nested tables
    /// are content of their outer cells, not additional cells of this range.
    pub fn cells(self, document: &XiaomuDocument) -> Result<Vec<Vec<NodeId>>, SessionError> {
        self.validate(document)?;
        let locate = |cell| {
            let row = document
                .parent_of(cell)
                .ok_or(SessionError::SelectionInvalid)?;
            let table = document
                .parent_of(row)
                .ok_or(SessionError::SelectionInvalid)?;
            let rows = super::structure::children_of(document, table);
            let cells = super::structure::children_of(document, row);
            Ok::<_, SessionError>((
                table,
                rows.iter()
                    .position(|id| *id == row)
                    .ok_or(SessionError::SelectionInvalid)?,
                cells
                    .iter()
                    .position(|id| *id == cell)
                    .ok_or(SessionError::SelectionInvalid)?,
            ))
        };
        let (table, ar, ac) = locate(self.anchor)?;
        super::table::require_unit_grid(document, table)?;
        let (_, fr, fc) = locate(self.focus)?;
        super::structure::children_of(document, table)[ar.min(fr)..=ar.max(fr)]
            .iter()
            .map(|row| {
                let cells = super::structure::children_of(document, *row);
                cells
                    .get(ac.min(fc)..=ac.max(fc))
                    .map(<[NodeId]>::to_vec)
                    .ok_or(SessionError::SelectionInvalid)
            })
            .collect()
    }
}
