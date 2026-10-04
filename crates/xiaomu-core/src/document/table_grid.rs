//! Checked logical occupancy for body and header cells, including spans.

use std::mem::size_of;

use crate::{Error, Result};

use super::{NodeId, NodeKind, NodeStore, TableCellAttrs, XiaomuDocument};

/// Maximum logical slots summed across every table in a snapshot, nested too.
///
/// This is a Core resource boundary, not a product-specific table-size rule.
pub const TABLE_MAX_LOGICAL_SLOTS: usize = 1_000_000;
/// Maximum physical cells summed across every table in one snapshot.
pub const TABLE_MAX_PHYSICAL_CELLS: usize = 100_000;
/// Maximum checked grid-vector payload bytes summed across one snapshot.
///
/// Accounts for occupancy, placements, identity indexes, and physical row IDs;
/// excludes the independently owned canonical tree and allocator bookkeeping.
pub const TABLE_MAX_GRID_BYTES: usize = 64 * 1024 * 1024;

/// Logical origin and extent of one physical cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPlacement {
    cell: NodeId,
    row: usize,
    column: usize,
    rowspan: usize,
    colspan: usize,
    row_id: NodeId,
    physical_index: usize,
}

impl CellPlacement {
    /// Returns the stable identity of this cell.
    #[must_use]
    pub const fn cell(&self) -> NodeId {
        self.cell
    }
    /// Returns the zero-based logical origin row.
    #[must_use]
    pub const fn row(&self) -> usize {
        self.row
    }
    /// Returns the zero-based logical origin column.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }
    /// Returns the positive number of covered logical rows.
    #[must_use]
    pub const fn rowspan(&self) -> usize {
        self.rowspan
    }
    /// Returns the positive number of covered logical columns.
    #[must_use]
    pub const fn colspan(&self) -> usize {
        self.colspan
    }
    /// Returns the physical row containing the cell's origin.
    #[must_use]
    pub const fn row_id(&self) -> NodeId {
        self.row_id
    }
    /// Returns the cell's index in its physical row's children.
    #[must_use]
    pub const fn physical_index(&self) -> usize {
        self.physical_index
    }

    fn rect(&self) -> TableRect {
        // Both additions were checked when this placement was constructed.
        TableRect {
            top: self.row,
            left: self.column,
            bottom: self.row + self.rowspan,
            right: self.column + self.colspan,
        }
    }
}

/// Nonempty half-open rectangle in logical table coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableRect {
    top: usize,
    left: usize,
    bottom: usize,
    right: usize,
}

impl TableRect {
    /// Creates `[top, bottom) × [left, right)`; empty/inverted bounds fail.
    /// Table bounds are checked by the grid consuming the rectangle.
    pub fn new(top: usize, left: usize, bottom: usize, right: usize) -> Result<Self> {
        if top >= bottom || left >= right {
            return Err(Error::InvalidSelection);
        }
        Ok(Self {
            top,
            left,
            bottom,
            right,
        })
    }
    /// Returns the inclusive first row.
    #[must_use]
    pub const fn top(self) -> usize {
        self.top
    }
    /// Returns the inclusive first column.
    #[must_use]
    pub const fn left(self) -> usize {
        self.left
    }
    /// Returns the exclusive last row.
    #[must_use]
    pub const fn bottom(self) -> usize {
        self.bottom
    }
    /// Returns the exclusive last column.
    #[must_use]
    pub const fn right(self) -> usize {
        self.right
    }

    fn intersects(self, other: Self) -> bool {
        self.top < other.bottom
            && other.top < self.bottom
            && self.left < other.right
            && other.left < self.right
    }
    fn contains(self, other: Self) -> bool {
        self.top <= other.top
            && self.left <= other.left
            && self.bottom >= other.bottom
            && self.right >= other.right
    }
}

/// Validated, bounded logical table occupancy for one immutable snapshot.
///
/// Slots may repeat the same cell identity; [`Self::origins`] visits each
/// physical cell once in logical row-major order. Rebuild after any document
/// revision change. This type never normalizes or repairs invalid geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableGrid {
    table: NodeId,
    columns: usize,
    rows: Vec<NodeId>,
    slots: Vec<u32>,
    cells: Vec<CellPlacement>,
    by_id: Vec<(NodeId, usize)>,
}

