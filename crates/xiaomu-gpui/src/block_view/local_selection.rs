//! Avoid building document-wide order for a selection confined to one block.
use super::{ParagraphView, SelectionProjection};
use xiaomu_core::document::NodeId;

impl ParagraphView {
    /// `order` is evaluated only for cross-block/structural selections. Local
    /// atom ordinals are mapped through the exact display projection, including
    /// zero-byte seams; virtual range inputs keep the established range path.
    pub(crate) fn projected_selection_with_order(
        &self,
        order: impl FnOnce() -> Vec<NodeId>,
    ) -> SelectionProjection {
        if !self.is_range_input()
            && let Some((anchor, focus)) = self.session.borrow().selection().as_same_node_inline()
        {
            if anchor.node_id() != self.node {
                return SelectionProjection::None;
            }
            let Some(projection) = self.atom_display_projection() else {
                return SelectionProjection::None;
            };
            let (Some(anchor), Some(focus)) = (
                projection.display_offset_for_inline_point(anchor),
                projection.display_offset_for_inline_point(focus),
            ) else {
                return SelectionProjection::None;
            };
            return if anchor == focus {
                SelectionProjection::Caret(anchor)
            } else {
                SelectionProjection::Highlight {
                    start: anchor.min(focus),
                    end: anchor.max(focus),
                }
            };
        }
        self.projected_display_selection(&order())
    }
}

#[cfg(test)]
mod tests;
