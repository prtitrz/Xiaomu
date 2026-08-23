//! P1.2 DocumentSession orchestration tests.

use std::cell::RefCell;
use std::rc::Rc;

use xiaomu_core::document::{
    InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint, TextSelection};
use xiaomu_core::text::{TextBuffer, TextOffset};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    CaretMove, DocumentChangeListener, DocumentSession, EditIntent, SessionError, SessionOutcome,
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

/// `Document > [p(runs)]` built from `(text, marks)` parts.
fn marked_document(parts: &[(&str, MarkSet)]) -> (XiaomuDocument, NodeId) {
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

fn document_with(text: &str) -> (XiaomuDocument, NodeId) {
    marked_document(&[(text, MarkSet::empty())])
}

fn session_with(document: &XiaomuDocument, selection: TextSelection) -> DocumentSession {
    DocumentSession::new(document.clone(), selection).unwrap()
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

fn insert(text: &str) -> EditIntent {
    EditIntent::InsertText {
        text: text.to_owned(),
    }
}

fn move_caret(caret_move: CaretMove, extend_selection: bool) -> EditIntent {
    EditIntent::MoveCaret {
        caret_move,
        extend_selection,
    }
}

fn caret_offset(session: &DocumentSession) -> usize {
    session.selection().focus().offset().as_usize()
}

/// Shared counters so tests can keep reading listener state after handing a
/// clone of the handle to the session.
#[derive(Clone, Default)]
struct ListenerHandle {
    state: Rc<RefCell<Counters>>,
}

#[derive(Clone, Default)]
struct Counters {
    document_changes: usize,
    selection_changes: usize,
    last_revision: Option<u64>,
}

impl DocumentChangeListener for ListenerHandle {
    fn document_changed(&mut self, document: &XiaomuDocument, _selection: TextSelection) {
        let mut state = self.state.borrow_mut();
        state.document_changes += 1;
        state.last_revision = Some(document.revision().as_u64());
    }

    fn selection_changed(&mut self, _selection: TextSelection) {
        self.state.borrow_mut().selection_changes += 1;
    }
}

impl ListenerHandle {
    fn counters(&self) -> Counters {
        self.state.borrow().clone()
    }
}

#[test]
fn insert_text_sets_caret_after_replacement_and_records_history() {
    let (document, paragraph) = document_with("你好世界");
    let mut session = session_with(&document, caret(&document, paragraph, 3));
    let listener = ListenerHandle::default();
    session.add_listener(Box::new(listener.clone()));

    assert_eq!(
        session.apply_intent(&insert("XY，")).unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(text_of(&session, paragraph), "你XY，好世界");
    assert_eq!(caret_offset(&session), 8); // 3 + len("XY，")
    assert_eq!(
        session.document().revision().as_u64(),
        document.revision().as_u64() + 1
    );
    assert_eq!(session.history_depths(), (1, 0));

    let counters = listener.counters();
    assert_eq!(counters.document_changes, 1);
    assert_eq!(counters.selection_changes, 0);
    assert_eq!(
        counters.last_revision,
        Some(document.revision().as_u64() + 1)
    );
}

#[test]
fn insert_replaces_selection_and_sets_caret_after_it() {
    let (document, paragraph) = document_with("你好世界");
    let mut session = session_with(&document, range_of(&document, paragraph, 3, 9));

    session.apply_intent(&insert("XY")).unwrap();
    assert_eq!(text_of(&session, paragraph), "你XY界");
    assert_eq!(caret_offset(&session), 5); // 3 + len("XY")
}

#[test]
fn insert_empty_over_selection_deletes_it() {
    let (document, paragraph) = document_with("你好世界");
    let mut session = session_with(&document, range_of(&document, paragraph, 3, 6));

    assert_eq!(
        session.apply_intent(&insert("")).unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(text_of(&session, paragraph), "你世界");
    assert_eq!(caret_offset(&session), 3);
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn no_op_intents_skip_core_history_and_notifications() {
    // "a👍中": boundaries are 0, 1, 5, 8.
    let (document, paragraph) = document_with("a👍中");
    let mut session = session_with(&document, caret(&document, paragraph, 0));
    let listener = ListenerHandle::default();
    session.add_listener(Box::new(listener.clone()));

    let no_ops = [
        EditIntent::Backspace,                       // at the very start
        insert(""),                                  // empty text, collapsed caret
        EditIntent::ToggleMark { mark: Mark::Bold }, // collapsed selection
        move_caret(CaretMove::Backward, false),      // at start
        move_caret(CaretMove::ToStart, false),       // already at start
    ];
    for intent in &no_ops {
        assert_eq!(
            session.apply_intent(intent).unwrap(),
            SessionOutcome::NoChange
        );
    }

    // Move to the end and exhaust the forward-side no-ops.
    session
        .apply_intent(&move_caret(CaretMove::ToEnd, false))
        .unwrap();
    session.apply_intent(&EditIntent::Delete).unwrap();
    session
        .apply_intent(&move_caret(CaretMove::Forward, false))
        .unwrap();
    session
        .apply_intent(&move_caret(CaretMove::ToEnd, false))
        .unwrap();
    session
        .apply_intent(&move_caret(CaretMove::Forward, true))
        .unwrap();

    assert_eq!(
        session.document().revision().as_u64(),
        document.revision().as_u64()
    );
    assert_eq!(text_of(&session, paragraph), "a👍中");
    assert_eq!(session.history_depths(), (0, 0));
    let counters = listener.counters();
    assert_eq!(counters.document_changes, 0);
    assert_eq!(counters.last_revision, None);
    // Only the initial ToEnd move fired a selection notification.
    assert_eq!(counters.selection_changes, 1);
}

#[test]
fn backspace_deletes_previous_unicode_scalar() {
    // "a👍中": a=0..1, 👍=1..5, 中=5..8.
    let (document, paragraph) = document_with("a👍中");
    let mut session = session_with(&document, caret(&document, paragraph, 5));

    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text_of(&session, paragraph), "a中");
    assert_eq!(caret_offset(&session), 1);

    // CJK scalar before the caret.
    let mut session = session_with(&document, caret(&document, paragraph, 8));
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text_of(&session, paragraph), "a👍");
    assert_eq!(caret_offset(&session), 5);
}

#[test]
fn backspace_over_selection_deletes_the_range() {
    let (document, paragraph) = document_with("你好世界");
    let mut session = session_with(&document, range_of(&document, paragraph, 3, 9));

    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text_of(&session, paragraph), "你界");
    assert_eq!(caret_offset(&session), 3);
}

#[test]
fn delete_removes_next_unicode_scalar() {
    let (document, paragraph) = document_with("a👍中");
    let mut session = session_with(&document, caret(&document, paragraph, 1));

    session.apply_intent(&EditIntent::Delete).unwrap();
    assert_eq!(text_of(&session, paragraph), "a中");
    assert_eq!(caret_offset(&session), 1);

    let mut session = session_with(&document, caret(&document, paragraph, 5));
    session.apply_intent(&EditIntent::Delete).unwrap();
    assert_eq!(text_of(&session, paragraph), "a👍");
    assert_eq!(caret_offset(&session), 5);
}

#[test]
fn caret_moves_walk_scalar_boundaries_and_respect_extend() {
    // "a👍中": boundaries are 0, 1, 5, 8.
    let (document, paragraph) = document_with("a👍中");
    let mut session = session_with(&document, caret(&document, paragraph, 5));
    let listener = ListenerHandle::default();
    session.add_listener(Box::new(listener.clone()));

    assert_eq!(
        session
            .apply_intent(&move_caret(CaretMove::Backward, false))
            .unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert_eq!(caret_offset(&session), 1);
    assert_eq!(
        session
            .apply_intent(&move_caret(CaretMove::Backward, false))
            .unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert_eq!(caret_offset(&session), 0);

    session
        .apply_intent(&move_caret(CaretMove::Forward, false))
        .unwrap();
    assert_eq!(caret_offset(&session), 1);
    session
        .apply_intent(&move_caret(CaretMove::Forward, false))
        .unwrap();
    assert_eq!(caret_offset(&session), 5);
    session
        .apply_intent(&move_caret(CaretMove::Forward, false))
        .unwrap();
    assert_eq!(caret_offset(&session), 8);

    session
        .apply_intent(&move_caret(CaretMove::ToStart, false))
        .unwrap();
    assert_eq!(caret_offset(&session), 0);
    session
        .apply_intent(&move_caret(CaretMove::ToEnd, false))
        .unwrap();
    assert_eq!(caret_offset(&session), 8);

    // Extending keeps the anchor and moves the focus; the selection can
    // legitimately become reversed.
    session
        .apply_intent(&move_caret(CaretMove::Backward, true))
        .unwrap();
    let selection = session.selection();
    assert_eq!(selection.anchor().offset().as_usize(), 8);
    assert_eq!(selection.focus().offset().as_usize(), 5);

    // The document never changed through any move.
    assert_eq!(
        session.document().revision().as_u64(),
        document.revision().as_u64()
    );
    assert_eq!(session.history_depths(), (0, 0));
    let counters = listener.counters();
    assert_eq!(counters.document_changes, 0);
    assert!(counters.selection_changes > 0);
}

#[test]
fn toggle_mark_adds_then_removes_and_preserves_selection() {
    let (document, paragraph) = document_with("你好世界");
    let selection = range_of(&document, paragraph, 3, 9); // "好世"
    let mut session = session_with(&document, selection);

    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    let runs = session
        .document()
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs.len(), 3);
    assert!(runs[1].marks().contains(MarkKind::Bold));
    // MapExisting keeps the selection covering the same text.
    assert_eq!(session.selection(), selection);

    // Fully marked now: the same intent removes the mark again and the
    // store round-trips to the original snapshot.
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (2, 0));
}

#[test]
fn toggle_mark_over_partially_marked_range_adds() {
    let bold = MarkSet::new([Mark::Bold]).unwrap();
    let (document, paragraph) = marked_document(&[("你好", bold), ("世界", MarkSet::empty())]);
    // Cover both runs: not fully marked, so the toggle adds bold everywhere.
    let mut session = session_with(&document, range_of(&document, paragraph, 0, 12));

    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    let runs = session
        .document()
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs.len(), 1); // fully bold runs merge
    assert!(runs[0].marks().contains(MarkKind::Bold));
}

#[test]
fn apply_removing_the_selection_node_is_rejected_atomically() {
    let (document, paragraph) = document_with("你好世界");
    let mut session = session_with(&document, caret(&document, paragraph, 3));
    let listener = ListenerHandle::default();
    session.add_listener(Box::new(listener.clone()));

    let removal = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode { node: paragraph });
    assert_eq!(
        session.apply(&removal).unwrap_err(),
        SessionError::SelectionDeleted
    );

    // Nothing escaped the failed commit.
    assert_eq!(
        session.document().revision().as_u64(),
        document.revision().as_u64()
    );
    assert_eq!(text_of(&session, paragraph), "你好世界");
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(caret_offset(&session), 3);
    assert_eq!(listener.counters().document_changes, 0);
}

