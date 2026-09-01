//! Mixed-inline position mapping across text, structure, and atom edits.
//!
//! The UTF-8 text component continues through the proven text/structural
//! mapping rules. Atom-only steps leave that component untouched and adjust
//! only the same-boundary ordinal carried by [`InlinePoint`].

use crate::mapping::{ChangeMap, MapBias, MappedPosition, StepMap};
use crate::selection::{InlinePoint, TextPoint};

impl StepMap {
    /// Maps one mixed-inline point across this step.
    ///
    /// Atom insertion shifts later same-boundary gaps by one; a caret exactly
    /// at the insertion gap resolves before or after the new atom by `bias`.
    /// Atom removal collapses the two gaps around the removed atom and shifts
    /// later same-boundary gaps left by one. Other steps map the UTF-8 text
    /// coordinate through [`StepMap::map_text_point`] and preserve ordinal.
    #[must_use]
    pub fn map_inline_point(
        &self,
        point: InlinePoint,
        bias: MapBias,
    ) -> MappedPosition<InlinePoint> {
        match self {
            StepMap::InlineAtomInserted {
                parent,
                text_offset,
                atom_index,
                ..
            } if point.node_id() == *parent && point.text_offset() == *text_offset => {
                let old = point.atom_index();
                let mapped = if old < *atom_index {
                    old
                } else if old > *atom_index || bias == MapBias::End {
                    old + 1
                } else {
                    old
                };
                MappedPosition::Mapped(InlinePoint::new(
                    point.node_id(),
                    point.text_offset(),
                    mapped,
                    point.affinity(),
                ))
            }
            StepMap::InlineAtomRemoved {
                parent,
                text_offset,
                atom_index,
                ..
            } if point.node_id() == *parent && point.text_offset() == *text_offset => {
                let old = point.atom_index();
                let mapped = if old > *atom_index { old - 1 } else { old };
                MappedPosition::Mapped(InlinePoint::new(
                    point.node_id(),
                    point.text_offset(),
                    mapped,
                    point.affinity(),
                ))
            }
            _ => {
                let text_point =
                    TextPoint::new(point.node_id(), point.text_offset(), point.affinity());
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
    }
}

impl ChangeMap {
    /// Maps one mixed-inline point through every step in this change map.
    ///
    /// The method folds the public per-step seam so text, structural, and atom
    /// edits compose in application order without a parallel mapping engine.
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
    fn atom_insert_maps_exact_gap_by_bias_and_shifts_later_ordinals() {
        let (parent, inserted) = ids();
        let at = TextOffset::from_validated_byte_index(1);
        let map = StepMap::InlineAtomInserted {
            parent,
            text_offset: at,
            atom_index: 1,
            inserted,
        };
        let before = InlinePoint::new(parent, at, 0, CursorAffinity::Before);
        let exact = InlinePoint::new(parent, at, 1, CursorAffinity::Before);
        let after = InlinePoint::new(parent, at, 2, CursorAffinity::Before);

        assert_eq!(
            map.map_inline_point(before, MapBias::End),
            MappedPosition::Mapped(before)
        );
        assert_eq!(
            map.map_inline_point(exact, MapBias::Start),
            MappedPosition::Mapped(exact)
        );
        assert_eq!(
            map.map_inline_point(exact, MapBias::End),
            MappedPosition::Mapped(InlinePoint::new(parent, at, 2, CursorAffinity::Before,))
        );
        assert_eq!(
            map.map_inline_point(after, MapBias::Start),
            MappedPosition::Mapped(InlinePoint::new(parent, at, 3, CursorAffinity::Before,))
        );
    }

    #[test]
    fn atom_remove_collapses_neighboring_gaps_and_shifts_later_ordinals() {
        let (parent, removed) = ids();
        let at = TextOffset::from_validated_byte_index(1);
        let map = StepMap::InlineAtomRemoved {
            parent,
            text_offset: at,
            atom_index: 1,
            removed,
        };

        assert_eq!(
            map.map_inline_point(
                InlinePoint::new(parent, at, 1, CursorAffinity::Before),
                MapBias::Start,
            ),
            MappedPosition::Mapped(InlinePoint::new(parent, at, 1, CursorAffinity::Before,))
        );
        assert_eq!(
            map.map_inline_point(
                InlinePoint::new(parent, at, 2, CursorAffinity::Before),
                MapBias::End,
            ),
            MappedPosition::Mapped(InlinePoint::new(parent, at, 1, CursorAffinity::Before,))
        );
        assert_eq!(
            map.map_inline_point(
                InlinePoint::new(parent, at, 3, CursorAffinity::Before),
                MapBias::Start,
            ),
            MappedPosition::Mapped(InlinePoint::new(parent, at, 2, CursorAffinity::Before,))
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
