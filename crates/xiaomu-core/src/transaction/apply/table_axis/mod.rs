//! Span-aware logical row/column edits; product default/type policy is explicit.

mod delete;
mod insert;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{
    AttrValue, Node, NodeAttrs, NodeContent, NodeId, NodeKind, TableGrid, TableGridBudget,
};
use crate::mapping::StepMap;
use crate::transaction::{TableCellRestore, TransactionStep};
use crate::{Error, Result};

use super::ApplyContext;

/// One staged table patch. No intermediate tree or allocated ID is published.
struct TableEdit {
    table: NodeId,
    expected: BTreeMap<NodeId, Node>,
    replacement: BTreeMap<NodeId, Node>,
    maps: Vec<StepMap>,
    inverse_maps: Vec<StepMap>,
    next_id: u64,
}

impl TableEdit {
    fn new(context: &ApplyContext, table: NodeId) -> Result<Self> {
        let node = context.store.get(table).ok_or(Error::UnknownNode)?.clone();
        Ok(Self {
            table,
            // Bind row/column inverses to this exact table row list as well as
            // affected payloads. Moving or reordering rows makes them stale.
            expected: BTreeMap::from([(table, node.clone())]),
            replacement: BTreeMap::from([(table, node)]),
            maps: Vec::new(),
            inverse_maps: Vec::new(),
            next_id: context.next_node_id,
        })
    }

    fn replace(&mut self, context: &ApplyContext, node: Node) -> Result<()> {
        let old = context.store.get(node.id()).ok_or(Error::UnknownNode)?;
        self.expected
            .entry(node.id())
            .or_insert_with(|| old.clone());
        self.replacement.insert(node.id(), node);
        Ok(())
    }

    fn rewrite_children(
        &mut self,
        context: &ApplyContext,
        id: NodeId,
        children: Vec<NodeId>,
    ) -> Result<()> {
        let old = context.store.get(id).ok_or(Error::UnknownNode)?;
        self.replace(
            context,
            Node::new(
                id,
                old.kind().clone(),
                old.attrs().clone(),
                NodeContent::children(children),
            )?,
        )
    }

    fn remove_only(&mut self, context: &ApplyContext, id: NodeId) -> Result<()> {
        let old = context.store.get(id).ok_or(Error::UnknownNode)?;
        self.expected.insert(id, old.clone());
        self.replacement.remove(&id);
        Ok(())
    }

    fn remove_subtree(&mut self, context: &ApplyContext, root: NodeId) -> Result<BTreeSet<NodeId>> {
        let removed = context.collect_subtree(root);
        for id in &removed {
            self.remove_only(context, *id)?;
        }
        Ok(removed)
    }

    fn allocate(&mut self, kind: NodeKind, content: NodeContent) -> Result<NodeId> {
        let id = NodeId::from_allocated(self.next_id);
        let next = self.next_id.checked_add(1).ok_or(Error::NodeIdExhausted)?;
        let node = Node::new(id, kind, NodeAttrs::empty(), content)?;
        self.replacement.insert(id, node);
        self.next_id = next;
        Ok(id)
    }

    fn empty_cell(&mut self, kind: &NodeKind) -> Result<(NodeId, NodeId)> {
        let paragraph = self.allocate(NodeKind::Paragraph, NodeContent::empty_inline())?;
        let cell = self.allocate(kind.clone(), NodeContent::children([paragraph]))?;
        Ok((cell, paragraph))
    }

    fn record(&mut self, forward: StepMap, inverse: StepMap) {
        self.maps.push(forward);
        self.inverse_maps.push(inverse);
    }

    fn finish(
        mut self,
        context: &mut ApplyContext,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        self.inverse_maps.reverse();
        context.commit_table_edit(
            TableCellRestore {
                table: self.table,
                expected: self.expected.into_values().collect(),
                expected_parents: BTreeMap::new(),
                replacement: self.replacement.into_values().collect(),
                maps: self.maps,
                inverse_maps: self.inverse_maps,
            },
            self.next_id,
        )
    }
}

impl ApplyContext {
    fn logical_grid(&self, table: NodeId) -> Result<TableGrid> {
        TableGrid::from_store(&self.store, table, &mut TableGridBudget::default())
    }

    fn check_axis_growth(
        &self,
        grid: &TableGrid,
        rows: usize,
        columns: usize,
        cells: usize,
        nodes: usize,
    ) -> Result<()> {
        self.next_node_id
            .checked_add(u64::try_from(nodes).map_err(|_| Error::NodeIdExhausted)?)
            .ok_or(Error::NodeIdExhausted)?;
        let mut budget = TableGridBudget::default();
        budget.reserve(rows, columns, cells)?;
        for node in self
            .store
            .iter()
            .filter(|node| matches!(node.kind(), NodeKind::Table) && node.id() != grid.table())
        {
            TableGrid::from_store(&self.store, node.id(), &mut budget)?;
        }
        // Bounds geometry and generated nodes, not arbitrary attribute/content
        // bytes or all execution memory. The staged edit clones the store once.
        Ok(())
    }
}

fn validate_kinds(kinds: &[NodeKind], dimension: usize) -> Result<()> {
    if kinds.len() != dimension || kinds.iter().any(|kind| !kind.is_table_cell()) {
        return Err(Error::InvalidTransaction);
    }
    Ok(())
}

fn validate_delete_range(start: usize, end: usize, dimension: usize) -> Result<()> {
    if start >= end || end > dimension || end - start == dimension {
        return Err(Error::InvalidTransaction);
    }
    Ok(())
}

enum WidthEdit {
    Keep,
    Insert(usize),
    Delete { start: usize, end: usize },
}

fn resize_cell(node: &Node, key: &str, span: usize, width_edit: WidthEdit) -> Result<Node> {
    if span == 0 {
        return Err(Error::InvalidTransaction);
    }
    let mut values: BTreeMap<_, _> = node
        .attrs()
        .iter()
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect();
    values.insert(
        key.to_owned(),
        AttrValue::Integer(i64::try_from(span).map_err(|_| Error::InvalidTableAttrs)?),
    );
    if let Some(AttrValue::List(widths)) = values.get_mut("colwidth") {
        match width_edit {
            WidthEdit::Keep => {}
            WidthEdit::Insert(index) => {
                if index > widths.len() {
                    return Err(Error::InvalidTableAttrs);
                }
                widths
                    .try_reserve_exact(1)
                    .map_err(|_| Error::TableResourceLimit)?;
                widths.insert(index, AttrValue::Integer(0));
            }
            WidthEdit::Delete { start, end } => {
                if start > end || end > widths.len() {
                    return Err(Error::InvalidTableAttrs);
                }
                widths.drain(start..end);
            }
        }
    }
    Node::new(
        node.id(),
        node.kind().clone(),
        NodeAttrs::new(values)?,
        node.content().clone(),
    )
}