impl TableGrid {
    /// Builds a checked grid for a table in a validated document.
    ///
    /// Unknown IDs, non-tables, malformed geometry, and resource exhaustion
    /// return typed errors before an invalid grid can escape.
    pub fn new(document: &XiaomuDocument, table: NodeId) -> Result<Self> {
        Self::from_store(document.store(), table, &mut TableGridBudget::default())
    }
    /// Returns the table identity.
    #[must_use]
    pub const fn table(&self) -> NodeId {
        self.table
    }
    /// Returns the physical/logical row count, including fully covered rows.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
    /// Returns the logical column count.
    #[must_use]
    pub const fn columns(&self) -> usize {
        self.columns
    }
    /// Returns the physical row identity at a logical row index.
    #[must_use]
    pub fn row_id(&self, row: usize) -> Option<NodeId> {
        self.rows.get(row).copied()
    }
    /// Returns the cell covering a logical slot, or `None` outside the table.
    #[must_use]
    pub fn slot(&self, row: usize, column: usize) -> Option<NodeId> {
        if row >= self.rows() || column >= self.columns {
            return None;
        }
        Some(self.cells[self.slots[row * self.columns + column] as usize].cell)
    }
    /// Returns one physical cell's origin and extent by identity.
    #[must_use]
    pub fn placement(&self, cell: NodeId) -> Option<&CellPlacement> {
        let index = self
            .by_id
            .binary_search_by_key(&cell, |entry| entry.0)
            .ok()?;
        Some(&self.cells[self.by_id[index].1])
    }
    /// Visits each physical cell once in logical origin order.
    pub fn origins(&self) -> impl ExactSizeIterator<Item = &CellPlacement> {
        self.cells.iter()
    }
    /// Returns whether any cell occupies more than one logical slot.
    #[must_use]
    pub fn has_spans(&self) -> bool {
        self.cells
            .iter()
            .any(|cell| cell.rowspan != 1 || cell.colspan != 1)
    }
    /// Returns the bounding rectangle of two complete cells.
    ///
    /// This does not expand around other intersecting cells; use
    /// [`Self::is_closed_rect`] before operations requiring a closed selection.
    pub fn rect_between(&self, anchor: NodeId, focus: NodeId) -> Result<TableRect> {
        let a = self
            .placement(anchor)
            .ok_or(Error::InvalidSelection)?
            .rect();
        let f = self.placement(focus).ok_or(Error::InvalidSelection)?.rect();
        TableRect::new(
            a.top.min(f.top),
            a.left.min(f.left),
            a.bottom.max(f.bottom),
            a.right.max(f.right),
        )
    }
    /// Returns each intersecting cell once in logical origin order.
    /// Out-of-grid rectangles return [`Error::InvalidSelection`].
    pub fn unique_cells_in(&self, rect: TableRect) -> Result<Vec<NodeId>> {
        self.validate_rect(rect)?;
        let mut cells = reserved_vec(self.cells.len())?;
        cells.extend(
            self.cells
                .iter()
                .filter(|cell| rect.intersects(cell.rect()))
                .map(|cell| cell.cell),
        );
        Ok(cells)
    }
    /// Returns whether every intersecting cell lies entirely in the rectangle.
    /// Out-of-grid rectangles are not closed.
    #[must_use]
    pub fn is_closed_rect(&self, rect: TableRect) -> bool {
        self.validate_rect(rect).is_ok()
            && self.cells.iter().all(|cell| {
                let cell = cell.rect();
                !rect.intersects(cell) || rect.contains(cell)
            })
    }
    fn validate_rect(&self, rect: TableRect) -> Result<()> {
        if rect.bottom > self.rows() || rect.right > self.columns {
            Err(Error::InvalidSelection)
        } else {
            Ok(())
        }
    }

    pub(crate) fn from_store(
        store: &NodeStore,
        table: NodeId,
        budget: &mut TableGridBudget,
    ) -> Result<Self> {
        let node = store.get(table).ok_or(Error::UnknownNode)?;
        if !matches!(node.kind(), NodeKind::Table) {
            return Err(Error::InvalidTableStructure);
        }
        let rows = node
            .content()
            .as_children()
            .ok_or(Error::InvalidTableStructure)?;
        if rows.is_empty() {
            return Err(Error::InvalidTableStructure);
        }
        if rows.len() > TABLE_MAX_LOGICAL_SLOTS {
            return Err(Error::TableResourceLimit);
        }
        let mut cell_count = 0usize;
        for row in rows {
            cell_count = cell_count
                .checked_add(row_cells(store, *row)?.len())
                .ok_or(Error::TableResourceLimit)?;
            if cell_count > TABLE_MAX_PHYSICAL_CELLS {
                return Err(Error::TableResourceLimit);
            }
        }
        let mut columns = 0usize;
        for cell in row_cells(store, rows[0])? {
            columns = columns
                .checked_add(cell_attrs(store, *cell)?.effective_colspan()?)
                .ok_or(Error::TableResourceLimit)?;
        }
        if columns == 0 {
            return Err(Error::InvalidTableStructure);
        }
        let slot_count = budget.reserve(rows.len(), columns, cell_count)?;
        let mut grid = Self {
            table,
            columns,
            rows: reserved_vec(rows.len())?,
            slots: reserved_vec(slot_count)?,
            cells: reserved_vec(cell_count)?,
            by_id: reserved_vec(cell_count)?,
        };
        grid.rows.extend_from_slice(rows);
        grid.slots.resize(slot_count, u32::MAX);
        for (row, row_id) in rows.iter().copied().enumerate() {
            let mut column = 0usize;
            for (physical_index, cell) in row_cells(store, row_id)?.iter().copied().enumerate() {
                while column < columns && grid.slots[row * columns + column] != u32::MAX {
                    column += 1;
                }
                let attrs = cell_attrs(store, cell)?;
                let rowspan = attrs.effective_rowspan()?;
                let colspan = attrs.effective_colspan()?;
                let bottom = row
                    .checked_add(rowspan)
                    .ok_or(Error::InvalidTableStructure)?;
                let right = column
                    .checked_add(colspan)
                    .ok_or(Error::InvalidTableStructure)?;
                if bottom > rows.len() || right > columns {
                    return Err(Error::InvalidTableStructure);
                }
                let origin =
                    u32::try_from(grid.cells.len()).map_err(|_| Error::TableResourceLimit)?;
                for covered_row in row..bottom {
                    let covered = &mut grid.slots
                        [covered_row * columns + column..covered_row * columns + right];
                    // Never move past a collision looking for a different origin.
                    if covered.iter().any(|slot| *slot != u32::MAX) {
                        return Err(Error::InvalidTableStructure);
                    }
                    covered.fill(origin);
                }
                grid.by_id.push((cell, grid.cells.len()));
                grid.cells.push(CellPlacement {
                    cell,
                    row,
                    column,
                    rowspan,
                    colspan,
                    row_id,
                    physical_index,
                });
                column = right;
            }
            if grid.slots[row * columns..(row + 1) * columns].contains(&u32::MAX) {
                return Err(Error::InvalidTableStructure);
            }
        }
        grid.by_id.sort_unstable_by_key(|entry| entry.0);
        if grid.by_id.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(Error::MultipleNodeParents);
        }
        Ok(grid)
    }
}

