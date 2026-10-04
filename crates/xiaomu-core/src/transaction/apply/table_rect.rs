//! Exact rich cell-forest replacement inside a closed logical rectangle.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{Node, NodeContent, NodeId, NodeKind, TableGrid, TableGridBudget, TableRect};
use crate::mapping::StepMap;
use crate::transaction::table_tree::TemplateContent;
use crate::transaction::{TableCellRestore, TableTreeTemplate, TransactionStep};
use crate::{Error, Result};

use super::ApplyContext;
use super::table_tree::materialize_node;

#[cfg(test)]
mod tests;

impl ApplyContext {
    pub(super) fn apply_replace_table_rect(
        &mut self,
        table: NodeId,
        rect: TableRect,
        tree: &TableTreeTemplate,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = TableGrid::from_store(&self.store, table, &mut TableGridBudget::default())?;
        if !grid.is_closed_rect(rect) {
            return Err(Error::InvalidSelection);
        }
        let rows = outer_rows(tree)?;
        let source_grid = tree
            .data
            .grids
            .first()
            .ok_or(Error::InvalidTableStructure)?;
        if source_grid.rows != rect.bottom() - rect.top()
            || source_grid.columns != rect.right() - rect.left()
        {
            return Err(Error::InvalidSelection);
        }
        let wrappers: BTreeSet<_> = std::iter::once(0).chain(rows.iter().copied()).collect();
        if wrappers.len() != rows.len() + 1 {
            return Err(Error::InvalidTransaction);
        }
        let count = tree
            .node_count()
            .checked_sub(wrappers.len())
            .ok_or(Error::InvalidTransaction)?;
        let next_id = self
            .next_node_id
            .checked_add(u64::try_from(count).map_err(|_| Error::NodeIdExhausted)?)
            .ok_or(Error::NodeIdExhausted)?;
        let cells = grid.unique_cells_in(rect)?;
        let removed: BTreeMap<_, _> = cells
            .iter()
            .map(|cell| (*cell, self.collect_subtree(*cell)))
            .collect();
        let removed_ids: BTreeSet<_> = removed.values().flatten().copied().collect();
        self.check_rect_budget(&grid, cells.len(), &removed_ids, tree)?;

        // The range is checked once; discarded wrappers never consume IDs.
        let mut locals = Vec::new();
        locals
            .try_reserve_exact(tree.node_count())
            .map_err(|_| Error::TableResourceLimit)?;
        let mut next = self.next_node_id;
        for local in 0..tree.node_count() {
            locals.push(if wrappers.contains(&local) {
                None
            } else {
                let id = NodeId::from_allocated(next);
                next += 1;
                Some(id)
            });
        }
        debug_assert_eq!(next, next_id);
        let fresh = |local: usize| {
            locals
                .get(local)
                .copied()
                .flatten()
                .ok_or(Error::InvalidTransaction)
        };
        let mut replacement = BTreeMap::new();
        for (local, node) in tree.data.nodes.iter().enumerate() {
            if let Some(id) = locals[local] {
                replacement.insert(id, materialize_node(node, id, fresh)?);
            }
        }
        let old_table = self.store.get(table).ok_or(Error::UnknownNode)?;
        let mut expected = BTreeMap::from([(table, old_table.clone())]);
        replacement.insert(table, old_table.clone());
        for id in &removed_ids {
            expected.insert(*id, self.store.get(*id).ok_or(Error::UnknownNode)?.clone());
        }
        let mut maps = Vec::new();
        let mut inverse_maps = Vec::new();
        for (offset, source_row) in rows.iter().enumerate() {
            let row = grid
                .row_id(rect.top() + offset)
                .ok_or(Error::InvalidTableStructure)?;
            let original = self.store.get(row).ok_or(Error::UnknownNode)?;
            let children = original
                .content()
                .as_children()
                .ok_or(Error::InvalidTableStructure)?;
            let mut start = 0;
            for cell in children {
                if grid
                    .placement(*cell)
                    .ok_or(Error::InvalidTableStructure)?
                    .column()
                    >= rect.left()
                {
                    break;
                }
                start += 1;
            }
            let mut next_children = children[..start].to_vec();
            let mut end = start;
            while let Some(cell) = children.get(end).filter(|cell| removed.contains_key(*cell)) {
                maps.push(StepMap::NodeRemoved {
                    parent: row,
                    index: start,
                    removed: removed[cell].clone(),
                });
                inverse_maps.push(StepMap::NodeInserted {
                    parent: row,
                    index: start,
                    inserted: *cell,
                });
                end += 1;
            }
            for local in template_children(tree, *source_row)? {
                let cell = fresh(*local)?;
                let index = next_children.len();
                let subtree = staged_subtree(&replacement, cell)?;
                maps.push(StepMap::NodeInserted {
                    parent: row,
                    index,
                    inserted: cell,
                });
                inverse_maps.push(StepMap::NodeRemoved {
                    parent: row,
                    index,
                    removed: subtree,
                });
                next_children.push(cell);
            }
            next_children.extend_from_slice(&children[end..]);
            expected.insert(row, original.clone());
            replacement.insert(
                row,
                Node::new(
                    row,
                    original.kind().clone(),
                    original.attrs().clone(),
                    NodeContent::children(next_children),
                )?,
            );
        }
        inverse_maps.reverse();
        self.commit_table_edit(
            TableCellRestore {
                table,
                expected: expected.into_values().collect(),
                expected_parents: BTreeMap::new(),
                replacement: replacement.into_values().collect(),
                maps,
                inverse_maps,
            },
            next_id,
        )
    }

