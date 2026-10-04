//! Host-defined visual list labels, independent of canonical content.

use xiaomu_core::document::{NodeId, XiaomuDocument};

/// The real list and item being painted in the current document snapshot.
#[derive(Clone, Copy)]
pub struct ListMarkerContext<'a> {
    pub(crate) document: &'a XiaomuDocument,
    pub(crate) list: NodeId,
    pub(crate) item: NodeId,
    pub(crate) index: usize,
    pub(crate) depth: usize,
}

impl<'a> ListMarkerContext<'a> {
    /// Returns the current snapshot, including preserved list and item attrs.
    #[must_use]
    pub const fn document(self) -> &'a XiaomuDocument {
        self.document
    }

    /// Returns the enclosing BulletList or OrderedList node's identity.
    #[must_use]
    pub const fn list(self) -> NodeId {
        self.list
    }

    /// Returns the ListItem node's identity.
    #[must_use]
    pub const fn item(self) -> NodeId {
        self.item
    }

    /// Returns the item's zero-based sibling index within its actual list.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    /// Returns nesting depth; top-level lists have depth one.
    #[must_use]
    pub const fn depth(self) -> usize {
        self.depth
    }
}

/// Whether to keep Xiaomu's label or render a host-provided label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListMarkerLabel {
    /// Keep the original depth-based bullet or one-based numeric label.
    Default,
    /// Paint this visual label using the existing GPUI text pipeline.
    ///
    /// No text, offsets, node attrs or selection coordinates are changed.
    /// The label column can grow beyond the default one-indent minimum.
    Label(String),
}

/// Pure, non-reentrant projection of a list item's visual label.
///
/// Called from the normal GPUI render chain with fresh canonical attrs on
/// each render. Return `Default` for list kinds or attrs the host does not
/// interpret. Do not mutate the session, initiate edits or perform external
/// effects inside this callback; rendering cannot roll back those effects.
/// No provider means the original bullet and `index + 1` labels are preserved.
pub trait ListMarkerLabelProvider {
    /// Projects a label from the actual document/list/item/index context.
    fn label(&self, context: ListMarkerContext<'_>) -> ListMarkerLabel;
}
