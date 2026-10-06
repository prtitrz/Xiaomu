use super::*;

#[test]
fn defaults_keep_timeless_typing_for_old_and_timestamped_entry_points() {
    struct DefaultPolicy;
    impl SessionPolicy for DefaultPolicy {}
    for mode in 0..3 {
        let (document, node, _) = fixture("", MarkSet::empty(), &[]);
        let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
        let mut session = match mode {
            0 => DocumentSession::new(document, selection),
            1 => DocumentSession::new_with_policy(document, selection, Box::new(DefaultPolicy)),
            _ => Ok(session_with(
                document,
                selection,
                HistoryOptions::new(),
                DefaultTextInputMarks::PreservePending,
            )),
        }
        .unwrap();
        session.apply_intent(&insertion("a")).unwrap();
        type_at(&mut session, "b", 0);
        type_at(&mut session, "c", u64::MAX);
        let target = session.selection();
        session
            .apply_intent_with_selection_at(target, &insertion("d"), timestamp(1))
            .unwrap();
        session
            .apply_intent_with_selection(session.selection(), &insertion("e"))
            .unwrap();
        assert_eq!(session.history_options(), HistoryOptions::new());
        round_trip(&mut session, node, &["", "abcde"]);
    }
}

#[test]
fn five_hundred_ms_is_inclusive_and_501_starts_a_new_group() {
    for gap in [0, 499, 500, 501, 600] {
        let (mut session, node) = setup(timed());
        type_at(&mut session, "a", 1000);
        type_at(&mut session, "b", 1000 + gap);
        if gap <= 500 {
            round_trip(&mut session, node, &["", "ab"]);
        } else {
            round_trip(&mut session, node, &["", "a", "ab"]);
        }
    }
}

#[test]
fn timeout_slides_from_the_last_successful_edit() {
    let (mut session, node) = setup(timed());
    for (value, time) in [("a", 1000), ("b", 1400), ("c", 1800), ("d", 2301)] {
        type_at(&mut session, value, time);
    }
    round_trip(&mut session, node, &["", "abc", "abcd"]);
}

#[test]
fn zero_is_real_time_and_zero_delay_only_groups_equal_times() {
    let (mut session, node) = setup(HistoryOptions::new().with_typing_group_delay_ms(0));
    type_at(&mut session, "a", 0);
    type_at(&mut session, "a", 0);
    type_at(&mut session, "b", 1);
    type_at(&mut session, "b", 1);
    // A repeated timestamp is another edit, not operation deduplication.
    round_trip(&mut session, node, &["", "aa", "aabb"]);
}

#[test]
fn timestamps_near_max_and_maximum_delay_do_not_overflow() {
    let (mut session, node) = setup(timed());
    type_at(&mut session, "a", u64::MAX - 500);
    type_at(&mut session, "b", u64::MAX);
    type_at(&mut session, "c", u64::MAX);
    round_trip(&mut session, node, &["", "abc"]);

    let (mut session, node) = setup(HistoryOptions::new().with_typing_group_delay_ms(u64::MAX));
    type_at(&mut session, "a", 0);
    type_at(&mut session, "b", u64::MAX);
    type_at(&mut session, "c", 0);
    type_at(&mut session, "d", u64::MAX);
    type_at(&mut session, "e", u64::MAX);
    round_trip(&mut session, node, &["", "ab", "abc", "abcde"]);
}

#[test]
fn regressing_successful_edits_commit_separately_without_lowering_high_water() {
    let (mut session, node) = setup(timed());
    for (value, time) in [
        ("a", 1000),
        ("b", 900),
        ("c", 950),
        ("d", 1000),
        ("e", 1001),
    ] {
        type_at(&mut session, value, time);
    }
    round_trip(&mut session, node, &["", "a", "ab", "abc", "abcde"]);
}

#[test]
fn missing_time_isolates_both_sides_and_does_not_reset_high_water() {
    let (mut session, node) = setup(timed());
    type_at(&mut session, "a", 1000);
    session.apply_intent(&insertion("b")).unwrap();
    session
        .apply_intent_with_selection(session.selection(), &insertion("c"))
        .unwrap();
    type_at(&mut session, "d", 999);
    type_at(&mut session, "e", 1000);
    type_at(&mut session, "f", 1001);
    round_trip(
        &mut session,
        node,
        &["", "a", "ab", "abc", "abcd", "abcdef"],
    );
}

