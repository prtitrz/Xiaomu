//! Prepared Cut orchestration tests, deliberately independent of product semantics.
//!
//! This test host replaces selected cell forests with a tagged empty Paragraph.
//! Its attributes and fixture are NOT an oracle for any editor's Cut contract.
//! Only Runtime's preparation, publication and identity guarantees are tested.

use std::{cell::RefCell, rc::Rc};

use xiaomu_core::document::{
    AttrValue, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::{TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::history::{HistoryEntry, HistoryGroup};
use super::input_rule_undo::InputRuleUndoToken;
use super::plan::HistoryPolicy;
use super::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, EditPlan,
    InputRuleUndoSpec, IntentDisposition, PolicyError, PrimaryEdit, SelectionUpdate,
    SessionContext, SessionError, SessionOutcome, SessionPolicy,
};
use crate::clipboard::{
    ClipboardExportPurpose, ClipboardExportSpec, ClipboardMetadataDecode, ClipboardTextProjection,
    decode_metadata_checked, encode_metadata,
};

fn transaction() -> Transaction {
    Transaction::new(TransactionOrigin::UserInput)
}

fn attrs(key: &str, value: AttrValue) -> NodeAttrs {
    NodeAttrs::new([(key.into(), value)].into()).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    let content = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::new([Mark::Bold]).unwrap()).unwrap()])
            .unwrap()
    };
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(content),
        )
        .unwrap()
}

struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    table: NodeId,
    row: NodeId,
    cells: Vec<NodeId>,
}

fn fixture(oversized: bool) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "intro");
    let mut cells = Vec::new();
    for (index, text) in ["alpha", "beta", "untouched"].into_iter().enumerate() {
        let large;
        let text = if oversized && index == 0 {
            large = "x".repeat(3 * 1024 * 1024);
            large.as_str()
        } else {
            text
        };
        let leaf = paragraph(&mut builder, text);
        let children = if index == 0 {
            let extra = paragraph(&mut builder, "nested");
            let quote = builder
                .insert(
                    NodeKind::Quote,
                    attrs("host-quote", AttrValue::Null),
                    NodeContent::children([leaf, extra]),
                )
                .unwrap();
            vec![quote]
        } else {
            vec![leaf]
        };
        cells.push(
            builder
                .insert(
                    if index == 0 {
                        NodeKind::TableHeader
                    } else {
                        NodeKind::TableCell
                    },
                    attrs("host-cell", AttrValue::Integer(index as i64)),
                    NodeContent::children(children),
                )
                .unwrap(),
        );
    }
    let row = builder
        .insert(
            NodeKind::TableRow,
            attrs("host-row", AttrValue::Bool(true)),
            NodeContent::children(cells.clone()),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            attrs("host-table", AttrValue::Null),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        intro,
        table,
        row,
        cells,
    }
}

fn export_spec() -> ClipboardExportSpec {
    ClipboardExportSpec::new()
        .with_clipped_cell_ranges(NodeAttrs::empty())
        .with_text_projection(ClipboardTextProjection::TextBetweenLfV1)
}

// No prepare_cut override: an export opt-in by itself must not enable Cut.
struct ExportOnly;
impl SessionPolicy for ExportOnly {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        _: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        Ok(Some(export_spec()))
    }
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    None,
    PreparePolicy,
    ExportPolicy,
    MissingExport,
    InvalidTransaction,
    InvalidAfterSelection,
    FinalCandidatePolicy,
    NonCellPlan,
    EmptyPlan,
    AncillaryPlan,
}

