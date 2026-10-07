//! Ordinary-history, explicitly selected fresh-lineage snapshot replacement.

use std::{cell::RefCell, rc::Rc};

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap};
use xiaomu_core::transaction::{DocumentTemplate, Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentChangeOrigin, DocumentPosition, DocumentSelection,
    DocumentSession, EditIntent, EditPlan, InputRuleUndoSpec, PolicyError, SelectionUpdate,
    SessionOutcome, SessionPolicy,
};

fn text_node(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap()
}

fn document(text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = text_node(&mut builder, text);
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}

fn point(doc: &XiaomuDocument, node: NodeId, raw: usize) -> InlinePoint {
    InlinePoint::new(
        node,
        doc.node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(raw)
            .unwrap(),
        0,
        CursorAffinity::After,
    )
}

fn plan(doc: &XiaomuDocument, selection: SelectionUpdate) -> EditPlan {
    EditPlan::new(
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::ReplaceDocument {
            template: DocumentTemplate::capture(doc).unwrap(),
        }),
        selection,
        None,
    )
    .with_change_origin(DocumentChangeOrigin::External)
}

fn insert(session: &mut DocumentSession, text: &str) {
    session
        .apply_intent(&EditIntent::InsertText { text: text.into() })
        .unwrap();
}

type Events = Rc<RefCell<Vec<(DocumentChangeOrigin, u64, DocumentSelection)>>>;
struct Listener(Events);
impl DocumentChangeListener for Listener {
    fn document_changed_with_origin(
        &mut self,
        doc: &XiaomuDocument,
        selection: DocumentSelection,
        origin: DocumentChangeOrigin,
    ) {
        selection.validate(doc).unwrap();
        self.0
            .borrow_mut()
            .push((origin, doc.revision().as_u64(), selection));
    }
}

#[test]
fn import_isolated_retains_earlier_undo_redo_exact_ids_and_origin() {
    let (original, old) = document("old🙂");
    let initial = DocumentSelection::new(point(&original, old, 7), point(&original, old, 1));
    let mut session = DocumentSession::new(original.clone(), initial).unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    session.add_listener(Box::new(Listener(events.clone())));
    insert(&mut session, "A");
    let local = session.document().clone();
    let local_selection = session.selection();
    let (source, source_node) = document("new🙂");
    session
        .apply_plan(plan(&source, SelectionUpdate::CaretAtDocumentEnd))
        .unwrap();
    let imported = session.document().clone();
    let imported_selection = session.selection();
    let DocumentPosition::Inline(end) = imported_selection.focus() else {
        panic!("inline caret")
    };
    assert_eq!(end.text_offset().as_usize(), 7);
    assert_ne!(end.node_id(), old);
    assert_ne!(end.node_id(), source_node);
    assert_eq!(session.document().root(), original.root());
    assert_eq!(session.history_depths(), (2, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), local.store());
    assert_eq!(session.selection(), local_selection);
    session.undo().unwrap();
    assert_eq!(session.document().store(), original.store());
    assert_eq!(session.selection(), initial);
    session.redo().unwrap();
    assert_eq!(session.document().store(), local.store());
    session.redo().unwrap();
    assert_eq!(session.document().store(), imported.store());
    assert_eq!(session.selection(), imported_selection);
    assert_eq!(
        events.borrow().iter().map(|e| e.0).collect::<Vec<_>>(),
        [
            DocumentChangeOrigin::Local,
            DocumentChangeOrigin::External,
            DocumentChangeOrigin::Local,
            DocumentChangeOrigin::Local,
            DocumentChangeOrigin::Local,
            DocumentChangeOrigin::Local
        ]
    );
}

#[test]
fn successful_import_clears_old_redo_and_pending_marks() {
    let (doc, node) = document("");
    let mut session = DocumentSession::new(
        doc,
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
    )
    .unwrap();
    insert(&mut session, "a");
    session.undo().unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert!(session.stored_marks().is_some());
    let (source, _) = document("saved");
    session
        .apply_plan(plan(&source, SelectionUpdate::CaretAtDocumentEnd))
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(session.stored_marks(), None);
    assert_eq!(session.redo().unwrap(), SessionOutcome::NoChange);
    session.undo().unwrap();
    assert_eq!(session.history_depths(), (0, 1));
}

