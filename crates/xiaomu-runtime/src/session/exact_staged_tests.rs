//! Private staged-commit regressions; hosts do not gain a staged public API.

use std::{cell::RefCell, rc::Rc};

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use crate::session::structure::StagedPlan;
use crate::session::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, PolicyError,
    SelectionUpdate, SessionError, SessionOutcome, SessionPolicy,
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

fn add_break(node: NodeId, raw: usize, ordinal: usize) -> TransactionStep {
    TransactionStep::InsertInlineAtom {
        at: point(node, raw, ordinal, CursorAffinity::Before),
        kind: AtomKind::hard_break(),
        attrs: NodeAttrs::empty(),
        content: InlineAtomContent::hard_break(),
    }
}

fn replace(node: NodeId, start: usize, end: usize, text: &str) -> TransactionStep {
    TransactionStep::ReplaceText {
        node,
        range: range(start, end),
        replacement: text.into(),
    }
}

// Enter the real staged commit with the same transient rollback and history
// boundary used by intent dispatch. Only tests can construct this private plan.
fn apply_staged(
    session: &mut DocumentSession,
    plan: StagedPlan,
) -> Result<SessionOutcome, SessionError> {
    session.with_transient_rollback(|session| {
        session.history.break_group();
        session.clear_stored_marks();
        session.commit_staged(plan)
    })
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

// Both committed representations are valid; the hidden zero/one-break
// intermediate snapshots deliberately fail this stable host rule.
struct ConversionPolicy;

impl SessionPolicy for ConversionPolicy {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        for node in document.store().iter() {
            let Some(inline) = node.content().as_inline() else {
                continue;
            };
            let text: String = inline
                .runs()
                .iter()
                .map(|run| run.text().as_str())
                .collect();
            if matches!(node.kind(), NodeKind::CodeBlock)
                && text == "a\n\n🙂z"
                && inline.atoms().is_empty()
                || matches!(node.kind(), NodeKind::Paragraph)
                    && text == "a🙂z"
                    && inline.atom_count_at(offset(1)) == 2
            {
                continue;
            }
            return Err(PolicyError::new("intermediate conversion is not canonical"));
        }
        Ok(())
    }
}

fn assert_round_trip(
    document: XiaomuDocument,
    before: DocumentSelection,
    after: DocumentSelection,
    plan: StagedPlan,
) {
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(ConversionPolicy))
            .unwrap();
    let events = listen(&mut session);
    assert_eq!(
        apply_staged(&mut session, plan),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(session.selection(), after);
    assert_eq!(session.history_depths(), (1, 0));
    let committed = session.document().clone();
    assert_eq!(
        committed.revision().as_u64(),
        document.revision().as_u64() + 3
    );
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
}

#[test]
fn staged_exact_lf_to_atoms_waits_for_final_ordinals_and_round_trips_both_directions() {
    for backward in [false, true] {
        let (document, node) = fixture(NodeKind::CodeBlock, "a\n\n🙂z");
        let before = selection(
            point(node, 3, 0, CursorAffinity::After),
            point(node, 7, 0, CursorAffinity::Before),
            backward,
        );
        let after = selection(
            point(node, 1, 2, CursorAffinity::After),
            point(node, 5, 0, CursorAffinity::Before),
            backward,
        );
        let plan = StagedPlan::new(SelectionUpdate::Exact { selection: after })
            .stage(move |document| {
                assert!(after.validate(document).is_err());
                Ok(transaction().with_step(replace(node, 1, 3, "")).with_step(
                    TransactionStep::SetNodeKind {
                        node,
                        kind: NodeKind::Paragraph,
                    },
                ))
            })
            .stage(move |document| {
                assert!(after.validate(document).is_err());
                Ok(transaction().with_step(add_break(node, 1, 0)))
            })
            .stage(move |document| {
                assert!(after.validate(document).is_err());
                Ok(transaction().with_step(add_break(node, 1, 1)))
            });
        assert_round_trip(document, before, after, plan);
    }
}

#[test]
fn staged_exact_atoms_to_lf_waits_for_final_utf8_boundaries_and_round_trips_both_directions() {
    for backward in [false, true] {
        let (document, node) = fixture(NodeKind::Paragraph, "a🙂z");
        let document = transaction()
            .with_step(add_break(node, 1, 0))
            .with_step(add_break(node, 1, 1))
            .apply(&document)
            .unwrap();
        let before = selection(
            point(node, 1, 2, CursorAffinity::After),
            point(node, 5, 0, CursorAffinity::Before),
            backward,
        );
        let after = selection(
            point(node, 3, 0, CursorAffinity::After),
            point(node, 7, 0, CursorAffinity::Before),
            backward,
        );
        let plan = StagedPlan::new(SelectionUpdate::Exact { selection: after })
            .stage(move |document| {
                assert!(after.validate(document).is_err());
                let mut remove = transaction();
                for placement in document
                    .node(node)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .atoms()
                {
                    remove.push_step(TransactionStep::RemoveInlineAtom {
                        atom: placement.atom(),
                    });
                }
                remove.push_step(TransactionStep::SetNodeKind {
                    node,
                    kind: NodeKind::CodeBlock,
                });
                Ok(remove)
            })
            .stage(move |document| {
                assert!(after.validate(document).is_err());
                Ok(transaction().with_step(replace(node, 1, 1, "\n")))
            })
            .stage(move |document| {
                assert!(after.validate(document).is_err());
                Ok(transaction().with_step(replace(node, 2, 2, "\n")))
            });
        assert_round_trip(document, before, after, plan);
    }
}

