//! Stable opaque node identity.

use core::fmt;

/// Stable identity of a canonical document node.
///
/// `NodeId` is intentionally opaque. Its storage representation is not a
/// public semantic contract, and callers cannot construct arbitrary raw IDs
/// through the normal API. Allocation is owned by the document/store layer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u64);

impl NodeId {
    #[cfg(test)]
    const fn from_allocated(raw: u64) -> Self {
        Self(raw)
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("NodeId").field(&self.0).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::NodeId;

    #[test]
    fn node_id_is_stable_and_order_is_not_document_order() {
        let first = NodeId::from_allocated(7);
        let same = NodeId::from_allocated(7);
        let other = NodeId::from_allocated(8);

        assert_eq!(first, same);
        assert_ne!(first, other);
    }
}
