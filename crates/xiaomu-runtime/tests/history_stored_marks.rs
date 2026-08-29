//! P3.4 history grouping / StoredMarks / IME regressions.

use xiaomu_core::document::{
    InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::text::TextRange;
use xiaomu_runtime::session::{
    CaretMove, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

fn document_with(text: &str, marks: MarkSet) -> (XiaomuDocument, NodeId) {
    let content = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, marks).unwrap()]).unwrap()
    };
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(content),
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

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
}

fn session_at(document: &XiaomuDocument, node: NodeId, raw: usize) -> DocumentSession {
    DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(point(document, node, raw)),
    )
    .unwrap()
}

fn text(session: &DocumentSession, node: NodeId) -> String {
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

fn mark_at(session: &DocumentSession, node: NodeId, raw: usize, kind: MarkKind) -> bool {
    let inline = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let end = cursor + run.len_bytes();
        if raw >= cursor && raw < end {
            return run.marks().contains(kind);
        }
        cursor = end;
    }
    false
}

fn insert(session: &mut DocumentSession, value: &str) {
    assert_eq!(
        session
            .apply_intent(&EditIntent::InsertText {
                text: value.to_owned(),
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
}

#[test]
fn adjacent_typing_coalesces_into_one_precise_undo_unit() {
    let (document, paragraph) = document_with("", MarkSet::empty());
    let initial_selection = DocumentSelection::collapsed(point(&document, paragraph, 0));
    let mut session = DocumentSession::new(document.clone(), initial_selection).unwrap();

    insert(&mut session, "a");
    insert(&mut session, "中");
    insert(&mut session, "👍");

    assert_eq!(text(&session, paragraph), "a中👍");
    assert_eq!(session.history_depths(), (1, 0));

    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(text(&session, paragraph), "");
    assert_eq!(session.selection(), initial_selection);
    assert_eq!(session.history_depths(), (0, 1));

    assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(text(&session, paragraph), "a中👍");
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn caret_movement_breaks_the_open_typing_group() {
    let (document, paragraph) = document_with("", MarkSet::empty());
    let mut session = session_at(&document, paragraph, 0);

    insert(&mut session, "a");
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));

    assert_eq!(
        session
            .apply_intent(&EditIntent::MoveCaret {
                caret_move: CaretMove::Backward,
                extend_selection: false,
            })
            .unwrap(),
        SessionOutcome::SelectionChanged
    );
    insert(&mut session, "X");

    assert_eq!(text(&session, paragraph), "aXb");
    assert_eq!(session.history_depths(), (2, 0));
    session.undo().unwrap();
    assert_eq!(text(&session, paragraph), "ab");
}

#[test]
fn caret_and_selection_movement_clear_stored_marks() {
    let (document, paragraph) = document_with("ab", MarkSet::empty());
    let mut session = session_at(&document, paragraph, 1);

    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert!(session.stored_marks().is_some());

    assert_eq!(
        session
            .apply_intent(&EditIntent::MoveCaret {
                caret_move: CaretMove::Backward,
                extend_selection: false,
            })
            .unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert!(session.stored_marks().is_none());

    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert!(session.stored_marks().is_some());

    assert_eq!(
        session
            .apply_intent(&EditIntent::SetSelection {
                anchor: point(session.document(), paragraph, 0),
                focus: point(session.document(), paragraph, 1),
            })
            .unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert!(session.stored_marks().is_none());
}

#[test]
fn collapsed_toggle_can_explicitly_remove_inherited_marks() {
    let bold = MarkSet::new([Mark::Bold]).unwrap();
    let (document, paragraph) = document_with("ab", bold);
    let revision = document.revision();
    let mut session = session_at(&document, paragraph, 1);

    assert_eq!(
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap(),
        SessionOutcome::NoChange
    );
    let stored = session.stored_marks().expect("explicit StoredMarks");
    assert!(!stored.contains(MarkKind::Bold));
    assert_eq!(session.document().revision(), revision);
    assert_eq!(session.history_depths(), (0, 0));

    insert(&mut session, "X");
    assert_eq!(text(&session, paragraph), "aXb");
    assert!(mark_at(&session, paragraph, 0, MarkKind::Bold));
    assert!(!mark_at(&session, paragraph, 1, MarkKind::Bold));
    assert!(mark_at(&session, paragraph, 2, MarkKind::Bold));
}

#[test]
fn stored_marks_survive_split_but_split_breaks_typing_history() {
    let (document, paragraph) = document_with("", MarkSet::empty());
    let root = document.root();
    let mut session = session_at(&document, paragraph, 0);

    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    insert(&mut session, "a");
    assert_eq!(session.history_depths(), (1, 0));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    assert!(
        session
            .stored_marks()
            .expect("split inherits pending marks")
            .contains(MarkKind::Bold)
    );
    assert_eq!(session.history_depths(), (2, 0));

    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (3, 0));
    let children = session
        .document()
        .node(root)
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(children.len(), 2);
    let tail = children[1];
    assert_eq!(text(&session, tail), "b");
    assert!(mark_at(&session, tail, 0, MarkKind::Bold));
}

#[test]
fn undo_and_redo_clear_pending_marks() {
    let (document, paragraph) = document_with("", MarkSet::empty());
    let mut session = session_at(&document, paragraph, 0);

    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    insert(&mut session, "a");
    assert!(session.stored_marks().is_some());

    session.undo().unwrap();
    assert!(session.stored_marks().is_none());
    session.redo().unwrap();
    assert!(session.stored_marks().is_none());
}

#[test]
fn ime_commit_is_isolated_and_uses_stored_marks() {
    let (document, paragraph) = document_with("", MarkSet::empty());
    let mut session = session_at(&document, paragraph, 0);

    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    insert(&mut session, "a");
    assert_eq!(session.history_depths(), (1, 0));

    let caret = session.text_selection().unwrap().focus().offset();
    let range = TextRange::new(caret, caret).unwrap();
    assert_eq!(
        session
            .apply_intent(&EditIntent::CommitComposition {
                range,
                text: "你".to_owned(),
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.history_depths(), (2, 0));
    assert!(mark_at(&session, paragraph, 1, MarkKind::Bold));

    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (3, 0));
    session.undo().unwrap();
    assert_eq!(text(&session, paragraph), "a你");
    session.undo().unwrap();
    assert_eq!(text(&session, paragraph), "a");
}

#[test]
fn plain_text_paste_is_a_history_boundary() {
    let (document, paragraph) = document_with("", MarkSet::empty());
    let mut session = session_at(&document, paragraph, 0);

    insert(&mut session, "a");
    assert_eq!(
        session
            .apply_intent(&EditIntent::PasteText {
                text: "P".to_owned(),
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    insert(&mut session, "b");

    assert_eq!(text(&session, paragraph), "aPb");
    assert_eq!(session.history_depths(), (3, 0));
    session.undo().unwrap();
    assert_eq!(text(&session, paragraph), "aP");
    session.undo().unwrap();
    assert_eq!(text(&session, paragraph), "a");
}
