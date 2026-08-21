//! Range selections over document coordinates.

use crate::document::{NodeId, XiaomuDocument};
use crate::text::TextRange;
use crate::{Error, Result};

use super::point::TextPoint;

/// An ordered or unordered selection spanning two text positions.
///
/// P0 keeps both endpoints inside one inline-bearing node; cross-block
/// selection belongs to the document/session layer introduced after P0. The
/// anchor/focus distinction preserves user intent (where selection started
/// versus where it ends); logical order is derived through
/// [`TextSelection::ordered_range`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextSelection {
    anchor: TextPoint,
    focus: TextPoint,
}

impl TextSelection {
    /// Creates an unchecked selection from anchor to focus.
    #[must_use]
    pub const fn new(anchor: TextPoint, focus: TextPoint) -> Self {
        Self { anchor, focus }
    }

    /// Creates a collapsed selection at one point.
    #[must_use]
    pub const fn collapsed(point: TextPoint) -> Self {
        Self {
            anchor: point,
            focus: point,
        }
    }

    /// Returns where the selection started.
    #[must_use]
    pub const fn anchor(self) -> TextPoint {
        self.anchor
    }

    /// Returns where the selection ends.
    #[must_use]
    pub const fn focus(self) -> TextPoint {
        self.focus
    }

    /// Returns whether anchor and focus are identical.
    #[must_use]
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }

    /// Validates both endpoints against one document snapshot.
    pub fn validate(&self, document: &XiaomuDocument) -> Result<()> {
        self.anchor.validate(document)?;
        self.focus.validate(document)?;

        if self.anchor.node_id() != self.focus.node_id() {
            return Err(Error::InvalidSelection);
        }

        Ok(())
    }

    /// Returns the logically ordered half-open text range of this selection.
    ///
    /// Anchor/focus order does not affect the result; affinity does not
    /// participate in ordering.
    pub fn ordered_range(&self) -> Result<TextRange> {
        if self.anchor.node_id() != self.focus.node_id() {
            return Err(Error::InvalidSelection);
        }

        let (start, end) = if self.anchor.offset() <= self.focus.offset() {
            (self.anchor.offset(), self.focus.offset())
        } else {
            (self.focus.offset(), self.anchor.offset())
        };

        TextRange::new(start, end)
    }
}

/// A selection covering exactly one whole node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeSelection {
    node_id: NodeId,
}

impl NodeSelection {
    /// Creates an unchecked node selection.
    #[must_use]
    pub const fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }

    /// Returns the selected node identity.
    #[must_use]
    pub const fn node_id(self) -> NodeId {
        self.node_id
    }

    /// Validates that the selected node exists in the snapshot.
    pub fn validate(&self, document: &XiaomuDocument) -> Result<()> {
        if document.node(self.node_id).is_some() {
            Ok(())
        } else {
            Err(Error::UnknownNode)
        }
    }
}
