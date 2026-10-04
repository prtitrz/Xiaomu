//! Explicit task checkbox updates preserve identity, selection and atomicity.

use std::{cell::Cell, rc::Rc};

use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentPosition, DocumentSelection, DocumentSession, EditIntent,
    IntentDisposition, PolicyError, SessionContext, SessionError, SessionOutcome, SessionPolicy,
};

struct Fixture {
    document: XiaomuDocument,
    first: NodeId,
    second: NodeId,
    first_text: NodeId,
    second_text: NodeId,
    tail: NodeId,
    list: NodeId,
    rule: NodeId,
    row: NodeId,
    cells: [NodeId; 2],
}

fn attrs(checked: Option<AttrValue>) -> NodeAttrs {
    let mut values = std::collections::BTreeMap::from([
        ("id".into(), AttrValue::String("host-task-中🙂".into())),
        ("empty".into(), AttrValue::String(String::new())),
        ("nullable".into(), AttrValue::Null),
        (
            "extension".into(),
            AttrValue::Object(
                [(
                    "nested".into(),
                    AttrValue::List(vec![AttrValue::Null, AttrValue::Integer(7)]),
                )]
                .into(),
            ),
        ),
    ]);
    if let Some(checked) = checked {
        values.insert("checked".into(), checked);
    }
    NodeAttrs::new(values).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, value: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(value, MarkSet::empty()).unwrap()]).unwrap(),
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

fn fixture(checked: Option<AttrValue>) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let first_text = paragraph(&mut builder, "first中🙂");
    let first = builder
        .insert(
            NodeKind::TaskItem,
            attrs(checked),
            NodeContent::children([first_text]),
        )
        .unwrap();
    let second_text = paragraph(&mut builder, "other中🙂");
    let second = builder
        .insert(
            NodeKind::TaskItem,
            attrs(Some(AttrValue::Null)),
            NodeContent::children([second_text]),
        )
        .unwrap();
    let list = container(&mut builder, NodeKind::TaskList, &[first, second]);
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::hard_break()),
        )
        .unwrap();
    let tail = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("tail", MarkSet::empty()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let cells = ["left", "right"].map(|text| {
        let paragraph = paragraph(&mut builder, text);
        container(&mut builder, NodeKind::TableCell, &[paragraph])
    });
    let row = container(&mut builder, NodeKind::TableRow, &cells);
    let table = container(&mut builder, NodeKind::Table, &[row]);
    let root = container(&mut builder, NodeKind::Document, &[list, tail, rule, table]);
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        second,
        first_text,
        second_text,
        tail,
        list,
        rule,
        row,
        cells,
    }
}

fn point(document: &XiaomuDocument, node: NodeId, offset: usize) -> InlinePoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    InlinePoint::new(
        node,
        inline.offset_at(offset).unwrap(),
        0,
        CursorAffinity::Before,
    )
}

fn session(fixture: &Fixture) -> DocumentSession {
    DocumentSession::new(
        fixture.document.clone(),
        DocumentSelection::collapsed(point(&fixture.document, fixture.second_text, 0)),
    )
    .unwrap()
}

fn set_checked(
    session: &mut DocumentSession,
    item: NodeId,
    checked: bool,
) -> Result<SessionOutcome, SessionError> {
    session.apply_intent(&EditIntent::SetTaskChecked { item, checked })
}

