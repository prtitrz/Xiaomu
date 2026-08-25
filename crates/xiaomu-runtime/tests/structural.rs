//! P2.3 structural command orchestration tests.

use xiaomu_core::document::{
    HeadingLevel, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint, TextSelection};
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, SessionError, SessionOutcome,
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

fn range_of(document: &XiaomuDocument, node: NodeId, start: usize, end: usize) -> TextSelection {
    TextSelection::new(
        TextPoint::new(
            node,
            offset_of(document, node, start),
            CursorAffinity::Before,
        ),
        TextPoint::new(node, offset_of(document, node, end), CursorAffinity::Before),
    )
}

fn session_with(document: &XiaomuDocument, selection: TextSelection) -> DocumentSession {
    DocumentSession::new(document.clone(), DocumentSelection::text(selection)).unwrap()
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

fn children_of(session: &DocumentSession, parent: NodeId) -> Vec<NodeId> {
    session
        .document()
        .node(parent)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}

fn root_children(session: &DocumentSession) -> Vec<NodeId> {
    children_of(session, session.document().root())
}

fn caret_node_and_offset(session: &DocumentSession) -> (NodeId, usize) {
    let selection = session.text_selection().expect("single-block selection");
    (
        selection.focus().node_id(),
        selection.focus().offset().as_usize(),
    )
}

fn run_flags(session: &DocumentSession, node: NodeId) -> Vec<(String, bool)> {
    session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| {
            (
                run.text().as_str().to_owned(),
                run.marks().as_slice().contains(&Mark::Bold),
            )
        })
        .collect()
}

/// `Document > [p(parts...)]`
fn marked_paragraph(parts: &[(&str, MarkSet)]) -> (XiaomuDocument, NodeId) {
    let runs = parts
        .iter()
        .map(|(text, marks)| TextRun::new(*text, marks.clone()).unwrap())
        .collect::<Vec<_>>();
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new(runs).unwrap()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
    )
}

fn two_paragraphs(first: &str, second: &str) -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let insert = |builder: &mut NodeStoreBuilder, text: &str| {
        builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
    };
    let a = insert(&mut builder, first);
    let b = insert(&mut builder, second);
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([a, b]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), a, b)
}

#[test]
fn split_block_puts_caret_at_the_new_tail_start() {
    let (document, paragraph) = marked_paragraph(&[("你好世界", MarkSet::empty())]);
    let mut session = session_with(&document, caret(&document, paragraph, 6));

    assert_eq!(
        session.apply_intent(&EditIntent::SplitBlock).unwrap(),
        SessionOutcome::DocumentChanged
    );

    let children = root_children(&session);
    assert_eq!(children.len(), 2);
    assert_eq!(children[0], paragraph);
    assert_eq!(text_of(&session, paragraph), "你好");
    assert_eq!(text_of(&session, children[1]), "世界");
    assert_eq!(caret_node_and_offset(&session), (children[1], 0));
}

#[test]
fn split_block_at_end_creates_an_empty_tail() {
    let (document, paragraph) = marked_paragraph(&[("你好", MarkSet::empty())]);
    let mut session = session_with(&document, caret(&document, paragraph, 6));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let children = root_children(&session);
    assert_eq!(text_of(&session, paragraph), "你好");
    assert_eq!(text_of(&session, children[1]), "");
    assert_eq!(caret_node_and_offset(&session), (children[1], 0));
}

#[test]
fn split_inside_a_marked_run_inherits_marks_on_both_halves() {
    let bold = MarkSet::new([Mark::Bold]).unwrap();
    let (document, paragraph) = marked_paragraph(&[("你好", bold), ("世界", MarkSet::empty())]);
    // Offset 3 is the scalar boundary inside the bold run ("你" | "好").
    let mut session = session_with(&document, caret(&document, paragraph, 3));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let children = root_children(&session);
    assert_eq!(
        run_flags(&session, paragraph),
        vec![("你".to_owned(), true)]
    );
    assert_eq!(
        run_flags(&session, children[1]),
        vec![("好".to_owned(), true), ("世界".to_owned(), false)]
    );
    assert_eq!(caret_node_and_offset(&session), (children[1], 0));
}

#[test]
fn split_over_a_selection_deletes_it_then_splits() {
    let (document, paragraph) = marked_paragraph(&[("你好世界", MarkSet::empty())]);
    // Delete "好世" (bytes 3..9), then split; caret lands in the tail "界".
    let mut session = session_with(&document, range_of(&document, paragraph, 3, 9));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let children = root_children(&session);
    assert_eq!(text_of(&session, paragraph), "你");
    assert_eq!(text_of(&session, children[1]), "界");
    assert_eq!(caret_node_and_offset(&session), (children[1], 0));
}

#[test]
fn split_block_undo_redo_restores_store_and_selection() {
    let (document, paragraph) = marked_paragraph(&[("a👍中", MarkSet::empty())]);
    let initial_store = document.store().clone();
    // "a👍中": split after the emoji (offset 5).
    let mut session = session_with(&document, caret(&document, paragraph, 5));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let tail = root_children(&session)[1];
    assert_eq!(text_of(&session, paragraph), "a👍");
    assert_eq!(text_of(&session, tail), "中");
    assert_eq!(caret_node_and_offset(&session), (tail, 0));
    let split_store = session.document().store().clone();

    session.undo().unwrap();
    assert_eq!(session.document().store(), &initial_store);
    assert_eq!(caret_node_and_offset(&session), (paragraph, 5));
    assert_eq!(session.history_depths(), (0, 1));

    session.redo().unwrap();
    assert_eq!(session.document().store(), &split_store);
    assert_eq!(caret_node_and_offset(&session), (tail, 0));
    assert_eq!(root_children(&session)[1], tail);

    session.undo().unwrap();
    session.redo().unwrap();
    assert_eq!(root_children(&session)[1], tail);
    assert_eq!(caret_node_and_offset(&session), (tail, 0));
}

