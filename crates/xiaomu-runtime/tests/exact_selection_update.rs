//! Host-planned exact selections are validated only against the final snapshot.
//! Every assertion exercises the public intent/commit path, including rollback.

use std::{cell::RefCell, rc::Rc};

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, EditPlan,
    IntentDisposition, PolicyError, SelectionUpdate, SessionContext, SessionError, SessionOutcome,
    SessionPolicy,
};

fn fixture(kind: NodeKind, text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
    let node = builder
        .insert(kind, NodeAttrs::empty(), NodeContent::Inline(inline))
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}

// Offsets are deliberately obtained from a different valid buffer. The commit
// must check their bounds and UTF-8 boundaries against its own final snapshot.
fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}

fn point(node: NodeId, raw: usize, ordinal: usize, affinity: CursorAffinity) -> InlinePoint {
    InlinePoint::new(node, offset(raw), ordinal, affinity)
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(offset(start), offset(end)).unwrap()
}

fn selection(start: InlinePoint, end: InlinePoint, backward: bool) -> DocumentSelection {
    if backward {
        DocumentSelection::new(end, start)
    } else {
        DocumentSelection::new(start, end)
    }
}

fn transaction() -> Transaction {
    Transaction::new(TransactionOrigin::UserInput)
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

#[derive(Clone)]
struct HostPlan {
    plan: EditPlan,
    reject_code: bool,
}

impl HostPlan {
    fn new(plan: EditPlan) -> Self {
        Self {
            plan,
            reject_code: false,
        }
    }
}

impl SessionPolicy for HostPlan {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::TurnInto { .. }) {
            Ok(IntentDisposition::Apply(self.plan.clone()))
        } else {
            Ok(IntentDisposition::Continue)
        }
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if self.reject_code
            && document
                .store()
                .iter()
                .any(|node| matches!(node.kind(), NodeKind::CodeBlock))
        {
            return Err(PolicyError::new("code snapshot rejected"));
        }
        Ok(())
    }
}

fn exact(transaction: Transaction, selection: DocumentSelection) -> EditPlan {
    EditPlan::new(transaction, SelectionUpdate::Exact { selection }, None)
}

fn apply_host(session: &mut DocumentSession) -> Result<SessionOutcome, SessionError> {
    session.apply_intent(&EditIntent::TurnInto {
        kind: NodeKind::CodeBlock,
    })
}

fn insert(session: &mut DocumentSession, text: &str) {
    session
        .apply_intent(&EditIntent::InsertText { text: text.into() })
        .unwrap();
}

fn bold(session: &mut DocumentSession) {
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
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
        // No observer may see a selection belonging to an intermediate state.
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

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    depths: (usize, usize),
    events: Vec<Event>,
}

impl Snapshot {
    fn capture(session: &DocumentSession, events: &Events) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            depths: session.history_depths(),
            events: events.borrow().clone(),
        }
    }

    fn assert_unchanged(&self, session: &DocumentSession, events: &Events) {
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().root(), self.document.root());
        assert_eq!(session.document().version(), self.document.version());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.depths);
        assert_eq!(*events.borrow(), self.events);
    }
}

fn add_break(node: NodeId, ordinal: usize) -> TransactionStep {
    TransactionStep::InsertInlineAtom {
        at: point(node, 1, ordinal, CursorAffinity::Before),
        kind: AtomKind::hard_break(),
        attrs: NodeAttrs::empty(),
        content: InlineAtomContent::hard_break(),
    }
}

fn lf_to_atoms(node: NodeId) -> Transaction {
    transaction()
        .with_step(TransactionStep::ReplaceText {
            node,
            range: range(1, 3),
            replacement: String::new(),
        })
        .with_step(TransactionStep::SetNodeKind {
            node,
            kind: NodeKind::Paragraph,
        })
        .with_step(add_break(node, 0))
        .with_step(add_break(node, 1))
}

fn assert_round_trip(
    document: XiaomuDocument,
    before: DocumentSelection,
    plan: EditPlan,
    after: DocumentSelection,
) -> DocumentSession {
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(HostPlan::new(plan)))
            .unwrap();
    let events = listen(&mut session);
    assert_eq!(
        apply_host(&mut session),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(session.selection(), after);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(session.stored_marks(), None);
    let committed = session.document().clone();
    assert_eq!(
        *events.borrow(),
        [Event::Document(committed.revision().as_u64(), after)]
    );

    assert_eq!(session.undo(), Ok(SessionOutcome::DocumentChanged));
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().root(), document.root());
    assert_eq!(session.selection(), before);
    assert_eq!(session.history_depths(), (0, 1));
    let undo_revision = session.document().revision().as_u64();

    assert_eq!(session.redo(), Ok(SessionOutcome::DocumentChanged));
    assert_eq!(session.document().store(), committed.store());
    assert_eq!(session.document().root(), committed.root());
    assert_eq!(session.selection(), after);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(
        *events.borrow(),
        [
            Event::Document(committed.revision().as_u64(), after),
            Event::Document(undo_revision, before),
            Event::Document(session.document().revision().as_u64(), after),
        ]
    );
    session
}

