use super::*;

fn when_bang(
    node: NodeId,
    reject: impl Fn(&Observation) -> Result<Option<EditFinalization>, PolicyError> + 'static,
) -> (Policy, Observations) {
    policy(move |observation| {
        if text(&observation.candidate, node).contains('!') {
            reject(observation)
        } else {
            Ok(None)
        }
    })
}

// Test both live typing-group recovery and a populated Redo stack. Snapshot
// equality alone would not catch a transiently closed group or consumed redo.
fn assert_rejection_preserves_typing_and_redo(
    make_policy: impl FnOnce(NodeId) -> (Policy, Observations),
    expected: Option<SessionError>,
) {
    let (document, nodes) = fixture(&[""]);
    let node = nodes[0];
    let original = caret(node, 0);
    let (policy, observations) = make_policy(node);
    let mut session =
        DocumentSession::new_with_policy(document.clone(), original, Box::new(policy)).unwrap();
    let events = listen(&mut session);
    bold(&mut session);
    insert(&mut session, "a").unwrap();
    let snapshot = Snapshot::capture(&session, &events);
    let rejected = insert(&mut session, "!");
    if let Some(expected) = &expected {
        assert_eq!(rejected, Err(expected.clone()));
    } else {
        assert!(rejected.is_err(), "{rejected:?}");
    }
    snapshot.assert_unchanged(&session, &events);
    assert_eq!(observations.borrow().len(), 2);
    insert(&mut session, "b").unwrap();
    assert_eq!(text(session.document(), node), "ab");
    assert_eq!(session.history_depths(), (1, 0));
    let typed = session.document().clone();
    let typed_selection = session.selection();
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), original);
    assert_eq!(session.history_depths(), (0, 1));
    bold(&mut session);
    let snapshot = Snapshot::capture(&session, &events);
    let rejected = insert(&mut session, "!");
    if let Some(expected) = expected {
        assert_eq!(rejected, Err(expected));
    } else {
        assert!(rejected.is_err(), "{rejected:?}");
    }
    snapshot.assert_unchanged(&session, &events);
    assert_eq!(observations.borrow().len(), 4);
    session.redo().unwrap();
    assert_eq!(session.document().store(), typed.store());
    assert_eq!(session.selection(), typed_selection);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(observations.borrow().len(), 4);
}

#[test]
fn finalizer_error_restores_pending_marks_typing_group_and_redo_without_notifications() {
    assert_rejection_preserves_typing_and_redo(
        |node| when_bang(node, |_| Err(PolicyError::new("finalizer refused"))),
        Some(SessionError::Policy(PolicyError::new("finalizer refused"))),
    );
}

#[test]
fn final_host_validation_runs_after_amendment_and_rejection_is_atomic() {
    assert_rejection_preserves_typing_and_redo(
        |node| {
            let (mut policy, observations) = when_bang(node, |observation| {
                finalized(append(&observation.candidate, "forbidden"), None)
            });
            policy.reject = Some(|document| {
                document.store().iter().any(|node| {
                    node.content().as_inline().is_some_and(|inline| {
                        inline
                            .runs()
                            .iter()
                            .any(|run| run.text().as_str() == "forbidden")
                    })
                })
            });
            (policy, observations)
        },
        Some(SessionError::Policy(PolicyError::new(
            "final candidate rejected",
        ))),
    );
}

#[test]
fn invalid_extra_transaction_rolls_back_even_after_an_earlier_node_allocation() {
    assert_rejection_preserves_typing_and_redo(
        |node| {
            when_bang(node, |observation| {
                finalized(
                    append(&observation.candidate, "allocated privately").with_step(
                        TransactionStep::RemoveNode {
                            node: observation.candidate.root(),
                        },
                    ),
                    None,
                )
            })
        },
        None,
    );
}

#[test]
fn none_selection_does_not_hide_a_deleted_resolved_endpoint() {
    assert_rejection_preserves_typing_and_redo(
        |node| {
            when_bang(node, move |observation| {
                finalized(
                    append(&observation.candidate, "replacement")
                        .with_step(TransactionStep::RemoveNode { node }),
                    None,
                )
            })
        },
        Some(SessionError::SelectionDeleted),
    );
}

