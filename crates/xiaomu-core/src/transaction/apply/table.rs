//! Whole-table transaction application and exact inverse capture.

use crate::document::{NodeAttrs, NodeContent, NodeId, NodeKind};
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
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        if rows == 0 || columns == 0 {
            return Err(Error::InvalidTransaction);
        }
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
        Ok((Some(step_map), inverse))
    }

    /// Applies one row append to an existing valid table.
    ///
    /// The new row mirrors the table's column count; every cell carries one
    /// empty paragraph. The step map reports the FIRST cell's paragraph as
    /// the inserted node, so frontends can move the caret into the new row's
    /// first cell after a Tab on the last cell.
    pub(super) fn apply_insert_table_row(
        &mut self,
        table: NodeId,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        let mut table_children = self.children(table)?;
        let first_row = table_children
            .first()
            .copied()
            .ok_or(Error::InvalidTableStructure)?;
        let columns = self.children(first_row)?.len();

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
        let insert_index = table_children.len();
        table_children.push(row);
        self.rewrite_node(
            table,
            self.attrs_of(table)?,
            NodeContent::children(table_children),
        )?;

        let step_map = StepMap::NodeInserted {
            parent: table,
            index: insert_index,
            inserted: paragraphs[0],
        };
        let inverse = vec![TransactionStep::RemoveNode { node: row }];
        Ok((Some(step_map), inverse))
    }
}