// Immutable host policy: no counters, mutable admission switches or recursive
// callbacks. In particular, final validation has a stable lifetime rule.
struct CutPolicy(Fault);
impl SessionPolicy for CutPolicy {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        // A prepare_cut implementation that substitutes Copy fails here.
        if purpose != ClipboardExportPurpose::Cut {
            return Err(PolicyError::new("test Cut policy does not admit Copy"));
        }
        match self.0 {
            Fault::ExportPolicy => Err(PolicyError::new("test export refusal")),
            Fault::MissingExport => Ok(None),
            _ => Ok(Some(export_spec())),
        }
    }

    fn prepare_cut(&self, context: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        if matches!(self.0, Fault::PreparePolicy) {
            return Err(PolicyError::new("test plan refusal"));
        }
        let Some(range) = context.selection().active_cell_range() else {
            return Ok(matches!(self.0, Fault::NonCellPlan).then(|| {
                EditPlan::new(
                    transaction().with_step(TransactionStep::InsertNode {
                        parent: context.document().root(),
                        index: 0,
                        kind: NodeKind::Paragraph,
                        attrs: NodeAttrs::empty(),
                        content: NodeContent::empty_inline(),
                    }),
                    SelectionUpdate::CaretAtLastInsertedOffset { offset: 0 },
                    None,
                )
            }));
        };
        if matches!(self.0, Fault::EmptyPlan) {
            return Ok(Some(EditPlan::new(
                transaction(),
                SelectionUpdate::PreserveSelection,
                None,
            )));
        }
        let document = context.document();
        let mut origins = range
            .unique_origins(document)
            .map_err(|error| PolicyError::new(error.to_string()))?;
        origins.sort_by_key(|cell| *cell == range.focus());
        let mut tx = transaction();
        let mut removed = None;
        for cell in origins {
            tx.push_step(TransactionStep::InsertNode {
                parent: cell,
                index: 0,
                kind: NodeKind::Paragraph,
                attrs: attrs("cut-test-marker", AttrValue::Bool(true)),
                content: NodeContent::empty_inline(),
            });
            for node in document
                .node(cell)
                .unwrap()
                .content()
                .as_children()
                .unwrap()
            {
                let mut leaf = *node;
                while let Some(children) = document.node(leaf).unwrap().content().as_children() {
                    leaf = children[0];
                }
                removed = Some(leaf);
                tx.push_step(TransactionStep::RemoveNode { node: *node });
            }
        }
        if matches!(self.0, Fault::InvalidTransaction) {
            // This fails only after actual earlier InsertNode allocations.
            tx.push_step(TransactionStep::RemoveNode {
                node: document.root(),
            });
        }
        let selection_update = if matches!(self.0, Fault::InvalidAfterSelection) {
            SelectionUpdate::Exact {
                selection: DocumentSelection::collapsed(InlinePoint::at_start_of(
                    removed.expect("fixture has a source forest"),
                )),
            }
        } else {
            SelectionUpdate::CaretAtLastInsertedOffset { offset: 0 }
        };
        let mut plan = EditPlan::new(tx, selection_update, None);
        if matches!(self.0, Fault::AncillaryPlan) {
            // Deliberately hostile Runtime-private metadata. A host normally
            // cannot request Typing, but Cut must normalize it regardless.
            let intro = document
                .node(document.root())
                .unwrap()
                .content()
                .as_children()
                .unwrap()[0];
            let primary = PrimaryEdit::new(
                intro,
                TextRange::new(TextOffset::ZERO, TextOffset::ZERO).unwrap(),
                1,
            );
            let bad_rule = InputRuleUndoSpec::new(
                transaction().with_step(TransactionStep::RemoveNode {
                    node: document.root(),
                }),
                context.selection(),
            )
            .unwrap();
            plan = EditPlan::new(plan.transaction().clone(), selection_update, Some(primary))
                .with_history_policy(HistoryPolicy::Typing)
                .with_stored_marks(Some(MarkSet::new([Mark::Italic]).unwrap()))
                .with_input_rule_undo(bad_rule);
        }
        Ok(Some(plan))
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        assert!(
            !matches!(intent, EditIntent::Delete),
            "prepared Cut must not dispatch Delete"
        );
        Ok(IntentDisposition::Continue)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if matches!(self.0, Fault::FinalCandidatePolicy)
            && document
                .store()
                .iter()
                .any(|node| node.attrs().get("cut-test-marker").is_some())
        {
            return Err(PolicyError::new("test final candidate refusal"));
        }
        Ok(())
    }
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

fn listen(session: &mut DocumentSession) -> Events {
    let events = Rc::new(RefCell::new(Vec::new()));
    session.add_listener(Box::new(Listener(events.clone())));
    events
}

fn session(f: &Fixture, policy: Option<Box<dyn SessionPolicy>>) -> DocumentSession {
    let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(f.intro));
    match policy {
        Some(policy) => DocumentSession::new_with_policy(f.document.clone(), selection, policy),
        None => DocumentSession::new(f.document.clone(), selection),
    }
    .unwrap()
}

fn insert(session: &mut DocumentSession, text: &str) {
    assert_eq!(
        session.apply_intent(&EditIntent::InsertText { text: text.into() }),
        Ok(SessionOutcome::DocumentChanged),
    );
}

fn select_cells(session: &mut DocumentSession, f: &Fixture, backward: bool) {
    let (anchor, focus) = if backward { (1, 0) } else { (0, 1) };
    session
        .set_cell_range_selection(f.cells[anchor], f.cells[focus])
        .unwrap();
}

fn seed_redo(session: &mut DocumentSession, f: &Fixture) {
    insert(session, "a");
    // A raw no-op transaction is a real isolated history entry by contract.
    session.apply(&transaction()).unwrap();
    session.undo().unwrap();
    select_cells(session, f, true);
    assert_eq!(session.history_depths(), (1, 1));
}

fn install_rule_token(session: &mut DocumentSession) {
    let selection = session.selection();
    let spec = InputRuleUndoSpec::new(transaction(), selection).unwrap();
    session
        .commit(
            EditPlan::new(transaction(), SelectionUpdate::Exact { selection }, None)
                .with_input_rule_undo(spec),
        )
        .unwrap();
    assert!(session.input_rule_undo_available());
}

