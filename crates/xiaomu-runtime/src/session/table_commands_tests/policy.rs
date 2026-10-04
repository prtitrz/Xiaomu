//! Host overrides and atomic refusals of the real typed table commands.

use super::*;

struct ExactCommandPolicy {
    intent: EditIntent,
    expected_target: DocumentSelection,
    table: NodeId,
    after: DocumentSelection,
}
impl SessionPolicy for ExactCommandPolicy {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if intent != &self.intent {
            return Ok(IntentDisposition::Continue);
        }
        assert_eq!(
            context.selection(),
            self.expected_target,
            "policy receives live target before default routing"
        );
        let range = context.selection().active_cell_range().unwrap();
        let step = match intent {
            EditIntent::MergeTableCells => TransactionStep::MergeTableCells {
                table: self.table,
                rect: range.logical_rect(context.document()).unwrap(),
            },
            EditIntent::SplitTableCell => TransactionStep::SplitTableCell {
                table: self.table,
                cell: range.anchor(),
            },
            _ => unreachable!(),
        };
        Ok(IntentDisposition::Apply(EditPlan::new(
            Transaction::new(TransactionOrigin::UserInput).with_step(step),
            SelectionUpdate::Exact {
                selection: self.after,
            },
            None,
        )))
    }
}

#[test]
fn real_typed_commands_reach_policy_first_and_accept_exact_plans_at_atomic_targets() {
    for (spans, intent) in [
        (false, EditIntent::MergeTableCells),
        (true, EditIntent::SplitTableCell),
    ] {
        let f = fixture(spans);
        let target = range(&f, 0, if spans { 0 } else { 3 });
        let before = caret(f.intro);
        let after = caret(f.paragraphs[0]);
        let mut session = DocumentSession::new_with_policy(
            f.document.clone(),
            before,
            Box::new(ExactCommandPolicy {
                intent: intent.clone(),
                expected_target: target,
                table: f.table,
                after,
            }),
        )
        .unwrap();
        let count = listen(&mut session);
        assert_eq!(
            session.apply_intent_with_selection(target, &intent),
            Ok(SessionOutcome::DocumentChanged)
        );
        assert_eq!(
            session.selection(),
            after,
            "host exact caret overrides generic range semantics"
        );
        assert_eq!(
            count.get(),
            1,
            "tentative target is never separately published"
        );
        let changed = session.document().clone();
        session.undo().unwrap();
        assert_eq!(session.document().store(), f.document.store());
        assert_eq!(
            session.selection(),
            before,
            "undo restores pre-action selection, not tentative target"
        );
        session.redo().unwrap();
        assert_eq!(session.document().store(), changed.store());
        assert_eq!(session.selection(), after);
    }
}

#[test]
fn invalid_policy_exact_selection_rolls_back_real_merge_atomically() {
    let f = fixture(false);
    let target = range(&f, 0, 3);
    let mut session = DocumentSession::new_with_policy(
        f.document.clone(),
        caret(f.intro),
        Box::new(ExactCommandPolicy {
            intent: EditIntent::MergeTableCells,
            expected_target: target,
            table: f.table,
            // Absorbed endpoint is invalid after merge; no candidate may escape.
            after: target,
        }),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    let before = State::capture(&session);
    let count = listen(&mut session);
    assert_eq!(
        session.apply_intent_with_selection(target, &EditIntent::MergeTableCells),
        Err(SessionError::SelectionInvalid)
    );
    before.assert_unchanged(&session);
    assert_eq!(count.get(), 0);
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
}

struct RejectShapeChange {
    table: NodeId,
    count: usize,
}
impl SessionPolicy for RejectShapeChange {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.table_grid(self.table).unwrap().origins().len() == self.count {
            Ok(())
        } else {
            Err(PolicyError::new("cell-count change rejected"))
        }
    }
}

#[test]
fn default_commands_roll_back_candidate_policy_errors_and_preserve_redo() {
    for (spans, intent) in [
        (false, EditIntent::MergeTableCells),
        (true, EditIntent::SplitTableCell),
    ] {
        let f = fixture(spans);
        let mut session = DocumentSession::new_with_policy(
            f.document.clone(),
            caret(f.intro),
            Box::new(RejectShapeChange {
                table: f.table,
                count: f.cells.len(),
            }),
        )
        .unwrap();
        session
            .apply_intent(&EditIntent::InsertText { text: "x".into() })
            .unwrap();
        session.undo().unwrap();
        let target = range(&f, 0, if spans { 0 } else { 3 });
        let before = State::capture(&session);
        let count = listen(&mut session);
        assert_eq!(
            session.apply_intent_with_selection(target, &intent),
            Err(SessionError::Policy(PolicyError::new(
                "cell-count change rejected"
            )))
        );
        before.assert_unchanged(&session);
        assert_eq!(count.get(), 0);
        session.redo().unwrap();
        assert_eq!(session.history_depths(), (1, 0));
    }
}

#[test]
fn real_core_split_resource_error_rolls_back_marks_group_history_and_listeners() {
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "A");
    let cell = builder
        .insert(
            NodeKind::TableCell,
            attrs(&[("colspan", AttrValue::Integer(100_001))]),
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
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mut session = DocumentSession::new(document.clone(), caret(p)).unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    let before = State::capture(&session);
    let count = listen(&mut session);
    assert_eq!(
        session.apply_intent(&EditIntent::SplitTableCell),
        Err(SessionError::Core(xiaomu_core::Error::TableResourceLimit))
    );
    before.assert_unchanged(&session);
    assert_eq!(count.get(), 0);
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
}
