//! Public contract for opt-in timed typing and selection-only grouping.
//! Explicit timestamps make every case deterministic without sleeping.

#[path = "timed_history/isolation.rs"]
mod isolation;
#[path = "timed_history/selection.rs"]
mod selection;
#[path = "support/mixed_marks.rs"]
mod support;
#[path = "timed_history/timing.rs"]
mod timing;

use std::{cell::Cell, rc::Rc};

use support::*;
use xiaomu_core::document::{Mark, MarkSet, NodeId, XiaomuDocument};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    CaretMove, DefaultTextInputMarks, DocumentSelection, DocumentSession, EditIntent, EditPlan,
    EmptyHistoryBehavior, HistoryOptions, HistorySelectionMode, HistoryTimestamp,
    IntentDisposition, PolicyError, SelectionOnlyGrouping, SelectionUpdate, SessionContext,
    SessionOutcome, SessionPolicy,
};

struct Options {
    history: HistoryOptions,
    marks: DefaultTextInputMarks,
}

impl SessionPolicy for Options {
    fn history_options(&self) -> HistoryOptions {
        self.history
    }

    fn default_text_input_marks(&self) -> DefaultTextInputMarks {
        self.marks
    }
}

fn session_with(
    document: XiaomuDocument,
    selection: DocumentSelection,
    history: HistoryOptions,
    marks: DefaultTextInputMarks,
) -> DocumentSession {
    DocumentSession::new_with_policy(document, selection, Box::new(Options { history, marks }))
        .unwrap()
}

fn setup(options: HistoryOptions) -> (DocumentSession, NodeId) {
    let (document, node, _) = fixture("", MarkSet::empty(), &[]);
    let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
    (
        session_with(
            document,
            selection,
            options,
            DefaultTextInputMarks::PreservePending,
        ),
        node,
    )
}

fn timed() -> HistoryOptions {
    HistoryOptions::new().with_typing_group_delay_ms(500)
}

fn combined() -> HistoryOptions {
    timed().with_selection_only_grouping(SelectionOnlyGrouping::Preserve)
}

fn timestamp(milliseconds: u64) -> HistoryTimestamp {
    HistoryTimestamp::from_millis(milliseconds)
}

fn insertion(value: &str) -> EditIntent {
    EditIntent::InsertText { text: value.into() }
}

fn type_at(session: &mut DocumentSession, value: &str, milliseconds: u64) {
    assert_eq!(
        session
            .apply_intent_at(&insertion(value), timestamp(milliseconds))
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
}

fn contents(session: &DocumentSession, node: NodeId) -> String {
    inline(session.document(), node)
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

fn caret(session: &DocumentSession, node: NodeId, raw: usize) -> DocumentSelection {
    DocumentSelection::collapsed(point(session.document(), node, raw, 0))
}

fn select(session: &mut DocumentSession, node: NodeId, raw: usize) {
    let selection = caret(session, node, raw);
    session.set_document_selection(selection).unwrap();
}

fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(offset(start), offset(end)).unwrap()
}

fn replace(node: NodeId, start: usize, end: usize, value: &str) -> Transaction {
    Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::ReplaceText {
        node,
        range: range(start, end),
        replacement: value.into(),
    })
}

fn move_to(caret_move: CaretMove) -> EditIntent {
    EditIntent::MoveCaret {
        caret_move,
        extend_selection: false,
    }
}