// A deliberately adversarial private-state fixture. Ordinary selection setters
// clear these transients; retaining them here catches any premature clearing.
// The selection/document and generated input-rule token remain individually
// valid. This is not a claim that this combined state is publicly reachable.
fn seed_transient_sentinels(session: &mut DocumentSession, f: &Fixture) {
    session.stored_marks = Some(MarkSet::new([Mark::Italic]).unwrap());
    session.history.restore_typing_group(true);
    session.history_selection_before = Some(DocumentSelection::collapsed(
        InlinePoint::at_start_of(f.intro),
    ));
    let selection = session.selection();
    let spec = InputRuleUndoSpec::new(transaction(), selection).unwrap();
    session.input_rule_undo = Some(
        session
            .prepare_input_rule_undo(spec, session.document(), selection)
            .unwrap(),
    );
    assert!(session.input_rule_undo_available());
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
    fn from(entry: &HistoryEntry) -> Self {
        Self {
            redo: entry.redo.clone(),
            undo: entry.undo.clone(),
            before: entry.before_selection,
            after: entry.after_selection,
            group: entry.group,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct HistoryImage {
    undo: Vec<EntryImage>,
    redo: Vec<EntryImage>,
    grouping: super::history::GroupingState,
}

// Existing crate-private stack operations let tests compare complete history
// transactions without adding production inspection APIs. Restore identical
// entries in identical order and restore full grouping state before returning.
fn history_image(session: &mut DocumentSession) -> HistoryImage {
    let grouping = session.history.grouping_state();
    let mut undo = Vec::new();
    while let Some(entry) = session.history.take_undo() {
        undo.push(entry);
    }
    let undo_image = undo.iter().map(EntryImage::from).collect();
    for entry in undo.into_iter().rev() {
        session.history.restore_undo(entry);
    }
    let mut redo = Vec::new();
    while let Some(entry) = session.history.take_redo() {
        redo.push(entry);
    }
    let redo_image = redo.iter().map(EntryImage::from).collect();
    for entry in redo.into_iter().rev() {
        session.history.park_undone(entry);
    }
    session.history.restore_grouping_state(grouping);
    HistoryImage {
        undo: undo_image,
        redo: redo_image,
        grouping,
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
    fn capture(session: &mut DocumentSession, events: &Events) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            history: history_image(session),
            history_selection_before: session.history_selection_before,
            token: session.input_rule_undo.clone(),
            token_available: session.input_rule_undo_available(),
            listener_count: session.listeners.len(),
            events: events.borrow().clone(),
        }
    }

    fn assert_unchanged(&self, session: &mut DocumentSession, events: &Events) {
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().root(), self.document.root());
        assert_eq!(session.document().version(), self.document.version());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(history_image(session), self.history);
        assert_eq!(
            session.history_selection_before,
            self.history_selection_before
        );
        match (&session.input_rule_undo, &self.token) {
            (Some(actual), Some(before)) => assert!(Rc::ptr_eq(actual, before)),
            (None, None) => {}
            _ => panic!("input-rule token changed"),
        }
        assert_eq!(session.input_rule_undo_available(), self.token_available);
        assert_eq!(session.listeners.len(), self.listener_count);
        assert_eq!(*events.borrow(), self.events);
    }
}

// This is only a synchronous test writer. It does not assert OS atomicity,
// GPUI admission, metadata failure handling or an OS-write acknowledgment.
#[derive(Debug, PartialEq, Eq)]
struct Writer {
    calls: usize,
    text: String,
    metadata: String,
}
impl Writer {
    fn seeded() -> Self {
        Self {
            calls: 0,
            text: "existing clipboard".into(),
            metadata: "existing metadata".into(),
        }
    }

    fn write_prepared(&mut self, text: String, metadata: String) {
        self.calls += 1;
        self.text = text;
        self.metadata = metadata;
    }
}

fn publish_through_writer(
    session: &mut DocumentSession,
    writer: &mut Writer,
) -> Result<Option<SessionOutcome>, SessionError> {
    let Some(prepared) = session.prepare_cut()? else {
        return Ok(None);
    };
    let slice = prepared.clipboard_slice();
    let metadata = encode_metadata(slice).expect("small admitted Runtime fixture");
    assert_eq!(
        decode_metadata_checked(slice.plain_text(), &metadata),
        ClipboardMetadataDecode::Valid(slice.clone()),
    );
    let text = slice.plain_text().to_owned();
    writer.write_prepared(text, metadata);
    // The type is deliberately asserted: publish is NOT a Result and cannot
    // run a new fallible validation after the external write.
    let outcome: SessionOutcome = prepared.publish();
    Ok(Some(outcome))
}

fn append_allocator_probe(session: &mut DocumentSession) -> NodeId {
    let parent = session.document().root();
    let index = session
        .document()
        .node(parent)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .len();
    session
        .apply(&transaction().with_step(TransactionStep::InsertNode {
            parent,
            index,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::empty_inline(),
        }))
        .unwrap();
    session
        .document()
        .node(parent)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[index]
}

fn assert_future_allocation_matches(session: &mut DocumentSession, control: &mut DocumentSession) {
    assert_eq!(
        append_allocator_probe(session),
        append_allocator_probe(control)
    );
    assert_eq!(session.document().store(), control.document().store());
    assert_eq!(session.document().revision(), control.document().revision());
    assert_eq!(session.selection(), control.selection());
    assert_eq!(history_image(session), history_image(control));
}

mod atomicity;
mod publish;
mod timed;
