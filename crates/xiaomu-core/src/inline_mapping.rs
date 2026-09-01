//! P4.1 mixed-inline position mapping compatibility.
//!
//! Canonical atom step maps do not exist until P4.2. This module establishes
//! the mapping seam now by mapping the UTF-8 text component through the
//! already-proven P0-P3 mapping rules while preserving the atom ordinal.
//! Pure-text documents only admit ordinal zero, so current behavior is exact.

use crate::mapping::{ChangeMap, MapBias, MappedPosition, StepMap};
use crate::selection::{InlinePoint, TextPoint};

impl StepMap {
    /// Maps one mixed-inline point across this step.
    ///
    /// P4.1 has no atom-changing step, so the text coordinate and target node
    /// follow [`StepMap::map_text_point`] exactly and `atom_index` is carried
    /// through unchanged. P4.2 extends the atom-specific step variants to map
    /// the ordinal explicitly.
    #[must_use]
    pub fn map_inline_point(
        &self,
        point: InlinePoint,
        bias: MapBias,
    ) -> MappedPosition<InlinePoint> {
        let text_point = TextPoint::new(point.node_id(), point.text_offset(), point.affinity());
        match self.map_text_point(text_point, bias) {
            MappedPosition::Mapped(mapped) => MappedPosition::Mapped(InlinePoint::new(
                mapped.node_id(),
                mapped.offset(),
                point.atom_index(),
                mapped.affinity(),
            )),
            MappedPosition::Deleted => MappedPosition::Deleted,
        }
    }
}

impl ChangeMap {
    /// Maps one mixed-inline point through every step in this change map.
    ///
    /// The method intentionally folds the public per-step seam so future atom
    /// step maps participate in composition without a parallel mapping engine.
    #[must_use]
    pub fn map_inline_point(
        &self,
        point: InlinePoint,
        bias: MapBias,
    ) -> MappedPosition<InlinePoint> {
        let mut current = point;
        for step in self.steps() {
            match step.map_inline_point(current, bias) {
                MappedPosition::Mapped(mapped) => current = mapped,
                MappedPosition::Deleted => return MappedPosition::Deleted,
            }
        }
        MappedPosition::Mapped(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::NodeStoreBuilder;
    use crate::selection::CursorAffinity;
    use crate::text::{TextOffset, TextRange};

    fn ids() -> (crate::document::NodeId, crate::document::NodeId) {
        let builder = NodeStoreBuilder::new();
        let first = builder.peek_next_id();
        let mut second_builder = NodeStoreBuilder::new();
        let _ = second_builder
            .insert(
                crate::document::NodeKind::Paragraph,
                crate::document::NodeAttrs::empty(),
                crate::document::NodeContent::empty_inline(),
            )
            .unwrap();
        (first, second_builder.peek_next_id())
    }

    #[test]
    fn text_replacement_maps_text_component_and_preserves_ordinal() {
        let (node, _) = ids();
        let at = TextOffset::from_validated_byte_index(1);
        let map = StepMap::TextReplaced {
            node,
            range: TextRange::empty(at),
            replacement_len: 3,
        };
        let point = InlinePoint::new(node, at, 0, CursorAffinity::Before);

        assert_eq!(
            map.map_inline_point(point, MapBias::End),
            MappedPosition::Mapped(InlinePoint::new(
                node,
                TextOffset::from_validated_byte_index(4),
                0,
                CursorAffinity::Before,
            ))
        );
    }

    #[test]
    fn node_removal_deletes_mixed_inline_point_like_text_point() {
        let (node, parent) = ids();
        let map = StepMap::NodeRemoved {
            parent,
            index: 0,
            removed: [node].into_iter().collect(),
        };
        let point = InlinePoint::new(node, TextOffset::ZERO, 0, CursorAffinity::Before);

        assert_eq!(
            map.map_inline_point(point, MapBias::Start),
            MappedPosition::Deleted
        );
    }
}
