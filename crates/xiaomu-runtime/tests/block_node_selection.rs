//! Whole-block selection identity is explicit, validated and history-safe.

use std::{cell::Cell, rc::Rc};

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{InlinePoint, NodeGap};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    CaretMove, DocumentChangeListener, DocumentPosition, DocumentSelection, DocumentSession,
    EditIntent, EditPlan, IntentDisposition, PolicyError, SelectionUpdate, SessionContext,
    SessionError, SessionOutcome, SessionPolicy,
};

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

fn container(builder: &mut NodeStoreBuilder, kind: NodeKind, children: &[NodeId]) -> NodeId {
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::children(children.iter().copied()),
        )
        .unwrap()
}

fn fixture() -> (XiaomuDocument, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let before = paragraph(&mut builder, "before");
    let inner = paragraph(&mut builder, "nested");
    let quote = container(&mut builder, NodeKind::Quote, &[inner]);
    let after = paragraph(&mut builder, "after");
    let root = container(&mut builder, NodeKind::Document, &[before, quote, after]);
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        before,
        quote,
        inner,
    )
}

type Counts = Rc<Cell<(usize, usize)>>;

struct Listener(Counts);

impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, document: &XiaomuDocument, selection: DocumentSelection) {
        selection.validate(document).unwrap();
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

fn assert_unchanged(
    session: &DocumentSession,
    document: &XiaomuDocument,
    selection: DocumentSelection,
    depths: (usize, usize),
    counts: &Counts,
) {
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().root(), document.root());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.document().version(), document.version());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), depths);
    assert_eq!(counts.get(), (0, 0));
}

#[test]
fn explicit_node_identity_is_not_inferred_from_gaps_or_legacy_atomic_selection() {
    let (document, _, quote, _) = fixture();
    let selection = DocumentSelection::node(&document, quote).unwrap();
    assert_eq!(selection.as_node_selection(), Some(quote));
    assert_eq!(selection.anchor(), NodeGap::new(document.root(), 1).into());
    assert_eq!(selection.focus(), NodeGap::new(document.root(), 2).into());
    assert!(!selection.is_collapsed());
    assert!(!selection.is_all(&document));
    assert!(selection.as_atomic_node().is_none());
    assert!(selection.as_same_node_inline().is_none());
    assert!(selection.as_single_node().is_none());
    assert!(selection.active_cell_range().is_none());
    let plain = DocumentSelection::new(selection.anchor(), selection.focus());
    assert_eq!(plain.as_node_selection(), None);
    assert_ne!(selection, plain);
    assert!(
        DocumentSession::new(document.clone(), plain)
            .unwrap()
            .clipboard_slice()
            .is_err()
    );

    let mut builder = NodeStoreBuilder::new();
    let image = builder
        .insert(NodeKind::Image, NodeAttrs::empty(), NodeContent::Atomic)
        .unwrap();
    let root = container(&mut builder, NodeKind::Document, &[image]);
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let node = DocumentSelection::node(&document, image).unwrap();
    let all = DocumentSelection::all(&document);
    assert_eq!((node.anchor(), node.focus()), (all.anchor(), all.focus()));
    assert_ne!(node, all);
    assert!(!node.is_all(&document));
    let atomic = DocumentSelection::collapsed(DocumentPosition::Atomic(image));
    assert!(atomic.is_collapsed());
    assert_eq!(atomic.as_atomic_node(), Some(image));
    assert_eq!(atomic.as_node_selection(), None);
    for (selection, closed) in [(node, true), (all, true), (atomic, false)] {
        assert_eq!(
            DocumentSession::new(document.clone(), selection)
                .unwrap()
                .clipboard_slice()
                .unwrap()
                .unwrap()
                .is_closed(),
            closed
        );
    }
}

#[test]
fn setting_a_node_is_selection_only_and_repeating_it_is_a_noop() {
    let (document, before, quote, _) = fixture();
    let mut session = DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(before)),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    let counts = listen(&mut session);
    assert_eq!(
        session.set_node_selection(quote),
        Ok(SessionOutcome::SelectionChanged)
    );
    assert_eq!(
        session.set_node_selection(quote),
        Ok(SessionOutcome::NoChange)
    );
    assert_eq!(session.selection().as_node_selection(), Some(quote));
    assert_eq!(session.stored_marks(), None);
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(counts.get(), (0, 1));
}