#[test]
fn exact_selection_out_of_bounds_or_on_removed_node_rejects_the_complete_edit() {
    for remove in [false, true] {
        assert_rejection_preserves_typing_and_redo(
            move |node| {
                when_bang(node, move |observation| {
                    let mut tx = append(&observation.candidate, "replacement");
                    if remove {
                        tx.push_step(TransactionStep::RemoveNode { node });
                    }
                    finalized(tx, Some(caret(node, if remove { 0 } else { 100 })))
                })
            },
            Some(SessionError::SelectionInvalid),
        );
    }
}

#[test]
fn finalizer_refusal_restores_atomic_target_original_caret_marks_and_typing_group() {
    let (document, nodes) = fixture(&[""]);
    let node = nodes[0];
    let (policy, observations) = when_bang(node, |_| Err(PolicyError::new("refused target")));
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 0), Box::new(policy)).unwrap();
    bold(&mut session);
    insert(&mut session, "ab").unwrap();
    let events = listen(&mut session);
    let before = Snapshot::capture(&session, &events);
    let target = DocumentSelection::new(point(node, 0), point(node, 1));
    assert!(
        session
            .apply_intent_with_selection(target, &EditIntent::InsertText { text: "!".into() })
            .is_err()
    );
    before.assert_unchanged(&session, &events);
    let observed = observations.borrow().last().unwrap().clone();
    assert_eq!(observed.original, before.selection);
    assert_eq!(observed.before.store(), before.document.store());
    assert_eq!(text(&observed.candidate, node), "!b");
    assert_eq!(
        observed
            .selection
            .as_same_node_inline()
            .unwrap()
            .1
            .text_offset(),
        offset(1)
    );
    insert(&mut session, "c").unwrap();
    assert_eq!(text(session.document(), node), "abc");
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn finalizer_cannot_invalidate_explicit_stored_marks_after_with_a_range_selection() {
    for marks in [
        None,
        Some(MarkSet::empty()),
        Some(MarkSet::new([Mark::Bold]).unwrap()),
    ] {
        let (document, nodes) = fixture(&["ab"]);
        let node = nodes[0];
        let (policy, observations) = policy(move |observation| {
            finalized(
                append(&observation.candidate, "finalized"),
                Some(DocumentSelection::new(point(node, 0), point(node, 2))),
            )
        });
        let mut session =
            DocumentSession::new_with_policy(document, caret(node, 1), Box::new(policy)).unwrap();
        bold(&mut session);
        let events = listen(&mut session);
        let snapshot = Snapshot::capture(&session, &events);
        assert_eq!(
            session.apply_plan(
                EditPlan::new(
                    transaction().with_step(replace(node, 1, 1, "X")),
                    SelectionUpdate::Exact {
                        selection: caret(node, 2)
                    },
                    None,
                )
                .with_stored_marks(marks)
            ),
            Err(SessionError::SelectionInvalid)
        );
        snapshot.assert_unchanged(&session, &events);
        assert_eq!(observations.borrow().len(), 1);
    }
}

#[test]
fn failed_original_selection_resolution_never_invokes_the_finalizer() {
    let (document, nodes) = fixture(&["ab"]);
    let node = nodes[0];
    let (policy, observations) = append_policy();
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 1), Box::new(policy)).unwrap();
    bold(&mut session);
    let events = listen(&mut session);
    let snapshot = Snapshot::capture(&session, &events);
    assert_eq!(
        session.apply_plan(EditPlan::new(
            transaction().with_step(replace(node, 1, 1, "X")),
            SelectionUpdate::Exact {
                selection: caret(node, 99)
            },
            None,
        )),
        Err(SessionError::SelectionInvalid)
    );
    snapshot.assert_unchanged(&session, &events);
    assert!(observations.borrow().is_empty());
}