#[test]
fn all_selection_is_explicit_and_import_undo_restores_reverse_cell_range() {
    let mut builder = NodeStoreBuilder::new();
    let first = text_node(&mut builder, "first");
    let second = text_node(&mut builder, "second");
    let a = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([first]),
        )
        .unwrap();
    let b = builder
        .insert(
            NodeKind::TableHeader,
            NodeAttrs::empty(),
            NodeContent::children([second]),
        )
        .unwrap();
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([a, b]),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, builder.finish()).unwrap();
    let (source, _) = document("replacement");
    for before in [
        DocumentSelection::all(&doc),
        DocumentSelection::cell_range(b, a, InlinePoint::at_start_of(second).into()),
    ] {
        let mut session = DocumentSession::new(doc.clone(), before).unwrap();
        let selection = if before.is_all(&doc) {
            SelectionUpdate::AllDocument
        } else {
            SelectionUpdate::CaretAtDocumentEnd
        };
        session.apply_plan(plan(&source, selection)).unwrap();
        if before.is_all(&doc) {
            assert!(session.selection().is_all(session.document()));
        } else {
            assert!(session.selection().is_collapsed());
            assert!(session.selection().active_cell_range().is_none());
            assert!(session.selection().as_same_node_inline().is_some());
        }
        session.undo().unwrap();
        assert_eq!(session.document().store(), doc.store());
        assert_eq!(session.selection(), before);
    }
}

#[test]
fn final_caret_includes_trailing_atoms_and_nested_last_cell() {
    let (source, text) = document("🙂");
    let source = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertInlineAtom {
            at: point(&source, text, 4),
            kind: AtomKind::hard_break(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::hard_break(),
        })
        .apply(&source)
        .unwrap();
    let (target, node) = document("old");
    let mut session = DocumentSession::new(
        target,
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
    )
    .unwrap();
    session
        .apply_plan(plan(&source, SelectionUpdate::CaretAtDocumentEnd))
        .unwrap();
    let DocumentPosition::Inline(end) = session.selection().focus() else {
        panic!("inline caret")
    };
    assert_eq!((end.text_offset().as_usize(), end.atom_index()), (4, 1));
    let mut builder = NodeStoreBuilder::new();
    let p = text_node(&mut builder, "");
    let cell = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            NodeContent::children([p]),
        )
        .unwrap();
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([cell]),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    let source = XiaomuDocument::new(root, builder.finish()).unwrap();
    session
        .apply_plan(plan(&source, SelectionUpdate::CaretAtDocumentEnd))
        .unwrap();
    let DocumentPosition::Inline(end) = session.selection().focus() else {
        panic!("last empty cell caret")
    };
    assert_eq!((end.text_offset().as_usize(), end.atom_index()), (0, 0));
    assert_eq!(
        session.document().node(end.node_id()).unwrap().kind(),
        &NodeKind::Paragraph
    );
}

#[test]
fn empty_and_atomic_only_documents_resolve_valid_root_end_gap() {
    for count in [0, 2] {
        let mut builder = NodeStoreBuilder::new();
        let nodes: Vec<_> = (0..count)
            .map(|_| {
                builder
                    .insert(
                        NodeKind::HorizontalRule,
                        NodeAttrs::empty(),
                        NodeContent::Atomic,
                    )
                    .unwrap()
            })
            .collect();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children(nodes),
            )
            .unwrap();
        let source = XiaomuDocument::new(root, builder.finish()).unwrap();
        let (target, node) = document("old");
        let mut session = DocumentSession::new(
            target,
            DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        )
        .unwrap();
        session
            .apply_plan(plan(&source, SelectionUpdate::CaretAtDocumentEnd))
            .unwrap();
        assert_eq!(
            session.selection(),
            DocumentSelection::collapsed(NodeGap::new(session.document().root(), count))
        );
        session.selection().validate(session.document()).unwrap();
    }
}