#[test]
fn empty_and_policy_no_change_do_not_publish_time_or_extend_timeout() {
    struct Ignore;
    impl SessionPolicy for Ignore {
        fn history_options(&self) -> HistoryOptions {
            timed()
        }
        fn prepare_intent(
            &self,
            _: SessionContext<'_>,
            intent: &EditIntent,
        ) -> Result<IntentDisposition, PolicyError> {
            Ok(
                if matches!(intent, EditIntent::InsertText { text } if text == "ignore") {
                    IntentDisposition::NoChange
                } else {
                    IntentDisposition::Continue
                },
            )
        }
    }
    let (document, node, _) = fixture("", MarkSet::empty(), &[]);
    let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
    let mut session =
        DocumentSession::new_with_policy(document, selection, Box::new(Ignore)).unwrap();
    let counts = listen(&mut session);
    type_at(&mut session, "a", 1000);
    let before = Snapshot::capture(&session, &counts);
    for value in ["", "ignore"] {
        assert_eq!(
            session
                .apply_intent_at(&insertion(value), timestamp(u64::MAX))
                .unwrap(),
            SessionOutcome::NoChange
        );
        before.assert_unchanged(&session, &counts);
    }
    type_at(&mut session, "b", 1200);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(
        session
            .apply_intent_at(&insertion(""), timestamp(1700))
            .unwrap(),
        SessionOutcome::NoChange
    );
    type_at(&mut session, "c", 1701);
    round_trip(&mut session, node, &["", "ab", "abc"]);
}

#[test]
fn replay_and_unrelated_session_origins_are_independent() {
    let (mut first, first_node) = setup(timed());
    let (mut second, second_node) = setup(timed());
    let (mut replay, replay_node) = setup(timed());
    for (value, time) in [("a", 0), ("b", 500), ("c", 1001), ("d", 1001)] {
        type_at(&mut first, value, time);
        type_at(&mut second, value, u64::MAX - 1001 + time);
        type_at(&mut replay, value, time);
        assert_eq!(first.history_depths(), replay.history_depths());
        assert_eq!(first.history_depths(), second.history_depths());
        assert_eq!(contents(&first, first_node), contents(&replay, replay_node));
    }
    round_trip(&mut first, first_node, &["", "ab", "abcd"]);
    round_trip(&mut second, second_node, &["", "ab", "abcd"]);
    round_trip(&mut replay, replay_node, &["", "ab", "abcd"]);
}

#[test]
fn empty_history_option_still_controls_the_existing_barrier() {
    for preserve in [false, true] {
        let options = if preserve {
            timed().with_empty_behavior(EmptyHistoryBehavior::PreserveEditingState)
        } else {
            timed()
        };
        let (mut session, node) = setup(options);
        session
            .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
            .unwrap();
        let counts = listen(&mut session);
        let empty = Snapshot::capture(&session, &counts);
        assert_eq!(session.undo().unwrap(), SessionOutcome::NoChange);
        if preserve {
            empty.assert_unchanged(&session, &counts);
        } else {
            assert_eq!(session.stored_marks(), None);
        }
        type_at(&mut session, "a", 1000);
        let before = Snapshot::capture(&session, &counts);
        assert_eq!(session.redo().unwrap(), SessionOutcome::NoChange);
        if preserve {
            before.assert_unchanged(&session, &counts);
        }
        type_at(&mut session, "b", 1200);
        if preserve {
            round_trip(&mut session, node, &["", "ab"]);
        } else {
            round_trip(&mut session, node, &["", "a", "ab"]);
        }
    }
}

#[test]
fn traversal_closes_groups_without_reviving_or_forgetting_edit_times() {
    let (mut session, node) = setup(timed());
    type_at(&mut session, "a", 1000);
    session.undo().unwrap();
    session.redo().unwrap();
    type_at(&mut session, "b", 1001);
    assert_eq!(session.history_depths(), (2, 0));
    session.undo().unwrap();
    type_at(&mut session, "c", 900);
    type_at(&mut session, "d", 950);
    type_at(&mut session, "e", 1001);
    type_at(&mut session, "f", 1002);
    assert_eq!(session.history_depths().1, 0);
    round_trip(&mut session, node, &["", "a", "ac", "acd", "acdef"]);
}