#[test]
fn rejected_prepared_text_cut_has_no_guard_or_live_side_effects() {
    let (document, nodes) = fixture(&["abcd"]);
    let node = nodes[0];
    let before = DocumentSelection::new(point(node, 1), point(node, 3));
    let (policy, observations) = policy(|_| Err(PolicyError::new("cut finalizer refused")));
    let mut session = DocumentSession::new_with_policy(document, before, Box::new(policy)).unwrap();
    let events = listen(&mut session);
    let snapshot = Snapshot::capture(&session, &events);
    assert!(matches!(
        session.prepare_cut(),
        Err(SessionError::Policy(_))
    ));
    snapshot.assert_unchanged(&session, &events);
    assert_eq!(observations.borrow().len(), 1);
}

#[test]
fn rejected_staged_finalization_never_publishes_intermediate_list_or_target_selection() {
    let (document, nodes) = fixture(&["abc"]);
    let node = nodes[0];
    let (policy, observations) = policy(|_| Err(PolicyError::new("staged finalizer refused")));
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(policy)).unwrap();
    bold(&mut session);
    let events = listen(&mut session);
    let snapshot = Snapshot::capture(&session, &events);
    assert!(
        session
            .apply_intent_with_selection(
                caret(node, 1),
                &EditIntent::TurnInto {
                    kind: NodeKind::BulletList
                }
            )
            .is_err()
    );
    snapshot.assert_unchanged(&session, &events);
    assert_eq!(observations.borrow().len(), 1);
    assert_eq!(observations.borrow()[0].original, snapshot.selection);
}

#[test]
fn finalizer_can_repair_a_candidate_before_final_host_validation() {
    let (document, nodes) = fixture(&["a"]);
    let node = nodes[0];
    let before = caret(node, 1);
    let (mut policy, observations) = policy(move |observation| {
        assert_eq!(text(&observation.candidate, node), "a!");
        finalized(transaction().with_step(replace(node, 1, 2, "?")), None)
    });
    policy.reject = Some(|document| {
        document.store().iter().any(|node| {
            node.content().as_inline().is_some_and(|inline| {
                inline
                    .runs()
                    .iter()
                    .any(|run| run.text().as_str().contains('!'))
            })
        })
    });
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
    let events = listen(&mut session);
    insert(&mut session, "!").unwrap();
    assert_eq!(text(session.document(), node), "a?");
    assert_round_trip(&mut session, &document, before, &observations, &events);
}

#[test]
fn exact_all_cannot_leave_retained_pending_marks_on_a_noncaret_selection() {
    let (document, nodes) = fixture(&["a"]);
    let node = nodes[0];
    let (policy, observations) = policy(|observation| {
        finalized(
            transaction(),
            Some(DocumentSelection::all(&observation.candidate)),
        )
    });
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 1), Box::new(policy)).unwrap();
    bold(&mut session);
    let events = listen(&mut session);
    let snapshot = Snapshot::capture(&session, &events);
    assert_eq!(
        insert(&mut session, "X"),
        Err(SessionError::SelectionInvalid)
    );
    snapshot.assert_unchanged(&session, &events);
    assert_eq!(observations.borrow().len(), 1);
}

#[test]
fn amendment_invalidating_input_rule_reversal_rejects_the_forward_edit() {
    use xiaomu_runtime::session::InputRuleUndoSpec;

    let (document, nodes) = fixture(&["rule", "survivor"]);
    let node = nodes[0];
    let survivor = nodes[1];
    let (policy, observations) = policy(move |_| {
        finalized(
            transaction().with_step(TransactionStep::RemoveNode { node }),
            Some(caret(survivor, 0)),
        )
    });
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 4), Box::new(policy)).unwrap();
    bold(&mut session);
    let events = listen(&mut session);
    let snapshot = Snapshot::capture(&session, &events);
    let reversal = InputRuleUndoSpec::new(
        transaction().with_step(replace(node, 0, 1, "rule ")),
        caret(node, 5),
    )
    .unwrap();
    let plan = EditPlan::new(
        transaction().with_step(replace(node, 0, 4, "R")),
        SelectionUpdate::Exact {
            selection: caret(node, 1),
        },
        None,
    )
    .with_input_rule_undo(reversal);
    assert!(matches!(
        session.apply_plan(plan),
        Err(SessionError::Core(_))
    ));
    snapshot.assert_unchanged(&session, &events);
    assert!(!session.input_rule_undo_available());
    assert_eq!(observations.borrow().len(), 1);
}