/// Expected document states in chronological history order, including seed.
/// Traverse every entry twice, checking full final store and selection as well
/// as each intermediate text and stack depth. Revision is deliberately not
/// compared: successful traversal publishes a fresh revision.
fn round_trip(session: &mut DocumentSession, node: NodeId, states: &[&str]) {
    let after = session.document().clone();
    let selection = session.selection();
    let total = states.len() - 1;
    assert_eq!(contents(session, node), states[total]);
    assert_eq!(session.history_depths(), (total, 0));
    for _ in 0..2 {
        for index in (0..total).rev() {
            assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
            assert_eq!(contents(session, node), states[index]);
            assert_eq!(session.history_depths(), (index, total - index));
            session.selection().validate(session.document()).unwrap();
        }
        for (index, expected) in states.iter().enumerate().skip(1) {
            assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
            assert_eq!(contents(session, node), *expected);
            assert_eq!(session.history_depths(), (index, total - index));
            session.selection().validate(session.document()).unwrap();
        }
        assert_eq!(session.document().store(), after.store());
        assert_eq!(session.document().root(), after.root());
        assert_eq!(session.document().version(), after.version());
        assert_eq!(session.selection(), selection);
    }
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    depths: (usize, usize),
    rule_available: bool,
    notifications: (usize, usize),
}

impl Snapshot {
    fn capture(session: &DocumentSession, counts: &Counts) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            depths: session.history_depths(),
            rule_available: session.input_rule_undo_available(),
            notifications: counts.get(),
        }
    }

    fn assert_unchanged(&self, session: &DocumentSession, counts: &Counts) {
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().root(), self.document.root());
        assert_eq!(session.document().version(), self.document.version());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.depths);
        assert_eq!(session.input_rule_undo_available(), self.rule_available);
        assert_eq!(counts.get(), self.notifications);
    }
}

#[test]
fn new_history_choices_are_independent_and_default_to_legacy_behavior() {
    assert_eq!(HistoryOptions::new(), HistoryOptions::default());
    assert_eq!(
        SelectionOnlyGrouping::default(),
        SelectionOnlyGrouping::Close
    );
    assert_eq!(HistoryOptions::new().typing_group_delay_ms(), None);
    assert_eq!(
        HistoryOptions::new().selection_only_grouping(),
        SelectionOnlyGrouping::Close
    );
    assert_eq!(
        timed().selection_only_grouping(),
        SelectionOnlyGrouping::Close
    );
    let preserve =
        HistoryOptions::new().with_selection_only_grouping(SelectionOnlyGrouping::Preserve);
    assert_eq!(preserve.typing_group_delay_ms(), None);
    let options = preserve
        .with_typing_group_delay_ms(500)
        .with_selection_mode(HistorySelectionMode::CaptureOnTraversal)
        .with_empty_behavior(EmptyHistoryBehavior::PreserveEditingState);
    assert_eq!(options.typing_group_delay_ms(), Some(500));
    assert_eq!(
        options.selection_only_grouping(),
        SelectionOnlyGrouping::Preserve
    );
    assert_eq!(
        options.selection_mode(),
        HistorySelectionMode::CaptureOnTraversal
    );
    assert_eq!(
        options.empty_behavior(),
        EmptyHistoryBehavior::PreserveEditingState
    );
    let (session, _) = setup(options);
    assert_eq!(session.history_options(), options);
    for value in [0, 1, 500, u64::MAX] {
        assert_eq!(timestamp(value).as_millis(), value);
    }
}

#[test]
fn construction_captures_options_once() {
    struct CountedOptions(Rc<Cell<usize>>);
    impl SessionPolicy for CountedOptions {
        fn history_options(&self) -> HistoryOptions {
            // Observation-only spy; the configuration is immutable.
            self.0.set(self.0.get() + 1);
            combined()
        }
    }
    let calls = Rc::new(Cell::new(0));
    let (document, node, _) = fixture("", MarkSet::empty(), &[]);
    let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
    let mut session = DocumentSession::new_with_policy(
        document,
        selection,
        Box::new(CountedOptions(calls.clone())),
    )
    .unwrap();
    assert_eq!(session.history_options(), combined());
    type_at(&mut session, "a", 1000);
    type_at(&mut session, "b", 1501);
    round_trip(&mut session, node, &["", "a", "ab"]);
    assert_eq!(calls.get(), 1);
}