#[test]
fn invalid_targets_leave_marks_listeners_history_and_typing_group_unchanged() {
    let (document, before, quote, _) = fixture();
    let mut session = DocumentSession::new(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(before)),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    let document = session.document().clone();
    let selection = session.selection();
    let marks = session.stored_marks().cloned();
    let counts = listen(&mut session);
    assert_eq!(
        session.set_node_selection(document.root()),
        Err(SessionError::SelectionInvalid)
    );
    assert_unchanged(&session, &document, selection, (1, 0), &counts);
    assert_eq!(session.stored_marks(), marks.as_ref());
    let stale = DocumentSelection::node(&document, quote).unwrap();
    let removed = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode { node: quote })
        .apply(&document)
        .unwrap();
    assert_eq!(
        stale.validate(&removed),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(
        DocumentSelection::node(&removed, quote),
        Err(SessionError::SelectionInvalid)
    );
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn root_provenance_rejects_a_different_root_even_when_target_id_exists() {
    let (document, _, quote, _) = fixture();
    let selection = DocumentSelection::node(&document, quote).unwrap();
    let mut builder = NodeStoreBuilder::new();
    let _ = paragraph(&mut builder, "unused allocator prefix");
    let inner = paragraph(&mut builder, "different tree");
    let same_id = container(&mut builder, NodeKind::Quote, &[inner]);
    assert_eq!(same_id, quote);
    // Include every built node and add a distinct root identity.
    let extra = paragraph(&mut builder, "extra");
    let extra2 = paragraph(&mut builder, "extra2");
    let first = document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    let root = container(
        &mut builder,
        NodeKind::Document,
        &[first, same_id, extra, extra2],
    );
    assert_ne!(root, document.root());
    let foreign = XiaomuDocument::new(root, builder.finish()).unwrap();
    assert_eq!(
        selection.validate(&foreign),
        Err(SessionError::SelectionInvalid)
    );
    let mut session =
        DocumentSession::new(foreign.clone(), DocumentSelection::all(&foreign)).unwrap();
    let before = session.selection();
    let counts = listen(&mut session);
    assert_eq!(
        session.set_document_selection(selection),
        Err(SessionError::SelectionInvalid)
    );
    assert_unchanged(&session, &foreign, before, (0, 0), &counts);
}

#[test]
fn unsupported_kinds_are_rejected_before_any_selection_state_changes() {
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::hard_break()),
        )
        .unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms([], [InlineAtomPlacement::new(atom, TextOffset::ZERO)])
                    .unwrap(),
            ),
        )
        .unwrap();
    let cell = container(&mut builder, NodeKind::TableCell, &[paragraph]);
    let row = container(&mut builder, NodeKind::TableRow, &[cell]);
    let table = container(&mut builder, NodeKind::Table, &[row]);
    let custom = builder
        .insert(
            NodeKind::custom("unknown-block").unwrap(),
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let list_item = container(&mut builder, NodeKind::ListItem, &[]);
    let list = container(&mut builder, NodeKind::BulletList, &[list_item]);
    let task_item = container(&mut builder, NodeKind::TaskItem, &[]);
    let tasks = container(&mut builder, NodeKind::TaskList, &[task_item]);
    let root = container(
        &mut builder,
        NodeKind::Document,
        &[table, custom, list, tasks],
    );
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::node(&document, table).unwrap();
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let counts = listen(&mut session);
    for target in [root, row, cell, atom, custom, list_item, task_item] {
        assert_eq!(
            session.set_node_selection(target),
            Err(SessionError::SelectionInvalid)
        );
        assert_unchanged(&session, &document, selection, (0, 0), &counts);
    }
}

