//! Exact, preconditioned inverse payload for semantic cell edits.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::document::{Node, NodeId, NodeKind, NodeStore};
use crate::mapping::StepMap;
use crate::{Error, Result};

/// Opaque inverse of a merge, split, logical row/column edit, or restoration.
///
/// Produced only by transaction application. Applying it requires every
/// affected live payload to match its recorded post-edit state and every
/// restored identity to be absent. Full-tree validation still runs before a
/// snapshot is published. Unaffected descendant content is never overwritten.
/// Undo after unrelated edits is permitted; stale edits to recorded rows or
/// cells fail atomically rather than clobbering them. Logical-axis inverses
/// also bind the table's exact row list and may restore complete removed rich
/// subtrees. Every affected live payload and its ancestor chain must keep the
/// recorded parent identities up to the target table. Moving an affected row
/// into a nested table makes the inverse stale even though it remains somewhere
/// inside the outer table's descendant tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCellRestore {
    pub(super) table: NodeId,
    pub(super) expected: Vec<Node>,
    pub(super) expected_parents: BTreeMap<NodeId, NodeId>,
    pub(super) replacement: Vec<Node>,
    pub(super) maps: Vec<StepMap>,
    pub(super) inverse_maps: Vec<StepMap>,
}

/// Captures only affected parent edges and their ancestor closure. Build the
/// actual table parent map once, rather than searching the tree per node.
pub(super) fn expected_parents(
    store: &NodeStore,
    table: NodeId,
    expected: &[Node],
) -> Result<BTreeMap<NodeId, NodeId>> {
    if !matches!(
        store.get(table).ok_or(Error::UnknownNode)?.kind(),
        NodeKind::Table
    ) {
        return Err(Error::InvalidTableStructure);
    }
    let mut parents = BTreeMap::new();
    let mut seen = BTreeSet::from([table]);
    let mut pending = VecDeque::from([table]);
    while let Some(parent) = pending.pop_front() {
        let node = store.get(parent).ok_or(Error::UnknownNode)?;
        let children = node.content().as_children().into_iter().flatten().copied();
        let atoms = node
            .content()
            .as_inline()
            .into_iter()
            .flat_map(|inline| inline.atoms().iter().map(|atom| atom.atom()));
        for child in children.chain(atoms) {
            if !seen.insert(child) {
                return Err(Error::InvalidTransaction);
            }
            parents.insert(child, parent);
            pending.push_back(child);
        }
    }
    let mut relevant = BTreeMap::new();
    for node in expected {
        let mut child = node.id();
        while child != table && !relevant.contains_key(&child) {
            let parent = *parents.get(&child).ok_or(Error::InvalidTransaction)?;
            relevant.insert(child, parent);
            child = parent;
        }
    }
    Ok(relevant)
}