fn insert(session: &mut DocumentSession, text: &str) {
    assert_eq!(
        session
            .apply_intent(&EditIntent::InsertText { text: text.into() })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
}

fn pending_bold(session: &mut DocumentSession) {
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
}

struct Listener(Rc<Cell<(usize, usize)>>);

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

fn listen(session: &mut DocumentSession) -> Rc<Cell<(usize, usize)>> {
    let counts = Rc::new(Cell::new((0, 0)));
    session.add_listener(Box::new(Listener(counts.clone())));
    counts
}

fn assert_same_document(actual: &XiaomuDocument, expected: &XiaomuDocument) {
    assert_eq!(actual.store(), expected.store());
    assert_eq!(actual.revision(), expected.revision());
    assert_eq!(actual.root(), expected.root());
    assert_eq!(actual.version(), expected.version());
}

#[test]
fn exact_checked_state_matrix_patches_only_one_attribute_and_roundtrips_undo_redo() {
    for before in [
        None,
        Some(AttrValue::Null),
        Some(AttrValue::Bool(false)),
        Some(AttrValue::Bool(true)),
    ] {
        for checked in [false, true] {
            let fixture = fixture(before.clone());
            // A reversed selection in another task must neither move nor
            // redirect the target to that task.
            let selection = DocumentSelection::new(
                point(&fixture.document, fixture.second_text, "other中🙂".len()),
                point(&fixture.document, fixture.second_text, 1),
            );
            let mut session = DocumentSession::new(fixture.document.clone(), selection).unwrap();
            let counts = listen(&mut session);
            let changed = before != Some(AttrValue::Bool(checked));
            assert_eq!(
                set_checked(&mut session, fixture.first, checked).unwrap(),
                if changed {
                    SessionOutcome::DocumentChanged
                } else {
                    SessionOutcome::NoChange
                }
            );
            assert_eq!(session.selection(), selection);
            assert_eq!(session.document().root(), fixture.document.root());
            assert_eq!(
                session.document().node_count(),
                fixture.document.node_count()
            );
            assert_eq!(
                session.document().revision().as_u64(),
                fixture.document.revision().as_u64() + u64::from(changed)
            );
            assert_eq!(session.history_depths(), (usize::from(changed), 0));
            assert_eq!(counts.get(), (usize::from(changed), 0));
            let updated = session.document().node(fixture.first).unwrap();
            assert_eq!(updated.attrs(), &attrs(Some(AttrValue::Bool(checked))));
            assert_eq!(updated.kind(), &NodeKind::TaskItem);
            assert_eq!(
                updated.content(),
                fixture.document.node(fixture.first).unwrap().content()
            );
            for node in fixture
                .document
                .store()
                .iter()
                .filter(|node| node.id() != fixture.first)
            {
                assert_eq!(session.document().node(node.id()), Some(node));
            }
            if changed {
                let after = session.document().clone();
                assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
                assert_eq!(session.document().store(), fixture.document.store());
                assert_eq!(
                    session
                        .document()
                        .node(fixture.first)
                        .unwrap()
                        .attrs()
                        .get("checked"),
                    before.as_ref()
                );
                assert_eq!(session.selection(), selection);
                assert_eq!(session.history_depths(), (0, 1));
                assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
                assert_eq!(session.document().store(), after.store());
                assert_eq!(session.selection(), selection);
                assert_eq!(session.history_depths(), (1, 0));
                assert_eq!(counts.get(), (3, 0));
            }
        }
    }
}

#[test]
fn checkbox_preserves_cross_item_mixed_gap_atomic_all_and_cell_selections_exactly() {
    let fixture = fixture(None);
    let cross = DocumentSelection::new(
        point(&fixture.document, fixture.second_text, "other中🙂".len()),
        point(&fixture.document, fixture.first_text, 2),
    );
    let mixed = InlinePoint::new(fixture.tail, TextOffset::ZERO, 1, CursorAffinity::After);
    for selection in [
        cross,
        DocumentSelection::collapsed(mixed),
        DocumentSelection::new(mixed, point(&fixture.document, fixture.tail, 4)),
        DocumentSelection::collapsed(NodeGap::new(fixture.list, 1)),
        DocumentSelection::collapsed(fixture.rule),
        DocumentSelection::all(&fixture.document),
        DocumentSelection::cell_range(
            fixture.cells[1],
            fixture.cells[0],
            DocumentPosition::Gap(NodeGap::new(fixture.row, 1)),
        ),
    ] {
        let mut session = DocumentSession::new(fixture.document.clone(), selection).unwrap();
        assert_eq!(
            set_checked(&mut session, fixture.first, true).unwrap(),
            SessionOutcome::DocumentChanged
        );
        assert_eq!(session.selection(), selection);
        assert_eq!(session.history_depths(), (1, 0));
        session.undo().unwrap();
        assert_eq!(session.document().store(), fixture.document.store());
        assert_eq!(session.selection(), selection);
        session.redo().unwrap();
        assert_eq!(session.selection(), selection);
    }
}

#[test]
fn successful_updates_clear_pending_marks_and_each_form_an_isolated_undo_unit() {
    let fixture = fixture(Some(AttrValue::Bool(false)));
    let mut session = session(&fixture);
    pending_bold(&mut session);
    insert(&mut session, "a");
    let before = session.document().clone();
    let selection = session.selection();
    assert!(session.stored_marks().is_some());
    assert_eq!(
        set_checked(&mut session, fixture.first, true).unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.stored_marks(), None);
    assert_eq!(session.selection(), selection);
    let checked = session.document().clone();
    assert_eq!(
        set_checked(&mut session, fixture.first, false).unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.history_depths(), (3, 0));
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (4, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), before.store());
    session.undo().unwrap();
    assert_eq!(session.document().store(), checked.store());
    session.undo().unwrap();
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), None);
    session.undo().unwrap();
    assert_eq!(session.document().store(), fixture.document.store());
}

