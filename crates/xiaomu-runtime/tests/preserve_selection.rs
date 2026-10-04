//! Exact post-document selection preservation for identity-preserving plans.

use std::{cell::Cell, rc::Rc};

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap, TextPoint};
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentPosition, DocumentSelection, DocumentSession, EditIntent,
    EditPlan, IntentDisposition, PolicyError, SelectionUpdate, SessionContext, SessionError,
    SessionOutcome, SessionPolicy,
};

struct ApplyOnDelete(EditPlan);

impl SessionPolicy for ApplyOnDelete {
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

fn fixture() -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let mut paragraph = || {
        builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("abcd", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
    };
    let first = paragraph();
    let second = paragraph();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        second,
    )
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize, after: bool) -> TextPoint {
    let offset = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(raw)
        .unwrap();
    TextPoint::new(
        node,
        offset,
        if after {
            CursorAffinity::After
        } else {
            CursorAffinity::Before
        },
    )
}

fn replacement(
    document: &XiaomuDocument,
    node: NodeId,
    start: usize,
    end: usize,
    text: &str,
) -> Transaction {
    Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::ReplaceText {
        node,
        range: TextRange::new(
            point(document, node, start, false).offset(),
            point(document, node, end, false).offset(),
        )
        .unwrap(),
        replacement: text.into(),
    })
}

fn session(
    document: XiaomuDocument,
    selection: DocumentSelection,
    transaction: Transaction,
    update: SelectionUpdate,
) -> DocumentSession {
    DocumentSession::new_with_policy(
        document,
        selection,
        Box::new(ApplyOnDelete(EditPlan::new(transaction, update, None))),
    )
    .unwrap()
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

fn assert_rejected_without_publication(session: &mut DocumentSession, counts: &Counts) {
    let before = session.document().clone();
    let selection = session.selection();
    let marks = session.stored_marks().cloned();
    let depths = session.history_depths();
    let notifications = counts.get();
    assert_eq!(
        session.apply_intent(&EditIntent::Delete),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.document().root(), before.root());
    assert_eq!(session.document().version(), before.version());
    assert_eq!(session.document().revision(), before.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), marks.as_ref());
    assert_eq!(session.history_depths(), depths);
    assert_eq!(counts.get(), notifications);
}

#[test]
fn forward_and_reverse_ranges_keep_exact_endpoints_through_undo_redo() {
    for reverse in [false, true] {
        let (document, node, _) = fixture();
        let start = point(&document, node, 1, true);
        let end = point(&document, node, 3, false);
        let selection = if reverse {
            DocumentSelection::new(end, start)
        } else {
            DocumentSelection::new(start, end)
        };
        let transaction = replacement(&document, node, 0, 0, "xx");
        let mut session = session(
            document.clone(),
            selection,
            transaction,
            SelectionUpdate::PreserveSelection,
        );
        let counts = listen(&mut session);
        assert_eq!(
            session.apply_intent(&EditIntent::Delete).unwrap(),
            SessionOutcome::DocumentChanged
        );
        let after = session.document().clone();
        assert_eq!(session.selection(), selection);
        assert_eq!(session.history_depths(), (1, 0));
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
        assert_eq!(session.selection(), selection);
        session.redo().unwrap();
        assert_eq!(session.document().store(), after.store());
        assert_eq!(session.selection(), selection);
        assert_eq!(counts.get(), (3, 0));
    }
}

#[test]
fn old_variants_still_collapse_focus_or_map_ranges_outward() {
    for reverse in [false, true] {
        for update in [SelectionUpdate::PreserveFocus, SelectionUpdate::MapExisting] {
            let (document, node, _) = fixture();
            let start = point(&document, node, 1, true);
            let end = point(&document, node, 3, false);
            let selection = if reverse {
                DocumentSelection::new(end, start)
            } else {
                DocumentSelection::new(start, end)
            };
            // Replacing the whole selected range tests outward bias at both
            // boundaries, including the anchor/focus roles of a reverse range.
            let transaction = replacement(&document, node, 1, 3, "xxxx");
            let mut session = session(document, selection, transaction, update);
            session.apply_intent(&EditIntent::Delete).unwrap();
            let expected = if update == SelectionUpdate::PreserveFocus {
                DocumentSelection::collapsed(selection.focus())
            } else {
                let start = point(session.document(), node, 1, true);
                let end = point(session.document(), node, 5, false);
                if reverse {
                    DocumentSelection::new(end, start)
                } else {
                    DocumentSelection::new(start, end)
                }
            };
            assert_eq!(session.selection(), expected);
        }
    }
}

#[test]
fn remove_restore_keeps_cross_block_range_but_map_existing_still_rejects() {
    let (document, first, second) = fixture();
    let mut transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: first });
    let removed = transaction.apply_with_changes(&document).unwrap();
    for step in removed.inverse().steps() {
        transaction.push_step(step.clone());
    }
    transaction.push_step(TransactionStep::SetNodeKind {
        node: first,
        kind: NodeKind::CodeBlock,
    });
    for reverse in [false, true] {
        let start = point(&document, first, 1, true);
        let end = point(&document, second, 3, false);
        let selection = if reverse {
            DocumentSelection::new(end, start)
        } else {
            DocumentSelection::new(start, end)
        };
        let mut exact = session(
            document.clone(),
            selection,
            transaction.clone(),
            SelectionUpdate::PreserveSelection,
        );
        exact.apply_intent(&EditIntent::Delete).unwrap();
        let after = exact.document().clone();
        assert_eq!(exact.selection(), selection);
        assert_eq!(exact.history_depths(), (1, 0));
        assert_eq!(after.node(first).unwrap().kind(), &NodeKind::CodeBlock);
        exact.undo().unwrap();
        assert_eq!(exact.document().store(), document.store());
        assert_eq!(exact.selection(), selection);
        exact.redo().unwrap();
        assert_eq!(exact.document().store(), after.store());
        assert_eq!(exact.selection(), selection);

        let mut mapped = session(
            document.clone(),
            selection,
            transaction.clone(),
            SelectionUpdate::MapExisting,
        );
        assert_eq!(
            mapped.apply_intent(&EditIntent::Delete),
            Err(SessionError::SelectionDeleted)
        );
        assert_eq!(mapped.document().store(), document.store());
        assert_eq!(mapped.selection(), selection);
        assert_eq!(mapped.history_depths(), (0, 0));
    }
}

