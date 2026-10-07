//! Fresh-lineage whole-document replacement through the normal apply pipeline.

use std::{collections::BTreeSet, mem::size_of};

use crate::document::{DocumentLineage, NodeId, NodeKind, NodeStore};
use crate::mapping::StepMap;
use crate::transaction::forest::{self, TemplateContent, budget::CaptureBudget};
use crate::transaction::{DocumentRestore, DocumentTemplate, TransactionStep};
use crate::{Error, Result};

use super::ApplyContext;

#[cfg(test)]
mod tests;

/// Per-transaction admission for all whole-snapshot operations. Other typed
/// operations retain their existing resource semantics.
pub(super) struct SnapshotBudget {
    steps: usize,
    payload: CaptureBudget,
}

impl Default for SnapshotBudget {
    fn default() -> Self {
        Self {
            steps: 0,
            payload: CaptureBudget::document(),
        }
    }
}

impl SnapshotBudget {
    fn next(&mut self) -> Result<CaptureBudget> {
        self.steps = self
            .steps
            .checked_add(1)
            .ok_or(Error::SnapshotResourceLimit)?;
        if self.steps > 64 {
            return Err(Error::SnapshotResourceLimit);
        }
        let mut budget = self.payload.clone();
        budget.operation(size_of::<TransactionStep>() + size_of::<DocumentRestore>())?;
        Ok(budget)
    }
}

impl ApplyContext {
    pub(super) fn apply_replace_document(
        &mut self,
        template: &DocumentTemplate,
        lineage: &DocumentLineage,
        aggregate: &mut SnapshotBudget,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let root = template.data.nodes.first().ok_or(Error::InvalidRootNode)?;
        let TemplateContent::Children(new_children) = &root.content else {
            return Err(Error::InvalidRootNode);
        };
        if !matches!(root.kind, NodeKind::Document) {
            return Err(Error::InvalidRootNode);
        }
        let count = template.node_count();
        let next_id = self
            .next_node_id
            .checked_add(u64::try_from(count).map_err(|_| Error::NodeIdExhausted)?)
            .ok_or(Error::NodeIdExhausted)?;

        // Account both the still-owned template and its materialized copy,
        // then every old payload the inverse retains, before copying anything.
        let mut budget = aggregate.next()?;
        budget.absorb(&template.data.budget)?;
        budget.absorb(&template.data.budget)?;
        forest::account_document(&self.store, self.root, &mut budget)?;
        let old_children = children(&self.store, self.root)?;
        account_maps(
            &mut budget,
            old_children.len(),
            new_children.len(),
            self.store.len(),
            count + 1,
        )?;
        let fresh = |local: usize| -> Result<NodeId> {
            if local == 0 {
                return Ok(self.root);
            }
            if local > count {
                return Err(Error::InvalidTransaction);
            }
            let offset = u64::try_from(local - 1).map_err(|_| Error::NodeIdExhausted)?;
            Ok(NodeId::from_allocated(
                self.next_node_id
                    .checked_add(offset)
                    .ok_or(Error::NodeIdExhausted)?,
            ))
        };
        let replacement = NodeStore::from_payloads(
            template
                .data
                .nodes
                .iter()
                .enumerate()
                .map(|(local, node)| forest::materialize_node(node, fresh(local)?, fresh)),
        )?;
        let maps = replacement_maps(self.root, &self.store, &replacement)?;
        let previous = std::mem::replace(&mut self.store, replacement);
        self.next_node_id = next_id;
        aggregate.payload = budget;
        Ok((
            maps,
            vec![TransactionStep::RestoreDocument {
                restore: DocumentRestore {
                    lineage: lineage.clone(),
                    root: self.root,
                    expected: self.store.clone(),
                    replacement: previous,
                    minimum_next_id: next_id,
                },
            }],
        ))
    }

    pub(super) fn apply_restore_document(
        &mut self,
        restore: &DocumentRestore,
        lineage: &DocumentLineage,
        aggregate: &mut SnapshotBudget,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        if lineage != &restore.lineage
            || self.root != restore.root
            || self.next_node_id < restore.minimum_next_id
            || self.store != restore.expected
        {
            return Err(Error::InvalidTransaction);
        }
        let mut budget = aggregate.next()?;
        forest::account_document(&self.store, self.root, &mut budget)?;
        forest::account_document(&restore.replacement, self.root, &mut budget)?;
        account_maps(
            &mut budget,
            children(&self.store, self.root)?.len(),
            children(&restore.replacement, self.root)?.len(),
            self.store.len(),
            restore.replacement.len(),
        )?;
        // Stores and their accounting are engine-produced and immutable.
        // A structural match is intentional: earlier Undo/Redo may rebuild an
        // equal expected store using different Arc allocations.
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
        let maps = replacement_maps(self.root, &self.store, &restore.replacement)?;
        let previous = std::mem::replace(&mut self.store, restore.replacement.clone());
        self.next_node_id = next_id;
        aggregate.payload = budget;
        Ok((
            maps,
            vec![TransactionStep::RestoreDocument {
                restore: DocumentRestore {
                    lineage: lineage.clone(),
                    root: self.root,
                    expected: self.store.clone(),
                    replacement: previous,
                    minimum_next_id: next_id,
                },
            }],
        ))
    }
}

fn account_maps(
    budget: &mut CaptureBudget,
    old_roots: usize,
    new_roots: usize,
    old_nodes: usize,
    new_nodes: usize,
) -> Result<()> {
    let roots = old_roots
        .checked_add(new_roots)
        .ok_or(Error::SnapshotResourceLimit)?;
    let nodes = old_nodes
        .checked_add(new_nodes)
        .ok_or(Error::SnapshotResourceLimit)?;
    budget.maps(roots, nodes)
}

fn children(store: &NodeStore, root: NodeId) -> Result<&[NodeId]> {
    store
        .get(root)
        .ok_or(Error::UnknownNode)?
        .content()
        .as_children()
        .ok_or(Error::InvalidRootNode)
}

/// Delete from the front, then append in order. This exposes the true root-gap
/// transformation and marks every removed descendant/inline atom as deleted.
fn replacement_maps(root: NodeId, before: &NodeStore, after: &NodeStore) -> Result<Vec<StepMap>> {
    let old = children(before, root)?;
    let new = children(after, root)?;
    let mut maps = Vec::new();
    maps.try_reserve_exact(
        old.len()
            .checked_add(new.len())
            .ok_or(Error::SnapshotResourceLimit)?,
    )
    .map_err(|_| Error::SnapshotResourceLimit)?;
    for child in old {
        let mut removed = BTreeSet::new();
        let mut pending = vec![*child];
        while let Some(id) = pending.pop() {
            if !removed.insert(id) {
                return Err(Error::InvalidTransaction);
            }
            let node = before.get(id).ok_or(Error::UnknownNode)?;
            if let Some(children) = node.content().as_children() {
                pending.extend_from_slice(children);
            }
            if let Some(inline) = node.content().as_inline() {
                pending.extend(inline.atoms().iter().map(|atom| atom.atom()));
            }
        }
        maps.push(StepMap::NodeRemoved {
            parent: root,
            index: 0,
            removed,
        });
    }
    for (index, inserted) in new.iter().enumerate() {
        maps.push(StepMap::NodeInserted {
            parent: root,
            index,
            inserted: *inserted,
        });
    }
    Ok(maps)
}
