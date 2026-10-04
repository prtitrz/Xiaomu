//! Bounded, identity-preserving logical cell merge/split and exact restoration.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{
    AttrValue, Node, NodeAttrs, NodeContent, NodeId, NodeKind, TableCellAttrs, TableGrid,
    TableGridBudget, TableRect,
};
use crate::mapping::StepMap;
use crate::transaction::{TableCellRestore, TransactionStep};
use crate::{Error, Result};

use super::ApplyContext;

#[cfg(test)]
mod tests;

impl ApplyContext {
    pub(super) fn apply_merge_table_cells(
        &mut self,
        table: NodeId,
        rect: TableRect,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = TableGrid::from_store(&self.store, table, &mut TableGridBudget::default())?;
        if !grid.is_closed_rect(rect) {
            return Err(Error::InvalidSelection);
        }
        let cells = grid.unique_cells_in(rect)?;
        if cells.len() < 2 {
            return Err(Error::InvalidTransaction);
        }
        let survivor = cells[0];
        let origin = grid
            .placement(survivor)
            .ok_or(Error::InvalidTableStructure)?;
        if origin.row() != rect.top() || origin.column() != rect.left() {
            return Err(Error::InvalidSelection);
        }
        let mut expected = BTreeMap::new();
        let mut replacement = BTreeMap::new();
        let mut removed_by_row: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        let mut maps = Vec::new();
        let mut inverse_maps = Vec::new();
        let total_children = cells.iter().try_fold(0usize, |total, cell| {
            let count = self
                .store
                .get(*cell)
                .ok_or(Error::UnknownNode)?
                .content()
                .as_children()
                .ok_or(Error::InvalidTableStructure)?
                .len();
            total.checked_add(count).ok_or(Error::TableResourceLimit)
        })?;
        let mut content = Vec::new();
        content
            .try_reserve_exact(total_children)
            .map_err(|_| Error::TableResourceLimit)?;
        for cell in cells {
            let original = self.store.get(cell).ok_or(Error::UnknownNode)?;
            let children = original
                .content()
                .as_children()
                .ok_or(Error::InvalidTableStructure)?;
            expected.insert(cell, original.clone());
            if cell != survivor {
                let placement = grid.placement(cell).ok_or(Error::InvalidTableStructure)?;
                let removed = removed_by_row.entry(placement.row_id()).or_default();
                let index = placement.physical_index() - removed.len();
                maps.push(StepMap::TableCellMerged {
                    row: placement.row_id(),
                    index,
                    survivor,
                    removed: cell,
                    at: content.len(),
                    child_count: children.len(),
                });
                inverse_maps.push(StepMap::TableCellRestored {
                    row: placement.row_id(),
                    index,
                    survivor,
                    restored: cell,
                    at: content.len(),
                    child_count: children.len(),
                });
                removed.insert(cell);
            }
            content.extend_from_slice(children);
        }
        for (row, removed) in removed_by_row {
            let original = self.store.get(row).ok_or(Error::UnknownNode)?;
            let children = original
                .content()
                .as_children()
                .ok_or(Error::InvalidTableStructure)?;
            let next = children
                .iter()
                .copied()
                .filter(|cell| !removed.contains(cell));
            replacement.insert(
                row,
                Node::new(
                    row,
                    original.kind().clone(),
                    original.attrs().clone(),
                    NodeContent::children(next),
                )?,
            );
            expected.insert(row, original.clone());
        }
        let original = self.store.get(survivor).ok_or(Error::UnknownNode)?;
        let attrs = merged_attrs(
            original.attrs(),
            rect.bottom() - rect.top(),
            rect.right() - rect.left(),
        )?;
        replacement.insert(
            survivor,
            Node::new(
                survivor,
                original.kind().clone(),
                attrs,
                NodeContent::children(content),
            )?,
        );
        inverse_maps.reverse();
        self.commit_table_edit(
            TableCellRestore {
                table,
                expected: expected.into_values().collect(),
                replacement: replacement.into_values().collect(),
                maps,
                inverse_maps,
            },
            self.next_node_id,
        )
    }

