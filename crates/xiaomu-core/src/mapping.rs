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
    /// An inline-bearing node was split at a text offset; the tail text
    /// entered the parent's child list as a freshly allocated sibling.
    NodeSplit {
        /// Parent whose child list gained one entry.
        parent: NodeId,
        /// Number of children before the inserted tail sibling.
        index: usize,
        /// The original node, which kept the text before the split point.
        node: NodeId,
        /// Split position in the original node's concatenated text; it is
        /// also the byte length the node kept.
        at: crate::text::TextOffset,
        /// Identity allocated for the tail sibling.
        inserted: NodeId,
    },
    /// Two adjacent inline-bearing siblings were merged into one; the
    /// absorbed node left the document together with its whole subtree.
    NodeJoined {
        /// Parent whose child list lost one entry.
        parent: NodeId,
        /// Index of the absorbed child before the join.
        index: usize,
        /// The surviving node that absorbed `second`'s content.
        first: NodeId,
        /// The absorbed node.
        second: NodeId,
        /// UTF-8 byte length of `first`'s text before the join.
        first_len: usize,
        /// The absorbed node together with every node of its subtree.
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
            Self::NodeSplit {
                node, at, inserted, ..
            } => {
                if point.node_id() != *node {
                    return MappedPosition::Mapped(point);
                }

                let split_at = at.as_usize();
                let old = point.offset().as_usize();
                // Offsets after the split move into the tail sibling; an
                // offset exactly at the split point resolves by bias between
                // the head's end boundary and the tail's start boundary.
                let to_tail = old > split_at || (old == split_at && bias == MapBias::End);
                let (mapped_node, mapped_offset) = if to_tail {
                    (*inserted, old - split_at)
                } else {
                    (*node, old)
                };
                MappedPosition::Mapped(TextPoint::new(
                    mapped_node,
                    TextOffset::from_validated_byte_index(mapped_offset),
                    point.affinity(),
                ))
            }
            Self::NodeJoined {
                first,
                second,
                first_len,
                ..
            } => {
                if point.node_id() != *second {
                    return MappedPosition::Mapped(point);
                }

                let joined = point.offset().as_usize() + first_len;
                MappedPosition::Mapped(TextPoint::new(
                    *first,
                    TextOffset::from_validated_byte_index(joined),
                    point.affinity(),
                ))
            }
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
            Self::NodeInserted { parent, index, .. } | Self::NodeSplit { parent, index, .. } => {
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
            }
            | Self::NodeJoined {
                parent,
                index,
                removed,
                ..
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
            Self::NodeRemoved { removed, .. } | Self::NodeJoined { removed, .. } => {
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
    /// Attribute, kind, and mark steps never move positions and produce no entries.
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