    fn check_rect_budget(
        &self,
        grid: &TableGrid,
        removed_cells: usize,
        removed_ids: &BTreeSet<NodeId>,
        tree: &TableTreeTemplate,
    ) -> Result<()> {
        let source = tree
            .data
            .grids
            .first()
            .ok_or(Error::InvalidTableStructure)?;
        let cells = grid
            .origins()
            .len()
            .checked_sub(removed_cells)
            .and_then(|cells| cells.checked_add(source.cells))
            .ok_or(Error::TableResourceLimit)?;
        let mut budget = TableGridBudget::default();
        budget.reserve(grid.rows(), grid.columns(), cells)?;
        for node in self.store.iter().filter(|node| {
            matches!(node.kind(), NodeKind::Table)
                && node.id() != grid.table()
                && !removed_ids.contains(&node.id())
        }) {
            TableGrid::from_store(&self.store, node.id(), &mut budget)?;
        }
        // The source outer grid is replaced by the destination grid above.
        // Deleted cell forests also remove all of their old nested grids.
        for nested in tree.data.grids.iter().skip(1) {
            budget.reserve(nested.rows, nested.columns, nested.cells)?;
        }
        Ok(())
    }
}

fn template_children(tree: &TableTreeTemplate, local: usize) -> Result<&[usize]> {
    match &tree
        .data
        .nodes
        .get(local)
        .ok_or(Error::InvalidTransaction)?
        .content
    {
        TemplateContent::Children(children) => Ok(children),
        _ => Err(Error::InvalidTableStructure),
    }
}

fn outer_rows(tree: &TableTreeTemplate) -> Result<&[usize]> {
    if !tree
        .data
        .nodes
        .first()
        .is_some_and(|node| matches!(node.kind, NodeKind::Table))
    {
        return Err(Error::InvalidTableStructure);
    }
    let rows = template_children(tree, 0)?;
    if tree
        .data
        .grids
        .first()
        .is_none_or(|grid| grid.rows != rows.len())
    {
        return Err(Error::InvalidTableStructure);
    }
    for row in rows {
        if !tree
            .data
            .nodes
            .get(*row)
            .is_some_and(|node| matches!(node.kind, NodeKind::TableRow))
        {
            return Err(Error::InvalidTableStructure);
        }
    }
    Ok(rows)
}

fn staged_subtree(nodes: &BTreeMap<NodeId, Node>, root: NodeId) -> Result<BTreeSet<NodeId>> {
    let mut ids = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if !ids.insert(id) {
            return Err(Error::InvalidTransaction);
        }
        let node = nodes.get(&id).ok_or(Error::InvalidTransaction)?;
        if let Some(children) = node.content().as_children() {
            pending.extend_from_slice(children);
        }
        if let Some(inline) = node.content().as_inline() {
            pending.extend(inline.atoms().iter().map(|atom| atom.atom()));
        }
    }
    Ok(ids)
}
