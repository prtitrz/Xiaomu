//! Cross-block text mutation planning.
//!
//! A document selection whose text endpoints live in different inline blocks
//! has one structural boundary as part of its selected content. Deleting that
//! range keeps the first block's identity/kind/attributes, moves the last
//! block's unselected suffix next to it, joins the two (preserving suffix
//! runs/marks), removes fully covered intermediate blocks, then prunes
//! containers that became empty solely because of the deletion.
//!
//! All steps live in one Core transaction, so the session exposes one atomic
//! history entry and Core inverse/mapping remain the only mutation mechanics.

use std::collections::BTreeSet;

use xiaomu_core::document::{NodeContent, NodeId, XiaomuDocument};
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::TransactionStep;

use super::intent::{EditPlan, PlannedAction, PrimaryEdit, SelectionUpdate};
use super::structure::{children_of, subtree_payloads, user_transaction};
use super::{DocumentPosition, DocumentSelection, SessionError};

/// Plans deletion of a non-collapsed cross-block text selection.
pub(crate) fn plan_delete_selection(
    document: &XiaomuDocument,
    selection: DocumentSelection,
) -> Result<PlannedAction, SessionError> {
    selection
        .validate(document)
        .map_err(|_| SessionError::SelectionInvalid)?;
    let (head, tail) = selection.ordered(document)?;
    let (DocumentPosition::Text(head), DocumentPosition::Text(tail)) = (head, tail) else {
        return Err(SessionError::SelectionInvalid);
    };
    if head.node_id() == tail.node_id() {
        return Err(SessionError::SelectionInvalid);
    }

    let blocks = inline_blocks(document);
    let head_index = blocks
        .iter()
        .position(|node| *node == head.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    let tail_index = blocks
        .iter()
        .position(|node| *node == tail.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    if head_index >= tail_index {
        return Err(SessionError::SelectionInvalid);
    }

    let head_inline = inline_of(document, head.node_id())?;
    let tail_inline = inline_of(document, tail.node_id())?;
    head_inline
        .validate_offset(head.offset())
        .map_err(SessionError::Core)?;
    tail_inline
        .validate_offset(tail.offset())
        .map_err(SessionError::Core)?;

    let head_parent = document
        .parent_of(head.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    let head_position = children_of(document, head_parent)
        .iter()
        .position(|child| *child == head.node_id())
        .ok_or(SessionError::SelectionInvalid)?;

    let mut transaction = user_transaction();

    // Keep only the unselected prefix of the first block.
    let head_end = head_inline
        .offset_at(head_inline.len_bytes())
        .map_err(SessionError::Core)?;
    if head.offset() != head_end {
        transaction.push_step(TransactionStep::ReplaceText {
            node: head.node_id(),
            range: TextRange::new(head.offset(), head_end).map_err(SessionError::Core)?,
            replacement: String::new(),
        });
    }

    // Keep only the unselected suffix of the last block. The node itself is
    // moved and joined below so its run/mark segmentation survives exactly.
    let tail_start = tail_inline.offset_at(0).map_err(SessionError::Core)?;
    if tail.offset() != tail_start {
        transaction.push_step(TransactionStep::ReplaceText {
            node: tail.node_id(),
            range: TextRange::new(tail_start, tail.offset()).map_err(SessionError::Core)?,
            replacement: String::new(),
        });
    }

    let tail_payloads = subtree_payloads(document, tail.node_id());
    transaction.push_step(TransactionStep::RemoveNode {
        node: tail.node_id(),
    });
    transaction.push_step(TransactionStep::RestoreSubtree {
        parent: head_parent,
        index: head_position + 1,
        root: tail.node_id(),
        nodes: tail_payloads,
    });
    transaction.push_step(TransactionStep::JoinNodes {
        first: head.node_id(),
        second: tail.node_id(),
    });

    let middle: Vec<NodeId> = blocks[head_index + 1..tail_index].to_vec();
    for node in &middle {
        transaction.push_step(TransactionStep::RemoveNode { node: *node });
    }

    // `tail` was detached from its original parent and all middle leaves were
    // removed. Prune only container subtrees whose original children are now
    // entirely gone; pre-existing unrelated empty containers are untouched.
    let removed_from_original: BTreeSet<_> = middle
        .iter()
        .copied()
        .chain(std::iter::once(tail.node_id()))
        .collect();
    for container in empty_container_roots(document, &removed_from_original) {
        transaction.push_step(TransactionStep::RemoveNode { node: container });
    }

    let caret_range = TextRange::new(head.offset(), head.offset()).map_err(SessionError::Core)?;
    Ok(PlannedAction::Commit(EditPlan::new(
        transaction,
        SelectionUpdate::CaretAtEditStart,
        Some(PrimaryEdit {
            node: head.node_id(),
            range: caret_range,
            inserted_len: 0,
        }),
    )))
}

fn inline_of(
    document: &XiaomuDocument,
    node: NodeId,
) -> Result<&xiaomu_core::document::InlineContent, SessionError> {
    document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)
}

fn inline_blocks(document: &XiaomuDocument) -> Vec<NodeId> {
    fn walk(document: &XiaomuDocument, id: NodeId, out: &mut Vec<NodeId>) {
        let Some(node) = document.node(id) else {
            return;
        };
        match node.content() {
            NodeContent::Inline(_) => out.push(id),
            NodeContent::Children(children) => {
                for child in children {
                    walk(document, *child, out);
                }
            }
            NodeContent::Atomic | _ => {}
        }
    }

    let mut blocks = Vec::new();
    walk(document, document.root(), &mut blocks);
    blocks
}

/// Finds highest container roots that become empty when `removed` leaves are
/// detached from their original positions.
fn empty_container_roots(
    document: &XiaomuDocument,
    removed: &BTreeSet<NodeId>,
) -> Vec<NodeId> {
    fn analyze(
        document: &XiaomuDocument,
        id: NodeId,
        removed: &BTreeSet<NodeId>,
        candidates: &mut BTreeSet<NodeId>,
    ) -> (bool, bool) {
        if removed.contains(&id) {
            return (true, true);
        }
        let Some(node) = document.node(id) else {
            return (false, false);
        };
        let NodeContent::Children(children) = node.content() else {
            return (false, false);
        };

        let mut affected = false;
        let mut all_removed = !children.is_empty();
        for child in children {
            let (child_removed, child_affected) = analyze(document, *child, removed, candidates);
            affected |= child_affected;
            all_removed &= child_removed;
        }

        let removable = id != document.root() && affected && all_removed;
        if removable {
            candidates.insert(id);
        }
        (removable, affected)
    }

    let mut candidates = BTreeSet::new();
    analyze(document, document.root(), removed, &mut candidates);

    candidates
        .iter()
        .copied()
        .filter(|candidate| {
            let mut parent = document.parent_of(*candidate);
            while let Some(id) = parent {
                if candidates.contains(&id) {
                    return false;
                }
                parent = document.parent_of(id);
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{
        InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeKind, NodeStoreBuilder, TextRun,
    };
    use xiaomu_core::selection::{CursorAffinity, TextPoint};

    fn inline(text: &str, marks: MarkSet) -> InlineContent {
        InlineContent::new([TextRun::new(text, marks).unwrap()]).unwrap()
    }

    fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
        let inline = document.node(node).unwrap().content().as_inline().unwrap();
        TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
    }

    fn text(document: &XiaomuDocument, node: NodeId) -> String {
        document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect()
    }

    fn root_children(document: &XiaomuDocument) -> Vec<NodeId> {
        document
            .node(document.root())
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec()
    }

    #[test]
    fn delete_across_plain_blocks_keeps_first_identity_and_tail_marks() {
        let mut builder = NodeStoreBuilder::new();
        let first = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline("abc", MarkSet::empty())),
            )
            .unwrap();
        let middle = builder
            .insert(
                NodeKind::Heading(xiaomu_core::document::HeadingLevel::new(2).unwrap()),
                NodeAttrs::empty(),
                NodeContent::Inline(inline("MID", MarkSet::empty())),
            )
            .unwrap();
        let last = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline(
                    "xyz",
                    MarkSet::new([Mark::Italic]).unwrap(),
                )),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([first, middle, last]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let selection = DocumentSelection::new(point(&document, first, 1), point(&document, last, 1));
        let action = plan_delete_selection(&document, selection).unwrap();
        let PlannedAction::Commit(plan) = action else {
            panic!("cross-block delete must commit");
        };
        let applied = plan.transaction().apply_with_changes(&document).unwrap();
        let after = applied.document();

        assert_eq!(root_children(after), vec![first]);
        assert_eq!(text(after, first), "ayz");
        let runs = after
            .node(first)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs();
        assert!(runs.last().unwrap().marks().contains(MarkKind::Italic));
    }

    #[test]
    fn delete_into_list_prunes_consumed_items_but_keeps_later_items() {
        let mut builder = NodeStoreBuilder::new();
        let first = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline("ab", MarkSet::empty())),
            )
            .unwrap();
        let item_one_text = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline("cd", MarkSet::empty())),
            )
            .unwrap();
        let item_one = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([item_one_text]),
            )
            .unwrap();
        let item_two_text = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline("ef", MarkSet::empty())),
            )
            .unwrap();
        let item_two = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([item_two_text]),
            )
            .unwrap();
        let item_three_text = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline("gh", MarkSet::empty())),
            )
            .unwrap();
        let item_three = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([item_three_text]),
            )
            .unwrap();
        let list = builder
            .insert(
                NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([item_one, item_two, item_three]),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([first, list]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let selection = DocumentSelection::new(
            point(&document, first, 1),
            point(&document, item_two_text, 1),
        );
        let PlannedAction::Commit(plan) = plan_delete_selection(&document, selection).unwrap()
        else {
            panic!("cross-block delete must commit");
        };
        let applied = plan.transaction().apply_with_changes(&document).unwrap();
        let after = applied.document();

        assert_eq!(text(after, first), "af");
        assert_eq!(root_children(after), vec![first, list]);
        let remaining_items = after.node(list).unwrap().content().as_children().unwrap();
        assert_eq!(remaining_items, &[item_three]);
        assert_eq!(text(after, item_three_text), "gh");
    }

    #[test]
    fn deleting_only_a_block_boundary_joins_without_losing_text() {
        let mut builder = NodeStoreBuilder::new();
        let first = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline("abc", MarkSet::empty())),
            )
            .unwrap();
        let second = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline("DEF", MarkSet::empty())),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([first, second]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let selection = DocumentSelection::new(
            point(&document, first, 3),
            point(&document, second, 0),
        );
        let PlannedAction::Commit(plan) = plan_delete_selection(&document, selection).unwrap()
        else {
            panic!("boundary delete must commit");
        };
        let applied = plan.transaction().apply_with_changes(&document).unwrap();
        assert_eq!(text(applied.document(), first), "abcDEF");
        assert_eq!(root_children(applied.document()), vec![first]);
    }
}
