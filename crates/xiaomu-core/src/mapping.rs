//! Position mapping across canonical document changes.
//!
//! Mapping is explicit change data produced by transaction application. Other
//! modules must not maintain ad hoc offset-repair logic.
//!
//! A [`StepMap`] describes how one applied step moved coordinates; a
//! [`ChangeMap`] composes the step maps of one transaction. Mapping is
//! forward-only: it translates positions of the snapshot a transaction was
//! applied to into coordinates of the snapshot it produced.
//!
//! Mapping never silently clamps. Positions whose target node was removed map
//! to [`MappedPosition::Deleted`], and positions that fall inside an edited
//! range are resolved by an explicit [`MapBias`]. The arithmetic is pure: it
//! does not consult a snapshot, so mapped results should be validated against
//! the target snapshot exactly like any other stale coordinate.
//!
//! The long-term bias and deletion policy is fixed by `docs/adr/0002` in the
//! repository: explicit `Mapped` / `Deleted` results, caller-supplied bias for
//! ambiguous boundaries, and no clamping anywhere in Core.

use std::collections::BTreeSet;

use crate::document::NodeId;
use crate::selection::{NodeGap, NodeSelection, TextPoint, TextSelection};
use crate::text::{TextOffset, TextRange};

/// How an ambiguous position is resolved during mapping.
///
/// Replacing a text range and inserting a child both create boundaries where
/// an old position has two defensible new locations. The bias makes that
/// choice explicit instead of clamping silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MapBias {
    /// Prefer the first coordinate of the edited region: before replacement
    /// text, and before a newly inserted child.
    Start,
    /// Prefer the coordinate just after the edited region: after replacement
    /// text, and after a newly inserted child.
    End,
}

/// Outcome of mapping one position or selection across a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MappedPosition<T> {
    /// The target still exists; the position was translated into the new
    /// coordinate space.
    Mapped(T),
    /// The target node was removed from the document by this change.
    Deleted,
}

/// Mapping data for one applied transaction step.
///
/// Step maps are declarative output of transaction application. Mapping
/// through them is pure coordinate arithmetic in the coordinates of the
/// intermediate state the step was applied to; composition across a whole
/// transaction is handled by [`ChangeMap`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StepMap {
    /// `[range.start, range.end)` of one inline node's text was replaced by
    /// `replacement_len` bytes.
    TextReplaced {
        /// Inline-bearing node whose concatenated text changed.
        node: NodeId,
        /// Half-open replaced byte range in the node's concatenated text.
        range: TextRange,
        /// UTF-8 byte length of the replacement text.
        replacement_len: usize,
    },
    /// A freshly allocated node entered one parent's child list.
    NodeInserted {
        /// Parent whose child list gained one entry.
        parent: NodeId,
        /// Number of children before the insertion point.
        index: usize,
        /// Identity allocated for the inserted node.
        inserted: NodeId,
    },
    /// A node and its whole subtree left one parent's child list.
    NodeRemoved {
        /// Parent whose child list lost one entry.
        parent: NodeId,
        /// Index of the removed child before removal.
        index: usize,
        /// The removed node together with every node of its subtree.
        removed: BTreeSet<NodeId>,
    },
}

impl StepMap {
    /// Maps one text point across this step.
    ///
    /// Offsets before the replaced range stay put, offsets after its end
    /// shift by the length delta, and offsets inside `[range.start,
    /// range.end)` resolve to the replacement boundary chosen by `bias`.
    /// An empty range is a pure insertion: the position exactly at its start
    /// is the insertion boundary and also resolves by `bias`. Affinity is
    /// preserved. Text points of other nodes are unaffected.
    #[must_use]
    pub fn map_text_point(&self, point: TextPoint, bias: MapBias) -> MappedPosition<TextPoint> {
        match self {
            Self::TextReplaced {
                node,
                range,
                replacement_len,
            } => {
                if point.node_id() != *node {
                    return MappedPosition::Mapped(point);
                }

                let start = range.start().as_usize();
                let end = range.end().as_usize();
                let old = point.offset().as_usize();

                let mapped = if old < start || (start == end && old == start) {
                    // Before the edit, or exactly at an empty-range insertion
                    // point, which is the insertion boundary itself.
                    if start == end && old == start && bias == MapBias::End {
                        start + *replacement_len
                    } else {
                        old
                    }
                } else if old > end || (old == end && old > start) {
                    old - (end - start) + *replacement_len
                } else {
                    match bias {
                        MapBias::Start => start,
                        MapBias::End => start + *replacement_len,
                    }
                };

                MappedPosition::Mapped(TextPoint::new(
                    point.node_id(),
                    TextOffset::from_validated_byte_index(mapped),
                    point.affinity(),
                ))
            }
            Self::NodeInserted { .. } => MappedPosition::Mapped(point),
            Self::NodeRemoved { removed, .. } => {
                if removed.contains(&point.node_id()) {
                    MappedPosition::Deleted
                } else {
                    MappedPosition::Mapped(point)
                }
            }
        }
    }

