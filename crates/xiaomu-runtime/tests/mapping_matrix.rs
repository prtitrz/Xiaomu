//! Session / structural-composition mapping matrix (P2.7 closeout 3.1).
//!
//! After-selection for structural commands is derived from [`ChangeMap`]
//! step identities or an explicit policy (`PreserveFocus`, `CaretAtSplitTail`).
//! These tests pin that the runtime does not keep a second implicit
//! offset-patching path.

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition, StepMap};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint, TextSelection};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionError, SessionOutcome,
};

fn offset_of(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextOffset {
    document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(raw)
        .unwrap()
}

fn caret(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextSelection {
    TextSelection::collapsed(TextPoint::new(
        node,
        offset_of(document, node, raw),
        CursorAffinity::Before,
    ))
}

fn session_with(document: &XiaomuDocument, selection: TextSelection) -> DocumentSession {
    DocumentSession::new(document.clone(), DocumentSelection::text(selection)).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn two_paragraphs(first: &str, second: &str) -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let a = paragraph(&mut builder, first);
    let b = paragraph(&mut builder, second);
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([a, b]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), a, b)
}

fn text_of(session: &DocumentSession, node: NodeId) -> String {
    session
        .document()
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

fn focus_point(session: &DocumentSession) -> TextPoint {
    match session.selection().focus() {
        DocumentPosition::Inline(point) => point.to_text_point().unwrap(),
        DocumentPosition::Gap(_) => panic!("expected a text focus"),
    }
}

fn split_inserted(changes: &xiaomu_core::mapping::ChangeMap) -> NodeId {
    changes
        .steps()
        .iter()
        .rev()
        .find_map(|step| match step {
            StepMap::NodeSplit { inserted, .. } => Some(*inserted),
            _ => None,
        })
        .expect("split must record NodeSplit")
}

#[test]
fn split_block_caret_is_the_changemap_tail_not_a_patched_offset() {
    let (document, first, _) = two_paragraphs("hello", "world");
    let mut session = session_with(&document, caret(&document, first, 2));

    let before = session.document().clone();
    let at = offset_of(&before, first, 2);
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SplitNode { node: first, at })
        .apply_with_changes(&before)
        .unwrap();
    let inserted = split_inserted(applied.changes());

    // ChangeMap moves offsets after the split point onto the tail node.
    let after_split = TextPoint::new(first, offset_of(&before, first, 4), CursorAffinity::Before);
    match applied
        .changes()
        .map_text_point(after_split, MapBias::Start)
    {
        MappedPosition::Mapped(mapped) => {
            assert_eq!(mapped.node_id(), inserted);
            assert_eq!(mapped.offset().as_usize(), 2);
        }
        MappedPosition::Deleted => panic!("split must map the tail, not delete it"),
    }

    assert_eq!(
        session.apply_intent(&EditIntent::SplitBlock).unwrap(),
        SessionOutcome::DocumentChanged
    );
    let focus = focus_point(&session);
    assert_eq!(focus.node_id(), inserted);
    assert_eq!(focus.offset().as_usize(), 0);
    assert_eq!(text_of(&session, first), "he");
    assert_eq!(text_of(&session, inserted), "llo");
}

#[test]
fn join_nodes_caret_is_the_changemap_seam() {
    let (document, first, second) = two_paragraphs("ab", "cd");
    let mut session = session_with(&document, caret(&document, second, 0));

    let before = session.document().clone();
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::JoinNodes { first, second })
        .apply_with_changes(&before)
        .unwrap();
    let (seam_id, seam_len) = applied
        .changes()
        .steps()
        .iter()
        .rev()
        .find_map(|step| match step {
            StepMap::NodeJoined {
                first, first_len, ..
            } => Some((*first, *first_len)),
            _ => None,
        })
        .expect("join must record NodeJoined");

    assert_eq!(
        session.apply_intent(&EditIntent::JoinWithPrevious).unwrap(),
        SessionOutcome::DocumentChanged
    );
    let focus = focus_point(&session);
    assert_eq!(focus.node_id(), seam_id);
    assert_eq!(focus.offset().as_usize(), seam_len);
    assert_eq!(text_of(&session, first), "abcd");
}

#[test]
fn remove_node_maps_selection_to_deleted_and_restore_reuses_identity() {
    let (document, first, second) = two_paragraphs("ab", "cd");
    let selection = DocumentSelection::collapsed(TextPoint::new(
        second,
        offset_of(&document, second, 0),
        CursorAffinity::Before,
    ));
    let removal = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode { node: second });
    let applied = removal.apply_with_changes(&document).unwrap();
    assert_eq!(
        selection.map_through(applied.changes(), &document),
        Err(SessionError::SelectionDeleted)
    );

    // A session whose caret is on a surviving node can apply the same
    // removal; undo restores the original identity rather than minting a
    // new one.
    let mut session = session_with(&document, caret(&document, first, 0));
    session.apply(&removal).unwrap();
    assert!(session.document().node(second).is_none());
    session.undo().unwrap();
    assert!(session.document().node(second).is_some());
    assert_eq!(session.document().store(), document.store());
}

fn wrap_paragraph() -> (DocumentSession, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let block = paragraph(&mut builder, "item");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let session = session_with(&document, caret(&document, block, 2));
    (session, block)
}