#[test]
fn undo_redo_round_trip_restores_stores_and_selections() {
    let (document, paragraph) = document_with("你好世界");
    let initial_store = document.store().clone();
    let mut session = session_with(&document, caret(&document, paragraph, 0));
    let listener = ListenerHandle::default();
    session.add_listener(Box::new(listener.clone()));

    session.apply_intent(&insert("A")).unwrap();
    assert_eq!(caret_offset(&session), 1);
    session.apply_intent(&insert("B")).unwrap();
    assert_eq!(caret_offset(&session), 2);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text_of(&session, paragraph), "A你好世界");
    assert_eq!(caret_offset(&session), 1);
    let peak_store = session.document().store().clone();

    // Undo walks the selections back through the recorded before-states.
    session.undo().unwrap();
    assert_eq!(text_of(&session, paragraph), "AB你好世界");
    assert_eq!(caret_offset(&session), 2);
    session.undo().unwrap();
    assert_eq!(text_of(&session, paragraph), "A你好世界");
    assert_eq!(caret_offset(&session), 1);
    session.undo().unwrap();
    assert_eq!(text_of(&session, paragraph), "你好世界");
    assert_eq!(caret_offset(&session), 0);
    assert_eq!(session.document().store(), &initial_store);
    assert_eq!(session.history_depths(), (0, 3));

    // Redo replays the recorded after-states in order.
    session.redo().unwrap();
    assert_eq!(caret_offset(&session), 1);
    session.redo().unwrap();
    assert_eq!(caret_offset(&session), 2);
    session.redo().unwrap();
    assert_eq!(text_of(&session, paragraph), "A你好世界");
    assert_eq!(caret_offset(&session), 1);
    assert_eq!(session.document().store(), &peak_store);
    assert_eq!(session.history_depths(), (3, 0));

    // Three commits, three undos, three redos: nine document changes.
    assert_eq!(listener.counters().document_changes, 9);
}