    pub(super) fn apply_split_table_cell(
        &mut self,
        table: NodeId,
        cell: NodeId,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = TableGrid::from_store(&self.store, table, &mut TableGridBudget::default())?;
        let placement = *grid.placement(cell).ok_or(Error::InvalidSelection)?;
        let slots = placement
            .rowspan()
            .checked_mul(placement.colspan())
            .ok_or(Error::TableResourceLimit)?;
        if slots == 1 {
            return Err(Error::InvalidTransaction);
        }
        let added = slots - 1;
        self.check_split_growth(&grid, added)?;
        let new_nodes = added.checked_mul(2).ok_or(Error::TableResourceLimit)?;
        let next_id = self
            .next_node_id
            .checked_add(u64::try_from(new_nodes).map_err(|_| Error::NodeIdExhausted)?)
            .ok_or(Error::NodeIdExhausted)?;
        let original = self.store.get(cell).ok_or(Error::UnknownNode)?;
        let mut expected = vec![original.clone()];
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(
                new_nodes
                    .checked_add(placement.rowspan())
                    .and_then(|n| n.checked_add(1))
                    .ok_or(Error::TableResourceLimit)?,
            )
            .map_err(|_| Error::TableResourceLimit)?;
        replacement.push(Node::new(
            cell,
            original.kind().clone(),
            split_attrs(
                original.attrs(),
                0,
                placement.rowspan(),
                placement.colspan(),
            )?,
            original.content().clone(),
        )?);
        let mut next = self.next_node_id;
        let mut maps = Vec::new();
        let mut inverse_maps = Vec::new();
        for row_index in placement.row()..placement.row() + placement.rowspan() {
            let row = grid.row_id(row_index).ok_or(Error::InvalidTableStructure)?;
            let original_row = self.store.get(row).ok_or(Error::UnknownNode)?;
            let children = original_row
                .content()
                .as_children()
                .ok_or(Error::InvalidTableStructure)?;
            let mut ordered = Vec::new();
            ordered
                .try_reserve_exact(
                    children
                        .len()
                        .checked_add(placement.colspan())
                        .ok_or(Error::TableResourceLimit)?,
                )
                .map_err(|_| Error::TableResourceLimit)?;
            for child in children {
                let column = grid
                    .placement(*child)
                    .ok_or(Error::InvalidTableStructure)?
                    .column();
                ordered.push((column, *child, None));
            }
            let mut changed = false;
            for column_offset in 0..placement.colspan() {
                if row_index == placement.row() && column_offset == 0 {
                    continue;
                }
                // The entire ID range was checked before any node construction.
                let paragraph = NodeId::from_allocated(next);
                let created = NodeId::from_allocated(next + 1);
                next += 2;
                replacement.push(Node::new(
                    paragraph,
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::empty_inline(),
                )?);
                replacement.push(Node::new(
                    created,
                    original.kind().clone(),
                    split_attrs(
                        original.attrs(),
                        column_offset,
                        placement.rowspan(),
                        placement.colspan(),
                    )?,
                    NodeContent::children([paragraph]),
                )?);
                ordered.push((placement.column() + column_offset, created, Some(paragraph)));
                changed = true;
            }
            if !changed {
                continue;
            }
            ordered.sort_unstable_by_key(|entry| entry.0);
            for (index, (_, created, paragraph)) in ordered.iter().enumerate() {
                if let Some(paragraph) = paragraph {
                    maps.push(StepMap::NodeInserted {
                        parent: row,
                        index,
                        inserted: *created,
                    });
                    inverse_maps.push(StepMap::NodeRemoved {
                        parent: row,
                        index,
                        removed: BTreeSet::from([*created, *paragraph]),
                    });
                }
            }
            replacement.push(Node::new(
                row,
                original_row.kind().clone(),
                original_row.attrs().clone(),
                NodeContent::children(ordered.into_iter().map(|entry| entry.1)),
            )?);
            expected.push(original_row.clone());
        }
        debug_assert_eq!(next, next_id);
        inverse_maps.reverse();
        self.commit_table_edit(
            TableCellRestore {
                table,
                expected,
                replacement,
                maps,
                inverse_maps,
            },
            next_id,
        )
    }

