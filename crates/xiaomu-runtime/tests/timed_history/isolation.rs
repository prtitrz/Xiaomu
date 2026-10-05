use super::*;
use xiaomu_core::document::{AtomKind, NodeKind};
use xiaomu_runtime::session::InputRuleUndoSpec;

#[test]
fn timed_multibyte_batches_keep_utf8_coordinates_atoms_and_both_marks_modes() {
    for mode in [
        DefaultTextInputMarks::PreservePending,
        DefaultTextInputMarks::ConsumePending,
    ] {
        for mixed in [false, true] {
            let atoms = if mixed {
                vec![(0, AtomKind::new("mention").unwrap(), marks([Mark::Italic]))]
            } else {
                vec![]
            };
            let (document, node, identities) = fixture("", MarkSet::empty(), &atoms);
            let initial = DocumentSelection::collapsed(point(&document, node, 0, 0));
            let mut session = session_with(document, initial, combined(), mode);
            session
                .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
                .unwrap();
            // At the leading marked atom the existing insertion inheritance
            // is Italic; SetMark(Bold) adds Bold instead of removing Italic.
            let expected = if mixed {
                marks([Mark::Bold, Mark::Italic])
            } else {
                marks([Mark::Bold])
            };
            assert_eq!(session.stored_marks(), Some(&expected));
            type_at(&mut session, "甲🙂", 1000);
            assert_eq!(
                session.stored_marks(),
                (mode == DefaultTextInputMarks::PreservePending).then_some(&expected)
            );
            type_at(&mut session, "e\u{301}\t", 1500);
            assert_eq!(contents(&session, node), "甲🙂e\u{301}\t");
            let (_, focus) = session.selection().as_same_node_inline().unwrap();
            assert_eq!(focus.text_offset().as_usize(), "甲🙂e\u{301}\t".len());
            assert_eq!(focus.atom_index(), 0);
            assert_eq!(text_marks(session.document(), node, 0), &expected);
            assert_eq!(
                text_marks(session.document(), node, "甲🙂".len()),
                &expected
            );
            if mixed {
                assert_eq!(
                    atom_marks(session.document(), identities[0]),
                    &marks([Mark::Italic])
                );
            }
            round_trip(&mut session, node, &["", "甲🙂e\u{301}\t"]);
        }
    }
}

#[test]
fn mixed_inline_empty_input_keeps_its_existing_isolated_commit_boundary() {
    let (document, node, _) = fixture(
        "",
        MarkSet::empty(),
        &[(0, AtomKind::hard_break(), MarkSet::empty())],
    );
    let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
    let mut session = session_with(
        document,
        selection,
        combined(),
        DefaultTextInputMarks::ConsumePending,
    );
    type_at(&mut session, "X", 1000);
    let store = session.document().store().clone();
    // Unlike the text-only empty planner, this existing atom route publishes
    // an isolated empty ReplaceInlineText. Do not silently redefine it here.
    assert_eq!(
        session.apply_intent_at(
            &EditIntent::InsertText {
                text: String::new()
            },
            timestamp(9000)
        ),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(session.document().store(), &store);
    assert_eq!(session.history_depths(), (2, 0));
    type_at(&mut session, "Y", 1100);
    type_at(&mut session, "Z", 1200);
    round_trip(&mut session, node, &["", "X", "X", "XYZ"]);
}

#[test]
fn stamped_paste_composition_deletion_and_line_break_keep_existing_isolation() {
    for (intent, middle) in [
        (EditIntent::PasteText { text: "Y".into() }, "XY"),
        (
            EditIntent::CommitComposition {
                range: range(1, 1),
                text: "Y".into(),
            },
            "XY",
        ),
        (EditIntent::InsertLineBreak, "X\n"),
        (EditIntent::Backspace, ""),
    ] {
        let (mut session, node) = setup(combined());
        type_at(&mut session, "X", 1000);
        assert_eq!(
            session
                .apply_intent_at(&intent, timestamp(u64::MAX))
                .unwrap(),
            SessionOutcome::DocumentChanged
        );
        assert_eq!(session.history_depths(), (2, 0));
        type_at(&mut session, "Z", 1100);
        type_at(&mut session, "W", 1200);
        let after = format!("{middle}ZW");
        round_trip(&mut session, node, &["", "X", middle, &after]);
    }
}

#[test]
fn no_change_delete_and_mark_commands_keep_their_explicit_boundary() {
    for intent in [EditIntent::Delete, EditIntent::SetMark { mark: Mark::Bold }] {
        let (mut session, node) = setup(combined());
        type_at(&mut session, "X", 1000);
        assert_eq!(
            session
                .apply_intent_at(&intent, timestamp(u64::MAX))
                .unwrap(),
            SessionOutcome::NoChange
        );
        type_at(&mut session, "Y", 1100);
        type_at(&mut session, "Z", 1200);
        round_trip(&mut session, node, &["", "X", "XYZ"]);
    }
}

#[test]
fn raw_apply_remains_an_isolated_recorded_entry_even_when_empty() {
    let (mut session, node) = setup(combined());
    type_at(&mut session, "X", 1000);
    session
        .apply(&Transaction::new(TransactionOrigin::System))
        .unwrap();
    type_at(&mut session, "Y", 1100);
    type_at(&mut session, "Z", 1200);
    round_trip(&mut session, node, &["", "X", "X", "XYZ"]);
}

struct Host;
impl SessionPolicy for Host {
    fn history_options(&self) -> HistoryOptions {
        combined()
    }
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::Backspace) && context.input_rule_undo_available() {
            return Ok(IntentDisposition::UndoInputRule);
        }
        let EditIntent::InsertText { text } = intent else {
            return Ok(IntentDisposition::Continue);
        };
        if text == "stored" {
            return Ok(IntentDisposition::StoredMarks(Some(marks([Mark::Bold]))));
        }
        if text != "host" && text != "rule" {
            return Ok(IntentDisposition::Continue);
        }
        let (_, focus) = context.selection().as_same_node_inline().unwrap();
        let node = focus.node_id();
        let at = focus.text_offset().as_usize();
        let after = DocumentSelection::collapsed(InlinePoint::new(
            node,
            offset(at + 1),
            0,
            focus.affinity(),
        ));
        let mut plan = EditPlan::new(
            replace(node, at, at, "R"),
            SelectionUpdate::Exact { selection: after },
            None,
        );
        if text == "rule" {
            plan = plan.with_input_rule_undo(
                InputRuleUndoSpec::new(
                    replace(node, at, at + 1, "rule"),
                    DocumentSelection::collapsed(InlinePoint::new(
                        node,
                        offset(at + 4),
                        0,
                        focus.affinity(),
                    )),
                )
                .unwrap(),
            );
        }
        Ok(IntentDisposition::Apply(plan))
    }
}
fn host() -> (DocumentSession, NodeId) {
    let (document, node, _) = fixture("", MarkSet::empty(), &[]);
    let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
    (
        DocumentSession::new_with_policy(document, selection, Box::new(Host)).unwrap(),
        node,
    )
}