#[test]
fn mapping_bias_keeps_adjacent_insertions_outside_the_selected_subtree() {
    let (document, _, quote, inner) = fixture();
    for selected in [quote, inner] {
        let selection = DocumentSelection::node(&document, selected).unwrap();
        let DocumentPosition::Gap(gap) = selection.anchor() else {
            panic!("node gap")
        };
        let mut session = DocumentSession::new(document.clone(), selection).unwrap();
        let transaction = Transaction::new(TransactionOrigin::System)
            .with_step(TransactionStep::InsertNode {
                parent: gap.parent(),
                index: gap.index(),
                kind: NodeKind::Paragraph,
                attrs: NodeAttrs::empty(),
                content: NodeContent::empty_inline(),
            })
            .with_step(TransactionStep::InsertNode {
                parent: gap.parent(),
                index: gap.index() + 2,
                kind: NodeKind::Paragraph,
                attrs: NodeAttrs::empty(),
                content: NodeContent::empty_inline(),
            });
        let applied = transaction.apply_with_changes(&document).unwrap();
        let mapped = selection.map_through(applied.changes(), &document).unwrap();
        assert_eq!(
            mapped,
            DocumentSelection::node(applied.document(), selected).unwrap()
        );
        session.apply(&transaction).unwrap();
        assert_eq!(session.selection(), mapped);
        assert_eq!(session.selection().as_node_selection(), Some(selected));
        session.undo().unwrap();
        assert_eq!(session.selection(), selection);
        session.redo().unwrap();
        assert_eq!(session.selection(), mapped);
    }
}

#[test]
fn splitting_selected_inline_block_follows_original_identity_only() {
    let (document, _, _, inner) = fixture();
    let selection = DocumentSelection::node(&document, inner).unwrap();
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let at = document
        .node(inner)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(2)
        .unwrap();
    session
        .apply(
            &Transaction::new(TransactionOrigin::System)
                .with_step(TransactionStep::SplitNode { node: inner, at }),
        )
        .unwrap();
    assert_eq!(
        session.selection(),
        DocumentSelection::node(session.document(), inner).unwrap()
    );
    assert_eq!(session.selected_text().as_deref(), Some("ne"));
    session.undo().unwrap();
    assert_eq!(session.selection(), selection);
    assert_eq!(session.document().store(), document.store());
    session.redo().unwrap();
    assert_eq!(session.selected_text().as_deref(), Some("ne"));
}

#[test]
fn deletion_or_invalid_kind_mapping_rejects_without_publication_or_redo_loss() {
    let (document, _, quote, inner) = fixture();
    for selected in [quote, inner] {
        let selection = DocumentSelection::node(&document, selected).unwrap();
        let mut session = DocumentSession::new(document.clone(), selection).unwrap();
        session
            .apply(&Transaction::new(TransactionOrigin::System))
            .unwrap();
        session.undo().unwrap();
        let before = session.document().clone();
        let counts = listen(&mut session);
        assert_eq!(
            session.apply(
                &Transaction::new(TransactionOrigin::System)
                    .with_step(TransactionStep::RemoveNode { node: quote })
            ),
            Err(SessionError::SelectionDeleted)
        );
        assert_unchanged(&session, &before, selection, (0, 1), &counts);
        assert_eq!(
            session.apply(&Transaction::new(TransactionOrigin::System).with_step(
                TransactionStep::SetNodeKind {
                    node: selected,
                    kind: NodeKind::custom("unsupported").unwrap()
                }
            )),
            Err(SessionError::SelectionInvalid)
        );
        assert_unchanged(&session, &before, selection, (0, 1), &counts);
        session.redo().unwrap();
        assert_eq!(session.selection(), selection);
    }
}

struct OnDelete(EditPlan);

impl SessionPolicy for OnDelete {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        Ok(if matches!(intent, EditIntent::Delete) {
            IntentDisposition::Apply(self.0.clone())
        } else {
            IntentDisposition::Continue
        })
    }
}

