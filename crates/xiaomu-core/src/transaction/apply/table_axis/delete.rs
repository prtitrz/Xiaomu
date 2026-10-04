use super::*;

impl ApplyContext {
    pub(in crate::transaction::apply) fn apply_delete_table_rows_logical(
        &mut self,
        table: NodeId,
        start: usize,
        end: usize,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = self.logical_grid(table)?;
        validate_delete_range(start, end, grid.rows())?;
        let mut edit = TableEdit::new(self, table)?;
        let mut deleted_by_row: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        let mut moved = BTreeMap::new();
        for placement in grid.origins() {
            let bottom = placement.row() + placement.rowspan();
            let overlap = bottom.min(end).saturating_sub(placement.row().max(start));
            if overlap == 0 {
                continue;
            }
            if overlap == placement.rowspan() {
                let removed = edit.remove_subtree(self, placement.cell())?;
                deleted_by_row
                    .entry(placement.row_id())
                    .or_default()
                    .extend(removed);
            } else {
                let node = self.store.get(placement.cell()).ok_or(Error::UnknownNode)?;
                edit.replace(
                    self,
                    resize_cell(
                        node,
                        "rowspan",
                        placement.rowspan() - overlap,
                        WidthEdit::Keep,
                    )?,
                )?;
                if placement.row() >= start {
                    // Its origin row is deleted, but content still owns slots
                    // below the deletion. Move the original cell, not a copy.
                    moved.insert(placement.cell(), *placement);
                }
            }
        }
        if !moved.is_empty() {
            let destination = grid.row_id(end).ok_or(Error::InvalidTableStructure)?;
            let mut ordered: Vec<_> = self
                .children(destination)?
                .into_iter()
                .map(|cell| {
                    let column = grid
                        .placement(cell)
                        .expect("validated grid contains each row cell")
                        .column();
                    (column, cell)
                })
                .collect();
            ordered.extend(moved.values().map(|cell| (cell.column(), cell.cell())));
            ordered.sort_unstable_by_key(|entry| entry.0);
            let mut removed_from_row: BTreeMap<NodeId, usize> = BTreeMap::new();
            for (new_index, (_, cell)) in ordered.iter().enumerate() {
                if let Some(placement) = moved.get(cell) {
                    let removed = removed_from_row.entry(placement.row_id()).or_default();
                    let old_index = placement.physical_index() - *removed;
                    *removed += 1;
                    edit.record(
                        StepMap::NodeReparented {
                            node: *cell,
                            old_parent: placement.row_id(),
                            old_index,
                            new_parent: destination,
                            new_index,
                        },
                        StepMap::NodeReparented {
                            node: *cell,
                            old_parent: destination,
                            old_index: new_index,
                            new_parent: placement.row_id(),
                            new_index: old_index,
                        },
                    );
                }
            }
            edit.rewrite_children(
                self,
                destination,
                ordered.into_iter().map(|entry| entry.1).collect(),
            )?;
        }
        let mut rows = self.children(table)?;
        for row in &rows[start..end] {
            edit.remove_only(self, *row)?;
            let mut removed = deleted_by_row.remove(row).unwrap_or_default();
            removed.insert(*row);
            edit.record(
                StepMap::NodeRemoved {
                    parent: table,
                    index: start,
                    removed,
                },
                StepMap::NodeInserted {
                    parent: table,
                    index: start,
                    inserted: *row,
                },
            );
        }
        rows.drain(start..end);
        edit.rewrite_children(self, table, rows)?;
        edit.finish(self)
    }

    pub(in crate::transaction::apply) fn apply_delete_table_columns_logical(
        &mut self,
        table: NodeId,
        start: usize,
        end: usize,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = self.logical_grid(table)?;
        validate_delete_range(start, end, grid.columns())?;
        let mut edit = TableEdit::new(self, table)?;
        let mut removed_by_row: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        for placement in grid.origins() {
            let right = placement.column() + placement.colspan();
            let overlap = right.min(end).saturating_sub(placement.column().max(start));
            if overlap == 0 {
                continue;
            }
            if overlap == placement.colspan() {
                let removed = edit.remove_subtree(self, placement.cell())?;
                let row_removed = removed_by_row.entry(placement.row_id()).or_default();
                let index = placement.physical_index() - row_removed.len();
                row_removed.insert(placement.cell());
                edit.record(
                    StepMap::NodeRemoved {
                        parent: placement.row_id(),
                        index,
                        removed,
                    },
                    StepMap::NodeInserted {
                        parent: placement.row_id(),
                        index,
                        inserted: placement.cell(),
                    },
                );
            } else {
                let node = self.store.get(placement.cell()).ok_or(Error::UnknownNode)?;
                edit.replace(
                    self,
                    resize_cell(
                        node,
                        "colspan",
                        placement.colspan() - overlap,
                        WidthEdit::Delete {
                            start: start.saturating_sub(placement.column()),
                            end: right.min(end) - placement.column(),
                        },
                    )?,
                )?;
            }
        }
        for (row, removed) in removed_by_row {
            let children = self
                .children(row)?
                .into_iter()
                .filter(|cell| !removed.contains(cell))
                .collect();
            edit.rewrite_children(self, row, children)?;
        }
        edit.finish(self)
    }
}
