//! Mixed-inline position mapping across text, structure, and atom edits.
//!
//! The UTF-8 text component continues through the proven text/structural
//! mapping rules. Atom-only steps leave that component untouched and adjust
//! only the same-boundary ordinal carried by [`InlinePoint`].

use std::cmp::Ordering;

use crate::mapping::{ChangeMap, MapBias, MappedPosition, StepMap};
use crate::selection::{InlinePoint, TextPoint};
use crate::text::TextOffset;

impl StepMap {
    /// Maps one mixed-inline point across this step.
    ///
    /// Atom insertion shifts later same-boundary gaps by one; a caret exactly
    /// at the insertion gap resolves before or after the new atom by `bias`.
    /// Atom removal collapses the two gaps around the removed atom and shifts
    /// later same-boundary gaps left by one. The mixed-inline replacement
    /// maps the UTF-8 text coordinate through the shared text rules and
    /// redistributes seam ordinals: gaps before the edited seam stay put, the
    /// edited gap resolves by `bias`, and gaps after it land after the
    /// replacement text with the seam atoms that moved there.
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
            StepMap::InlineTextReplaced {
                node,
                range,
                replacement_len,
                seam_atom_index,
            } if point.node_id() == *node => {
                let start = range.start().as_usize();
                let end = range.end().as_usize();
                let old = point.text_offset().as_usize();
                let seam = *seam_atom_index;
                let len = *replacement_len;

                let (offset, atom_index) = if old < start {
                    (old, point.atom_index())
                } else if old == start {
                    match point.atom_index().cmp(&seam) {
                        Ordering::Less => (old, point.atom_index()),
                        // The edited gap itself: Start keeps it at the seam,
                        // End lands after the replacement text. A pure
                        // deletion has no replacement text to cross.
                        Ordering::Equal => {
                            if len > 0 && bias == MapBias::End {
                                (start + len, 0)
                            } else {
                                (old, point.atom_index())
                            }
                        }
                        // Pure seam insertion: gaps after the insertion point
                        // follow the seam atoms that moved after the
                        // inserted text.
                        Ordering::Greater => {
                            if len > 0 {
                                (start + len, point.atom_index() - seam)
                            } else {
                                (old, point.atom_index())
                            }
                        }
                    }
                } else if old < end {
                    match bias {
                        MapBias::Start => (start, seam),
                        MapBias::End => {
                            if len > 0 {
                                (start + len, 0)
                            } else {
                                (start, seam)
                            }
                        }
                    }
                } else if old == end {
                    // With a replacement the end-seam atoms anchor at their
                    // own boundary after the replacement; a pure deletion
                    // merges them after the preserved seam atoms.
                    if len > 0 {
                        (start + len, point.atom_index())
                    } else {
                        (start, seam + point.atom_index())
                    }
                } else {
                    (old - (end - start) + len, point.atom_index())
                };

                MappedPosition::Mapped(InlinePoint::new(
                    point.node_id(),
                    TextOffset::from_validated_byte_index(offset),
                    atom_index,
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

    fn seam_replacement(
        node: crate::document::NodeId,
        start: usize,
        end: usize,
        replacement_len: usize,
        seam_atom_index: usize,
    ) -> StepMap {
        StepMap::InlineTextReplaced {
            node,
            range: TextRange::new(
                TextOffset::from_validated_byte_index(start),
                TextOffset::from_validated_byte_index(end),
            )
            .unwrap(),
            replacement_len,
            seam_atom_index,
        }
    }

    #[test]
    fn inline_replacement_maps_text_component_like_text_replacement() {
        let (node, _) = ids();
        let inline = seam_replacement(node, 1, 3, 4, 0);
        let text_only = StepMap::TextReplaced {
            node,
            range: TextRange::new(
                TextOffset::from_validated_byte_index(1),
                TextOffset::from_validated_byte_index(3),
            )
            .unwrap(),
            replacement_len: 4,
        };

        for raw in [0usize, 1, 2, 3, 5] {
            let point = TextPoint::new(
                node,
                TextOffset::from_validated_byte_index(raw),
                CursorAffinity::Before,
            );
            for bias in [MapBias::Start, MapBias::End] {
                assert_eq!(
                    inline.map_text_point(point, bias),
                    text_only.map_text_point(point, bias),
                    "raw {raw} bias {bias:?}"
                );
            }
        }
    }

    #[test]
    fn seam_insertion_redistributes_ordinal_gaps_around_the_inserted_text() {
        let (node, _) = ids();
        // A [a1] [a2] B, insert "X" between the two atoms: seam 1, len 1.
        let map = seam_replacement(node, 1, 1, 1, 1);
        let at = |offset: usize, ordinal: usize| {
            InlinePoint::new(
                node,
                TextOffset::from_validated_byte_index(offset),
                ordinal,
                CursorAffinity::Before,
            )
        };

        // Before the seam: unchanged.
        assert_eq!(
            map.map_inline_point(at(1, 0), MapBias::End),
            MappedPosition::Mapped(at(1, 0))
        );
        // The edited gap: Start stays, End lands after the inserted text.
        assert_eq!(
            map.map_inline_point(at(1, 1), MapBias::Start),
            MappedPosition::Mapped(at(1, 1))
        );
        assert_eq!(
            map.map_inline_point(at(1, 1), MapBias::End),
            MappedPosition::Mapped(at(2, 0))
        );
        // Gaps after the insertion follow the seam atom that moved.
        assert_eq!(
            map.map_inline_point(at(1, 2), MapBias::Start),
            MappedPosition::Mapped(at(2, 1))
        );
        // Past the edited boundary: plain text shift.
        assert_eq!(
            map.map_inline_point(at(2, 0), MapBias::Start),
            MappedPosition::Mapped(at(3, 0))
        );
    }

    #[test]
    fn pure_deletion_merges_end_seam_ordinals_behind_preserved_atoms() {
        let (node, _) = ids();
        // A [a1] B [a2] C, delete "B" starting after a1: seam 1, len 0.
        let map = seam_replacement(node, 1, 2, 0, 1);
        let at = |offset: usize, ordinal: usize| {
            InlinePoint::new(
                node,
                TextOffset::from_validated_byte_index(offset),
                ordinal,
                CursorAffinity::Before,
            )
        };

        // The edited gap coincides with itself under both biases.
        assert_eq!(
            map.map_inline_point(at(1, 1), MapBias::End),
            MappedPosition::Mapped(at(1, 1))
        );
        // End-boundary gaps merge after the preserved seam atom.
        assert_eq!(
            map.map_inline_point(at(2, 0), MapBias::Start),
            MappedPosition::Mapped(at(1, 1))
        );
        assert_eq!(
            map.map_inline_point(at(2, 1), MapBias::Start),
            MappedPosition::Mapped(at(1, 2))
        );
        // Later boundaries shift by the removed length.
        assert_eq!(
            map.map_inline_point(at(3, 0), MapBias::Start),
            MappedPosition::Mapped(at(2, 0))
        );
    }

    #[test]
    fn replacement_keeps_end_seam_atoms_at_their_own_boundary() {
        let (node, _) = ids();
        // A [a1] B [a2] C, replace "B" with "XY" starting after a1.
        let map = seam_replacement(node, 1, 2, 2, 1);
        let at = |offset: usize, ordinal: usize| {
            InlinePoint::new(
                node,
                TextOffset::from_validated_byte_index(offset),
                ordinal,
                CursorAffinity::Before,
            )
        };

        // The edited gap resolves outward by bias.
        assert_eq!(
            map.map_inline_point(at(1, 1), MapBias::Start),
            MappedPosition::Mapped(at(1, 1))
        );
        assert_eq!(
            map.map_inline_point(at(1, 1), MapBias::End),
            MappedPosition::Mapped(at(3, 0))
        );
        // End-boundary gaps keep their ordinals at the shifted boundary.
        assert_eq!(
            map.map_inline_point(at(2, 0), MapBias::Start),
            MappedPosition::Mapped(at(3, 0))
        );
        assert_eq!(
            map.map_inline_point(at(2, 1), MapBias::Start),
            MappedPosition::Mapped(at(3, 1))
        );
        // Before the seam: unchanged.
        assert_eq!(
            map.map_inline_point(at(1, 0), MapBias::End),
            MappedPosition::Mapped(at(1, 0))
        );
        assert_eq!(
            map.map_inline_point(at(0, 0), MapBias::End),
            MappedPosition::Mapped(at(0, 0))
        );
    }

    #[test]
    fn inline_replacement_deletes_points_inside_removed_nodes_like_text() {
        let (node, parent) = ids();
        let map = StepMap::NodeRemoved {
            parent,
            index: 0,
            removed: [node].into_iter().collect(),
        };
        let point = InlinePoint::new(
            node,
            TextOffset::from_validated_byte_index(1),
            2,
            CursorAffinity::Before,
        );
        assert_eq!(
            map.map_inline_point(point, MapBias::End),
            MappedPosition::Deleted
        );
    }
}
