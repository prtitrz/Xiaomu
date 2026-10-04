use super::*;

impl ApplyContext {
    pub(in crate::transaction::apply) fn apply_insert_table_row_logical(
        &mut self,
        table: NodeId,
        index: usize,
        cell_kinds: &[NodeKind],
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = self.logical_grid(table)?;
        if index > grid.rows() {
            return Err(Error::InvalidTransaction);
        }
        validate_kinds(cell_kinds, grid.columns())?;
        let crossing: Vec<_> = grid
            .origins()
            .filter(|cell| cell.row() < index && cell.row() + cell.rowspan() > index)
            .copied()
            .collect();
        let covered: usize = crossing.iter().map(|cell| cell.colspan()).sum();
        let added = grid.columns() - covered;
        let rows = grid
            .rows()
            .checked_add(1)
            .ok_or(Error::TableResourceLimit)?;
        let cells = grid
            .origins()
            .len()
            .checked_add(added)
            .ok_or(Error::TableResourceLimit)?;
        let nodes = added
            .checked_mul(2)
            .and_then(|n| n.checked_add(1))
            .ok_or(Error::TableResourceLimit)?;
        self.check_axis_growth(&grid, rows, grid.columns(), cells, nodes)?;
        let mut edit = TableEdit::new(self, table)?;
        for cell in crossing {
            let node = self.store.get(cell.cell()).ok_or(Error::UnknownNode)?;
            edit.replace(
                self,
                resize_cell(node, "rowspan", cell.rowspan() + 1, WidthEdit::Keep)?,
            )?;
        }
        let mut children = Vec::new();
        children
            .try_reserve_exact(added)
            .map_err(|_| Error::TableResourceLimit)?;
        let mut inserted = BTreeSet::new();
        for (column, kind) in cell_kinds.iter().enumerate() {
            let covered = index > 0
                && index < grid.rows()
                && grid.slot(index - 1, column) == grid.slot(index, column);
            if !covered {
                let (cell, paragraph) = edit.empty_cell(kind)?;
                children.push(cell);
                inserted.extend([cell, paragraph]);
            }
        }
        let row = edit.allocate(NodeKind::TableRow, NodeContent::children(children))?;
        inserted.insert(row);
        let mut rows = self.children(table)?;
        rows.insert(index, row);
        edit.rewrite_children(self, table, rows)?;
        edit.record(
            StepMap::NodeInserted {
                parent: table,
                index,
                inserted: row,
            },
            StepMap::NodeRemoved {
                parent: table,
                index,
                removed: inserted,
            },
        );
        edit.finish(self)
    }

    pub(in crate::transaction::apply) fn apply_insert_table_column_logical(
        &mut self,
        table: NodeId,
        index: usize,
        cell_kinds: &[NodeKind],
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = self.logical_grid(table)?;
        if index > grid.columns() {
            return Err(Error::InvalidTransaction);
        }
        validate_kinds(cell_kinds, grid.rows())?;
        let crossing: Vec<_> = grid
            .origins()
            .filter(|cell| cell.column() < index && cell.column() + cell.colspan() > index)
            .copied()
            .collect();
        let covered: usize = crossing.iter().map(|cell| cell.rowspan()).sum();
        let added = grid.rows() - covered;
        let columns = grid
            .columns()
            .checked_add(1)
            .ok_or(Error::TableResourceLimit)?;
        let cells = grid
            .origins()
            .len()
            .checked_add(added)
            .ok_or(Error::TableResourceLimit)?;
        let nodes = added.checked_mul(2).ok_or(Error::TableResourceLimit)?;
        self.check_axis_growth(&grid, grid.rows(), columns, cells, nodes)?;
        let mut edit = TableEdit::new(self, table)?;
        for cell in crossing {
            let node = self.store.get(cell.cell()).ok_or(Error::UnknownNode)?;
            edit.replace(
                self,
                resize_cell(
                    node,
                    "colspan",
                    cell.colspan() + 1,
                    WidthEdit::Insert(index - cell.column()),
                )?,
            )?;
        }
        for (row_index, kind) in cell_kinds.iter().enumerate() {
            let covered = index > 0
                && index < grid.columns()
                && grid.slot(row_index, index - 1) == grid.slot(row_index, index);
            if covered {
                continue;
            }
            let row = grid.row_id(row_index).ok_or(Error::InvalidTableStructure)?;
            let mut children = self.children(row)?;
            let physical_index = children
                .iter()
                .filter(|cell| {
                    grid.placement(**cell)
                        .is_some_and(|cell| cell.column() < index)
                })
                .count();
            let (cell, paragraph) = edit.empty_cell(kind)?;
            children.insert(physical_index, cell);
            edit.rewrite_children(self, row, children)?;
            edit.record(
                StepMap::NodeInserted {
                    parent: row,
                    index: physical_index,
                    inserted: cell,
                },
                StepMap::NodeRemoved {
                    parent: row,
                    index: physical_index,
                    removed: BTreeSet::from([cell, paragraph]),
                },
            );
        }
        edit.finish(self)
    }
}