#[test]
fn host_apply_and_stored_marks_ignore_stamps_and_keep_isolation() {
    for text in ["host", "stored"] {
        let (mut session, node) = host();
        type_at(&mut session, "X", 1000);
        let outcome = session
            .apply_intent_at(&insertion(text), timestamp(u64::MAX))
            .unwrap();
        assert_eq!(
            outcome,
            if text == "host" {
                SessionOutcome::DocumentChanged
            } else {
                SessionOutcome::NoChange
            }
        );
        type_at(&mut session, "Y", 1100);
        type_at(&mut session, "Z", 1200);
        if text == "host" {
            round_trip(&mut session, node, &["", "X", "XR", "XRYZ"]);
        } else {
            round_trip(&mut session, node, &["", "X", "XYZ"]);
        }
    }
}

#[test]
fn input_rule_reversal_is_isolated_and_does_not_publish_supplied_time() {
    let (mut session, node) = host();
    type_at(&mut session, "X", 1000);
    type_at(&mut session, "rule", u64::MAX);
    assert!(session.input_rule_undo_available());
    assert_eq!(
        session
            .apply_intent_at(&EditIntent::Backspace, timestamp(u64::MAX))
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert!(!session.input_rule_undo_available());
    type_at(&mut session, "Y", 1100);
    type_at(&mut session, "Z", 1200);
    round_trip(&mut session, node, &["", "X", "XR", "Xrule", "XruleYZ"]);
}

#[test]
fn structural_split_remains_isolated_and_following_typing_starts_fresh() {
    let (mut session, first) = setup(combined());
    type_at(&mut session, "X", 1000);
    session
        .apply_intent_at(&EditIntent::SplitBlock, timestamp(u64::MAX))
        .unwrap();
    let (_, focus) = session.selection().as_same_node_inline().unwrap();
    let tail = focus.node_id();
    assert_ne!(tail, first);
    type_at(&mut session, "Y", 1100);
    type_at(&mut session, "Z", 1200);
    assert_eq!(session.history_depths(), (3, 0));
    let final_document = session.document().clone();
    let final_selection = session.selection();
    session.undo().unwrap();
    assert_eq!(contents(&session, tail), "");
    session.undo().unwrap();
    assert!(session.document().node(tail).is_none());
    assert_eq!(contents(&session, first), "X");
    session.undo().unwrap();
    assert_eq!(contents(&session, first), "");
    for _ in 0..3 {
        session.redo().unwrap();
    }
    assert_eq!(session.document().store(), final_document.store());
    assert_eq!(session.selection(), final_selection);
    assert_eq!(
        session.document().node(tail).unwrap().kind(),
        &NodeKind::Paragraph
    );
}
