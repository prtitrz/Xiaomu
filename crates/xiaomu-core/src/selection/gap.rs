//! Structural boundary positions between child nodes.

use crate::document::{NodeId, XiaomuDocument};
use crate::{Error, Result};

/// A boundary position in a parent node's child list.
///
/// `index` is the number of children before the boundary, so `0` is before
/// the first child and `children.len()` is after the last child. This is a
/// structural coordinate, not a text coordinate; it never points inside a
/// node's inline content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeGap {
    parent: NodeId,
    index: usize,
}

impl NodeGap {
    /// Creates an unchecked structural boundary.
    ///
    /// Like [`crate::selection::TextPoint`], construction does not validate;
    /// use [`NodeGap::validate`] against a concrete snapshot.
    #[must_use]
    pub const fn new(parent: NodeId, index: usize) -> Self {
        Self { parent, index }
    }

    /// Returns the parent node identity.
    #[must_use]
    pub const fn parent(self) -> NodeId {
        self.parent
    }

    /// Returns the number of children before this boundary.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    /// Validates this boundary against one document snapshot.
    pub fn validate(&self, document: &XiaomuDocument) -> Result<()> {
        let Some(node) = document.node(self.parent) else {
            return Err(Error::UnknownNode);
        };

        let Some(children) = node.content().as_children() else {
            return Err(Error::InvalidSelection);
        };

        if self.index > children.len() {
            return Err(Error::InvalidSelection);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder};

    #[test]
    fn gap_boundaries_are_validated_against_children_length() {
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
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([first, second]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();

        assert_eq!(NodeGap::new(root, 0).validate(&document), Ok(()));
        assert_eq!(NodeGap::new(root, 2).validate(&document), Ok(()));
        assert_eq!(
            NodeGap::new(root, 3).validate(&document),
            Err(Error::InvalidSelection)
        );
    }

    #[test]
    fn gaps_reject_unknown_and_non_child_parents() {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
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
                NodeContent::children([paragraph]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();

        assert_eq!(
            NodeGap::new(NodeId::from_allocated(999), 0).validate(&document),
            Err(Error::UnknownNode)
        );
        assert_eq!(
            NodeGap::new(paragraph, 0).validate(&document),
            Err(Error::InvalidSelection)
        );
    }
}
