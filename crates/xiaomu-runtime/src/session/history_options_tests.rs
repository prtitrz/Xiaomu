//! Generic history traversal contracts; no consumer or browser expectations.
//!
//! `traversal` and `editing_state` drive public Session APIs. `faults` labels
//! deliberately corrupted private state separately; it does not suggest that
//! immutable host policy can invalidate an already admitted history entry.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use xiaomu_core::document::{
    AttrValue, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::history::{HistoryEntry, HistoryGroup};
use super::input_rule_undo::InputRuleUndoToken;
use super::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, EditPlan,
    EmptyHistoryBehavior, HistoryOptions, HistorySelectionMode, InputRuleUndoSpec,
    IntentDisposition, PolicyError, SelectionUpdate, SessionContext, SessionError, SessionOutcome,
    SessionPolicy,
};

mod editing_state;
mod faults;
mod traversal;

fn transaction() -> Transaction {
    Transaction::new(TransactionOrigin::UserInput)
}

fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}

fn point(node: NodeId, raw: usize) -> InlinePoint {
    InlinePoint::new(node, offset(raw), 0, CursorAffinity::After)
}

fn caret(node: NodeId, raw: usize) -> DocumentSelection {
    DocumentSelection::collapsed(point(node, raw))
}

fn replace(node: NodeId, start: usize, end: usize, text: &str) -> TransactionStep {
    TransactionStep::ReplaceText {
        node,
        range: TextRange::new(offset(start), offset(end)).unwrap(),
        replacement: text.into(),
    }
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

struct Fixture {
    document: XiaomuDocument,
    first: NodeId,
    second: NodeId,
    table: NodeId,
    cells: Vec<NodeId>,
    cell_paragraphs: Vec<NodeId>,
}

fn fixture(first_text: &str) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let first = paragraph(&mut builder, first_text);
    let second = paragraph(&mut builder, "tail");
    let mut cells = Vec::new();
    let mut cell_paragraphs = Vec::new();
    let mut rows = Vec::new();
    for values in [["A", "B"], ["C", "D"]] {
        let mut row_cells = Vec::new();
        for value in values {
            let p = paragraph(&mut builder, value);
            let cell = builder
                .insert(
                    NodeKind::TableCell,
                    NodeAttrs::empty(),
                    NodeContent::children([p]),
                )
                .unwrap();
            cell_paragraphs.push(p);
            cells.push(cell);
            row_cells.push(cell);
        }
        rows.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(row_cells),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second, table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        second,
        table,
        cells,
        cell_paragraphs,
    }
}

fn cell_range(f: &Fixture) -> DocumentSelection {
    // Preserve reverse direction, exact cell identities and the parked endpoint.
    DocumentSelection::cell_range(
        f.cells[3],
        f.cells[0],
        point(f.cell_paragraphs[3], 0).into(),
    )
}

fn capture_options() -> HistoryOptions {
    HistoryOptions::new()
        .with_selection_mode(HistorySelectionMode::CaptureOnTraversal)
        .with_empty_behavior(EmptyHistoryBehavior::PreserveEditingState)
}

struct OptionsPolicy(HistoryOptions);
impl SessionPolicy for OptionsPolicy {
    fn history_options(&self) -> HistoryOptions {
        self.0
    }
}

fn session(f: &Fixture, options: HistoryOptions) -> DocumentSession {
    DocumentSession::new_with_policy(
        f.document.clone(),
        caret(f.first, 0),
        Box::new(OptionsPolicy(options)),
    )
    .unwrap()
}

fn insert(s: &mut DocumentSession, text: &str) {
    assert_eq!(
        s.apply_intent(&EditIntent::InsertText { text: text.into() }),
        Ok(SessionOutcome::DocumentChanged)
    );
}

fn select(s: &mut DocumentSession, selection: DocumentSelection) {
    let revision = s.document().revision();
    let depths = s.history_depths();
    s.set_document_selection(selection).unwrap();
    assert_eq!(s.selection(), selection);
    assert_eq!(s.document().revision(), revision, "selection-only move");
    assert_eq!(s.history_depths(), depths, "selection-only move");
}

fn plain(s: &DocumentSession, node: NodeId) -> String {
    s.document()
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Document(u64, DocumentSelection),
    Selection(DocumentSelection),
}
type Events = Rc<RefCell<Vec<Event>>>;
struct Listener(Events);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, document: &XiaomuDocument, selection: DocumentSelection) {
        selection.validate(document).unwrap();
        self.0
            .borrow_mut()
            .push(Event::Document(document.revision().as_u64(), selection));
    }
    fn selection_changed(&mut self, selection: DocumentSelection) {
        self.0.borrow_mut().push(Event::Selection(selection));
    }
}
fn listen(s: &mut DocumentSession) -> Events {
    let events = Rc::new(RefCell::new(Vec::new()));
    s.add_listener(Box::new(Listener(events.clone())));
    events
}

