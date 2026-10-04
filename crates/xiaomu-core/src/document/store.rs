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
/// A private transaction writer separates the ordered map on its first write
/// with copy-on-write, then reuses that working map for subsequent steps.
/// Unchanged node payloads stay shared. Public snapshots expose no mutation
/// capability; this prototype representation is not part of their contract.
///
/// Equality compares stored node payloads by identity; structural sharing of
/// payloads is deliberately not part of equality.
#[derive(Clone, Debug, PartialEq, Eq)]
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

    /// Returns a store with one node's payload replaced.
    ///
    /// Unchanged node payloads are reused through `Arc`, keeping the
    /// structural-sharing prototype intact.
    #[cfg(test)]
    pub(crate) fn replace_node(&self, node: Node) -> Result<Self> {
        let mut next = self.clone();
        next.replace_node_mut(node)?;
        Ok(next)
    }

    /// Replaces one payload in an unpublished working store. The first write
    /// separates a shared map; further writes keep its unique allocation.
    pub(crate) fn replace_node_mut(&mut self, node: Node) -> Result<()> {
        let id = node.id();
        if !self.nodes.contains_key(&id) {
            return Err(Error::UnknownNode);
        }

        Arc::make_mut(&mut self.nodes).insert(id, Arc::new(node));
        Ok(())
    }

    /// Adds one previously absent node to an unpublished working store.
    pub(crate) fn insert_node_mut(&mut self, node: Node) -> Result<()> {
        let id = node.id();
        if self.nodes.contains_key(&id) {
            return Err(Error::DuplicateChildReference);
        }

        Arc::make_mut(&mut self.nodes).insert(id, Arc::new(node));
        Ok(())
    }

    /// Removes node identities from an unpublished working store.
    ///
    /// Missing identities are ignored so callers can remove whole subtrees in
    /// one pass.
    pub(crate) fn remove_nodes_mut(&mut self, removed: &BTreeSet<NodeId>) {
        if removed.iter().all(|id| !self.contains(*id)) {
            return;
        }
        let nodes = Arc::make_mut(&mut self.nodes);
        for id in removed {
            nodes.remove(id);
        }
    }

    /// Exchanges exact payloads after complete preflight. Replacement-only IDs
    /// must be absent and expected payloads must match. Copy-on-write separates
    /// the map only if it is still shared with a published snapshot.
    pub(crate) fn exchange_mut(&mut self, expected: &[Node], replacement: &[Node]) -> Result<()> {
        let expected_ids: BTreeSet<_> = expected.iter().map(Node::id).collect();
        let replacement_ids: BTreeSet<_> = replacement.iter().map(Node::id).collect();
        if expected_ids.len() != expected.len()
            || replacement_ids.len() != replacement.len()
            || expected
                .iter()
                .any(|node| self.get(node.id()) != Some(node))
            || replacement_ids
                .difference(&expected_ids)
                .any(|id| self.contains(*id))
        {
            return Err(Error::InvalidTransaction);
        }
        let next = Arc::make_mut(&mut self.nodes);
        for id in expected_ids.difference(&replacement_ids) {
            next.remove(id);
        }
        for node in replacement {
            next.insert(node.id(), Arc::new(node.clone()));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn shares_node_payload(&self, other: &Self, id: NodeId) -> bool {
        match (self.nodes.get(&id), other.nodes.get(&id)) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }

    #[cfg(test)]
    pub(crate) fn map_storage_id(&self) -> usize {
        Arc::as_ptr(&self.nodes) as usize
    }
}

#[cfg(test)]
mod cow_tests;

/// Safe bottom-up builder for an initial canonical node store.
///
/// Structural children and inline-atom placements must already have been
/// allocated by this builder. This keeps ordinary construction deterministic
/// and prevents dangling references before full-document validation runs.
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
        self.validate_references(&kind, &content)?;

        let id = NodeId::from_allocated(self.next_id);
        let next_id = self.next_id.checked_add(1).ok_or(Error::NodeIdExhausted)?;
        let node = Node::new(id, kind, attrs, content)?;

        self.nodes.insert(id, Arc::new(node));
        self.next_id = next_id;
        Ok(id)
    }

    /// Returns the identity that the next successful [`insert`](Self::insert)
    /// would allocate.
    ///
    /// This gives tests a deterministic way to obtain a node identity that is
    /// guaranteed to be absent from a finished store, without exposing raw
    /// construction of `NodeId`.
    #[must_use]
    pub const fn peek_next_id(&self) -> NodeId {
        NodeId::from_allocated(self.next_id)
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

    fn validate_references(&self, parent_kind: &NodeKind, content: &NodeContent) -> Result<()> {
        if let Some(children) = content.as_children() {
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
        }

        if let Some(inline) = content.as_inline() {
            for placement in inline.atoms() {
                let atom = self
                    .nodes
                    .get(&placement.atom())
                    .ok_or(Error::UnknownNode)?;
                if !matches!(atom.kind(), NodeKind::InlineAtom(_)) {
                    return Err(Error::InvalidInlineAtomReference);
                }
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
        NodeKind::TaskList => matches!(child, NodeKind::TaskItem),
        NodeKind::Table => matches!(child, NodeKind::TableRow),
        NodeKind::TableRow => child.is_table_cell(),
        NodeKind::TaskItem => !matches!(
            child,
            NodeKind::Document
                | NodeKind::ListItem
                | NodeKind::TaskItem
                | NodeKind::TableRow
                | NodeKind::TableCell
                | NodeKind::TableHeader
                | NodeKind::InlineAtom(_)
        ),
        NodeKind::Document
        | NodeKind::Quote
        | NodeKind::ListItem
        | NodeKind::TableCell
        | NodeKind::TableHeader => !matches!(
            child,
            NodeKind::Document
                | NodeKind::ListItem
                | NodeKind::TaskItem
                | NodeKind::TableRow
                | NodeKind::TableCell
                | NodeKind::TableHeader
                | NodeKind::InlineAtom(_)
        ),
        NodeKind::Custom(_) => !matches!(child, NodeKind::InlineAtom(_)),
        NodeKind::Paragraph
        | NodeKind::Heading(_)
        | NodeKind::CodeBlock
        | NodeKind::HorizontalRule
        | NodeKind::Image
        | NodeKind::InlineAtom(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, MarkSet, TextRun,
    };

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
    fn builder_requires_existing_inline_atom_target() {
        let mut builder = NodeStoreBuilder::new();
        let missing = builder.peek_next_id();
        let mixed = InlineContent::with_atoms(
            [TextRun::new("a", MarkSet::empty()).unwrap()],
            [InlineAtomPlacement::new(
                missing,
                crate::text::TextOffset::ZERO,
            )],
        )
        .unwrap();

        assert_eq!(
            builder.insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(mixed),
            ),
            Err(Error::UnknownNode)
        );
    }

    #[test]
    fn inline_atom_can_only_be_referenced_from_inline_content() {
        let mut builder = NodeStoreBuilder::new();
        let atom = builder
            .insert(
                NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(InlineAtomContent::new("@A").unwrap()),
            )
            .unwrap();

        assert_eq!(
            builder.insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([atom]),
            ),
            Err(Error::InvalidChildKind)
        );

        let mixed = InlineContent::with_atoms(
            [TextRun::new("a", MarkSet::empty()).unwrap()],
            [InlineAtomPlacement::new(
                atom,
                crate::text::TextOffset::ZERO,
            )],
        )
        .unwrap();
        assert!(
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(mixed),
                )
                .is_ok()
        );
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
