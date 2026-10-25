//! Reconcile native children in document order without shifting a Vec per node.
use std::collections::HashMap;
use xiaomu_core::document::NodeId;

pub(super) fn reconcile<T>(
    previous: Vec<(NodeId, T)>,
    nodes: impl IntoIterator<Item = NodeId>,
    mut create: impl FnMut(NodeId) -> T,
) -> Vec<(NodeId, T)> {
    let mut pool: HashMap<_, _> = previous.into_iter().collect();
    nodes
        .into_iter()
        .map(|node| {
            let child = pool.remove(&node).unwrap_or_else(|| create(node));
            (node, child)
        })
        .collect()
    // Unreferenced entities are dropped with the remaining pool.
}
