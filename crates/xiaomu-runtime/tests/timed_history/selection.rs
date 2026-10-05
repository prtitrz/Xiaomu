use super::*;
use xiaomu_core::document::{AtomKind, NodeStoreBuilder};
use xiaomu_runtime::session::InputRuleUndoSpec;

#[test]
fn default_close_and_delay_only_close_on_selection_away_and_back() {
    for options in [HistoryOptions::new(), timed()] {
        let (mut session, node) = setup(options);
        type_at(&mut session, "a", 1000);
        select(&mut session, node, 0);
        select(&mut session, node, 1);
        type_at(&mut session, "b", 1200);
        round_trip(&mut session, node, &["", "a", "ab"]);
    }
}

#[test]
fn preserve_without_delay_allows_untimed_away_and_back() {
    let options =
        HistoryOptions::new().with_selection_only_grouping(SelectionOnlyGrouping::Preserve);
    let (mut session, node) = setup(options);
    session.apply_intent(&insertion("a")).unwrap();
    select(&mut session, node, 0);
    select(&mut session, node, 1);
    type_at(&mut session, "b", u64::MAX);
    round_trip(&mut session, node, &["", "ab"]);
}

#[test]
fn timed_away_and_back_obeys_the_edit_gap_including_exact_500() {
    for gap in [499, 500, 501] {
        let (mut session, node) = setup(combined());
        let counts = listen(&mut session);
        type_at(&mut session, "a", 1000);
        let revision = session.document().revision();
        session
            .apply_intent_at(&move_to(CaretMove::ToStart), timestamp(1499))
            .unwrap();
        session
            .apply_intent_at(&move_to(CaretMove::ToEnd), timestamp(1500))
            .unwrap();
        assert_eq!(session.document().revision(), revision);
        assert_eq!(counts.get(), (1, 2));
        type_at(&mut session, "b", 1000 + gap);
        if gap <= 500 {
            round_trip(&mut session, node, &["", "ab"]);
        } else {
            round_trip(&mut session, node, &["", "a", "ab"]);
        }
    }
}

#[test]
fn arbitrarily_late_stamped_selection_does_not_advance_the_history_clock() {
    let (mut session, node) = setup(combined());
    type_at(&mut session, "a", 1000);
    for direction in [CaretMove::ToStart, CaretMove::ToEnd] {
        session
            .apply_intent_at(&move_to(direction), timestamp(u64::MAX))
            .unwrap();
    }
    type_at(&mut session, "b", 1200);
    round_trip(&mut session, node, &["", "ab"]);
}

#[test]
fn preserve_still_requires_exact_position_node_affinity_and_atom_gap() {
    let (mut session, node) = setup(combined());
    type_at(&mut session, "a", 1000);
    select(&mut session, node, 0);
    type_at(&mut session, "b", 1100);
    type_at(&mut session, "c", 1200);
    round_trip(&mut session, node, &["", "a", "bca"]);

    let mut builder = NodeStoreBuilder::new();
    let (first, _) = block(&mut builder, "", MarkSet::empty(), &[]);
    let (second, _) = block(&mut builder, "", MarkSet::empty(), &[]);
    let document = finish(builder, vec![first, second]);
    let initial = DocumentSelection::collapsed(point(&document, first, 0, 0));
    let mut session = session_with(
        document,
        initial,
        combined(),
        DefaultTextInputMarks::PreservePending,
    );
    type_at(&mut session, "a", 1000);
    select(&mut session, second, 0);
    type_at(&mut session, "b", 1100);
    assert_eq!(session.history_depths(), (2, 0));
    session.undo().unwrap();
    assert_eq!(contents(&session, first), "a");
    assert_eq!(contents(&session, second), "");
    session.undo().unwrap();
    assert_eq!(session.selection(), initial);

    let (mut session, node) = setup(combined());
    type_at(&mut session, "a", 1000);
    let after = InlinePoint::new(node, offset(1), 0, CursorAffinity::After);
    session.set_inline_selection(after, after).unwrap();
    type_at(&mut session, "b", 1100);
    round_trip(&mut session, node, &["", "a", "ab"]);

    let (document, node, atoms) = fixture(
        "",
        MarkSet::empty(),
        &[(0, AtomKind::new("mention").unwrap(), marks([Mark::Italic]))],
    );
    let initial = DocumentSelection::collapsed(point(&document, node, 0, 0));
    let mut session = session_with(
        document.clone(),
        initial,
        combined(),
        DefaultTextInputMarks::PreservePending,
    );
    type_at(&mut session, "a", 1000);
    let after_atom = point(session.document(), node, 1, 1);
    session
        .set_inline_selection(after_atom, after_atom)
        .unwrap();
    type_at(&mut session, "b", 1100);
    assert_eq!(
        atom_marks(session.document(), atoms[0]),
        &marks([Mark::Italic])
    );
    round_trip(&mut session, node, &["", "a", "ab"]);
}

#[test]
fn atomic_changed_target_is_a_barrier_even_when_only_affinity_changes() {
    for changed in [false, true] {
        let (mut session, node) = setup(combined());
        type_at(&mut session, "a", 1000);
        let target = if changed {
            DocumentSelection::collapsed(InlinePoint::new(
                node,
                offset(1),
                0,
                CursorAffinity::After,
            ))
        } else {
            session.selection()
        };
        let counts = listen(&mut session);
        session
            .apply_intent_with_selection_at(target, &insertion("b"), timestamp(1100))
            .unwrap();
        assert_eq!(counts.get(), (1, 0));
        type_at(&mut session, "c", 1200);
        if changed {
            round_trip(&mut session, node, &["", "a", "abc"]);
        } else {
            round_trip(&mut session, node, &["", "abc"]);
        }
    }
}