#[test]
fn invalid_anchor_rejects_even_when_focus_survives_and_keeps_redo() {
    let (document, first, second) = fixture();
    let selection = DocumentSelection::new(
        point(&document, first, 4, true),
        point(&document, second, 1, false),
    );
    let transaction = replacement(&document, first, 0, 4, "a");
    let mut session = session(
        document,
        selection,
        transaction,
        SelectionUpdate::PreserveSelection,
    );
    session
        .apply(&Transaction::new(TransactionOrigin::System))
        .unwrap();
    session.undo().unwrap();
    assert_eq!(session.history_depths(), (0, 1));
    let counts = listen(&mut session);
    assert_rejected_without_publication(&mut session, &counts);
    session.redo().unwrap();
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn invalid_caret_restores_stored_marks_and_the_open_typing_group() {
    let (document, node, _) = fixture();
    let selection = DocumentSelection::collapsed(point(&document, node, 4, true));
    let transaction = replacement(&document, node, 0, 4, "");
    let mut session = session(
        document.clone(),
        selection,
        transaction,
        SelectionUpdate::PreserveSelection,
    );
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    assert_eq!(
        session.stored_marks(),
        Some(&MarkSet::new([Mark::Bold]).unwrap())
    );
    let counts = listen(&mut session);
    assert_rejected_without_publication(&mut session, &counts);
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), selection);
}

#[test]
fn mixed_inline_ordinals_and_affinities_are_preserved() {
    let (document, node, _) = fixture();
    let at = point(&document, node, 1, false);
    let document = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::InsertInlineAtom {
            at: at.into(),
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new("@A").unwrap(),
        })
        .apply(&document)
        .unwrap();
    let before_atom = DocumentPosition::Inline(InlinePoint::new(
        node,
        at.offset(),
        0,
        CursorAffinity::After,
    ));
    let after_atom = DocumentPosition::Inline(InlinePoint::new(
        node,
        at.offset(),
        1,
        CursorAffinity::Before,
    ));
    for selection in [
        DocumentSelection::new(before_atom, after_atom),
        DocumentSelection::new(after_atom, before_atom),
        DocumentSelection::collapsed(after_atom),
    ] {
        let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::SetNodeKind {
                node,
                kind: NodeKind::CodeBlock,
            },
        );
        let mut session = session(
            document.clone(),
            selection,
            transaction,
            SelectionUpdate::PreserveSelection,
        );
        session.apply_intent(&EditIntent::Delete).unwrap();
        assert_eq!(session.selection(), selection);
        session.undo().unwrap();
        assert_eq!(session.selection(), selection);
        session.redo().unwrap();
        assert_eq!(session.selection(), selection);
    }
}

#[test]
fn structural_gap_endpoints_are_preserved_and_validated() {
    let (document, first, _) = fixture();
    let selection = DocumentSelection::new(
        NodeGap::new(document.root(), 2),
        NodeGap::new(document.root(), 0),
    );
    let mut valid = session(
        document.clone(),
        selection,
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SetNodeKind {
            node: first,
            kind: NodeKind::CodeBlock,
        }),
        SelectionUpdate::PreserveSelection,
    );
    valid.apply_intent(&EditIntent::Delete).unwrap();
    assert_eq!(valid.selection(), selection);

    let mut invalid = session(
        document,
        selection,
        Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::RemoveNode { node: first }),
        SelectionUpdate::PreserveSelection,
    );
    let counts = listen(&mut invalid);
    assert_rejected_without_publication(&mut invalid, &counts);
}

#[test]
fn complete_cell_range_is_preserved_and_validated_beyond_parked_caret() {
    let (document, first, _) = fixture();
    let document = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::InsertTable {
            parent: document.root(),
            index: 1,
            rows: 2,
            columns: 1,
        })
        .apply(&document)
        .unwrap();
    let children = |node| {
        document
            .node(node)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
    };
    let table = children(document.root())[1];
    let rows = children(table);
    let first_cell = children(rows[0])[0];
    let second_cell = children(rows[1])[0];
    let park = point(&document, first, 1, true).into();
    let selection = DocumentSelection::cell_range(second_cell, first_cell, park);
    let mut valid = session(
        document.clone(),
        selection,
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SetNodeKind {
            node: first,
            kind: NodeKind::CodeBlock,
        }),
        SelectionUpdate::PreserveSelection,
    );
    valid.apply_intent(&EditIntent::Delete).unwrap();
    assert_eq!(valid.selection(), selection);
    valid.undo().unwrap();
    assert_eq!(valid.selection(), selection);
    valid.redo().unwrap();
    assert_eq!(valid.selection(), selection);

    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: rows[1] });
    let candidate = transaction.apply(&document).unwrap();
    // Both parked endpoints survive; only full-selection validation can
    // detect that the active rectangle's anchor cell no longer exists.
    DocumentSelection::collapsed(park)
        .validate(&candidate)
        .unwrap();
    let mut invalid = session(
        document,
        selection,
        transaction,
        SelectionUpdate::PreserveSelection,
    );
    let counts = listen(&mut invalid);
    assert_rejected_without_publication(&mut invalid, &counts);
}