#[test]
fn preserved_selection_refreshes_moved_node_gaps_and_history_restores_exact_identity() {
    let (document, _, quote, inner) = fixture();
    let selection = DocumentSelection::node(&document, quote).unwrap();
    let removal = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: quote });
    let removed = removal.apply_with_changes(&document).unwrap();
    let TransactionStep::RestoreSubtree { root, nodes, .. } = &removed.inverse().steps()[0] else {
        panic!("restore subtree")
    };
    let moved = removal.clone().with_step(TransactionStep::RestoreSubtree {
        parent: document.root(),
        index: 0,
        root: *root,
        nodes: nodes.clone(),
    });
    let mut mapped = DocumentSession::new(document.clone(), selection).unwrap();
    assert_eq!(mapped.apply(&moved), Err(SessionError::SelectionDeleted));
    let mut session = DocumentSession::new_with_policy(
        document.clone(),
        selection,
        Box::new(OnDelete(EditPlan::new(
            moved,
            SelectionUpdate::PreserveSelection,
            None,
        ))),
    )
    .unwrap();
    session.apply_intent(&EditIntent::Delete).unwrap();
    let after = session.selection();
    assert_eq!(after.as_node_selection(), Some(quote));
    assert_eq!(after.anchor(), NodeGap::new(document.root(), 0).into());
    assert_eq!(session.document().node(inner), document.node(inner));
    assert_ne!(after, selection);
    session.undo().unwrap();
    assert_eq!(session.selection(), selection);
    assert_eq!(session.document().store(), document.store());
    session.redo().unwrap();
    assert_eq!(session.selection(), after);
    assert_eq!(session.document().node(inner), document.node(inner));
}

#[test]
fn explicit_policy_deletion_undo_restores_the_same_selected_subtree() {
    let (document, before, quote, inner) = fixture();
    let selection = DocumentSelection::node(&document, quote).unwrap();
    let after = DocumentSelection::collapsed(InlinePoint::at_start_of(before));
    let plan = EditPlan::new(
        Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::RemoveNode { node: quote }),
        SelectionUpdate::Exact { selection: after },
        None,
    );
    let mut session =
        DocumentSession::new_with_policy(document.clone(), selection, Box::new(OnDelete(plan)))
            .unwrap();
    session.apply_intent(&EditIntent::Delete).unwrap();
    assert!(session.document().node(quote).is_none());
    assert!(session.document().node(inner).is_none());
    assert_eq!(session.selection(), after);
    session.undo().unwrap();
    assert_eq!(session.selection(), selection);
    assert_eq!(session.document().store(), document.store());
    session.redo().unwrap();
    assert_eq!(session.selection(), after);
    assert!(session.document().node(quote).is_none());
}

#[test]
fn every_default_selection_driven_edit_fails_closed_before_changing_state() {
    let (document, _, quote, inner) = fixture();
    let selection = DocumentSelection::node(&document, quote).unwrap();
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let closed = session.clipboard_slice().unwrap().unwrap();
    let open = DocumentSession::new(
        document.clone(),
        DocumentSelection::new(
            InlinePoint::at_start_of(inner),
            InlinePoint::new(
                inner,
                document
                    .node(inner)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .offset_at(1)
                    .unwrap(),
                0,
                xiaomu_core::selection::CursorAffinity::Before,
            ),
        ),
    )
    .unwrap()
    .clipboard_slice()
    .unwrap()
    .unwrap();
    let counts = listen(&mut session);
    for intent in [
        EditIntent::InsertText { text: "x".into() },
        EditIntent::InsertText {
            text: String::new(),
        },
        EditIntent::PasteText {
            text: "paste".into(),
        },
        EditIntent::PasteSlice { slice: open },
        EditIntent::PasteSlice { slice: closed },
        EditIntent::Backspace,
        EditIntent::Delete,
        EditIntent::InsertLineBreak,
        EditIntent::SplitBlock,
        EditIntent::JoinWithPrevious,
        EditIntent::ToggleMark { mark: Mark::Bold },
        EditIntent::MoveCaret {
            caret_move: CaretMove::Forward,
            extend_selection: false,
        },
        EditIntent::PlaceCaret {
            offset: TextOffset::ZERO,
            extend_selection: false,
        },
        EditIntent::MoveToNextCell,
        EditIntent::MoveToPreviousCell,
        EditIntent::InsertTable {
            rows: 1,
            columns: 1,
        },
    ] {
        assert_eq!(
            session.apply_intent(&intent),
            Err(SessionError::UnsupportedEdit)
        );
        assert_unchanged(&session, &document, selection, (0, 0), &counts);
        assert_eq!(session.stored_marks(), None);
    }
    let target = InlinePoint::at_start_of(inner).to_text_point().unwrap();
    assert_eq!(
        session.apply_intent(&EditIntent::SetSelection {
            anchor: target,
            focus: target
        }),
        Ok(SessionOutcome::SelectionChanged)
    );
    assert_eq!(session.selection().as_node_selection(), None);
}

