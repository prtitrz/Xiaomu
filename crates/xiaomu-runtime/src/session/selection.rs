//! Document-level selection: cross-block anchor / focus positions.
//!
//! P1 kept the session selection inside one inline node (`TextSelection`,
//! a Core type). P2 upgrades the session to a runtime-owned
//! [`DocumentSelection`] whose endpoints may sit anywhere in the tree: an
//! [`InlinePoint`] inside one inline-bearing node, or a [`NodeGap`] between
//! two children of one container.
//!
//! Since P4.3 the text endpoints carry the full mixed-inline coordinate
//! `(text_offset, atom_index)`: a caret between same-boundary atoms is a
//! first-class position. Plain-text endpoints keep `atom_index == 0`, which
//! is exactly the P0-P3 `TextPoint` coordinate space. The visual projection
//! of a cross-block selection onto mounted blocks is a frontend concern and
//! does not belong here.

use std::collections::HashMap;

use xiaomu_core::document::{NodeId, XiaomuDocument};
use xiaomu_core::mapping::{ChangeMap, MapBias, MappedPosition};
use xiaomu_core::selection::{InlinePoint, NodeGap, TextPoint, TextSelection};

use super::SessionError;

/// One endpoint of a [`DocumentSelection`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentPosition {
    /// A mixed-inline text position inside one inline-bearing node.
    Inline(InlinePoint),
    /// A structural boundary between two children of one container.
    Gap(NodeGap),
}

impl From<TextPoint> for DocumentPosition {
    fn from(point: TextPoint) -> Self {
        Self::Inline(InlinePoint::from(point))
    }
}

impl From<NodeGap> for DocumentPosition {
    fn from(gap: NodeGap) -> Self {
        Self::Gap(gap)
    }
}

/// Cross-block selection over two independent endpoints.
///
/// The session validates both endpoints against the current snapshot at
/// every public read. Ordering across blocks requires the snapshot, so
/// unlike `TextSelection`, head/tail resolution goes through
/// [`DocumentSelection::ordered`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentSelection {
    anchor: DocumentPosition,
    focus: DocumentPosition,
}

impl DocumentSelection {
    /// Creates a selection from two endpoints without validating.
    pub fn new(anchor: impl Into<DocumentPosition>, focus: impl Into<DocumentPosition>) -> Self {
        Self {
            anchor: anchor.into(),
            focus: focus.into(),
        }
    }

    /// Creates a collapsed caret at `position`.
    #[must_use]
    pub fn collapsed(position: impl Into<DocumentPosition>) -> Self {
        let position = position.into();
        Self {
            anchor: position,
            focus: position,
        }
    }

    /// Adapts a single-block Core selection into the document-level form.
    ///
    /// Both endpoints become inline positions with atom ordinal zero;
    /// validation against a snapshot happens through [`Self::validate`],
    /// exactly like for native gap endpoints.
    #[must_use]
    pub fn text(selection: TextSelection) -> Self {
        Self::new(
            DocumentPosition::from(selection.anchor()),
            DocumentPosition::from(selection.focus()),
        )
    }

    /// Returns the anchor endpoint.
    #[must_use]
    pub const fn anchor(&self) -> DocumentPosition {
        self.anchor
    }

    /// Returns the focus endpoint.
    #[must_use]
    pub const fn focus(&self) -> DocumentPosition {
        self.focus
    }