#[test]
fn reverse_replacement_is_isolated_and_undo_restores_the_whole_atomic_action() {
    let (mut session, node) = setup(combined());
    type_at(&mut session, "abc", 1000);
    let live = session.selection();
    let target = DocumentSelection::new(
        point(session.document(), node, 2, 0),
        point(session.document(), node, 1, 0),
    );
    session
        .apply_intent_with_selection_at(target, &insertion("X"), timestamp(1100))
        .unwrap();
    type_at(&mut session, "Y", 1200);
    assert_eq!(contents(&session, node), "aXYc");
    assert_eq!(session.history_depths(), (3, 0));
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(contents(&session, node), "abc");
    assert_eq!(session.selection(), live);
}

#[test]
fn preserved_selection_grouping_still_clears_pending_marks() {
    let (mut session, node) = setup(combined());
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
    type_at(&mut session, "a", 1000);
    assert_eq!(session.stored_marks(), Some(&marks([Mark::Bold])));
    select(&mut session, node, 0);
    assert_eq!(session.stored_marks(), None);
    select(&mut session, node, 1);
    type_at(&mut session, "b", 1200);
    assert_eq!(
        text_marks(session.document(), node, 1),
        &marks([Mark::Bold])
    );
    assert_eq!(session.stored_marks(), None);
    round_trip(&mut session, node, &["", "ab"]);
}

#[test]
fn explicit_close_is_idempotent_and_only_closes_before_the_next_group() {
    let (mut session, node) = setup(combined());
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
    let counts = listen(&mut session);
    type_at(&mut session, "X", 1000);
    let before = Snapshot::capture(&session, &counts);
    session.close_history_group();
    session.close_history_group();
    before.assert_unchanged(&session, &counts);
    type_at(&mut session, "Y", 1100);
    type_at(&mut session, "Z", 1200);
    round_trip(&mut session, node, &["", "X", "XYZ"]);
}

#[test]
fn explicit_close_does_not_reset_the_successful_time_high_water() {
    let (mut session, node) = setup(combined());
    type_at(&mut session, "a", 1000);
    session.close_history_group();
    type_at(&mut session, "b", 900);
    type_at(&mut session, "c", 950);
    type_at(&mut session, "d", 1000);
    type_at(&mut session, "e", 1001);
    round_trip(&mut session, node, &["", "a", "ab", "abc", "abcde"]);
}

#[test]
fn close_preserves_rule_token_but_selection_installation_still_invalidates_it() {
    struct Rule(EditPlan);
    impl SessionPolicy for Rule {
        fn history_options(&self) -> HistoryOptions {
            combined()
        }
        fn prepare_intent(
            &self,
            _: SessionContext<'_>,
            intent: &EditIntent,
        ) -> Result<IntentDisposition, PolicyError> {
            Ok(
                if matches!(intent, EditIntent::InsertText { text } if text == "rule") {
                    IntentDisposition::Apply(self.0.clone())
                } else {
                    IntentDisposition::Continue
                },
            )
        }
    }
    for changed in [false, true] {
        let (document, node, _) = fixture("", MarkSet::empty(), &[]);
        let initial = DocumentSelection::collapsed(point(&document, node, 0, 0));
        let after = DocumentSelection::collapsed(InlinePoint::new(
            node,
            offset(1),
            0,
            CursorAffinity::Before,
        ));
        let plan = EditPlan::new(
            replace(node, 0, 0, "R"),
            SelectionUpdate::Exact { selection: after },
            None,
        )
        .with_stored_marks(Some(marks([Mark::Bold])))
        .with_input_rule_undo(InputRuleUndoSpec::new(replace(node, 0, 1, ""), initial).unwrap());
        let mut session =
            DocumentSession::new_with_policy(document, initial, Box::new(Rule(plan))).unwrap();
        let counts = listen(&mut session);
        type_at(&mut session, "rule", 1000);
        assert!(session.input_rule_undo_available());
        let before = Snapshot::capture(&session, &counts);
        session.close_history_group();
        before.assert_unchanged(&session, &counts);
        let selection = if changed { initial } else { after };
        let outcome = session.set_document_selection(selection).unwrap();
        assert_eq!(
            outcome,
            if changed {
                SessionOutcome::SelectionChanged
            } else {
                SessionOutcome::NoChange
            }
        );
        assert!(!session.input_rule_undo_available());
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(session.document().store(), before.document.store());
        assert_eq!(session.document().revision(), before.document.revision());
        assert_eq!(
            session.stored_marks(),
            if changed { None } else { before.marks.as_ref() }
        );
        assert_eq!(counts.get(), (1, usize::from(changed)));
    }
}

#[test]
fn pure_all_or_whole_node_selection_can_return_to_exact_caret_without_refreshing_time() {
    for whole_node in [false, true] {
        let (mut session, node) = setup(combined());
        type_at(&mut session, "a", 1000);
        let original = session.selection();
        if whole_node {
            session.set_node_selection(node).unwrap();
        } else {
            let all = DocumentSelection::all(session.document());
            session.set_document_selection(all).unwrap();
        }
        session.set_document_selection(original).unwrap();
        type_at(&mut session, "b", 1500);
        round_trip(&mut session, node, &["", "ab"]);
    }
}
