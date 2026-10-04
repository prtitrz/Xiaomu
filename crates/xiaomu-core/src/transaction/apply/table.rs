//! Whole-table transaction application and exact inverse capture.

use crate::document::{NodeAttrs, NodeContent, NodeId, NodeKind, TableGrid, TableGridBudget};
use crate::mapping::StepMap;
use crate::{Error, Result};

use super::ApplyContext;
use crate::transaction::step::TransactionStep;

impl ApplyContext {
    /// Applies one whole-table insertion with fresh stable identities.
    ///
    /// Every row shares `columns` cells and every cell carries one empty
    /// paragraph, so the produced snapshot satisfies the table invariants
    /// directly. The inverse removes the whole subtree.
    pub(super) fn apply_insert_table(
        &mut self,
        parent: NodeId,
        index: usize,
        rows: usize,
        columns: usize,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        if rows == 0 || columns == 0 {
            return Err(Error::InvalidTransaction);
        }
        let cells = rows.checked_mul(columns).ok_or(Error::TableResourceLimit)?;
        let nodes = cells
            .checked_mul(2)
            .and_then(|value| value.checked_add(rows))
            .and_then(|value| value.checked_add(1))
            .ok_or(Error::TableResourceLimit)?;
        self.check_table_growth(rows, columns, cells, nodes)?;
        let mut children = self.children(parent)?;
        if index > children.len() {
            return Err(Error::InvalidTransaction);
        }

        let mut table_children = Vec::with_capacity(rows);
        for _ in 0..rows {
            let mut row_children = Vec::with_capacity(columns);
            for _ in 0..columns {
                let cell = self.allocate_node(
                    NodeKind::TableCell,
                    NodeAttrs::empty(),
                    NodeContent::children([]),
                )?;
                let paragraph = self.allocate_node(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::empty_inline(),
                )?;
                self.rewrite_node(cell, NodeAttrs::empty(), NodeContent::children([paragraph]))?;
                row_children.push(cell);
            }
            let row = self.allocate_node(
                NodeKind::TableRow,
                NodeAttrs::empty(),
                NodeContent::children([]),
            )?;
            self.rewrite_node(row, NodeAttrs::empty(), NodeContent::children(row_children))?;
            table_children.push(row);
        }
        let table = self.allocate_node(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(table_children),
        )?;

        children.insert(index, table);
        self.rewrite_node(
            parent,
            self.attrs_of(parent)?,
            NodeContent::children(children),
        )?;

        let step_map = StepMap::NodeInserted {
            parent,
            index,
            inserted: table,
        };
        let inverse = vec![TransactionStep::RemoveNode { node: table }];
        Ok((vec![step_map], inverse))
    }

    /// Applies one row insertion to an existing valid table.
    ///
    /// The new row mirrors the table's column count; every cell carries one
    /// empty paragraph. New cells remain body cells; existing headers keep
    /// their kind. Spanning tables require a span-aware semantic operation.
    /// The step map names the inserted row, not a
    /// descendant caret target. `index` may be the current row count to append.
    pub(super) fn apply_insert_table_row(
        &mut self,
        table: NodeId,
        index: usize,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = self.require_unit_table(table)?;
        let mut table_children = self.children(table)?;
        if index > table_children.len() {
            return Err(Error::InvalidTransaction);
        }
        let columns = grid.columns();
        let nodes = columns
            .checked_mul(2)
            .and_then(|value| value.checked_add(1))
            .ok_or(Error::TableResourceLimit)?;
        self.check_table_growth(1, columns, columns, nodes)?;
        let mut cells = Vec::with_capacity(columns);
        for _ in 0..columns {
            cells.push(self.allocate_node(
                NodeKind::TableCell,
                NodeAttrs::empty(),
                NodeContent::children([]),
            )?);
        }
        let mut paragraphs = Vec::with_capacity(columns);
        for _ in 0..columns {
            paragraphs.push(self.allocate_node(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )?);
        }
        for (cell, paragraph) in cells.iter().zip(paragraphs.iter()) {
            self.rewrite_node(
                *cell,
                NodeAttrs::empty(),
                NodeContent::children([*paragraph]),
            )?;
        }
        let row = self.allocate_node(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells),
        )?;
        table_children.insert(index, row);
        self.rewrite_node(
            table,
            self.attrs_of(table)?,
            NodeContent::children(table_children),
        )?;

