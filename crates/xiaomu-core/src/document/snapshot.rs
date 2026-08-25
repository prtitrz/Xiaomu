//! Immutable canonical document snapshot and full-tree validation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{Error, Result};

use super::{DocumentRevision, DocumentVersion, Node, NodeId, NodeKind, NodeStore, allows_child};

/// Immutable canonical Xiaomu document snapshot.
///
/// Hosts can query this value but cannot mutate its node store directly.
/// P0.4 transactions will become the public mutation path that produces new
/// validated snapshots.
#[derive(Clone, Debug)]
pub struct XiaomuDocument {
    version: DocumentVersion,
    revision: DocumentRevision,
    root: NodeId,
    store: NodeStore,
    next_node_id: u64,
}

impl XiaomuDocument {
    /// Creates an initial snapshot using the current schema version.
    ///
    /// The entire reachable tree is validated before the snapshot is returned.
    pub fn new(root: NodeId, store: NodeStore) -> Result<Self> {
        validate_tree(root, &store)?;
        Ok(Self {
            version: DocumentVersion::CURRENT,
            revision: DocumentRevision::INITIAL,
            root,
            next_node_id: next_id_after(&store),
            store,
        })
    }

    /// Creates a validated snapshot from application output.
    pub(crate) fn from_applied_parts(
        version: DocumentVersion,
        revision: DocumentRevision,
        root: NodeId,
        store: NodeStore,
        next_node_id: u64,
    ) -> Result<Self> {
        validate_tree(root, &store)?;
        Ok(Self {
            version,
            revision,
            root,
            store,
            next_node_id,
        })
    }

    /// Returns the node identity that the next inserted node will receive.
    pub(crate) const fn next_node_id(&self) -> u64 {
        self.next_node_id
    }

    /// Returns the canonical schema version.
    #[must_use]
    pub const fn version(&self) -> DocumentVersion {
        self.version
    }

    /// Returns this snapshot's local document revision.
    #[must_use]
    pub const fn revision(&self) -> DocumentRevision {
        self.revision
    }

    /// Returns the root document node identity.
    #[must_use]
    pub const fn root(&self) -> NodeId {
        self.root
    }

    /// Returns one canonical node by identity.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.store.get(id)
    }

    /// Returns the parent of `id` in this snapshot.
    ///
    /// The document root has no parent. Unknown identities also yield
    /// `None`; callers that need to distinguish a missing node should query
    /// [`Self::node`] first.
    #[must_use]
    pub fn parent_of(&self, id: NodeId) -> Option<NodeId> {
        if id == self.root {
            return None;
        }

        let mut queue = VecDeque::from([self.root]);
        while let Some(current) = queue.pop_front() {
            let Some(children) = self
                .node(current)
                .and_then(|node| node.content().as_children())
            else {
                continue;
            };
            if children.contains(&id) {
                return Some(current);
            }
            queue.extend(children.iter().copied());
        }
        None
    }

    /// Returns the read-only node store.
    #[must_use]
    pub const fn store(&self) -> &NodeStore {
        &self.store
    }

    /// Returns the number of canonical nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.store.len()
    }

    /// Revalidates the full snapshot.
    ///
    /// This is useful at trust boundaries and in invariant tests. Normal safe
    /// construction already validates before returning a document.
    pub fn validate(&self) -> Result<()> {
        validate_tree(self.root, &self.store)
    }
}

fn next_id_after(store: &NodeStore) -> u64 {
    store
        .iter()
        .map(Node::id)
        .map(NodeId::raw)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .expect("node id space exhausted in a validated store is unreachable")
}

fn validate_tree(root: NodeId, store: &NodeStore) -> Result<()> {
    let root_node = store.get(root).ok_or(Error::UnknownNode)?;
    if !matches!(root_node.kind(), NodeKind::Document) {
        return Err(Error::InvalidRootNode);
    }

    let mut visited = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut parent_counts: BTreeMap<NodeId, usize> = BTreeMap::new();
    let mut stack = vec![(root, true)];

    while let Some((id, entering)) = stack.pop() {
        if !entering {
            active.remove(&id);
            continue;
        }

        if active.contains(&id) {
            return Err(Error::CyclicDocument);
        }

        if !visited.insert(id) {
            continue;
        }

        active.insert(id);
        stack.push((id, false));

        let node = store.get(id).ok_or(Error::UnknownNode)?;
        let Some(children) = node.content().as_children() else {
            continue;
        };

        for child_id in children.iter().rev() {
            let child = store.get(*child_id).ok_or(Error::UnknownNode)?;
            if !allows_child(node.kind(), child.kind()) {
                return Err(Error::InvalidChildKind);
            }

            if active.contains(child_id) {
                return Err(Error::CyclicDocument);
            }

            let count = parent_counts.entry(*child_id).or_default();
            *count += 1;
            if *count > 1 {
                return Err(Error::MultipleNodeParents);
            }

            stack.push((*child_id, true));
        }
    }

    if parent_counts.contains_key(&root) {
        return Err(Error::InvalidRootNode);
    }

    if visited.len() != store.len() {
        return Err(Error::UnreachableNode);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        InlineContent, MarkSet, NodeAttrs, NodeContent, NodeStoreBuilder, TextRun,
    };

    #[test]
    fn cycles_are_reported_as_cycles_before_multiple_parent_errors() {
        let mut builder = NodeStoreBuilder::new();
        let leaf_quote = builder
            .insert(
                NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children([]),
            )
            .unwrap();
        let parent_quote = builder
            .insert(
                NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children([leaf_quote]),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([parent_quote]),
            )
            .unwrap();
        let store = builder.finish();

        let cyclic_leaf = store
            .get(leaf_quote)
            .unwrap()
            .with_content(NodeContent::children([parent_quote]))
            .unwrap();
        let cyclic_store = store.replace_node(cyclic_leaf).unwrap();

        assert_eq!(
            validate_tree(root, &cyclic_store),
            Err(Error::CyclicDocument)
        );
    }

    #[test]
    fn a_new_revision_reuses_unchanged_node_payloads() {
        let mut builder = NodeStoreBuilder::new();
        let changed = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let unchanged = builder
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
                NodeContent::children([changed, unchanged]),
            )
            .unwrap();
        let original = XiaomuDocument::new(root, builder.finish()).unwrap();

        let replacement = original
            .store
            .get(changed)
            .unwrap()
            .with_content(NodeContent::Inline(
                InlineContent::new([TextRun::new("changed", MarkSet::empty()).unwrap()]).unwrap(),
            ))
            .unwrap();
        let next_store = original.store.replace_node(replacement).unwrap();
        let next = XiaomuDocument {
            version: original.version,
            revision: original.revision.next().unwrap(),
            root: original.root,
            store: next_store,
            next_node_id: original.next_node_id,
        };

        assert!(next.validate().is_ok());
        assert_eq!(next.revision().as_u64(), 1);
        assert!(!original.store.shares_node_payload(&next.store, changed));
        assert!(original.store.shares_node_payload(&next.store, unchanged));
        assert!(original.store.shares_node_payload(&next.store, root));
    }
}