    /// Maps one structural boundary across this step.
    ///
    /// Boundaries after an insertion point shift by one; a boundary exactly
    /// at the insertion point resolves by `bias`. Removing a child shifts
    /// only later boundaries: the boundary that pointed at the removed child
    /// survives as the boundary between its former neighbors.
    #[must_use]
    pub fn map_node_gap(&self, gap: NodeGap, bias: MapBias) -> MappedPosition<NodeGap> {
        match self {
            Self::TextReplaced { .. } => MappedPosition::Mapped(gap),
            Self::NodeInserted { parent, index, .. } => {
                if gap.parent() != *parent || gap.index() < *index {
                    return MappedPosition::Mapped(gap);
                }

                let after_edit = gap.index() > *index || bias == MapBias::End;
                let index = if after_edit {
                    gap.index() + 1
                } else {
                    gap.index()
                };
                MappedPosition::Mapped(NodeGap::new(gap.parent(), index))
            }
            Self::NodeRemoved {
                parent,
                index,
                removed,
            } => {
                if removed.contains(&gap.parent()) {
                    MappedPosition::Deleted
                } else if gap.parent() == *parent && gap.index() > *index {
                    MappedPosition::Mapped(NodeGap::new(gap.parent(), gap.index() - 1))
                } else {
                    MappedPosition::Mapped(gap)
                }
            }
        }
    }

    /// Maps one node selection across this step.
    ///
    /// Only removal deletes a selected node; every other step keeps the
    /// node identity intact.
    #[must_use]
    pub fn map_node_selection(&self, selection: NodeSelection) -> MappedPosition<NodeSelection> {
        match self {
            Self::NodeRemoved { removed, .. } => {
                if removed.contains(&selection.node_id()) {
                    MappedPosition::Deleted
                } else {
                    MappedPosition::Mapped(selection)
                }
            }
            _ => MappedPosition::Mapped(selection),
        }
    }
}

/// Composed mapping data for one applied transaction.
///
/// Change maps are produced by `Transaction::apply_with_changes` and cannot
/// be built by callers. Positions are mapped by folding them through the
/// step maps in application order; once any step deletes the target node,
/// the result stays [`MappedPosition::Deleted`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeMap {
    steps: Vec<StepMap>,
}

impl ChangeMap {
    pub(crate) fn from_steps(steps: Vec<StepMap>) -> Self {
        Self { steps }
    }

    /// Returns per-step mapping data in application order.
    ///
    /// Attribute and mark steps never move positions and produce no entries.
    #[must_use]
    pub fn steps(&self) -> &[StepMap] {
        &self.steps
    }

    /// Maps one text point from the old snapshot into the new one.
    #[must_use]
    pub fn map_text_point(&self, point: TextPoint, bias: MapBias) -> MappedPosition<TextPoint> {
        let mut current = point;
        for step in &self.steps {
            match step.map_text_point(current, bias) {
                MappedPosition::Mapped(mapped) => current = mapped,
                MappedPosition::Deleted => return MappedPosition::Deleted,
            }
        }
        MappedPosition::Mapped(current)
    }