#[test]
fn exact_lf_to_hard_break_range_preserves_direction_affinity_and_atom_ordinal() {
    for backward in [false, true] {
        let (document, node) = fixture(NodeKind::CodeBlock, "a\n\n🙂z");
        let before = selection(
            point(node, 2, 0, CursorAffinity::After),
            point(node, 7, 0, CursorAffinity::Before),
            backward,
        );
        let after = selection(
            point(node, 1, 1, CursorAffinity::After),
            point(node, 5, 0, CursorAffinity::Before),
            backward,
        );
        assert!(after.validate(&document).is_err());
        let session = assert_round_trip(document, before, exact(lf_to_atoms(node), after), after);
        assert_eq!(text(session.document(), node), "a🙂z");
        assert_eq!(
            session.document().node(node).unwrap().kind(),
            &NodeKind::Paragraph
        );
        assert_eq!(
            session
                .document()
                .node(node)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .atom_count_at(offset(1)),
            2
        );
    }
}

#[test]
fn exact_hard_break_to_lf_range_accepts_final_only_byte_offsets_in_both_directions() {
    for backward in [false, true] {
        let (code, node) = fixture(NodeKind::CodeBlock, "a\n\n🙂z");
        let document = lf_to_atoms(node).apply(&code).unwrap();
        let before = selection(
            point(node, 1, 1, CursorAffinity::After),
            point(node, 5, 0, CursorAffinity::Before),
            backward,
        );
        let after = selection(
            point(node, 2, 0, CursorAffinity::After),
            point(node, 7, 0, CursorAffinity::Before),
            backward,
        );
        assert!(after.validate(&document).is_err());
        let mut convert = transaction();
        for atom in document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms()
        {
            convert.push_step(TransactionStep::RemoveInlineAtom { atom: atom.atom() });
        }
        convert.push_step(TransactionStep::ReplaceText {
            node,
            range: range(1, 1),
            replacement: "\n\n".into(),
        });
        convert.push_step(TransactionStep::SetNodeKind {
            node,
            kind: NodeKind::CodeBlock,
        });
        let session = assert_round_trip(document, before, exact(convert, after), after);
        assert_eq!(text(session.document(), node), "a\n\n🙂z");
        assert_eq!(
            session.document().node(node).unwrap().kind(),
            &NodeKind::CodeBlock
        );
        assert!(
            session
                .document()
                .node(node)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .atoms()
                .is_empty()
        );
    }
}

// Exercise each rejected plan with both an open typing group and a populated
// redo stack. State equality alone cannot prove the next typing edit coalesces
// or that the original redo transaction remains replayable.
fn assert_rejection_preserves_typing_and_redo(
    make_policy: impl FnOnce(NodeId) -> HostPlan,
    error: SessionError,
) {
    let (document, node) = fixture(NodeKind::Paragraph, "");
    let initial = DocumentSelection::collapsed(InlinePoint::at_start_of(node));
    let mut session =
        DocumentSession::new_with_policy(document.clone(), initial, Box::new(make_policy(node)))
            .unwrap();
    let events = listen(&mut session);
    bold(&mut session);
    insert(&mut session, "a");
    let before = Snapshot::capture(&session, &events);
    assert_eq!(apply_host(&mut session), Err(error.clone()));
    before.assert_unchanged(&session, &events);

    insert(&mut session, "b");
    assert_eq!(text(session.document(), node), "ab");
    assert_eq!(session.history_depths(), (1, 0));
    let typed = session.document().clone();
    let typed_selection = session.selection();
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), initial);
    assert_eq!(session.history_depths(), (0, 1));

    bold(&mut session);
    let before_redo = Snapshot::capture(&session, &events);
    assert_eq!(apply_host(&mut session), Err(error));
    before_redo.assert_unchanged(&session, &events);
    session.redo().unwrap();
    assert_eq!(session.document().store(), typed.store());
    assert_eq!(session.selection(), typed_selection);
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn exact_selection_on_a_removed_node_is_rejected_without_publishing() {
    assert_rejection_preserves_typing_and_redo(
        |node| {
            HostPlan::new(exact(
                transaction().with_step(TransactionStep::RemoveNode { node }),
                DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
            ))
        },
        SessionError::SelectionInvalid,
    );
}

#[test]
fn exact_out_of_bounds_endpoint_is_rejected_without_publishing() {
    assert_rejection_preserves_typing_and_redo(
        |node| {
            HostPlan::new(exact(
                transaction().with_step(TransactionStep::SetNodeKind {
                    node,
                    kind: NodeKind::CodeBlock,
                }),
                DocumentSelection::new(
                    InlinePoint::at_start_of(node),
                    point(node, 64, 0, CursorAffinity::Before),
                ),
            ))
        },
        SessionError::SelectionInvalid,
    );
}

#[test]
fn exact_endpoint_inside_final_utf8_scalar_is_rejected_without_publishing() {
    assert_rejection_preserves_typing_and_redo(
        |node| {
            HostPlan::new(exact(
                transaction().with_step(TransactionStep::ReplaceText {
                    node,
                    range: range(0, 0),
                    replacement: "🙂".into(),
                }),
                DocumentSelection::new(
                    point(node, 1, 0, CursorAffinity::After),
                    point(node, 4, 0, CursorAffinity::Before),
                ),
            ))
        },
        SessionError::SelectionInvalid,
    );
}