#[test]
fn tentative_node_target_rejection_restores_original_marks_and_typing_group() {
    let (document, before, quote, _) = fixture();
    let mut session = DocumentSession::new(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(before)),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    let document = session.document().clone();
    let selection = session.selection();
    let marks = session.stored_marks().cloned();
    let target = DocumentSelection::node(&document, quote).unwrap();
    let counts = listen(&mut session);
    assert_eq!(
        session.apply_intent_with_selection(target, &EditIntent::Delete),
        Err(SessionError::UnsupportedEdit)
    );
    assert_unchanged(&session, &document, selection, (1, 0), &counts);
    assert_eq!(session.stored_marks(), marks.as_ref());
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(
        session
            .document()
            .node(before)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .len_bytes(),
        "before".len()
    );
}

#[test]
fn stale_node_and_unknown_id_fail_in_setters_without_clearing_marks() {
    let (document, before, quote, _) = fixture();
    let stale = DocumentSelection::node(&document, quote).unwrap();
    let document = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode { node: quote })
        .apply(&document)
        .unwrap();
    let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(before));
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
        .unwrap();
    let marks = session.stored_marks().cloned();
    let counts = listen(&mut session);
    assert_eq!(
        session.set_node_selection(quote),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(
        session.set_document_selection(stale),
        Err(SessionError::SelectionInvalid)
    );
    assert_unchanged(&session, &document, selection, (0, 0), &counts);
    assert_eq!(session.stored_marks(), marks.as_ref());
}

#[test]
fn preserved_node_can_change_parent_without_becoming_a_text_caret() {
    let (document, _, _, inner) = fixture();
    let selection = DocumentSelection::node(&document, inner).unwrap();
    let removal = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: inner });
    let removed = removal.apply_with_changes(&document).unwrap();
    let TransactionStep::RestoreSubtree { root, nodes, .. } = &removed.inverse().steps()[0] else {
        panic!("restore subtree")
    };
    let moved = removal.with_step(TransactionStep::RestoreSubtree {
        parent: document.root(),
        index: 0,
        root: *root,
        nodes: nodes.clone(),
    });
    let plan = EditPlan::new(moved, SelectionUpdate::PreserveSelection, None);
    let mut session =
        DocumentSession::new_with_policy(document.clone(), selection, Box::new(OnDelete(plan)))
            .unwrap();
    session.apply_intent(&EditIntent::Delete).unwrap();
    let after = session.selection();
    assert_eq!(
        after,
        DocumentSelection::node(session.document(), inner).unwrap()
    );
    assert_eq!(after.anchor(), NodeGap::new(document.root(), 0).into());
    assert!(after.as_single_node().is_none());
    assert_eq!(session.document().node(inner), document.node(inner));
    session.undo().unwrap();
    assert_eq!(session.selection(), selection);
    assert_eq!(session.document().store(), document.store());
    session.redo().unwrap();
    assert_eq!(session.selection(), after);
}

#[test]
fn identity_addressed_checkbox_preserves_a_selected_task_list_and_undo_state() {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = paragraph(&mut builder, "task");
    let item = container(&mut builder, NodeKind::TaskItem, &[paragraph]);
    let list = container(&mut builder, NodeKind::TaskList, &[item]);
    let root = container(&mut builder, NodeKind::Document, &[list]);
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::node(&document, list).unwrap();
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let counts = listen(&mut session);
    let intent = EditIntent::SetTaskChecked {
        item,
        checked: true,
    };
    assert_eq!(
        session.apply_intent(&intent),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(session.apply_intent(&intent), Ok(SessionOutcome::NoChange));
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(counts.get(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), selection);
    session.redo().unwrap();
    assert_eq!(session.selection(), selection);
    assert_eq!(
        session
            .document()
            .node(item)
            .unwrap()
            .attrs()
            .get("checked"),
        Some(&xiaomu_core::document::AttrValue::Bool(true))
    );
}