    pub(super) fn apply_restore_table_cells(
        &mut self,
        restore: &TableCellRestore,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        // Check current table membership/occupancy, not just opacity of the
        // payload. Exact row/cell preconditions and absent IDs are checked by
        // exchange; full-tree validation checks the resulting references.
        let grid =
            TableGrid::from_store(&self.store, restore.table, &mut TableGridBudget::default())?;
        let rows: BTreeSet<_> = (0..grid.rows())
            .filter_map(|row| grid.row_id(row))
            .collect();
        let mut cell_children = BTreeSet::new();
        for node in &restore.expected {
            if node.kind().is_table_cell() {
                if grid.placement(node.id()).is_none() {
                    return Err(Error::InvalidTransaction);
                }
                cell_children.extend(
                    node.content()
                        .as_children()
                        .ok_or(Error::InvalidTransaction)?
                        .iter()
                        .copied(),
                );
            }
        }
        for node in &restore.expected {
            let belongs = match node.kind() {
                NodeKind::TableRow => rows.contains(&node.id()),
                NodeKind::TableCell | NodeKind::TableHeader => grid.placement(node.id()).is_some(),
                NodeKind::Paragraph => cell_children.contains(&node.id()),
                _ => false,
            };
            if !belongs {
                return Err(Error::InvalidTransaction);
            }
        }
        let next_id = restore
            .replacement
            .iter()
            .try_fold(self.next_node_id, |next, node| {
                Ok::<_, Error>(
                    next.max(
                        node.id()
                            .raw()
                            .checked_add(1)
                            .ok_or(Error::NodeIdExhausted)?,
                    ),
                )
            })?;
        self.commit_table_edit(restore.clone(), next_id)
    }

    fn commit_table_edit(
        &mut self,
        edit: TableCellRestore,
        next_id: u64,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let next = self.store.exchange(&edit.expected, &edit.replacement)?;
        let inverse = TableCellRestore {
            table: edit.table,
            expected: edit.replacement,
            replacement: edit.expected,
            maps: edit.inverse_maps,
            inverse_maps: edit.maps.clone(),
        };
        self.store = next;
        self.next_node_id = next_id;
        Ok((
            edit.maps,
            vec![TransactionStep::RestoreTableCells { restore: inverse }],
        ))
    }

    fn check_split_growth(&self, grid: &TableGrid, added: usize) -> Result<()> {
        let cells = grid
            .origins()
            .len()
            .checked_add(added)
            .ok_or(Error::TableResourceLimit)?;
        let mut budget = TableGridBudget::default();
        budget.reserve(grid.rows(), grid.columns(), cells)?;
        for node in self
            .store
            .iter()
            .filter(|node| matches!(node.kind(), NodeKind::Table) && node.id() != grid.table())
        {
            TableGrid::from_store(&self.store, node.id(), &mut budget)?;
        }
        // This bounds grid work and generated node count, not arbitrary attrs,
        // canonical descendants, or total execution memory. exchange clones
        // the store map once, rather than once per allocated cell/paragraph.
        Ok(())
    }
}

fn attrs_map(attrs: &NodeAttrs) -> BTreeMap<String, AttrValue> {
    attrs
        .iter()
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect()
}

fn set_span(
    values: &mut BTreeMap<String, AttrValue>,
    key: &str,
    old: usize,
    new: usize,
) -> Result<()> {
    if old != new {
        values.insert(
            key.to_owned(),
            AttrValue::Integer(i64::try_from(new).map_err(|_| Error::InvalidTableAttrs)?),
        );
    }
    Ok(())
}

fn merged_attrs(attrs: &NodeAttrs, rows: usize, columns: usize) -> Result<NodeAttrs> {
    let geometry = TableCellAttrs::read(attrs)?;
    let mut values = attrs_map(attrs);
    set_span(&mut values, "rowspan", geometry.effective_rowspan()?, rows)?;
    set_span(
        &mut values,
        "colspan",
        geometry.effective_colspan()?,
        columns,
    )?;
    if let Some(AttrValue::List(widths)) = values.get_mut("colwidth") {
        widths
            .try_reserve_exact(columns - widths.len())
            .map_err(|_| Error::TableResourceLimit)?;
        widths.resize(columns, AttrValue::Integer(0));
    }
    NodeAttrs::new(values)
}

fn split_attrs(
    attrs: &NodeAttrs,
    column: usize,
    old_rows: usize,
    old_columns: usize,
) -> Result<NodeAttrs> {
    // Do not clone a wide source list for every unit cell: that would make a
    // one-row split quadratic in its logical width before store insertion.
    let mut values: BTreeMap<_, _> = attrs
        .iter()
        .filter(|(key, _)| *key != "colwidth")
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect();
    // The source geometry was validated once by TableGrid. Re-reading its
    // entire width list for each output cell would also be quadratic.
    set_span(&mut values, "rowspan", old_rows, 1)?;
    set_span(&mut values, "colspan", old_columns, 1)?;
    if let Some(widths) = attrs.get("colwidth") {
        let width = match widths {
            AttrValue::List(widths) => AttrValue::List(vec![
                widths.get(column).ok_or(Error::InvalidTableAttrs)?.clone(),
            ]),
            other => other.clone(),
        };
        values.insert("colwidth".to_owned(), width);
    }
    NodeAttrs::new(values)
}
