//! Cached structural order and textblock prefixes for host-defined statistics.

use std::cmp::Ordering;

use xiaomu_core::{
    document::{NodeId, XiaomuDocument},
    selection::NodeGap,
};

use super::position::point_key;
use super::{ReadingBudget, ReadingError, ReadingProjection, ReadingSpanKind, ReadingTextBlock};
use crate::session::DocumentPosition;

/// Canonical structural traversal events; inline atom events live in each block's spans.
///
/// The document root has ordinary enter/leave events and is identified by
/// [`ReadingProjection::root`]. Consumers may count structural tokens using
/// their own serialization rules without scanning the canonical tree again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingEvent {
    /// Start of a structural node's ordered child content.
    EnterContainer(NodeId),
    /// End of that structural node's child content.
    LeaveContainer(NodeId),
    /// A complete inline-bearing node, including empty blocks and headings.
    TextBlock(NodeId),
    /// A block with atomic content and no editable text interior.
    AtomicBlock(NodeId),
    /// Extension-defined leaf with a non-atomic opaque payload.
    /// It emits no text and has no valid Atomic/Inline position.
    OpaqueLeaf(NodeId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Inline,
    Container,
    Atomic,
    Unaddressable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PositionIndex {
    nodes: Vec<(NodeId, usize, Shape)>,
    gaps: Vec<(NodeGap, usize)>,
    pub(super) events: Vec<ReadingEvent>,
}

impl PositionIndex {
    pub(super) fn build(document: &XiaomuDocument) -> Result<Self, ReadingError> {
        enum Visit {
            Node(NodeId),
            Gap(NodeGap),
            Leave(NodeId),
        }
        let count = document.node_count();
        let capacity = count
            .checked_mul(3)
            .ok_or(ReadingError::BudgetExceeded(ReadingBudget::Nodes))?;
        let mut nodes = Vec::new();
        let mut gaps = Vec::new();
        let mut stack = Vec::new();
        let mut events = Vec::new();
        events
            .try_reserve_exact(capacity)
            .map_err(|_| ReadingError::AllocationFailed)?;
        nodes
            .try_reserve_exact(count)
            .map_err(|_| ReadingError::AllocationFailed)?;
        gaps.try_reserve_exact(capacity)
            .map_err(|_| ReadingError::AllocationFailed)?;
        stack
            .try_reserve_exact(capacity)
            .map_err(|_| ReadingError::AllocationFailed)?;
        stack.push(Visit::Node(document.root()));
        let mut next_slot = 0;
        while let Some(visit) = stack.pop() {
            if let Visit::Leave(id) = visit {
                events.push(ReadingEvent::LeaveContainer(id));
                continue;
            }
            let slot = next_slot;
            next_slot += 1;
            match visit {
                Visit::Leave(_) => unreachable!("handled without taking a slot"),
                Visit::Gap(gap) => gaps.push((gap, slot)),
                Visit::Node(id) => {
                    let node = document.node(id).expect("validated tree");
                    if let Some(children) = node.content().as_children() {
                        nodes.push((id, slot, Shape::Container));
                        events.push(ReadingEvent::EnterContainer(id));
                        stack.push(Visit::Leave(id));
                        stack.push(Visit::Gap(NodeGap::new(id, children.len())));
                        for (index, child) in children.iter().enumerate().rev() {
                            stack.push(Visit::Node(*child));
                            if index > 0 {
                                stack.push(Visit::Gap(NodeGap::new(id, index)));
                            }
                        }
                        if !children.is_empty() {
                            gaps.push((NodeGap::new(id, 0), slot));
                        }
                    } else {
                        events.push(if node.content().as_inline().is_some() {
                            ReadingEvent::TextBlock(id)
                        } else if node.content().is_atomic() {
                            ReadingEvent::AtomicBlock(id)
                        } else {
                            ReadingEvent::OpaqueLeaf(id)
                        });
                        nodes.push((
                            id,
                            slot,
                            if node.content().as_inline().is_some() {
                                Shape::Inline
                            } else if node.content().is_atomic() {
                                Shape::Atomic
                            } else {
                                Shape::Unaddressable
                            },
                        ));
                    }
                }
            }
        }
        nodes.sort_unstable_by_key(|entry| entry.0);
        gaps.sort_unstable_by_key(|entry| entry.0);
        Ok(Self {
            nodes,
            gaps,
            events,
        })
    }

    fn node(&self, id: NodeId) -> Result<(usize, Shape), ReadingError> {
        let index = self
            .nodes
            .binary_search_by_key(&id, |entry| entry.0)
            .map_err(|_| ReadingError::InvalidPoint)?;
        Ok((self.nodes[index].1, self.nodes[index].2))
    }

    fn gap(&self, gap: NodeGap) -> Result<usize, ReadingError> {
        let index = self
            .gaps
            .binary_search_by_key(&gap, |entry| entry.0)
            .map_err(|_| ReadingError::InvalidPoint)?;
        Ok(self.gaps[index].1)
    }
}

impl ReadingProjection {
    fn position_key(
        &self,
        position: DocumentPosition,
    ) -> Result<(usize, usize, usize), ReadingError> {
        match position {
            DocumentPosition::Inline(point) => {
                self.block(point.node_id())
                    .ok_or(ReadingError::InvalidPoint)?
                    .projected_offset(point)?;
                let (slot, _) = self.positions.node(point.node_id())?;
                let (offset, ordinal) = point_key(point);
                Ok((slot, offset, ordinal))
            }
            DocumentPosition::Gap(gap) => Ok((self.positions.gap(gap)?, 0, 0)),
            DocumentPosition::Atomic(id) => {
                let (slot, shape) = self.positions.node(id)?;
                if shape != Shape::Atomic {
                    return Err(ReadingError::InvalidPoint);
                }
                Ok((slot, 0, 0))
            }
        }
    }

    /// Orders validated inline, structural-gap or atomic positions using the
    /// cached traversal. No snapshot scan or allocation occurs in this query.
    pub fn compare_positions(
        &self,
        left: DocumentPosition,
        right: DocumentPosition,
    ) -> Result<Ordering, ReadingError> {
        Ok(self.position_key(left)?.cmp(&self.position_key(right)?))
    }

    /// Returns original text before a validated document position, grouped by
    /// textblocks whose opening has already been traversed.
    ///
    /// An inline point at offset zero includes its empty block prefix. A gap
    /// before that block does not include it. Empty earlier blocks remain in
    /// order; atoms and containers emit no text. The caller chooses separators,
    /// whitespace rules and scalar/UTF-16/grapheme accounting. No allocation or
    /// canonical-tree scan occurs; iterating the result visits included blocks.
    pub fn prefix(&self, position: DocumentPosition) -> Result<ReadingPrefix<'_>, ReadingError> {
        let (slot, _, _) = self.position_key(position)?;
        let count = self.blocks().partition_point(|block| {
            self.positions.node(block.node_id).expect("indexed block").0 <= slot
        });
        let partial = match position {
            DocumentPosition::Inline(point) => Some((
                point.node_id(),
                self.block(point.node_id())
                    .expect("validated block")
                    .projected_offset(point)?,
            )),
            _ => None,
        };
        Ok(ReadingPrefix {
            blocks: &self.blocks()[..count],
            partial,
        })
    }
}

