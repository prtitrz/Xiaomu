//! Fresh-ID materialization of a bounded opaque table tree in one store batch.

use crate::document::{
    Node, NodeContent, NodeId, NodeKind, TableGrid, TableGridBudget, allows_child,
};
use crate::mapping::StepMap;
#[cfg(test)]
use crate::transaction::forest::TemplateContent;
use crate::transaction::forest::materialize_node;
use crate::transaction::{TableTreeTemplate, TransactionStep};
use crate::{Error, Result};

use super::ApplyContext;

#[cfg(test)]
mod tests;

impl ApplyContext {
    pub(super) fn apply_insert_table_tree(
        &mut self,
        parent: NodeId,
        index: usize,
        tree: &TableTreeTemplate,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let old_parent = self.store.get(parent).ok_or(Error::UnknownNode)?;
        if !allows_child(old_parent.kind(), &NodeKind::Table) {
            return Err(Error::InvalidChildKind);
        }
        let mut children = self.children(parent)?;
        if index > children.len() {
            return Err(Error::InvalidTransaction);
        }
        if !tree
            .data
            .nodes
            .first()
            .is_some_and(|node| matches!(node.kind, NodeKind::Table))
        {
            return Err(Error::InvalidTableStructure);
        }
        let count = tree.node_count();
        let next_id = self
            .next_node_id
            .checked_add(u64::try_from(count).map_err(|_| Error::NodeIdExhausted)?)
            .ok_or(Error::NodeIdExhausted)?;
        let mut budget = TableGridBudget::default();
        for grid in &tree.data.grids {
            budget.reserve(grid.rows, grid.columns, grid.cells)?;
        }
        for node in self
            .store
            .iter()
            .filter(|node| matches!(node.kind(), NodeKind::Table))
        {
            TableGrid::from_store(&self.store, node.id(), &mut budget)?;
        }
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(count.checked_add(1).ok_or(Error::TableResourceLimit)?)
            .map_err(|_| Error::TableResourceLimit)?;
        let fresh = |local: usize| -> Result<NodeId> {
            if local >= count {
                return Err(Error::InvalidTransaction);
            }
            let offset = u64::try_from(local).map_err(|_| Error::NodeIdExhausted)?;
            Ok(NodeId::from_allocated(
                self.next_node_id
                    .checked_add(offset)
                    .ok_or(Error::NodeIdExhausted)?,
            ))
        };
        for (local, node) in tree.data.nodes.iter().enumerate() {
            replacement.push(materialize_node(node, fresh(local)?, fresh)?);
        }
        let root = fresh(0)?;
        children.insert(index, root);
        replacement.push(Node::new(
            parent,
            old_parent.kind().clone(),
            old_parent.attrs().clone(),
            NodeContent::children(children),
        )?);
        let expected = old_parent.clone();
        self.store
            .exchange_mut(std::slice::from_ref(&expected), &replacement)?;
        self.next_node_id = next_id;
        Ok((
            vec![StepMap::NodeInserted {
                parent,
                index,
                inserted: root,
            }],
            vec![TransactionStep::RemoveNode { node: root }],
        ))
    }
}