#[test]
fn identical_boolean_preserves_pending_marks_typing_group_revision_history_and_listeners() {
    for checked in [false, true] {
        let fixture = fixture(Some(AttrValue::Bool(checked)));
        let mut session = session(&fixture);
        pending_bold(&mut session);
        insert(&mut session, "a");
        let before = session.document().clone();
        let selection = session.selection();
        let marks = session.stored_marks().cloned();
        let counts = listen(&mut session);
        assert_eq!(
            set_checked(&mut session, fixture.first, checked).unwrap(),
            SessionOutcome::NoChange
        );
        assert_same_document(session.document(), &before);
        assert_eq!(session.selection(), selection);
        assert_eq!(session.stored_marks(), marks.as_ref());
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(counts.get(), (0, 0));
        insert(&mut session, "b");
        assert_eq!(
            session.history_depths(),
            (1, 0),
            "no-op must not split typing"
        );
        session.undo().unwrap();
        assert_eq!(session.document().store(), fixture.document.store());
        let before_noop = session.document().clone();
        let before_counts = counts.get();
        assert_eq!(
            set_checked(&mut session, fixture.first, checked).unwrap(),
            SessionOutcome::NoChange
        );
        assert_same_document(session.document(), &before_noop);
        assert_eq!(session.history_depths(), (0, 1), "no-op must retain redo");
        assert_eq!(counts.get(), before_counts);
        session.redo().unwrap();
        assert_eq!(session.history_depths(), (1, 0));
    }
}

struct RejectChecked {
    preflight: bool,
}

impl SessionPolicy for RejectChecked {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if let EditIntent::SetTaskChecked { item, checked } = intent {
            assert!(*checked);
            assert_eq!(
                context.document().node(*item).unwrap().kind(),
                &NodeKind::TaskItem
            );
            assert!(context.stored_marks().is_some());
            if self.preflight {
                return Err(PolicyError::new("checkbox intent refused"));
            }
        }
        Ok(IntentDisposition::Continue)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.store().iter().any(|node| {
            matches!(node.kind(), NodeKind::TaskItem)
                && node.attrs().get("checked") == Some(&AttrValue::Bool(true))
        }) {
            return Err(PolicyError::new("checked task candidate refused"));
        }
        Ok(())
    }
}

#[test]
fn preflight_and_candidate_rejection_restore_all_state_including_typing_group() {
    for preflight in [false, true] {
        let fixture = fixture(Some(AttrValue::Bool(false)));
        let mut session = DocumentSession::new_with_policy(
            fixture.document.clone(),
            DocumentSelection::collapsed(point(&fixture.document, fixture.second_text, 0)),
            Box::new(RejectChecked { preflight }),
        )
        .unwrap();
        pending_bold(&mut session);
        insert(&mut session, "a");
        let before = session.document().clone();
        let selection = session.selection();
        let marks = session.stored_marks().cloned();
        let counts = listen(&mut session);
        assert!(matches!(
            set_checked(&mut session, fixture.first, true),
            Err(SessionError::Policy(_))
        ));
        assert_same_document(session.document(), &before);
        assert_eq!(session.selection(), selection);
        assert_eq!(session.stored_marks(), marks.as_ref());
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(counts.get(), (0, 0));
        insert(&mut session, "b");
        assert_eq!(session.history_depths(), (1, 0));
        session.undo().unwrap();
        assert_eq!(session.document().store(), fixture.document.store());
    }
}