    /// Returns whether both endpoints coincide.
    #[must_use]
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }

    /// Returns the single-block Core selection when both endpoints are
    /// plain-text positions (atom ordinal zero) of the same inline node.
    ///
    /// A selection that carries a non-zero atom ordinal cannot degrade to
    /// the text-only `TextSelection` without losing the seam information;
    /// callers that must edit such a selection use the mixed-inline
    /// transaction contract instead.
    #[must_use]
    pub fn as_single_node(&self) -> Option<TextSelection> {
        let (anchor, focus) = self.as_same_node_inline()?;
        Some(TextSelection::new(
            anchor.to_text_point().ok()?,
            focus.to_text_point().ok()?,
        ))
    }

    /// Returns both endpoints as mixed-inline points of one inline node.
    ///
    /// `None` for gap endpoints or endpoints on different nodes: the
    /// cross-block code paths own those selections.
    #[must_use]
    pub fn as_same_node_inline(&self) -> Option<(InlinePoint, InlinePoint)> {
        match (self.anchor, self.focus) {
            (DocumentPosition::Inline(anchor), DocumentPosition::Inline(focus))
                if anchor.node_id() == focus.node_id() =>
            {
                Some((anchor, focus))
            }
            _ => None,
        }
    }

    /// Validates both endpoints against `document`.
    pub fn validate(&self, document: &XiaomuDocument) -> Result<(), SessionError> {
        let check = |position: DocumentPosition| match position {
            DocumentPosition::Inline(point) => point.validate(document),
            DocumentPosition::Gap(gap) => gap.validate(document),
        };
        check(self.anchor).map_err(|_| SessionError::SelectionInvalid)?;
        check(self.focus).map_err(|_| SessionError::SelectionInvalid)?;
        Ok(())
    }

    /// Maps both endpoints through `changes`, whose coordinates are those
    /// of `document` (the snapshot the transaction was applied to).
    ///
    /// A non-collapsed selection maps outward so it still covers mapped
    /// content: the head endpoint biases toward `MapBias::Start`, the tail
    /// toward `MapBias::End`. A collapsed selection stays collapsed. An
    /// endpoint deleted by the change fails the whole mapping; on any error
    /// the caller must keep its previous state unchanged.
    pub fn map_through(
        &self,
        changes: &ChangeMap,
        document: &XiaomuDocument,
    ) -> Result<Self, SessionError> {
        let (head, tail) = if self.is_collapsed() {
            (self.anchor, self.focus)
        } else {
            self.ordered(document)?
        };
        let anchor_is_head = self.anchor == head;

        let bias_of = |endpoint| {
            if endpoint == head {
                MapBias::Start
            } else {
                MapBias::End
            }
        };
        let map_one = |endpoint: DocumentPosition, bias| -> Result<Self, SessionError> {
            match endpoint {
                DocumentPosition::Inline(point) => match changes.map_inline_point(point, bias) {
                    MappedPosition::Mapped(mapped) => Ok(Self::collapsed(mapped)),
                    MappedPosition::Deleted => Err(SessionError::SelectionDeleted),
                },
                DocumentPosition::Gap(gap) => match changes.map_node_gap(gap, bias) {
                    MappedPosition::Mapped(mapped) => Ok(Self::collapsed(mapped)),
                    MappedPosition::Deleted => Err(SessionError::SelectionDeleted),
                },
            }
        };

        if self.is_collapsed() {
            return map_one(head, bias_of(head));
        }
        let mapped_head = map_one(head, bias_of(head))?;
        let mapped_tail = map_one(tail, bias_of(tail))?;

        // Restore the original orientation: the user's anchor / focus roles
        // survive even when mapping moved both endpoints.
        Ok(if anchor_is_head {
            Self::new(mapped_head.focus(), mapped_tail.focus())
        } else {
            Self::new(mapped_tail.focus(), mapped_head.focus())
        })
    }

    /// Returns `(head, tail)`: both endpoints ordered by document order,
    /// computed against `document`. Both endpoints must be valid for it.
    pub fn ordered(
        &self,
        document: &XiaomuDocument,
    ) -> Result<(DocumentPosition, DocumentPosition), SessionError> {
        let slots = Slots::build(document);
        if slots.key(self.anchor)? <= slots.key(self.focus)? {
            Ok((self.anchor, self.focus))
        } else {
            Ok((self.focus, self.anchor))
        }
    }
}

/// Pre-order slot assignment over one snapshot.
///
/// Every node receives a monotonically increasing base slot and each child
/// boundary receives its own slot interleaved between the subtrees. Any two
/// valid positions then compare lexicographically by `(slot, sub, ordinal)`,
/// where a gap's sub-component is zero and an inline position uses its UTF-8
/// byte offset plus atom ordinal. The ordinal is the canonical same-boundary
/// order (ADR 0005), so seam gaps order deterministically.
struct Slots {
    counter: u64,
    node_base: HashMap<NodeId, u64>,
    gap_keys: HashMap<(NodeId, usize), (u64, u64, u64)>,
}

impl Slots {
    fn build(document: &XiaomuDocument) -> Self {
        let mut slots = Self {
            counter: 0,
            node_base: HashMap::new(),
            gap_keys: HashMap::new(),
        };
        slots.walk(document.root(), document);
        slots
    }