struct RejectSaved;
impl SessionPolicy for RejectSaved {
    fn validate_document(&self, doc: &XiaomuDocument) -> Result<(), PolicyError> {
        if doc.store().iter().any(|node| {
            node.content().as_inline().is_some_and(|inline| {
                inline
                    .runs()
                    .iter()
                    .any(|run| run.text().as_str().contains("saved"))
            })
        }) {
            Err(PolicyError::new("unsupported incoming document"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn invalid_selection_and_policy_refusal_preserve_all_state_and_listener_silence() {
    let (doc, node) = document("");
    let mut session = DocumentSession::new_with_policy(
        doc,
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        Box::new(RejectSaved),
    )
    .unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    session.add_listener(Box::new(Listener(events.clone())));
    insert(&mut session, "a");
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    let selected = session.selection();
    let token =
        InputRuleUndoSpec::new(Transaction::new(TransactionOrigin::UserInput), selected).unwrap();
    session
        .apply_plan(
            EditPlan::new(
                Transaction::new(TransactionOrigin::UserInput),
                SelectionUpdate::Exact {
                    selection: selected,
                },
                None,
            )
            .with_stored_marks(Some(MarkSet::new([Mark::Bold]).unwrap()))
            .with_input_rule_undo(token),
        )
        .unwrap();
    let before = session.document().clone();
    let marks = session.stored_marks().cloned();
    let depths = session.history_depths();
    let notifications = events.borrow().len();
    let (safe, _) = document("safe");
    let (blocked, _) = document("saved");
    for candidate in [
        plan(
            &safe,
            SelectionUpdate::Exact {
                selection: selected,
            },
        ),
        plan(&blocked, SelectionUpdate::CaretAtDocumentEnd),
    ] {
        assert!(session.apply_plan(candidate).is_err());
        assert_eq!(session.document().store(), before.store());
        assert_eq!(session.document().revision(), before.revision());
        assert_eq!(session.selection(), selected);
        assert_eq!(session.stored_marks(), marks.as_ref());
        assert_eq!(session.history_depths(), depths);
        assert!(session.input_rule_undo_available());
        assert_eq!(events.borrow().len(), notifications);
    }
}

#[test]
fn empty_plan_commits_explicit_selection_and_legacy_listener_receives_external() {
    struct Legacy(Rc<RefCell<usize>>);
    impl DocumentChangeListener for Legacy {
        fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
            *self.0.borrow_mut() += 1;
        }
    }
    let (doc, node) = document("abc");
    let before = DocumentSelection::collapsed(InlinePoint::at_start_of(node));
    let after = DocumentSelection::collapsed(point(&doc, node, 3));
    let mut session = DocumentSession::new(doc, before).unwrap();
    let count = Rc::new(RefCell::new(0));
    session.add_listener(Box::new(Legacy(count.clone())));
    let revision = session.document().revision().as_u64();
    session
        .apply_plan(
            EditPlan::new(
                Transaction::new(TransactionOrigin::System),
                SelectionUpdate::Exact { selection: after },
                None,
            )
            .with_change_origin(DocumentChangeOrigin::External),
        )
        .unwrap();
    assert_eq!(session.selection(), after);
    assert_eq!(session.document().revision().as_u64(), revision + 1);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(*count.borrow(), 1);
    session.undo().unwrap();
    assert_eq!(session.selection(), before);
    assert_eq!(*count.borrow(), 2);
}

#[test]
fn rejected_import_retains_open_typing_group_and_existing_redo() {
    let (doc, node) = document("");
    let before = DocumentSelection::collapsed(InlinePoint::at_start_of(node));
    let mut session = DocumentSession::new(doc.clone(), before).unwrap();
    insert(&mut session, "a");
    let (source, _) = document("new");
    let invalid = plan(
        &source,
        SelectionUpdate::Exact {
            selection: session.selection(),
        },
    );
    assert!(session.apply_plan(invalid).is_err());
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
    let typed = session.document().clone();
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
    let invalid = plan(&source, SelectionUpdate::Exact { selection: before });
    assert!(session.apply_plan(invalid).is_err());
    assert_eq!(session.history_depths(), (0, 1));
    session.redo().unwrap();
    assert_eq!(session.document().store(), typed.store());
}

#[test]
fn direct_reverse_text_import_and_identical_body_still_keep_ordinary_history() {
    let (doc, node) = document("a🙂z");
    let before = DocumentSelection::new(point(&doc, node, 6), point(&doc, node, 1));
    let mut session = DocumentSession::new(doc.clone(), before).unwrap();
    session
        .apply_plan(plan(&doc, SelectionUpdate::CaretAtDocumentEnd))
        .unwrap();
    assert_ne!(session.document().store(), doc.store());
    assert_eq!(session.history_depths(), (1, 0));
    let imported = session.document().clone();
    assert_eq!(session.selection().focus(), {
        let id = *imported
            .node(imported.root())
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .last()
            .unwrap();
        point(&imported, id, 6).into()
    });
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    assert_eq!(session.document().store(), imported.store());
}

#[test]
fn whole_snapshot_payloads_do_not_bypass_bounded_input_rule_undo_admission() {
    let (doc, node) = document("a");
    let before = DocumentSelection::collapsed(InlinePoint::at_start_of(node));
    let replacement = plan(&doc, SelectionUpdate::CaretAtDocumentEnd);
    assert!(InputRuleUndoSpec::new(replacement.transaction().clone(), before).is_err());
    let applied = replacement.transaction().apply_with_changes(&doc).unwrap();
    assert!(InputRuleUndoSpec::new(applied.inverse().clone(), before).is_err());
}
