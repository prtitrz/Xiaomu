//! Persistent-ish node storage and safe initial construction.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{Error, Result};

use super::{Node, NodeAttrs, NodeContent, NodeId, NodeKind};

/// Read-only canonical node storage shared by document snapshots.
///
/// The map itself is wrapped in `Arc`, and each node payload is also an `Arc`.
/// Replacing one node clones only the ordered map and reuses all unchanged node
/// payloads. This is the P0 structural-sharing prototype; the public contract
/// does not depend on this concrete representation.
#[derive(Clone, Debug)]
pub struct NodeStore {
    nodes: Arc<BTreeMap<NodeId, Arc<Node>>>,
}

impl NodeStore {
    fn from_nodes(nodes: BTreeMap<NodeId, Arc<Node>>) -> Self {
        Self {
            nodes: Arc::new(nodes),
        }
    }

    /// Returns a node by stable identity.
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id).map(Arc::as_ref)
    }

    /// Returns whether a node exists.
    #[must_use]
    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Returns the number of canonical nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Iterates nodes in deterministic ID order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Node> {
        self.nodes.values().map(Arc::as_ref)
    }

    #[cfg(test)]
    pub(crate) fn replace_node(&self, node: Node) -> Result<Self> {
        let id = node.id();
        if !self.nodes.contains_key(&id) {
            return Err(Error::UnknownNode);
        }

        let mut next = self.nodes.as_ref().clone();
        next.insert(id, Arc::new(node));
        Ok(Self::from_nodes(next))
    }

    #[cfg(test)]
    pub(crate) fn shares_node_payload(&self, other: &Self, id: NodeId) -> bool {
        match (self.nodes.get(&id), other.nodes.get(&id)) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }
}

/// Safe bottom-up builder for an initial canonical node store.
///
/// Child references must already have been allocated by this builder. This
/// makes ordinary construction deterministic and prevents dangling references
/// before full-document validation runs.
#[derive(Debug)]
pub struct NodeStoreBuilder {
    nodes: BTreeMap<NodeId, Arc<Node>>,
    next_id: u64,
}

impl NodeStoreBuilder {
    /// Creates an empty deterministic builder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Allocates and inserts one validated node.
    pub fn insert(
        &mut self,
        kind: NodeKind,
        attrs: NodeAttrs,
        content: NodeContent,
    ) -> Result<NodeId> {
        self.validate_child_references(&kind, &content)?;

        let id = NodeId::from_allocated(self.next_id);
        let next_id = self.next_id.checked_add(1).ok_or(Error::NodeIdExhausted)?;
        let node = Node::new(id, kind, attrs, content)?;

        self.nodes.insert(id, Arc::new(node));
        self.next_id = next_id;
        Ok(id)
    }

    /// Returns the number of allocated nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns whether no nodes have been allocated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Finishes the builder into immutable storage.
    #[must_use]
    pub fn finish(self) -> NodeStore {
        NodeStore::from_nodes(self.nodes)
    }

    fn validate_child_references(
        &self,
        parent_kind: &NodeKind,
        content: &NodeContent,
    ) -> Result<()> {
        let Some(children) = content.as_children() else {
            return Ok(());
        };

        let mut unique = BTreeSet::new();
        for child_id in children {
            if !unique.insert(*child_id) {
                return Err(Error::DuplicateChildReference);
            }

            let child = self.nodes.get(child_id).ok_or(Error::UnknownNode)?;
            if !allows_child(parent_kind, child.kind()) {
                return Err(Error::InvalidChildKind);
            }
        }

        Ok(())
    }
}

impl Default for NodeStoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn allows_child(parent: &NodeKind, child: &NodeKind) -> bool {
    match parent {
        NodeKind::BulletList | NodeKind::OrderedList => matches!(child, NodeKind::ListItem),
        NodeKind::Document | NodeKind::Quote | NodeKind::ListItem => {
            !matches!(child, NodeKind::Document | NodeKind::ListItem)
        }
        NodeKind::Custom(_) => true,
        NodeKind::Paragraph
        | NodeKind::Heading(_)
        | NodeKind::CodeBlock
        | NodeKind::HorizontalRule
        | NodeKind::Image => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{InlineContent, MarkSet, TextRun};

    #[test]
    fn failed_insert_does_not_consume_a_node_id() {
        let mut builder = NodeStoreBuilder::new();

        assert_eq!(
            builder.insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::children([]),
            ),
            Err(Error::InvalidNodeContent)
        );

        let first_valid = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();

        assert_eq!(first_valid, NodeId::from_allocated(1));
    }

    #[test]
    fn replacement_reuses_unchanged_node_payloads() {
        let mut builder = NodeStoreBuilder::new();
        let first = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let second = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let store = builder.finish();

        let replacement = store
            .get(first)
            .unwrap()
            .with_content(NodeContent::Inline(
                InlineContent::new([TextRun::new("changed", MarkSet::empty()).unwrap()]).unwrap(),
            ))
            .unwrap();
        let next = store.replace_node(replacement).unwrap();

        assert!(!store.shares_node_payload(&next, first));
        assert!(store.shares_node_payload(&next, second));
    }
}