fn row_cells(store: &NodeStore, row: NodeId) -> Result<&[NodeId]> {
    let row = store.get(row).ok_or(Error::UnknownNode)?;
    if !matches!(row.kind(), NodeKind::TableRow) {
        return Err(Error::InvalidTableStructure);
    }
    row.content()
        .as_children()
        .ok_or(Error::InvalidTableStructure)
}

fn cell_attrs(store: &NodeStore, cell: NodeId) -> Result<TableCellAttrs<'_>> {
    let cell = store.get(cell).ok_or(Error::UnknownNode)?;
    if !cell.kind().is_table_cell()
        || cell
            .content()
            .as_children()
            .is_none_or(<[NodeId]>::is_empty)
    {
        return Err(Error::InvalidTableStructure);
    }
    let attrs = TableCellAttrs::read(cell.attrs())?;
    attrs.validate_geometry()?;
    Ok(attrs)
}

fn reserved_vec<T>(capacity: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| Error::TableResourceLimit)?;
    Ok(values)
}

/// Cumulative validation work across all tables, including nested tables.
#[derive(Default)]
pub(crate) struct TableGridBudget {
    slots: usize,
    cells: usize,
    bytes: usize,
}

impl TableGridBudget {
    pub(crate) fn reserve(&mut self, rows: usize, columns: usize, cells: usize) -> Result<usize> {
        let slots = rows.checked_mul(columns).ok_or(Error::TableResourceLimit)?;
        let bytes = slots
            .checked_mul(size_of::<u32>())
            .and_then(|value| value.checked_add(cells.checked_mul(size_of::<CellPlacement>())?))
            .and_then(|value| value.checked_add(cells.checked_mul(size_of::<(NodeId, usize)>())?))
            .and_then(|value| value.checked_add(rows.checked_mul(size_of::<NodeId>())?))
            .ok_or(Error::TableResourceLimit)?;
        let total_slots = self
            .slots
            .checked_add(slots)
            .ok_or(Error::TableResourceLimit)?;
        let total_cells = self
            .cells
            .checked_add(cells)
            .ok_or(Error::TableResourceLimit)?;
        let total_bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(Error::TableResourceLimit)?;
        if total_slots > TABLE_MAX_LOGICAL_SLOTS
            || total_cells > TABLE_MAX_PHYSICAL_CELLS
            || total_bytes > TABLE_MAX_GRID_BYTES
        {
            return Err(Error::TableResourceLimit);
        }
        self.slots = total_slots;
        self.cells = total_cells;
        self.bytes = total_bytes;
        Ok(slots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_accumulates_cells_and_failed_reservation_does_not_commit() {
        let mut budget = TableGridBudget::default();
        assert_eq!(budget.reserve(1, 50_000, 50_000), Ok(50_000));
        assert_eq!(budget.reserve(1, 50_000, 50_000), Ok(50_000));
        let before = (budget.slots, budget.cells, budget.bytes);
        assert_eq!(budget.reserve(1, 1, 1), Err(Error::TableResourceLimit));
        assert_eq!((budget.slots, budget.cells, budget.bytes), before);
    }

    #[test]
    fn budget_checks_slot_and_byte_arithmetic_before_reserving_vectors() {
        let mut budget = TableGridBudget::default();
        assert_eq!(
            budget.reserve(usize::MAX, 2, 1),
            Err(Error::TableResourceLimit)
        );
        assert_eq!(
            budget.reserve(1, 1, usize::MAX),
            Err(Error::TableResourceLimit)
        );
        budget.bytes = TABLE_MAX_GRID_BYTES;
        assert_eq!(budget.reserve(1, 1, 1), Err(Error::TableResourceLimit));
        budget.bytes = usize::MAX;
        assert_eq!(budget.reserve(1, 1, 1), Err(Error::TableResourceLimit));
    }
}
