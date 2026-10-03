//! Generic host policy contracts, with no downstream schema dependency.

use std::{cell::Cell, rc::Rc};

use xiaomu_core::document::{
    InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::text::{TextBuffer, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::ClipboardSlice;
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, EditPlan,
    IntentDisposition, PolicyError, PrimaryEdit, SelectionUpdate, SessionContext, SessionError,
    SessionOutcome, SessionPolicy,
};

fn fixture(text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
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

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let offset = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(raw)
        .unwrap();
    TextPoint::new(node, offset, CursorAffinity::Before)
}

fn session(policy: impl SessionPolicy + 'static) -> (DocumentSession, NodeId) {
    let (document, node) = fixture("");
    let selection = DocumentSelection::collapsed(point(&document, node, 0));
    (
        DocumentSession::new_with_policy(document, selection, Box::new(policy)).unwrap(),
        node,
    )
}

fn insert(session: &mut DocumentSession, text: &str) {
    session
        .apply_intent(&EditIntent::InsertText { text: text.into() })
        .unwrap();
}

fn toggle(session: &mut DocumentSession, mark: Mark) {
    session
        .apply_intent(&EditIntent::ToggleMark { mark })
        .unwrap();
}

fn text(document: &XiaomuDocument) -> String {
    document
        .store()
        .iter()
        .filter_map(|node| node.content().as_inline())
        .flat_map(|inline| inline.runs())
        .map(|run| run.text().as_str())
        .collect()
}

fn slice(text: &str) -> ClipboardSlice {
    let (document, node) = fixture(text);
    let selection = DocumentSelection::new(
        point(&document, node, 0),
        point(&document, node, text.len()),
    );
    DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

#[derive(Default)]
struct Rules {
    replacement_mark: Option<Mark>,
    refuse_lists: bool,
    reject_snapshot: Option<fn(&XiaomuDocument) -> bool>,
}

impl SessionPolicy for Rules {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if let EditIntent::PasteSlice { slice } = intent
            && slice.plain_text() == "blocked"
        {
            // Preflight sees all original typing state, including after
            // a prior successful typing command opened a history group.
            assert!(context.stored_marks().unwrap().contains(MarkKind::Bold));
            assert_eq!(text(context.document()), "a");
            assert!(context.selection().is_collapsed());
            return Err(PolicyError::new("blocked paste"));
        }
        if let EditIntent::TurnInto {
            kind: NodeKind::CodeBlock,
        } = intent
        {
            return Ok(IntentDisposition::NoChange);
        }
        if let EditIntent::ToggleMark { .. } = intent
            && let Some(mark) = &self.replacement_mark
        {
            if context.selection().is_collapsed() {
                return Ok(IntentDisposition::StoredMarks(Some(
                    MarkSet::new([mark.clone()]).unwrap(),
                )));
            }
            let selection = context.selection().as_single_node().unwrap();
            let range =
                TextRange::new(selection.anchor().offset(), selection.focus().offset()).unwrap();
            let transaction = Transaction::new(TransactionOrigin::UserInput)
                .with_step(TransactionStep::RemoveMark {
                    node: selection.focus().node_id(),
                    range,
                    mark_kind: MarkKind::Bold,
                })
                .with_step(TransactionStep::AddMark {
                    node: selection.focus().node_id(),
                    range,
                    mark: mark.clone(),
                });
            return Ok(IntentDisposition::Apply(EditPlan::new(
                transaction,
                SelectionUpdate::MapExisting,
                None,
            )));
        }
        Ok(IntentDisposition::Continue)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if text(document).contains('!')
            || self.refuse_lists
                && document
                    .store()
                    .iter()
                    .any(|node| matches!(node.kind(), NodeKind::BulletList))
            || self.reject_snapshot.is_some_and(|reject| reject(document))
        {
            return Err(PolicyError::new("candidate rejected"));
        }
        Ok(())
    }
}

type Counts = Rc<Cell<(usize, usize)>>;

struct Listener(Counts);

impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents + 1, selections));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents, selections + 1));
    }
}

fn listen(session: &mut DocumentSession) -> Counts {
    let counts = Rc::new(Cell::new((0, 0)));
    session.add_listener(Box::new(Listener(counts.clone())));
    counts
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    depths: (usize, usize),
    counts: (usize, usize),
}