#[test]
fn list_wrap_preserves_focus_identity_instead_of_mapping_through_remove() {
    let (mut session, block) = wrap_paragraph();
    let before_offset = focus_point(&session).offset();
    assert_eq!(
        session
            .apply_intent(&EditIntent::TurnInto {
                kind: NodeKind::BulletList,
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    let focus = focus_point(&session);
    assert_eq!(focus.node_id(), block, "wrap must keep the block identity");
    assert_eq!(focus.offset(), before_offset);
    assert_eq!(
        session.document().node(block).unwrap().kind(),
        &NodeKind::Paragraph
    );
}

#[test]
fn list_indent_outdent_and_lift_keep_focus_identity() {
    let mut builder = NodeStoreBuilder::new();
    let a = paragraph(&mut builder, "a");
    let b = paragraph(&mut builder, "b");
    let item_a = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([a]),
        )
        .unwrap();
    let item_b = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([b]),
        )
        .unwrap();
    let ul = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item_a, item_b]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([ul]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let mut session = session_with(&document, caret(&document, b, 1));
    session.apply_intent(&EditIntent::IndentListItem).unwrap();
    assert_eq!(focus_point(&session).node_id(), b);
    session.apply_intent(&EditIntent::OutdentListItem).unwrap();
    assert_eq!(focus_point(&session).node_id(), b);

    session
        .apply_intent(&EditIntent::TurnInto {
            kind: NodeKind::Paragraph,
        })
        .unwrap();
    assert_eq!(focus_point(&session).node_id(), b);
    assert_eq!(
        session.document().node(b).unwrap().kind(),
        &NodeKind::Paragraph
    );
}

#[test]
fn list_enter_caret_follows_staged_split_identity() {
    let mut builder = NodeStoreBuilder::new();
    let block = paragraph(&mut builder, "hello");
    let item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap();
    let ul = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([ul]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mut session = session_with(&document, caret(&document, block, 2));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let focus = focus_point(&session);
    assert_ne!(focus.node_id(), block);
    assert_eq!(focus.offset().as_usize(), 0);
    assert_eq!(text_of(&session, block), "he");
    assert_eq!(text_of(&session, focus.node_id()), "llo");
}

#[test]
fn empty_item_enter_exits_the_list_and_undo_restores_the_store() {
    let mut builder = NodeStoreBuilder::new();
    let empty = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
        )
        .unwrap();
    let kept = paragraph(&mut builder, "kept");
    let item_kept = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([kept]),
        )
        .unwrap();
    let item_empty = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([empty]),
        )
        .unwrap();
    let ul = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item_kept, item_empty]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([ul]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let initial_store = document.store().clone();
    let mut session = session_with(&document, caret(&document, empty, 0));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    assert_eq!(
        session.document().node(empty).unwrap().kind(),
        &NodeKind::Paragraph
    );
    session.undo().unwrap();
    assert_eq!(session.document().store(), &initial_store);
    session.redo().unwrap();
    session.selection().validate(session.document()).unwrap();
}

#[test]
fn undo_redo_across_structural_edits_restores_recorded_selections() {
    let (document, first, _) = two_paragraphs("hello", "world");
    let mut session = session_with(&document, caret(&document, first, 2));
    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let after_split = session.selection();
    session.apply_intent(&EditIntent::JoinWithPrevious).unwrap();
    let after_join = session.selection();

    session.undo().unwrap();
    assert_eq!(session.selection(), after_split);
    session.undo().unwrap();
    assert_eq!(
        session.selection().focus(),
        DocumentPosition::Inline(InlinePoint::new(
            first,
            offset_of(&document, first, 2),
            0,
            CursorAffinity::Before
        ))
    );
    session.redo().unwrap();
    assert_eq!(session.selection(), after_split);
    session.redo().unwrap();
    assert_eq!(session.selection(), after_join);
}

#[test]
fn cross_block_map_through_preserves_anchor_focus_direction() {
    let (document, first, second) = two_paragraphs("aaaa", "bbbb");
    let selection = DocumentSelection::new(
        TextPoint::new(
            second,
            offset_of(&document, second, 2),
            CursorAffinity::Before,
        ),
        TextPoint::new(
            first,
            offset_of(&document, first, 1),
            CursorAffinity::Before,
        ),
    );
    let transaction =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::ReplaceText {
            node: first,
            range: xiaomu_core::text::TextRange::new(
                offset_of(&document, first, 0),
                offset_of(&document, first, 2),
            )
            .unwrap(),
            replacement: "xxx".to_owned(),
        });
    let applied = transaction.apply_with_changes(&document).unwrap();
    let mapped = selection.map_through(applied.changes(), &document).unwrap();
    assert_eq!(
        mapped.anchor(),
        DocumentPosition::Inline(InlinePoint::new(
            second,
            offset_of(&document, second, 2),
            0,
            CursorAffinity::Before
        ))
    );
    match mapped.focus() {
        DocumentPosition::Inline(point) => {
            assert_eq!(point.node_id(), first);
            assert_eq!(point.text_offset().as_usize(), 0);
        }
        DocumentPosition::Gap(_) => panic!("focus must remain a text point"),
    }
}
