//! Rectangular cell geometry shared by commands, clipboard and frontends.
use super::SessionError;
use xiaomu_core::document::{NodeId, NodeKind, XiaomuDocument};

/// A rectangular cell selection inside one table (P5.5).
///
/// Endpoints are cell identities, so row/column insertions never move the
/// rectangle — it only shrinks when an endpoint cell's subtree is removed.
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
    /// Returns unit-cell rows of unique canonical cell identities.
    ///
    /// This is not a logical-slot API: merged cells can occupy several slots
    /// with the same identity. Until range editing defines an origin-aware
    /// contract, a table containing spans returns `UnsupportedTableOperation`
    /// rather than repeating identities or interpreting physical indexes.
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