    fn take_slot(&mut self) -> u64 {
        let slot = self.counter;
        self.counter += 1;
        slot
    }

    fn walk(&mut self, id: NodeId, document: &XiaomuDocument) {
        let base = self.take_slot();
        self.node_base.insert(id, base);

        let Some(node) = document.node(id) else {
            return;
        };
        let Some(children) = node.content().as_children() else {
            return;
        };

        for (index, child) in children.iter().enumerate() {
            // The boundary before the first child shares the parent's slot;
            // every later boundary sits after the previous subtree.
            let gap_slot = if index == 0 { base } else { self.take_slot() };
            self.gap_keys.insert((id, index), (gap_slot, 0, 0));
            self.walk(*child, document);
        }
        let after_last = self.take_slot();
        self.gap_keys
            .insert((id, children.len()), (after_last, 0, 0));
    }

    fn key(&self, position: DocumentPosition) -> Result<(u64, u64, u64), SessionError> {
        match position {
            DocumentPosition::Inline(point) => self
                .node_base
                .get(&point.node_id())
                .map(|base| {
                    (
                        *base,
                        point.text_offset().as_usize() as u64,
                        point.atom_index() as u64,
                    )
                })
                .ok_or(SessionError::SelectionInvalid),
            DocumentPosition::Gap(gap) => self
                .gap_keys
                .get(&(gap.parent(), gap.index()))
                .copied()
                .ok_or(SessionError::SelectionInvalid),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder};
    use xiaomu_core::text::TextBuffer;
    use xiaomu_core::text::TextOffset;
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

    /// `Document > [p("aaaa"), p("bbbb"), quote > [p("cccc")]]`
    fn fixture() -> XiaomuDocument {
        let mut builder = NodeStoreBuilder::new();
        let inline = |text: &str| -> NodeContent {
            NodeContent::Inline(
                xiaomu_core::document::InlineContent::new([xiaomu_core::document::TextRun::new(
                    text,
                    xiaomu_core::document::MarkSet::empty(),
                )
                .unwrap()])
                .unwrap(),
            )
        };
        let p0 = builder
            .insert(NodeKind::Paragraph, NodeAttrs::empty(), inline("aaaa"))
            .unwrap();
        let p1 = builder
            .insert(NodeKind::Paragraph, NodeAttrs::empty(), inline("bbbb"))
            .unwrap();
        let inner = builder
            .insert(NodeKind::Paragraph, NodeAttrs::empty(), inline("cccc"))
            .unwrap();
        let quote = builder
            .insert(
                NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children([inner]),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([p0, p1, quote]),
            )
            .unwrap();
        XiaomuDocument::new(root, builder.finish()).unwrap()
    }

    fn offset(raw: usize) -> TextOffset {
        const SCRATCH: &str = "00000000000000000000000000000000";
        TextBuffer::from(SCRATCH).offset_at(raw).unwrap()
    }

    fn point(node: NodeId, raw: usize) -> DocumentPosition {
        TextPoint::new(
            node,
            offset(raw),
            xiaomu_core::selection::CursorAffinity::Before,
        )
        .into()
    }

    fn children_of(document: &XiaomuDocument) -> Vec<NodeId> {
        document
            .node(document.root())
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec()
    }

    #[test]
    fn validate_rejects_unknown_nodes_and_bad_gaps() {
        let document = fixture();
        let [p0, _, _] = children_of(&document)[..] else {
            unreachable!()
        };

        // An endpoint whose node left the snapshot fails: remove p1 via a
        // transaction and validate the stale text position against it.
        let transaction =
            Transaction::new(TransactionOrigin::System).with_step(TransactionStep::RemoveNode {
                node: children_of(&document)[1],
            });
        let shrunk = transaction.apply(&document).unwrap();
        let removed = children_of(&document)[1];
        let stale = DocumentSelection::collapsed(point(removed, 0));
        assert_eq!(stale.validate(&shrunk), Err(SessionError::SelectionInvalid));

        // A gap beyond the child count fails.
        let root = document.root();
        let bad_gap = DocumentSelection::collapsed(NodeGap::new(root, 4));
        assert_eq!(
            bad_gap.validate(&document),
            Err(SessionError::SelectionInvalid)
        );

        // A gap on an inline node has no children to bound into.
        let on_inline = DocumentSelection::collapsed(NodeGap::new(p0, 0));
        assert_eq!(
            on_inline.validate(&document),
            Err(SessionError::SelectionInvalid)
        );

        assert_eq!(
            DocumentSelection::collapsed(NodeGap::new(root, 3)).validate(&document),
            Ok(())
        );
    }

    #[test]
    fn as_single_node_requires_one_inline_node() {
        let document = fixture();
        let [p0, p1, _] = children_of(&document)[..] else {
            unreachable!()
        };

        let single = DocumentSelection::text(TextSelection::new(
            TextPoint::new(
                p0,
                offset(1),
                xiaomu_core::selection::CursorAffinity::Before,
            ),
            TextPoint::new(
                p0,
                offset(3),
                xiaomu_core::selection::CursorAffinity::Before,
            ),
        ));
        assert!(single.as_single_node().is_some());

        let cross_block = DocumentSelection::new(point(p0, 1), point(p1, 2));
        assert!(cross_block.as_single_node().is_none());
    }

    #[test]
    fn ordered_resolves_document_order_across_blocks() {
        let document = fixture();
        let [p0, p1, _] = children_of(&document)[..] else {
            unreachable!()
        };

        // Anchor later in the document than focus.
        let selection = DocumentSelection::new(point(p1, 2), point(p0, 1));
        let (head, tail) = selection.ordered(&document).unwrap();
        assert_eq!(head, point(p0, 1));
        assert_eq!(tail, point(p1, 2));

        // Gaps sort between blocks.
        let root = document.root();
        let before_p0 = DocumentPosition::Gap(NodeGap::new(root, 0));
        let after_p0 = DocumentPosition::Gap(NodeGap::new(root, 1));
        let (head, tail) = DocumentSelection::new(after_p0, before_p0)
            .ordered(&document)
            .unwrap();
        assert_eq!(head, before_p0);
        assert_eq!(tail, after_p0);
    }

    #[test]
    fn text_replacement_maps_cross_block_endpoints_outward() {
        let document = fixture();
        let [p0, p1, _] = children_of(&document)[..] else {
            unreachable!()
        };
        let root = document.root();

        // Replace the first two bytes of the first paragraph with three
        // bytes; the change map shifts later offsets by +1.
        let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::ReplaceText {
                node: p0,
                range: xiaomu_core::text::TextRange::new(offset(0), offset(2)).unwrap(),
                replacement: "xxx".to_owned(),
            },
        );
        let applied = transaction.apply_with_changes(&document).unwrap();
        let changes = applied.changes();

        // A selection from inside p0 into p1 keeps covering both blocks;
        // the head biases outward (start) and the tail keeps its offset.
        let selection = DocumentSelection::new(point(p0, 1), point(p1, 2));
        let mapped = selection.map_through(changes, &document).unwrap();
        assert_eq!(mapped.anchor(), point(p0, 0));
        assert_eq!(mapped.focus(), point(p1, 2));

        // The user's orientation survives: anchor was the tail here.
        let reversed = DocumentSelection::new(point(p1, 2), point(p0, 1));
        let mapped = reversed.map_through(changes, &document).unwrap();
        assert_eq!(mapped.anchor(), point(p1, 2));
        assert_eq!(mapped.focus(), point(p0, 0));

        // A collapsed caret inside the replaced range resolves outward and
        // stays collapsed.
        let mapped = DocumentSelection::collapsed(point(p0, 2))
            .map_through(changes, &document)
            .unwrap();
        assert!(mapped.is_collapsed());
        assert_eq!(mapped.focus(), point(p0, 3));

        // Deleting the tail's block deletes the whole mapping.
        let removal = Transaction::new(TransactionOrigin::System)
            .with_step(TransactionStep::RemoveNode { node: p1 });
        let applied = removal.apply_with_changes(&document).unwrap();
        assert_eq!(
            selection.map_through(applied.changes(), &document),
            Err(SessionError::SelectionDeleted)
        );
        assert!(
            DocumentSelection::new(NodeGap::new(root, 2), point(p1, 0))
                .map_through(applied.changes(), &document)
                .is_err()
        );
    }
}
