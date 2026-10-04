//! Exact subtree restoration with one ordered-store clone and ID preflight.

use std::collections::BTreeSet;

use crate::document::{Node, NodeContent, NodeId};
use crate::mapping::StepMap;
use crate::transaction::TransactionStep;
use crate::{Error, Result};

use super::ApplyContext;

impl ApplyContext {
    pub(super) fn apply_restore_subtree_batched(
        &mut self,
        parent: NodeId,
        index: usize,
        root: NodeId,
        nodes: &[Node],
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        if !nodes.iter().any(|node| node.id() == root)
            || nodes.iter().any(|node| self.store.contains(node.id()))
        {
            return Err(Error::InvalidTransaction);
        }
        let mut children = self.children(parent)?;
        if index > children.len() {
            return Err(Error::InvalidTransaction);
        }
        let mut unique = BTreeSet::new();
        let mut next_id = self.next_node_id;
        for node in nodes {
            if !unique.insert(node.id()) {
                return Err(Error::DuplicateChildReference);
            }
            next_id = next_id.max(
                node.id()
                    .raw()
                    .checked_add(1)
                    .ok_or(Error::NodeIdExhausted)?,
            );
        }
        let old_parent = self.store.get(parent).ok_or(Error::UnknownNode)?;
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(
                nodes
                    .len()
                    .checked_add(1)
                    .ok_or(Error::TableResourceLimit)?,
            )
            .map_err(|_| Error::TableResourceLimit)?;
        replacement.extend_from_slice(nodes);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{NodeAttrs, NodeKind, NodeStoreBuilder};

    fn context() -> ApplyContext {
        let mut builder = NodeStoreBuilder::new();
        let p = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([p]),
            )
            .unwrap();
        let next_node_id = builder.peek_next_id().raw();
        ApplyContext {
            root,
            store: builder.finish(),
            next_node_id,
        }
    }

    fn absent(context: &ApplyContext, delta: u64) -> Node {
        Node::new(
            NodeId::from_allocated(context.next_node_id + delta),
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap()
    }

    #[test]
    fn restore_rejects_duplicate_live_and_absent_root_payloads_without_id_consumption() {
        let mut context = context();
        let future = absent(&context, 12);
        let live = context.store.get(context.root).unwrap().clone();
        let store = context.store.clone();
        let ceiling = context.next_node_id;
        for (root, nodes, error) in [
            (
                future.id(),
                vec![future.clone(), future.clone()],
                Error::DuplicateChildReference,
            ),
            (
                future.id(),
                vec![future.clone(), live],
                Error::InvalidTransaction,
            ),
            (future.id(), vec![], Error::InvalidTransaction),
        ] {
            assert_eq!(
                context
                    .apply_restore_subtree_batched(context.root, 1, root, &nodes)
                    .unwrap_err(),
                error
            );
            assert_eq!(context.store, store);
            assert_eq!(context.next_node_id, ceiling);
        }
    }

    #[test]
    fn restore_honors_future_identity_ceiling_and_preserves_unrelated_payloads() {
        let mut context = context();
        let future = absent(&context, 12);
        let unrelated = context.children(context.root).unwrap()[0];
        let before = context.store.clone();
        context
            .apply_restore_subtree_batched(
                context.root,
                1,
                future.id(),
                std::slice::from_ref(&future),
            )
            .unwrap();
        assert_eq!(context.next_node_id, future.id().raw() + 1);
        assert!(before.shares_node_payload(&context.store, unrelated));
        assert_eq!(context.store.get(future.id()), Some(&future));
    }

    #[test]
    fn restore_rejects_exhausted_identity_before_store_clone_or_allocator_mutation() {
        let mut context = context();
        let impossible = Node::new(
            NodeId::from_allocated(u64::MAX),
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
        let before = context.store.clone();
        let ceiling = context.next_node_id;
        assert_eq!(
            context
                .apply_restore_subtree_batched(context.root, 1, impossible.id(), &[impossible])
                .unwrap_err(),
            Error::NodeIdExhausted
        );
        assert_eq!(context.store, before);
        assert_eq!(context.next_node_id, ceiling);
    }
}