#[test]
fn exact_ordinal_past_final_atom_count_is_rejected_without_publishing() {
    assert_rejection_preserves_typing_and_redo(
        |node| {
            HostPlan::new(exact(
                transaction().with_step(TransactionStep::InsertInlineAtom {
                    at: InlinePoint::at_start_of(node),
                    kind: AtomKind::hard_break(),
                    attrs: NodeAttrs::empty(),
                    content: InlineAtomContent::hard_break(),
                }),
                DocumentSelection::new(
                    point(node, 0, 0, CursorAffinity::Before),
                    point(node, 0, 2, CursorAffinity::After),
                ),
            ))
        },
        SessionError::SelectionInvalid,
    );
}

#[test]
fn exact_ordinal_valid_in_an_intermediate_step_is_rejected_when_invalid_at_commit() {
    assert_rejection_preserves_typing_and_redo(
        |node| {
            HostPlan::new(exact(
                transaction()
                    .with_step(TransactionStep::InsertInlineAtom {
                        at: InlinePoint::at_start_of(node),
                        kind: AtomKind::hard_break(),
                        attrs: NodeAttrs::empty(),
                        content: InlineAtomContent::hard_break(),
                    })
                    // The new atom moves from byte zero to byte one. Ordinal
                    // one at byte zero was valid only before this final step.
                    .with_step(TransactionStep::ReplaceInlineText {
                        at: InlinePoint::at_start_of(node),
                        end: TextOffset::ZERO,
                        replacement: "Q".into(),
                    }),
                DocumentSelection::new(
                    point(node, 0, 0, CursorAffinity::Before),
                    point(node, 0, 1, CursorAffinity::After),
                ),
            ))
        },
        SessionError::SelectionInvalid,
    );
}

#[test]
fn exact_range_with_explicit_stored_marks_is_rejected_even_when_marks_restore_inheritance() {
    for marks in [
        None,
        Some(MarkSet::empty()),
        Some(MarkSet::new([Mark::Italic]).unwrap()),
    ] {
        assert_rejection_preserves_typing_and_redo(
            |node| {
                HostPlan::new(
                    exact(
                        transaction().with_step(TransactionStep::ReplaceText {
                            node,
                            range: range(0, 0),
                            replacement: "Q".into(),
                        }),
                        DocumentSelection::new(
                            InlinePoint::at_start_of(node),
                            point(node, 1, 0, CursorAffinity::Before),
                        ),
                    )
                    .with_stored_marks(marks),
                )
            },
            SessionError::SelectionInvalid,
        );
    }
}

#[test]
fn exact_selection_does_not_bypass_final_host_document_validation() {
    assert_rejection_preserves_typing_and_redo(
        |node| HostPlan {
            plan: exact(
                transaction().with_step(TransactionStep::SetNodeKind {
                    node,
                    kind: NodeKind::CodeBlock,
                }),
                DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
            ),
            reject_code: true,
        },
        SessionError::Policy(PolicyError::new("code snapshot rejected")),
    );
}

#[test]
fn map_existing_still_rejects_deleted_endpoints_instead_of_using_exact_fallback() {
    assert_rejection_preserves_typing_and_redo(
        |node| {
            HostPlan::new(EditPlan::new(
                transaction().with_step(TransactionStep::RemoveNode { node }),
                SelectionUpdate::MapExisting,
                None,
            ))
        },
        SessionError::SelectionDeleted,
    );
}

#[test]
fn collapsed_exact_selection_can_install_explicit_marks_for_subsequent_typing() {
    let (document, node) = fixture(NodeKind::Paragraph, "ab");
    let before = DocumentSelection::collapsed(InlinePoint::at_start_of(node));
    let after = DocumentSelection::collapsed(point(node, 5, 0, CursorAffinity::After));
    assert!(after.validate(&document).is_err());
    let marks = MarkSet::new([Mark::Italic]).unwrap();
    let plan = exact(
        transaction().with_step(TransactionStep::ReplaceText {
            node,
            range: range(0, 0),
            replacement: "🙂".into(),
        }),
        after,
    )
    .with_stored_marks(Some(marks.clone()));
    let mut session =
        DocumentSession::new_with_policy(document, before, Box::new(HostPlan::new(plan))).unwrap();
    assert_eq!(
        apply_host(&mut session),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(session.selection(), after);
    assert_eq!(session.stored_marks(), Some(&marks));
    insert(&mut session, "X");
    assert_eq!(text(session.document(), node), "🙂aXb");
    let inline = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    assert!(
        inline
            .runs()
            .iter()
            .any(|run| run.text().as_str() == "X" && run.marks() == &marks)
    );
    assert_eq!(session.history_depths(), (2, 0));
    session.undo().unwrap();
    assert_eq!(session.selection(), after);
    session.undo().unwrap();
    assert_eq!(session.selection(), before);
}