impl Snapshot {
    fn capture(session: &DocumentSession, counts: &Counts) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            depths: session.history_depths(),
            counts: counts.get(),
        }
    }
    fn assert_unchanged(&self, session: &DocumentSession, counts: &Counts) {
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.document().root(), self.document.root());
        assert_eq!(session.document().version(), self.document.version());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.depths);
        assert_eq!(counts.get(), self.counts);
    }
}

fn assert_policy_error(result: Result<SessionOutcome, SessionError>) {
    assert!(matches!(result, Err(SessionError::Policy(_))), "{result:?}");
}

#[test]
fn initial_document_is_validated_but_plain_sessions_remain_generic() {
    let (document, node) = fixture("!");
    let selection = DocumentSelection::collapsed(point(&document, node, 0));
    assert!(
        matches!(DocumentSession::new_with_policy(document.clone(), selection, Box::new(Rules::default())), Err(SessionError::Policy(error)) if error.message() == "candidate rejected")
    );
    assert!(DocumentSession::new(document, selection).is_ok());
}

#[test]
fn collapsed_override_sets_explicit_marks_without_document_or_listener_changes() {
    let (mut session, _) = session(Rules {
        replacement_mark: Some(Mark::Italic),
        ..Rules::default()
    });
    let counts = listen(&mut session);
    let original = session.document().clone();
    toggle(&mut session, Mark::Bold);
    assert_eq!(
        session.stored_marks(),
        Some(&MarkSet::new([Mark::Italic]).unwrap())
    );
    assert_eq!(session.document().store(), original.store());
    assert_eq!(session.document().revision(), original.revision());
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(counts.get(), (0, 0));
    insert(&mut session, "中");
    assert!(
        session
            .document()
            .store()
            .iter()
            .filter_map(|node| node.content().as_inline())
            .flat_map(|inline| inline.runs())
            .all(|run| run.marks().contains(MarkKind::Italic))
    );
}

#[test]
fn range_override_is_one_transaction_one_undo_and_keeps_selection() {
    let (mut session, node) = session(Rules {
        replacement_mark: Some(Mark::Italic),
        ..Rules::default()
    });
    insert(&mut session, "text");
    let start = point(session.document(), node, 0);
    let end = point(session.document(), node, 4);
    session
        .apply_intent(&EditIntent::SetSelection {
            anchor: start,
            focus: end,
        })
        .unwrap();
    let before = session.document().clone();
    let selection = session.selection();
    let counts = listen(&mut session);
    toggle(&mut session, Mark::Bold);
    let after = session.document().clone();
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (2, 0));
    assert_eq!(counts.get(), (1, 0));
    assert!(
        after
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .marks()
            .contains(MarkKind::Italic)
    );
    session.undo().unwrap();
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.selection(), selection);
    session.redo().unwrap();
    assert_eq!(session.document().store(), after.store());
    assert_eq!(session.selection(), selection);
}