#[derive(Clone, Copy, Debug)]
enum Direction {
    Undo,
    Redo,
}
impl Direction {
    fn run(self, s: &mut DocumentSession) -> Result<SessionOutcome, SessionError> {
        match self {
            Self::Undo => s.undo(),
            Self::Redo => s.redo(),
        }
    }
    const fn opposite(self) -> Self {
        match self {
            Self::Undo => Self::Redo,
            Self::Redo => Self::Undo,
        }
    }
    fn take(self, s: &mut DocumentSession) -> HistoryEntry {
        match self {
            Self::Undo => s.history.take_undo().unwrap(),
            Self::Redo => s.history.take_redo().unwrap(),
        }
    }
    fn restore(self, s: &mut DocumentSession, entry: HistoryEntry) {
        match self {
            Self::Undo => s.history.restore_undo(entry),
            Self::Redo => s.history.restore_redo(entry),
        }
    }
}

fn traverse(
    s: &mut DocumentSession,
    events: &Events,
    direction: Direction,
    document: &XiaomuDocument,
    selection: DocumentSelection,
    depths: (usize, usize),
) {
    events.borrow_mut().clear();
    let revision = s.document().revision().as_u64();
    assert_eq!(direction.run(s), Ok(SessionOutcome::DocumentChanged));
    assert_eq!(s.document().store(), document.store());
    assert_eq!(s.document().root(), document.root());
    assert_eq!(s.selection(), selection);
    assert_eq!(s.history_depths(), depths);
    assert_eq!(s.stored_marks(), None);
    assert_eq!(s.document().revision().as_u64(), revision + 1);
    assert_eq!(
        *events.borrow(),
        vec![Event::Document(revision + 1, selection)],
        "one atomic notification, no intermediate selection publication"
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EntryImage {
    redo: Transaction,
    undo: Transaction,
    before: DocumentSelection,
    after: DocumentSelection,
    group: HistoryGroup,
}
impl From<&HistoryEntry> for EntryImage {
    fn from(e: &HistoryEntry) -> Self {
        Self {
            redo: e.redo.clone(),
            undo: e.undo.clone(),
            before: e.before_selection,
            after: e.after_selection,
            group: e.group,
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
struct HistoryImage {
    undo: Vec<EntryImage>,
    redo: Vec<EntryImage>,
    group_open: bool,
}

// Observe full transactions and selections, not just stack depths. Restore each
// stack's exact order and the grouping bit; no production inspection API needed.
fn history_image(s: &mut DocumentSession) -> HistoryImage {
    let group_open = s.history.typing_group_open();
    let mut undo = Vec::new();
    while let Some(entry) = s.history.take_undo() {
        undo.push(entry);
    }
    let undo_image = undo.iter().map(EntryImage::from).collect();
    for entry in undo.into_iter().rev() {
        s.history.restore_undo(entry);
    }
    let mut redo = Vec::new();
    while let Some(entry) = s.history.take_redo() {
        redo.push(entry);
    }
    let redo_image = redo.iter().map(EntryImage::from).collect();
    for entry in redo.into_iter().rev() {
        s.history.restore_redo(entry);
    }
    s.history.restore_typing_group(group_open);
    HistoryImage {
        undo: undo_image,
        redo: redo_image,
        group_open,
    }
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    history: HistoryImage,
    history_selection_before: Option<DocumentSelection>,
    token: Option<Rc<InputRuleUndoToken>>,
    token_available: bool,
    listener_count: usize,
    events: Vec<Event>,
}
impl Snapshot {
    fn capture(s: &mut DocumentSession, events: &Events) -> Self {
        Self {
            document: s.document().clone(),
            selection: s.selection(),
            marks: s.stored_marks().cloned(),
            history: history_image(s),
            history_selection_before: s.history_selection_before,
            token: s.input_rule_undo.clone(),
            token_available: s.input_rule_undo_available(),
            listener_count: s.listeners.len(),
            events: events.borrow().clone(),
        }
    }
    fn assert_unchanged(&self, s: &mut DocumentSession, events: &Events) {
        assert_eq!(s.document().store(), self.document.store());
        assert_eq!(s.document().root(), self.document.root());
        assert_eq!(s.document().version(), self.document.version());
        assert_eq!(s.document().revision(), self.document.revision());
        assert_eq!(s.selection(), self.selection);
        assert_eq!(s.stored_marks(), self.marks.as_ref());
        assert_eq!(history_image(s), self.history);
        assert_eq!(s.history_selection_before, self.history_selection_before);
        match (&s.input_rule_undo, &self.token) {
            (Some(actual), Some(before)) => assert!(Rc::ptr_eq(actual, before)),
            (None, None) => {}
            _ => panic!("input-rule token changed"),
        }
        assert_eq!(s.input_rule_undo_available(), self.token_available);
        assert_eq!(s.listeners.len(), self.listener_count);
        assert_eq!(*events.borrow(), self.events);
    }
}