/// Borrowed text-only prefix of a projection, retaining textblock boundaries.
#[derive(Clone, Copy, Debug)]
pub struct ReadingPrefix<'a> {
    blocks: &'a [ReadingTextBlock],
    partial: Option<(NodeId, usize)>,
}

impl<'a> ReadingPrefix<'a> {
    /// Included block prefixes in canonical order, including empty blocks.
    pub fn blocks(&self) -> impl ExactSizeIterator<Item = ReadingBlockPrefix<'a>> + use<'a> {
        let partial = self.partial;
        self.blocks.iter().map(move |block| ReadingBlockPrefix {
            block,
            end: partial
                .filter(|entry| entry.0 == block.node_id)
                .map_or(block.text.len(), |entry| entry.1),
        })
    }
}

/// Original text-only content in one included textblock prefix.
#[derive(Clone, Copy, Debug)]
pub struct ReadingBlockPrefix<'a> {
    block: &'a ReadingTextBlock,
    end: usize,
}

impl<'a> ReadingBlockPrefix<'a> {
    /// The complete source block, preserving identity/kind/order.
    #[must_use]
    pub const fn block(&self) -> &'a ReadingTextBlock {
        self.block
    }

    /// Unmodified text fragments before the endpoint, excluding all atoms.
    pub fn text_fragments(&self) -> impl Iterator<Item = &'a str> + use<'a> {
        let end = self.end;
        let block = self.block;
        block
            .spans
            .iter()
            .filter(move |span| {
                matches!(span.kind, ReadingSpanKind::Text) && span.projected.start < end
            })
            .map(move |span| &block.text[span.projected.start..span.projected.end.min(end)])
    }
}