        let step_map = StepMap::NodeInserted {
            parent: table,
            index,
            inserted: row,
        };
        let inverse = vec![TransactionStep::RemoveNode { node: row }];
        Ok((vec![step_map], inverse))
    }

    /// Applies one column insertion to an existing valid table.
    ///
    /// Every row gains one cell (carrying one empty paragraph) at `index`,
    /// so the uniform column count holds in the produced snapshot. One
    /// insertion map per row tracks every changed child list. New cells remain
    /// body cells; existing headers keep their kind. Spanning tables are rejected.
    /// The inverse
    /// removes one cell per row in reverse order.
    pub(super) fn apply_insert_table_column(
        &mut self,
        table: NodeId,
        index: usize,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = self.require_unit_table(table)?;
        let table_children = self.children(table)?;
        let columns = grid.columns();
        if index > columns {
            return Err(Error::InvalidTransaction);
        }

        let nodes = grid
            .rows()
            .checked_mul(2)
            .ok_or(Error::TableResourceLimit)?;
        self.check_table_growth(grid.rows(), 1, grid.rows(), nodes)?;
        let mut inserted_cells = Vec::with_capacity(table_children.len());
        let mut step_maps = Vec::with_capacity(table_children.len());
        for row in &table_children {
            let mut row_children = self.children(*row)?;
            let cell = self.allocate_node(
                NodeKind::TableCell,
                NodeAttrs::empty(),
                NodeContent::children([]),
            )?;
            let paragraph = self.allocate_node(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )?;
            self.rewrite_node(cell, NodeAttrs::empty(), NodeContent::children([paragraph]))?;
            row_children.insert(index, cell);
            self.rewrite_node(
                *row,
                self.attrs_of(*row)?,
                NodeContent::children(row_children),
            )?;
            inserted_cells.push(cell);
            step_maps.push(StepMap::NodeInserted {
                parent: *row,
                index,
                inserted: cell,
            });
        }

        let inverse = inserted_cells
            .into_iter()
            .rev()
            .map(|cell| TransactionStep::RemoveNode { node: cell })
            .collect();
        Ok((step_maps, inverse))
    }

    fn require_unit_table(&self, table: NodeId) -> Result<TableGrid> {
        let grid = TableGrid::from_store(&self.store, table, &mut TableGridBudget::default())?;
        if grid.has_spans() {
            return Err(Error::UnsupportedTableOperation);
        }
        Ok(grid)
    }

    // Reject impossible growth before constructing vectors or allocating IDs.
    // Existing nested and sibling tables share the same cumulative budget.
    fn check_table_growth(
        &self,
        rows: usize,
        columns: usize,
        cells: usize,
        nodes: usize,
    ) -> Result<()> {
        let nodes = u64::try_from(nodes).map_err(|_| Error::NodeIdExhausted)?;
        self.next_node_id
            .checked_add(nodes)
            .ok_or(Error::NodeIdExhausted)?;
        let mut budget = TableGridBudget::default();
        budget.reserve(rows, columns, cells)?;
        for node in self
            .store
            .iter()
            .filter(|node| matches!(node.kind(), NodeKind::Table))
        {
            TableGrid::from_store(&self.store, node.id(), &mut budget)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::NodeStoreBuilder;

    #[test]
    fn insertion_checks_identity_ceiling_before_any_allocation() {
        let mut builder = NodeStoreBuilder::new();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([]),
            )
            .unwrap();
        let store = builder.finish();
        let mut context = ApplyContext {
            root,
            store: store.clone(),
            next_node_id: u64::MAX - 3,
        };
        assert_eq!(
            context.apply_insert_table(root, 0, 1, 1).unwrap_err(),
            Error::NodeIdExhausted
        );
        assert_eq!(context.store, store);
        assert_eq!(context.next_node_id, u64::MAX - 3);
    }
}