#[test]
fn policy_no_change_preserves_marks_and_an_open_typing_group() {
    let (mut session, _) = session(Rules::default());
    toggle(&mut session, Mark::Bold);
    insert(&mut session, "a");
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_eq!(
        session
            .apply_intent(&EditIntent::TurnInto {
                kind: NodeKind::CodeBlock
            })
            .unwrap(),
        SessionOutcome::NoChange
    );
    before.assert_unchanged(&session, &counts);
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn paste_preflight_rejection_happens_before_marks_or_group_are_cleared() {
    let (mut session, _) = session(Rules::default());
    toggle(&mut session, Mark::Bold);
    insert(&mut session, "a");
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_policy_error(session.apply_intent(&EditIntent::PasteSlice {
        slice: slice("blocked"),
    }));
    before.assert_unchanged(&session, &counts);
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(text(session.document()), "");
}

#[test]
fn paste_candidate_rejection_restores_marks_and_open_group() {
    let (mut session, _) = session(Rules::default());
    toggle(&mut session, Mark::Bold);
    insert(&mut session, "a");
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_policy_error(session.apply_intent(&EditIntent::PasteSlice { slice: slice("!") }));
    before.assert_unchanged(&session, &counts);
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn staged_candidate_rejection_never_publishes_intermediate_list() {
    let (mut session, _) = session(Rules {
        refuse_lists: true,
        ..Rules::default()
    });
    toggle(&mut session, Mark::Bold);
    insert(&mut session, "a");
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_policy_error(session.apply_intent(&EditIntent::TurnInto {
        kind: NodeKind::BulletList,
    }));
    before.assert_unchanged(&session, &counts);
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn raw_candidate_rejection_restores_transient_state() {
    let (mut session, node) = session(Rules::default());
    toggle(&mut session, Mark::Bold);
    insert(&mut session, "a");
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    let at = point(session.document(), node, 1).offset();
    let transaction =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::ReplaceText {
            node,
            range: TextRange::new(at, at).unwrap(),
            replacement: "!".into(),
        });
    assert_policy_error(session.apply(&transaction));
    before.assert_unchanged(&session, &counts);
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn failed_undo_restores_entry_marks_selection_and_typing_group() {
    // An intentionally revision-sensitive test rule exercises the defensive
    // validator during inverse replay without changing policy configuration.
    let (mut session, _) = session(Rules {
        reject_snapshot: Some(|doc| doc.revision().as_u64() > 0 && text(doc).is_empty()),
        ..Rules::default()
    });
    toggle(&mut session, Mark::Bold);
    insert(&mut session, "a");
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_policy_error(session.undo());
    before.assert_unchanged(&session, &counts);
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn failed_redo_restores_entry_marks_selection_and_revision() {
    let (mut session, _) = session(Rules {
        reject_snapshot: Some(|doc| doc.revision().as_u64() == 3),
        ..Rules::default()
    });
    insert(&mut session, "a");
    session.undo().unwrap();
    toggle(&mut session, Mark::Bold);
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_policy_error(session.redo());
    before.assert_unchanged(&session, &counts);
    assert_policy_error(session.redo());
    before.assert_unchanged(&session, &counts);
}

#[test]
fn no_policy_core_failure_also_restores_marks_and_group() {
    let (document, node) = fixture("");
    let selection = DocumentSelection::collapsed(point(&document, node, 0));
    let mut session = DocumentSession::new(document, selection).unwrap();
    toggle(&mut session, Mark::Bold);
    insert(&mut session, "a");
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert!(
        session
            .apply_intent(&EditIntent::TurnInto {
                kind: NodeKind::Quote
            })
            .is_err()
    );
    before.assert_unchanged(&session, &counts);
    assert!(
        session
            .apply(
                &Transaction::new(TransactionOrigin::System)
                    .with_step(TransactionStep::RemoveNode { node })
            )
            .is_err()
    );
    before.assert_unchanged(&session, &counts);
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn policies_and_default_session_are_isolated() {
    let (mut a, _) = session(Rules {
        replacement_mark: Some(Mark::Italic),
        ..Rules::default()
    });
    let (mut b, _) = session(Rules {
        replacement_mark: Some(Mark::Strike),
        ..Rules::default()
    });
    let (doc, node) = fixture("");
    let selection = DocumentSelection::collapsed(point(&doc, node, 0));
    let mut plain = DocumentSession::new(doc, selection).unwrap();
    for session in [&mut a, &mut b, &mut plain] {
        toggle(session, Mark::Bold);
    }
    assert_eq!(
        a.stored_marks(),
        Some(&MarkSet::new([Mark::Italic]).unwrap())
    );
    assert_eq!(
        b.stored_marks(),
        Some(&MarkSet::new([Mark::Strike]).unwrap())
    );
    assert_eq!(
        plain.stored_marks(),
        Some(&MarkSet::new([Mark::Bold]).unwrap())
    );
    insert(&mut plain, "!");
    assert_policy_error(a.apply_intent(&EditIntent::InsertText { text: "!".into() }));
    assert_eq!(b.history_depths(), (0, 0));
    assert_eq!(text(b.document()), "");
}

#[test]
fn invalid_primary_edit_overflow_is_a_rejection_not_a_panic() {
    struct InvalidPlan;
    impl SessionPolicy for InvalidPlan {
        fn prepare_intent(
            &self,
            context: SessionContext<'_>,
            _: &EditIntent,
        ) -> Result<IntentDisposition, PolicyError> {
            let point = context.selection().as_single_node().unwrap().focus();
            let range = TextRange::new(point.offset(), point.offset()).unwrap();
            Ok(IntentDisposition::Apply(EditPlan::new(
                Transaction::new(TransactionOrigin::UserInput),
                SelectionUpdate::CaretAfterReplacement,
                Some(PrimaryEdit::new(point.node_id(), range, usize::MAX)),
            )))
        }
    }
    let (doc, node) = fixture("a");
    let selection = DocumentSelection::collapsed(point(&doc, node, 1));
    let mut session =
        DocumentSession::new_with_policy(doc, selection, Box::new(InvalidPlan)).unwrap();
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_eq!(
        session.apply_intent(&EditIntent::Delete),
        Err(SessionError::SelectionInvalid)
    );
    before.assert_unchanged(&session, &counts);
}

fn table_session(policy: Rules) -> (DocumentSession, NodeId) {
    let (doc, node) = fixture("");
    let selection = DocumentSelection::collapsed(point(&doc, node, 0));
    let doc = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::InsertTable {
            parent: doc.root(),
            index: 1,
            rows: 1,
            columns: 1,
        })
        .apply(&doc)
        .unwrap();
    let cell = doc
        .store()
        .iter()
        .find(|node| matches!(node.kind(), NodeKind::TableCell))
        .unwrap()
        .id();
    let mut session = DocumentSession::new_with_policy(doc, selection, Box::new(policy)).unwrap();
    session.set_cell_range_selection(cell, cell).unwrap();
    (session, cell)
}

#[test]
fn failed_cell_range_navigation_does_not_publish_its_tentative_collapse() {
    let (mut session, _) = table_session(Rules::default());
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    let stale = TextBuffer::from_string("long".into()).offset_at(4).unwrap();
    assert!(
        session
            .apply_intent(&EditIntent::PlaceCaret {
                offset: stale,
                extend_selection: false
            })
            .is_err()
    );
    before.assert_unchanged(&session, &counts);
}

#[test]
fn rejected_tab_append_restores_cell_range_without_any_listener_notification() {
    let (mut session, _) = table_session(Rules {
        reject_snapshot: Some(|doc| {
            doc.store()
                .iter()
                .filter(|node| matches!(node.kind(), NodeKind::TableRow))
                .count()
                > 1
        }),
        ..Rules::default()
    });
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_policy_error(session.apply_intent(&EditIntent::MoveToNextCell));
    before.assert_unchanged(&session, &counts);
}

#[test]
fn successful_cell_range_convergence_notifies_only_final_selection_once() {
    let (mut session, _) = table_session(Rules::default());
    let counts = listen(&mut session);
    assert_eq!(
        session
            .apply_intent(&EditIntent::MoveToPreviousCell)
            .unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert!(session.selection().active_cell_range().is_none());
    assert_eq!(counts.get(), (0, 1));
}

#[test]
fn atomic_target_no_change_does_not_install_selection_or_clear_marks() {
    let (mut session, node) = session(Rules::default());
    toggle(&mut session, Mark::Bold);
    insert(&mut session, "a");
    let target = DocumentSelection::collapsed(point(session.document(), node, 0));
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_eq!(
        session
            .apply_intent_with_selection(
                target,
                &EditIntent::TurnInto {
                    kind: NodeKind::CodeBlock
                }
            )
            .unwrap(),
        SessionOutcome::NoChange
    );
    before.assert_unchanged(&session, &counts);
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn atomic_target_candidate_failure_restores_state_and_successful_undo_restores_original_selection()
{
    let (mut session, node) = session(Rules::default());
    toggle(&mut session, Mark::Bold);
    insert(&mut session, "a");
    let target = DocumentSelection::new(
        point(session.document(), node, 0),
        point(session.document(), node, 1),
    );
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_policy_error(
        session.apply_intent_with_selection(target, &EditIntent::InsertText { text: "!".into() }),
    );
    before.assert_unchanged(&session, &counts);
    session
        .apply_intent_with_selection(target, &EditIntent::InsertText { text: "QZ".into() })
        .unwrap();
    assert_eq!(counts.get(), (1, 0));
    assert_eq!(text(session.document()), "QZ");
    assert_eq!(session.stored_marks(), None);
    session.undo().unwrap();
    assert_eq!(session.document().store(), before.document.store());
    assert_eq!(session.selection(), before.selection);
}

#[test]
fn atomic_target_staged_undo_restores_selection_before_the_whole_operation() {
    let (mut session, node) = session(Rules::default());
    insert(&mut session, "abc");
    let original = session.document().clone();
    let before = session.selection();
    let target = DocumentSelection::collapsed(point(session.document(), node, 0));
    let counts = listen(&mut session);
    session
        .apply_intent_with_selection(
            target,
            &EditIntent::TurnInto {
                kind: NodeKind::BulletList,
            },
        )
        .unwrap();
    assert_eq!(counts.get(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), original.store());
    assert_eq!(session.selection(), before);
}
