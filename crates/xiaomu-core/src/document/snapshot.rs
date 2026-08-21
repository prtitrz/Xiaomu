//! Immutable canonical document snapshot and full-tree validation.

use std::collections::{BTreeMap, BTreeSet};

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
            store,
        })
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

            let count = parent_counts.entry(*child_id).or_default();
            *count += 1;
            if *count > 1 {
                return Err(Error::MultipleNodeParents);
            }

            if active.contains(child_id) {
                return Err(Error::CyclicDocument);
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