#[test]
fn backspace_at_block_start_joins_with_previous() {
    let (document, first, second) = two_paragraphs("你好", "世界");
    let mut session = session_with(&document, caret(&document, second, 0));

    assert_eq!(
        session.apply_intent(&EditIntent::Backspace).unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(root_children(&session), vec![first]);
    assert_eq!(text_of(&session, first), "你好世界");
    assert_eq!(caret_node_and_offset(&session), (first, 6));
    assert!(session.document().node(second).is_none());
}

#[test]
fn backspace_at_start_of_first_block_remains_a_no_op() {
    let (document, paragraph) = marked_paragraph(&[("你好", MarkSet::empty())]);
    let mut session = session_with(&document, caret(&document, paragraph, 0));

    assert_eq!(
        session.apply_intent(&EditIntent::Backspace).unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(text_of(&session, paragraph), "你好");
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn join_with_previous_undo_redo_restores_identities_and_caret() {
    let (document, first, second) = two_paragraphs("ab", "c👍");
    let initial_store = document.store().clone();
    let mut session = session_with(&document, caret(&document, second, 0));

    session.apply_intent(&EditIntent::JoinWithPrevious).unwrap();
    assert_eq!(text_of(&session, first), "abc👍");
    assert_eq!(caret_node_and_offset(&session), (first, 2));
    let joined_store = session.document().store().clone();

    session.undo().unwrap();
    assert_eq!(session.document().store(), &initial_store);
    assert_eq!(caret_node_and_offset(&session), (second, 0));

    session.redo().unwrap();
    assert_eq!(session.document().store(), &joined_store);
    assert_eq!(caret_node_and_offset(&session), (first, 2));
}

#[test]
fn join_with_previous_on_the_first_block_is_a_no_op() {
    let (document, first, _) = two_paragraphs("你好", "世界");
    let mut session = session_with(&document, caret(&document, first, 3));

    assert_eq!(
        session.apply_intent(&EditIntent::JoinWithPrevious).unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn turn_into_keeps_identity_and_maps_the_caret() {
    let (document, paragraph) = marked_paragraph(&[("标题", MarkSet::empty())]);
    let heading = NodeKind::Heading(HeadingLevel::new(2).unwrap());
    let mut session = session_with(&document, caret(&document, paragraph, 3));

    assert_eq!(
        session
            .apply_intent(&EditIntent::TurnInto {
                kind: heading.clone(),
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.document().node(paragraph).unwrap().kind(), &heading);
    assert_eq!(text_of(&session, paragraph), "标题");
    assert_eq!(caret_node_and_offset(&session), (paragraph, 3));

    session.undo().unwrap();
    assert_eq!(
        session.document().node(paragraph).unwrap().kind(),
        &NodeKind::Paragraph
    );
    assert_eq!(caret_node_and_offset(&session), (paragraph, 3));

    session.redo().unwrap();
    assert_eq!(session.document().node(paragraph).unwrap().kind(), &heading);
}

#[test]
fn turn_into_the_same_kind_is_a_no_op() {
    let (document, paragraph) = marked_paragraph(&[("你好", MarkSet::empty())]);
    let mut session = session_with(&document, caret(&document, paragraph, 0));

    assert_eq!(
        session
            .apply_intent(&EditIntent::TurnInto {
                kind: NodeKind::Paragraph,
            })
            .unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn turn_into_a_container_kind_is_rejected_atomically() {
    let (document, paragraph) = marked_paragraph(&[("你好", MarkSet::empty())]);
    let mut session = session_with(&document, caret(&document, paragraph, 0));
    let before = session.document().revision();

    assert!(matches!(
        session.apply_intent(&EditIntent::TurnInto {
            kind: NodeKind::Quote,
        }),
        Err(SessionError::Core(_))
    ));
    assert_eq!(session.document().revision(), before);
    assert_eq!(
        session.document().node(paragraph).unwrap().kind(),
        &NodeKind::Paragraph
    );
}

#[test]
fn join_keeps_the_surviving_nodes_kind() {
    let mut builder = NodeStoreBuilder::new();
    let heading = builder
        .insert(
            NodeKind::Heading(HeadingLevel::new(1).unwrap()),
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("标题", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("正文", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([heading, paragraph]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mut session = session_with(&document, caret(&document, paragraph, 0));

    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(
        session.document().node(heading).unwrap().kind(),
        &NodeKind::Heading(HeadingLevel::new(1).unwrap())
    );
    assert_eq!(text_of(&session, heading), "标题正文");
    assert_eq!(caret_node_and_offset(&session), (heading, 6));
}

#[test]
fn backspace_at_start_after_a_container_is_a_no_op() {
    let mut builder = NodeStoreBuilder::new();
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("正文", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([quote, paragraph]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mut session = session_with(&document, caret(&document, paragraph, 0));

    assert_eq!(
        session.apply_intent(&EditIntent::Backspace).unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(text_of(&session, paragraph), "正文");
    assert_eq!(session.history_depths(), (0, 0));
}