#[test]
fn undo_then_new_edit_clears_the_redo_stack() {
    let (document, paragraph) = document_with("你好世界");
    let mut session = session_with(&document, caret(&document, paragraph, 0));

    session.apply_intent(&insert("A")).unwrap();
    session.undo().unwrap();
    assert_eq!(session.history_depths(), (0, 1));

    session.apply_intent(&insert("B")).unwrap();
    assert_eq!(session.history_depths(), (1, 0));

    // Both entries are still undoable.
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(text_of(&session, paragraph), "你好世界");
    assert_eq!(caret_offset(&session), 0);
}

#[test]
fn raw_empty_transaction_still_commits() {
    let (document, paragraph) = document_with("你好世界");
    let mut session = session_with(&document, caret(&document, paragraph, 3));

    assert_eq!(
        session
            .apply(&Transaction::new(TransactionOrigin::System))
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(
        session.document().revision().as_u64(),
        document.revision().as_u64() + 1
    );
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn new_rejects_selections_invalid_for_the_snapshot() {
    let (document, paragraph) = document_with("你好");
    const SCRATCH: &str = "00000000000000000000000000000000000000000000000000000000000000000000";
    // Valid boundary of the scratch buffer, but far beyond "你好" (len 6).
    let beyond_end = TextBuffer::from(SCRATCH).offset_at(20).unwrap();
    let selection = TextSelection::collapsed(TextPoint::new(
        paragraph,
        beyond_end,
        CursorAffinity::Before,
    ));

    let error = match DocumentSession::new(document, selection) {
        Err(error) => error,
        Ok(_) => panic!("selection beyond the text must be rejected"),
    };
    assert_eq!(error, SessionError::SelectionInvalid);
}

#[test]
fn undo_and_redo_on_empty_history_are_no_ops() {
    let (document, paragraph) = document_with("你好");
    let mut session = session_with(&document, caret(&document, paragraph, 3));

    assert_eq!(session.undo().unwrap(), SessionOutcome::NoChange);
    assert_eq!(session.redo().unwrap(), SessionOutcome::NoChange);
    assert_eq!(
        session.document().revision().as_u64(),
        document.revision().as_u64()
    );
    assert_eq!(session.history_depths(), (0, 0));
}