    /// Maps one structural boundary from the old snapshot into the new one.
    #[must_use]
    pub fn map_node_gap(&self, gap: NodeGap, bias: MapBias) -> MappedPosition<NodeGap> {
        let mut current = gap;
        for step in &self.steps {
            match step.map_node_gap(current, bias) {
                MappedPosition::Mapped(mapped) => current = mapped,
                MappedPosition::Deleted => return MappedPosition::Deleted,
            }
        }
        MappedPosition::Mapped(current)
    }

    /// Maps one node selection from the old snapshot into the new one.
    #[must_use]
    pub fn map_node_selection(&self, selection: NodeSelection) -> MappedPosition<NodeSelection> {
        let mut current = selection;
        for step in &self.steps {
            match step.map_node_selection(current) {
                MappedPosition::Mapped(mapped) => current = mapped,
                MappedPosition::Deleted => return MappedPosition::Deleted,
            }
        }
        MappedPosition::Mapped(current)
    }

    /// Maps a text selection so a selection covering replaced content still
    /// covers the replacement.
    ///
    /// Both endpoints are biased outward: the earlier endpoint resolves
    /// toward [`MapBias::Start`] and the later one toward [`MapBias::End`]. A
    /// collapsed selection stays collapsed by mapping both endpoints with
    /// [`MapBias::Start`]. If either endpoint's node was removed, the whole
    /// selection maps to [`MappedPosition::Deleted`].
    #[must_use]
    pub fn map_text_selection(&self, selection: TextSelection) -> MappedPosition<TextSelection> {
        let (anchor_bias, focus_bias) = if selection.is_collapsed() {
            (MapBias::Start, MapBias::Start)
        } else if selection.anchor().offset() <= selection.focus().offset() {
            (MapBias::Start, MapBias::End)
        } else {
            (MapBias::End, MapBias::Start)
        };

        let MappedPosition::Mapped(anchor) = self.map_text_point(selection.anchor(), anchor_bias)
        else {
            return MappedPosition::Deleted;
        };
        let MappedPosition::Mapped(focus) = self.map_text_point(selection.focus(), focus_bias)
        else {
            return MappedPosition::Deleted;
        };

        MappedPosition::Mapped(TextSelection::new(anchor, focus))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::CursorAffinity;
    use crate::text::TextBuffer;

    fn node(raw: u64) -> NodeId {
        NodeId::from_allocated(raw)
    }

    fn offset(raw: usize) -> TextOffset {
        const SCRATCH: &str = "00000000000000000000000000000000";
        TextBuffer::from(SCRATCH).offset_at(raw).unwrap()
    }

    fn point(raw_node: u64, raw_offset: usize) -> TextPoint {
        TextPoint::new(node(raw_node), offset(raw_offset), CursorAffinity::Before)
    }

    fn range(start: usize, end: usize) -> TextRange {
        TextRange::new(offset(start), offset(end)).unwrap()
    }

    fn replaced(start: usize, end: usize, len: usize) -> StepMap {
        StepMap::TextReplaced {
            node: node(1),
            range: range(start, end),
            replacement_len: len,
        }
    }

    #[test]
    fn empty_range_insertion_resolves_the_boundary_by_bias() {
        let step = replaced(3, 3, 4);

        // A pure insertion at byte 3: the caret sitting exactly there is the
        // insertion boundary and resolves by bias.
        assert_eq!(
            step.map_text_point(point(1, 3), MapBias::Start),
            MappedPosition::Mapped(point(1, 3))
        );
        assert_eq!(
            step.map_text_point(point(1, 3), MapBias::End),
            MappedPosition::Mapped(point(1, 7))
        );
        // Earlier and later positions shift as usual.
        assert_eq!(
            step.map_text_point(point(1, 1), MapBias::End),
            MappedPosition::Mapped(point(1, 1))
        );
        assert_eq!(
            step.map_text_point(point(1, 6), MapBias::Start),
            MappedPosition::Mapped(point(1, 10))
        );
    }

    #[test]
    fn text_replacement_moves_only_later_offsets() {
        let step = replaced(3, 6, 2);

        assert_eq!(
            step.map_text_point(point(1, 0), MapBias::Start),
            MappedPosition::Mapped(point(1, 0))
        );
        assert_eq!(
            step.map_text_point(point(1, 6), MapBias::Start),
            MappedPosition::Mapped(point(1, 5))
        );
        assert_eq!(
            step.map_text_point(point(1, 12), MapBias::End),
            MappedPosition::Mapped(point(1, 11))
        );
        // Unrelated nodes are untouched.
        assert_eq!(
            step.map_text_point(point(2, 9), MapBias::Start),
            MappedPosition::Mapped(point(2, 9))
        );
    }

    #[test]
    fn inside_replacement_resolves_by_bias() {
        let step = replaced(3, 6, 2);

        for raw in 3..6 {
            assert_eq!(
                step.map_text_point(point(1, raw), MapBias::Start),
                MappedPosition::Mapped(point(1, 3))
            );
            assert_eq!(
                step.map_text_point(point(1, raw), MapBias::End),
                MappedPosition::Mapped(point(1, 5))
            );
        }
    }

    #[test]
    fn insertion_bias_resolves_the_exact_gap() {
        let step = StepMap::NodeInserted {
            parent: node(0),
            index: 1,
            inserted: node(7),
        };

        assert_eq!(
            step.map_node_gap(NodeGap::new(node(0), 0), MapBias::Start),
            MappedPosition::Mapped(NodeGap::new(node(0), 0))
        );
        assert_eq!(
            step.map_node_gap(NodeGap::new(node(0), 1), MapBias::Start),
            MappedPosition::Mapped(NodeGap::new(node(0), 1))
        );
        assert_eq!(
            step.map_node_gap(NodeGap::new(node(0), 1), MapBias::End),
            MappedPosition::Mapped(NodeGap::new(node(0), 2))
        );
        assert_eq!(
            step.map_node_gap(NodeGap::new(node(0), 2), MapBias::Start),
            MappedPosition::Mapped(NodeGap::new(node(0), 3))
        );
        // Gaps of other parents are untouched.
        assert_eq!(
            step.map_node_gap(NodeGap::new(node(9), 1), MapBias::End),
            MappedPosition::Mapped(NodeGap::new(node(9), 1))
        );
    }

    #[test]
    fn removal_shifts_only_later_gaps() {
        let step = StepMap::NodeRemoved {
            parent: node(0),
            index: 1,
            removed: BTreeSet::from([node(2), node(3)]),
        };

        assert_eq!(
            step.map_node_gap(NodeGap::new(node(0), 0), MapBias::Start),
            MappedPosition::Mapped(NodeGap::new(node(0), 0))
        );
        // The boundary that pointed at the removed child survives.
        assert_eq!(
            step.map_node_gap(NodeGap::new(node(0), 1), MapBias::Start),
            MappedPosition::Mapped(NodeGap::new(node(0), 1))
        );
        assert_eq!(
            step.map_node_gap(NodeGap::new(node(0), 2), MapBias::Start),
            MappedPosition::Mapped(NodeGap::new(node(0), 1))
        );
        assert_eq!(
            step.map_node_gap(NodeGap::new(node(0), 3), MapBias::Start),
            MappedPosition::Mapped(NodeGap::new(node(0), 2))
        );
    }

    #[test]
    fn removed_subtrees_delete_positions_and_selections() {
        let step = StepMap::NodeRemoved {
            parent: node(0),
            index: 1,
            removed: BTreeSet::from([node(2), node(3)]),
        };

        assert_eq!(
            step.map_text_point(point(2, 0), MapBias::Start),
            MappedPosition::Deleted
        );
        assert_eq!(
            step.map_text_point(point(3, 4), MapBias::End),
            MappedPosition::Deleted
        );
        assert_eq!(
            step.map_node_gap(NodeGap::new(node(3), 0), MapBias::Start),
            MappedPosition::Deleted
        );
        assert_eq!(
            step.map_node_selection(NodeSelection::new(node(2))),
            MappedPosition::Deleted
        );

        assert_eq!(
            step.map_text_point(point(1, 4), MapBias::Start),
            MappedPosition::Mapped(point(1, 4))
        );
        assert_eq!(
            step.map_node_selection(NodeSelection::new(node(1))),
            MappedPosition::Mapped(NodeSelection::new(node(1)))
        );
    }
}
