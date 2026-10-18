//! Generic final-candidate policy contracts, independent of any host schema.
//! Instrumentation records callback boundaries; it never drives policy decisions.

use std::{cell::RefCell, rc::Rc};

use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::{ClipboardExportPurpose, ClipboardExportSpec};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentChangeOrigin, DocumentSelection, DocumentSession,
    EditFinalization, EditIntent, EditPlan, FinalizeEditContext, IntentDisposition, PolicyError,
    SelectionUpdate, SessionContext, SessionError, SessionOutcome, SessionPolicy,
};

#[path = "edit_finalization/budget.rs"]
mod budget;
#[path = "edit_finalization/rejection.rs"]
mod rejection;
#[path = "edit_finalization/success.rs"]
mod success;

fn fixture(texts: &[&str]) -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let nodes: Vec<_> = texts
        .iter()
        .map(|text| {
            builder
                .insert(NodeKind::Paragraph, NodeAttrs::empty(), inline(text))
                .unwrap()
        })
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(nodes.clone()),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), nodes)
}

fn inline(text: &str) -> NodeContent {
    NodeContent::Inline(if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    })
}

fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}

fn point(node: NodeId, raw: usize) -> InlinePoint {
    InlinePoint::new(node, offset(raw), 0, CursorAffinity::Before)
}

fn caret(node: NodeId, raw: usize) -> DocumentSelection {
    DocumentSelection::collapsed(point(node, raw))
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

fn children(document: &XiaomuDocument) -> &[NodeId] {
    document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()
}

fn transaction() -> Transaction {
    Transaction::new(TransactionOrigin::UserInput)
}

fn replace(node: NodeId, start: usize, end: usize, value: &str) -> TransactionStep {
    TransactionStep::ReplaceText {
        node,
        range: TextRange::new(offset(start), offset(end)).unwrap(),
        replacement: value.into(),
    }
}

fn append(document: &XiaomuDocument, value: &str) -> Transaction {
    transaction().with_step(TransactionStep::InsertNode {
        parent: document.root(),
        index: children(document).len(),
        kind: NodeKind::Paragraph,
        attrs: NodeAttrs::empty(),
        content: inline(value),
    })
}

fn finalized(
    transaction: Transaction,
    selection: Option<DocumentSelection>,
) -> Result<Option<EditFinalization>, PolicyError> {
    EditFinalization::new(transaction, selection)
        .map(Some)
        .map_err(|error| PolicyError::new(error.to_string()))
}

#[derive(Clone)]
struct Observation {
    before: XiaomuDocument,
    candidate: XiaomuDocument,
    original: DocumentSelection,
    selection: DocumentSelection,
    origin: DocumentChangeOrigin,
}

type Observations = Rc<RefCell<Vec<Observation>>>;
type Finalizer = dyn Fn(&Observation) -> Result<Option<EditFinalization>, PolicyError>;

struct Policy {
    observations: Observations,
    finalizer: Box<Finalizer>,
    reject: Option<fn(&XiaomuDocument) -> bool>,
    host_plan: Option<EditPlan>,
}

impl SessionPolicy for Policy {
    fn finalize_edit(
        &self,
        context: FinalizeEditContext<'_>,
    ) -> Result<Option<EditFinalization>, PolicyError> {
        context
            .original_selection()
            .validate(context.before_document())
            .unwrap();
        context
            .selection()
            .validate(context.candidate_document())
            .unwrap();
        let observation = Observation {
            before: context.before_document().clone(),
            candidate: context.candidate_document().clone(),
            original: context.original_selection(),
            selection: context.selection(),
            origin: context.change_origin(),
        };
        self.observations.borrow_mut().push(observation.clone());
        (self.finalizer)(&observation)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if self.reject.is_some_and(|reject| reject(document)) {
            Err(PolicyError::new("final candidate rejected"))
        } else {
            Ok(())
        }
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::TurnInto { .. })
            && let Some(plan) = &self.host_plan
        {
            return Ok(IntentDisposition::Apply(plan.clone()));
        }
        Ok(IntentDisposition::Continue)
    }

    fn prepare_text_cut(&self, _: SessionContext<'_>) -> Result<bool, PolicyError> {
        Ok(true)
    }

    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        _: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        Ok(Some(ClipboardExportSpec::new()))
    }
}

fn policy(
    finalizer: impl Fn(&Observation) -> Result<Option<EditFinalization>, PolicyError> + 'static,
) -> (Policy, Observations) {
    let observations = Rc::new(RefCell::new(Vec::new()));
    (
        Policy {
            observations: observations.clone(),
            finalizer: Box::new(finalizer),
            reject: None,
            host_plan: None,
        },
        observations,
    )
}

fn append_policy() -> (Policy, Observations) {
    policy(|observation| finalized(append(&observation.candidate, "finalized"), None))
}

fn insert(session: &mut DocumentSession, value: &str) -> Result<SessionOutcome, SessionError> {
    session.apply_intent(&EditIntent::InsertText { text: value.into() })
}

fn bold(session: &mut DocumentSession) {
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
}

struct Event {
    document: XiaomuDocument,
    selection: DocumentSelection,
    origin: DocumentChangeOrigin,
}

type Events = Rc<RefCell<Vec<Event>>>;
struct Listener(Events);

impl DocumentChangeListener for Listener {
    fn document_changed_with_origin(
        &mut self,
        document: &XiaomuDocument,
        selection: DocumentSelection,
        origin: DocumentChangeOrigin,
    ) {
        selection.validate(document).unwrap();
        self.0.borrow_mut().push(Event {
            document: document.clone(),
            selection,
            origin,
        });
    }

    fn selection_changed(&mut self, _: DocumentSelection) {
        panic!("document edits must not publish intermediate selection changes");
    }
}

fn listen(session: &mut DocumentSession) -> Events {
    let events = Rc::new(RefCell::new(Vec::new()));
    session.add_listener(Box::new(Listener(events.clone())));
    events
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    depths: (usize, usize),
    events: usize,
}

impl Snapshot {
    fn capture(session: &DocumentSession, events: &Events) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            depths: session.history_depths(),
            events: events.borrow().len(),
        }
    }

    fn assert_unchanged(&self, session: &DocumentSession, events: &Events) {
        assert_eq!(session.document().root(), self.document.root());
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().version(), self.document.version());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.depths);
        assert_eq!(events.borrow().len(), self.events);
    }
}

fn assert_round_trip(
    session: &mut DocumentSession,
    before: &XiaomuDocument,
    before_selection: DocumentSelection,
    observations: &Observations,
    events: &Events,
) {
    let after = session.document().clone();
    let after_selection = session.selection();
    let calls = observations.borrow().len();
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(events.borrow()[0].document.store(), after.store());
    assert_eq!(events.borrow()[0].selection, after_selection);
    assert_eq!(events.borrow()[0].origin, DocumentChangeOrigin::Local);
    assert_eq!(session.undo(), Ok(SessionOutcome::DocumentChanged));
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.document().root(), before.root());
    assert_eq!(session.selection(), before_selection);
    assert_eq!(session.history_depths(), (0, 1));
    assert_eq!(session.redo(), Ok(SessionOutcome::DocumentChanged));
    assert_eq!(session.document().store(), after.store());
    assert_eq!(session.document().root(), after.root());
    assert_eq!(session.selection(), after_selection);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(
        observations.borrow().len(),
        calls,
        "Undo/Redo must replay, not finalize"
    );
    assert_eq!(events.borrow().len(), 3);
}