#[derive(Clone, Copy)]
enum InvalidFinal {
    RemovedNode,
    Utf8Interior,
    OutOfBounds,
    AtomOrdinal,
}

fn invalid_final_plan(node: NodeId, invalid: InvalidFinal) -> StagedPlan {
    let (selected, first, last) = match invalid {
        InvalidFinal::RemovedNode => (
            DocumentSelection::new(
                InlinePoint::at_start_of(node),
                point(node, 1, 0, CursorAffinity::After),
            ),
            transaction().with_step(replace(node, 0, 0, "Q")),
            transaction().with_step(TransactionStep::RemoveNode { node }),
        ),
        InvalidFinal::Utf8Interior => (
            DocumentSelection::new(
                InlinePoint::at_start_of(node),
                point(node, 1, 0, CursorAffinity::After),
            ),
            transaction().with_step(replace(node, 0, 0, "Q")),
            transaction().with_step(replace(node, 0, 1, "🙂")),
        ),
        InvalidFinal::OutOfBounds => (
            DocumentSelection::new(
                InlinePoint::at_start_of(node),
                point(node, 4, 0, CursorAffinity::After),
            ),
            transaction().with_step(replace(node, 0, 0, "long")),
            transaction().with_step(replace(node, 0, 4, "")),
        ),
        InvalidFinal::AtomOrdinal => (
            DocumentSelection::new(
                InlinePoint::at_start_of(node),
                point(node, 0, 1, CursorAffinity::After),
            ),
            transaction().with_step(add_break(node, 0, 0)),
            transaction().with_step(TransactionStep::ReplaceInlineText {
                at: InlinePoint::at_start_of(node),
                end: TextOffset::ZERO,
                replacement: "Q".into(),
            }),
        ),
    };
    StagedPlan::new(SelectionUpdate::Exact {
        selection: selected,
    })
    .stage(move |_| Ok(first))
    .stage(move |document| {
        // This proves rejection belongs to the final commit snapshot,
        // rather than a plan that was already invalid at every stage.
        selected.validate(document).unwrap();
        Ok(last)
    })
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    depths: (usize, usize),
    group_open: bool,
    events: Vec<Event>,
}

impl Snapshot {
    fn capture(session: &DocumentSession, events: &Events) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            depths: session.history_depths(),
            group_open: session.history.typing_group_open(),
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
        assert_eq!(session.history.typing_group_open(), self.group_open);
        assert_eq!(*events.borrow(), self.events);
    }
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

#[test]
fn staged_exact_final_invalid_range_rolls_back_all_state_typing_group_and_redo() {
    for invalid in [
        InvalidFinal::RemovedNode,
        InvalidFinal::Utf8Interior,
        InvalidFinal::OutOfBounds,
        InvalidFinal::AtomOrdinal,
    ] {
        let (document, node) = fixture(NodeKind::Paragraph, "");
        let initial = DocumentSelection::collapsed(InlinePoint::at_start_of(node));
        let mut session = DocumentSession::new(document.clone(), initial).unwrap();
        let events = listen(&mut session);
        bold(&mut session);
        insert(&mut session, "a");
        let before = Snapshot::capture(&session, &events);
        assert!(before.group_open);
        assert_eq!(
            apply_staged(&mut session, invalid_final_plan(node, invalid)),
            Err(SessionError::SelectionInvalid),
        );
        before.assert_unchanged(&session, &events);
        insert(&mut session, "b");
        assert_eq!(session.history_depths(), (1, 0));
        let typed = session.document().clone();
        let typed_selection = session.selection();
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
        assert_eq!(session.selection(), initial);
        assert_eq!(session.history_depths(), (0, 1));

        bold(&mut session);
        let before_redo = Snapshot::capture(&session, &events);
        assert_eq!(
            apply_staged(&mut session, invalid_final_plan(node, invalid)),
            Err(SessionError::SelectionInvalid),
        );
        before_redo.assert_unchanged(&session, &events);
        session.redo().unwrap();
        assert_eq!(session.document().store(), typed.store());
        assert_eq!(session.selection(), typed_selection);
        assert_eq!(session.history_depths(), (1, 0));
    }
}