#[test]
fn stale_identity_and_wrong_kinds_fail_without_retargeting_or_touching_editing_state() {
    let fixture = fixture(None);
    let without_first = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode {
            node: fixture.first,
        })
        .apply(&fixture.document)
        .unwrap();
    for (target, expected) in [
        (fixture.first, xiaomu_core::Error::UnknownNode),
        (fixture.second_text, xiaomu_core::Error::InvalidNodeContent),
        (fixture.list, xiaomu_core::Error::InvalidNodeContent),
        (
            fixture.document.root(),
            xiaomu_core::Error::InvalidNodeContent,
        ),
        (fixture.rule, xiaomu_core::Error::InvalidNodeContent),
    ] {
        let mut session = DocumentSession::new(
            without_first.clone(),
            DocumentSelection::collapsed(point(&without_first, fixture.second_text, 0)),
        )
        .unwrap();
        pending_bold(&mut session);
        insert(&mut session, "a");
        let before = session.document().clone();
        let selection = session.selection();
        let marks = session.stored_marks().cloned();
        let counts = listen(&mut session);
        assert_eq!(
            set_checked(&mut session, target, true),
            Err(SessionError::Core(expected))
        );
        assert_same_document(session.document(), &before);
        assert_eq!(session.selection(), selection);
        assert_eq!(session.stored_marks(), marks.as_ref());
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(counts.get(), (0, 0));
        assert_eq!(
            session
                .document()
                .node(fixture.second)
                .unwrap()
                .attrs()
                .get("checked"),
            Some(&AttrValue::Null)
        );
        insert(&mut session, "b");
        assert_eq!(session.history_depths(), (1, 0));
        session.undo().unwrap();
        assert_eq!(session.document().store(), without_first.store());
    }
}

#[test]
fn atomic_selection_plus_checkbox_undo_restores_the_original_selection() {
    let fixture = fixture(None);
    let mut session = session(&fixture);
    let before_selection = session.selection();
    let target_selection = DocumentSelection::all(&fixture.document);
    assert_eq!(
        session
            .apply_intent_with_selection(
                target_selection,
                &EditIntent::SetTaskChecked {
                    item: fixture.first,
                    checked: true
                },
            )
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.selection(), target_selection);
    session.undo().unwrap();
    assert_eq!(session.document().store(), fixture.document.store());
    assert_eq!(session.selection(), before_selection);
    session.redo().unwrap();
    assert_eq!(session.selection(), target_selection);
}

struct IgnoreCheckbox;

impl SessionPolicy for IgnoreCheckbox {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::SetTaskChecked { .. }) {
            assert!(context.stored_marks().is_some());
            return Ok(IntentDisposition::NoChange);
        }
        Ok(IntentDisposition::Continue)
    }
}

#[test]
fn policy_nochange_keeps_checkbox_typing_marks_and_group_untouched() {
    let fixture = fixture(Some(AttrValue::Bool(false)));
    let mut session = DocumentSession::new_with_policy(
        fixture.document.clone(),
        DocumentSelection::collapsed(point(&fixture.document, fixture.second_text, 0)),
        Box::new(IgnoreCheckbox),
    )
    .unwrap();
    pending_bold(&mut session);
    insert(&mut session, "a");
    let before = session.document().clone();
    let selection = session.selection();
    let marks = session.stored_marks().cloned();
    let counts = listen(&mut session);
    assert_eq!(
        set_checked(&mut session, fixture.first, true).unwrap(),
        SessionOutcome::NoChange
    );
    assert_same_document(session.document(), &before);
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), marks.as_ref());
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(counts.get(), (0, 0));
    insert(&mut session, "b");
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), fixture.document.store());
}
