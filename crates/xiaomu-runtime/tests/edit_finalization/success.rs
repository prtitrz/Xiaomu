use super::*;
use xiaomu_core::selection::NodeGap;
use xiaomu_core::transaction::DocumentTemplate;

#[test]
fn default_text_input_is_finalized_once_with_resolved_selection_and_one_undo_unit() {
    let (document, nodes) = fixture(&["a"]);
    let node = nodes[0];
    let before = caret(node, 1);
    let (policy, observations) = append_policy();
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
    assert!(
        observations.borrow().is_empty(),
        "construction is not an edit"
    );
    bold(&mut session);
    let marks = session.stored_marks().cloned();
    let events = listen(&mut session);

    assert_eq!(
        insert(&mut session, "中"),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(text(session.document(), node), "a中");
    assert_eq!(children(session.document()).len(), 2);
    assert_eq!(
        text(session.document(), children(session.document())[1]),
        "finalized"
    );
    assert_eq!(session.stored_marks(), marks.as_ref());
    assert_eq!(observations.borrow().len(), 1);
    let observed = observations.borrow()[0].clone();
    assert_eq!(observed.before.store(), document.store());
    assert_eq!(observed.original, before);
    assert_eq!(observed.origin, DocumentChangeOrigin::Local);
    assert_eq!(text(&observed.candidate, node), "a中");
    assert_eq!(children(&observed.candidate), nodes);
    assert_eq!(
        observed
            .selection
            .as_same_node_inline()
            .unwrap()
            .1
            .text_offset(),
        offset(4)
    );
    assert_eq!(session.selection(), observed.selection);
    assert_round_trip(&mut session, &document, before, &observations, &events);
}

#[test]
fn none_selection_maps_the_resolved_plan_selection_through_the_amendment() {
    let (document, nodes) = fixture(&["abc"]);
    let node = nodes[0];
    let before = caret(node, 0);
    let resolved = caret(node, 3);
    let (policy, observations) = policy(move |observation| {
        assert_eq!(observation.selection, resolved);
        finalized(transaction().with_step(replace(node, 0, 0, "Q")), None)
    });
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
    let events = listen(&mut session);
    session
        .apply_plan(EditPlan::new(
            transaction().with_step(replace(node, 1, 1, "X")),
            SelectionUpdate::Exact {
                selection: resolved,
            },
            None,
        ))
        .unwrap();
    assert_eq!(text(session.document(), node), "QaXbc");
    assert_eq!(session.selection(), caret(node, 4));
    assert_eq!(observations.borrow().len(), 1);
    assert_round_trip(&mut session, &document, before, &observations, &events);
}

#[test]
fn raw_core_apply_uses_the_same_finalization_and_exact_identity_history() {
    let (document, nodes) = fixture(&["abc"]);
    let node = nodes[0];
    let before = caret(node, 1);
    let (policy, observations) = append_policy();
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
    let events = listen(&mut session);
    session
        .apply(&Transaction::new(TransactionOrigin::System).with_step(replace(node, 2, 2, "X")))
        .unwrap();
    assert_eq!(text(session.document(), node), "abXc");
    assert_eq!(session.selection(), before);
    assert_eq!(observations.borrow().len(), 1);
    assert_eq!(observations.borrow()[0].origin, DocumentChangeOrigin::Local);
    assert_round_trip(&mut session, &document, before, &observations, &events);
}

#[test]
fn prepare_intent_host_plan_is_finalized_without_reentering_the_planner() {
    let (document, nodes) = fixture(&["abc"]);
    let node = nodes[0];
    let before = DocumentSelection::new(point(node, 3), point(node, 1));
    let (mut policy, observations) = append_policy();
    policy.host_plan = Some(EditPlan::new(
        transaction().with_step(TransactionStep::SetNodeKind {
            node,
            kind: NodeKind::CodeBlock,
        }),
        SelectionUpdate::PreserveSelection,
        None,
    ));
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
    let events = listen(&mut session);
    session
        .apply_intent(&EditIntent::TurnInto {
            kind: NodeKind::Quote,
        })
        .unwrap();
    assert_eq!(
        session.document().node(node).unwrap().kind(),
        &NodeKind::CodeBlock
    );
    assert_eq!(session.selection(), before);
    assert_eq!(observations.borrow().len(), 1);
    assert_round_trip(&mut session, &document, before, &observations, &events);
}

#[test]
fn staged_list_conversion_finalizes_only_the_completed_candidate_and_original_target_context() {
    let (document, nodes) = fixture(&["abc"]);
    let node = nodes[0];
    let before = caret(node, 3);
    let target = caret(node, 1);
    let (policy, observations) = append_policy();
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
    let events = listen(&mut session);
    session
        .apply_intent_with_selection(
            target,
            &EditIntent::TurnInto {
                kind: NodeKind::BulletList,
            },
        )
        .unwrap();
    assert_eq!(observations.borrow().len(), 1);
    let observed = observations.borrow()[0].clone();
    assert_eq!(observed.before.store(), document.store());
    assert_eq!(observed.original, before);
    assert_eq!(observed.selection, target);
    assert_eq!(children(&observed.candidate).len(), 1);
    assert_eq!(
        observed
            .candidate
            .node(children(&observed.candidate)[0])
            .unwrap()
            .kind(),
        &NodeKind::BulletList
    );
    assert_eq!(children(session.document()).len(), 2);
    assert_round_trip(&mut session, &document, before, &observations, &events);
}

#[test]
fn prepared_text_cut_finalizes_before_guard_and_never_again_at_publication() {
    let (document, nodes) = fixture(&["abcd"]);
    let node = nodes[0];
    let before = DocumentSelection::new(point(node, 1), point(node, 3));
    let (policy, observations) = append_policy();
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
    let events = listen(&mut session);
    let snapshot = Snapshot::capture(&session, &events);
    {
        let prepared = session.prepare_cut().unwrap().unwrap();
        assert_eq!(prepared.clipboard_slice().plain_text(), "bc");
        assert_eq!(observations.borrow().len(), 1);
        assert!(events.borrow().is_empty());
        assert_eq!(text(&observations.borrow()[0].candidate, node), "ad");
        // Dropping a fully finalized guard has no live editing side effects.
    }
    snapshot.assert_unchanged(&session, &events);
    let prepared = session.prepare_cut().unwrap().unwrap();
    assert_eq!(observations.borrow().len(), 2);
    assert_eq!(prepared.publish(), SessionOutcome::DocumentChanged);
    assert_eq!(observations.borrow().len(), 2);
    assert_eq!(text(session.document(), node), "ad");
    assert_eq!(session.selection(), caret(node, 1));
    assert_eq!(children(session.document()).len(), 2);
    assert_round_trip(&mut session, &document, before, &observations, &events);
}

#[test]
fn map_existing_and_exact_all_include_appended_root_children_in_both_directions() {
    for exact in [false, true] {
        for backward in [false, true] {
            let (document, nodes) = fixture(&["abc"]);
            let node = nodes[0];
            let all = DocumentSelection::all(&document);
            let before = if backward {
                DocumentSelection::new(all.focus(), all.anchor())
            } else {
                all
            };
            let (policy, observations) = policy(move |observation| {
                assert!(observation.selection.is_all(&observation.candidate));
                let selection = exact.then(|| {
                    let start = NodeGap::new(observation.candidate.root(), 0);
                    let end = NodeGap::new(
                        observation.candidate.root(),
                        children(&observation.candidate).len() + 1,
                    );
                    if backward {
                        DocumentSelection::new(end, start)
                    } else {
                        DocumentSelection::new(start, end)
                    }
                });
                finalized(append(&observation.candidate, "finalized"), selection)
            });
            let mut session =
                DocumentSession::new_with_policy(document.clone(), before, Box::new(policy))
                    .unwrap();
            let events = listen(&mut session);
            session
                .apply_plan(EditPlan::new(
                    transaction().with_step(TransactionStep::SetNodeKind {
                        node,
                        kind: NodeKind::CodeBlock,
                    }),
                    SelectionUpdate::PreserveSelection,
                    None,
                ))
                .unwrap();
            let after_all = DocumentSelection::all(session.document());
            let expected = if backward {
                DocumentSelection::new(after_all.focus(), after_all.anchor())
            } else {
                after_all
            };
            assert_eq!(session.selection(), expected);
            assert_eq!(observations.borrow().len(), 1);
            assert_round_trip(&mut session, &document, before, &observations, &events);
        }
    }
}

#[test]
fn exact_final_selection_preserves_range_direction_and_affinity_after_extra_text() {
    for backward in [false, true] {
        let (document, nodes) = fixture(&["abc"]);
        let node = nodes[0];
        let before = caret(node, 1);
        let start = InlinePoint::new(node, offset(1), 0, CursorAffinity::After);
        let end = InlinePoint::new(node, offset(5), 0, CursorAffinity::Before);
        let after = if backward {
            DocumentSelection::new(end, start)
        } else {
            DocumentSelection::new(start, end)
        };
        let (policy, observations) = policy(move |_| {
            finalized(
                transaction().with_step(replace(node, 0, 0, "Q")),
                Some(after),
            )
        });
        let mut session =
            DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
        let events = listen(&mut session);
        insert(&mut session, "X").unwrap();
        assert_eq!(text(session.document(), node), "QaXbc");
        assert_eq!(session.selection(), after);
        assert_round_trip(&mut session, &document, before, &observations, &events);
    }
}

#[test]
fn none_and_append_finalizers_preserve_native_typing_coalescence() {
    for append_nodes in [false, true] {
        let (document, nodes) = fixture(&[""]);
        let node = nodes[0];
        let before = caret(node, 0);
        let (policy, observations) = policy(move |observation| {
            if append_nodes {
                finalized(append(&observation.candidate, "finalized"), None)
            } else {
                Ok(None)
            }
        });
        let mut session =
            DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
        bold(&mut session);
        let events = listen(&mut session);
        insert(&mut session, "a").unwrap();
        insert(&mut session, "b").unwrap();
        assert_eq!(text(session.document(), node), "ab");
        assert_eq!(
            children(session.document()).len(),
            if append_nodes { 3 } else { 1 }
        );
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(observations.borrow().len(), 2);
        assert_eq!(events.borrow().len(), 2);
        let after = session.document().clone();
        let after_selection = session.selection();
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
        assert_eq!(session.selection(), before);
        session.redo().unwrap();
        assert_eq!(session.document().store(), after.store());
        assert_eq!(session.selection(), after_selection);
        assert_eq!(observations.borrow().len(), 2);
    }
}

#[test]
fn selection_marks_and_empty_intents_never_invoke_finalization() {
    let (document, nodes) = fixture(&["ab"]);
    let node = nodes[0];
    let (policy, observations) = append_policy();
    let mut session =
        DocumentSession::new_with_policy(document.clone(), caret(node, 0), Box::new(policy))
            .unwrap();
    session.set_document_selection(caret(node, 1)).unwrap();
    bold(&mut session);
    let events = listen(&mut session);
    let snapshot = Snapshot::capture(&session, &events);
    assert_eq!(insert(&mut session, ""), Ok(SessionOutcome::NoChange));
    snapshot.assert_unchanged(&session, &events);
    assert!(observations.borrow().is_empty());
}

#[test]
fn empty_and_same_value_raw_commits_retain_legacy_history_but_skip_the_hook() {
    for host_plan in [false, true] {
        let (document, nodes) = fixture(&["abc"]);
        let node = nodes[0];
        let (policy, observations) = append_policy();
        let mut session =
            DocumentSession::new_with_policy(document.clone(), caret(node, 1), Box::new(policy))
                .unwrap();
        let events = listen(&mut session);
        for tx in [
            transaction(),
            transaction().with_step(replace(node, 0, 3, "abc")),
        ] {
            let outcome = if host_plan {
                session.apply_plan(EditPlan::new(tx, SelectionUpdate::MapExisting, None))
            } else {
                session.apply(&tx)
            };
            assert_eq!(outcome, Ok(SessionOutcome::DocumentChanged));
            assert_eq!(session.document().store(), document.store());
        }
        assert!(observations.borrow().is_empty());
        assert_eq!(session.history_depths(), (2, 0));
        assert_eq!(events.borrow().len(), 2);
        assert!(session.document().revision().as_u64() > document.revision().as_u64());
    }
}

#[test]
fn external_snapshot_import_and_its_undo_redo_never_finalize() {
    let (document, nodes) = fixture(&["old"]);
    let before = caret(nodes[0], 1);
    let (incoming, _) = fixture(&["imported", "content"]);
    let (policy, observations) = append_policy();
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(policy)).unwrap();
    let events = listen(&mut session);
    session
        .apply_plan(
            EditPlan::new(
                Transaction::new(TransactionOrigin::System).with_step(
                    TransactionStep::ReplaceDocument {
                        template: DocumentTemplate::capture(&incoming).unwrap(),
                    },
                ),
                SelectionUpdate::AllDocument,
                None,
            )
            .with_change_origin(DocumentChangeOrigin::External),
        )
        .unwrap();
    assert!(observations.borrow().is_empty());
    assert_eq!(children(session.document()).len(), 2);
    assert_eq!(events.borrow()[0].origin, DocumentChangeOrigin::External);
    let imported = session.document().clone();
    let after = session.selection();
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    assert_eq!(session.document().store(), imported.store());
    assert_eq!(session.selection(), after);
    assert!(observations.borrow().is_empty());
    assert_eq!(events.borrow().len(), 3);
    assert_eq!(events.borrow()[1].origin, DocumentChangeOrigin::Local);
    assert_eq!(events.borrow()[2].origin, DocumentChangeOrigin::Local);
}

#[test]
fn input_rule_reversal_is_validated_after_finalization_and_finalizes_as_a_new_forward_edit() {
    use xiaomu_runtime::session::InputRuleUndoSpec;

    struct UndoPolicy(Policy);
    impl SessionPolicy for UndoPolicy {
        fn finalize_edit(
            &self,
            context: FinalizeEditContext<'_>,
        ) -> Result<Option<EditFinalization>, PolicyError> {
            self.0.finalize_edit(context)
        }
        fn prepare_intent(
            &self,
            context: SessionContext<'_>,
            intent: &EditIntent,
        ) -> Result<IntentDisposition, PolicyError> {
            if matches!(intent, EditIntent::Backspace) && context.input_rule_undo_available() {
                Ok(IntentDisposition::UndoInputRule)
            } else {
                Ok(IntentDisposition::Continue)
            }
        }
    }

    let (document, nodes) = fixture(&["rule"]);
    let node = nodes[0];
    let before = caret(node, 4);
    let (policy, observations) = append_policy();
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(UndoPolicy(policy)))
            .unwrap();
    let events = listen(&mut session);
    session
        .apply_plan(
            EditPlan::new(
                transaction().with_step(replace(node, 0, 4, "R")),
                SelectionUpdate::Exact {
                    selection: caret(node, 1),
                },
                None,
            )
            .with_input_rule_undo(
                InputRuleUndoSpec::new(
                    transaction().with_step(replace(node, 0, 1, "rule ")),
                    caret(node, 5),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    assert!(session.input_rule_undo_available());
    assert_eq!(
        observations.borrow().len(),
        1,
        "reversal validation must not recursively finalize"
    );
    let transformed = session.document().clone();
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "rule ");
    assert_eq!(children(session.document()).len(), 3);
    assert!(!session.input_rule_undo_available());
    assert_eq!(observations.borrow().len(), 2);
    assert_eq!(session.history_depths(), (2, 0));
    let restored = session.document().clone();
    session.undo().unwrap();
    assert_eq!(session.document().store(), transformed.store());
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    session.redo().unwrap();
    assert_eq!(session.document().store(), restored.store());
    assert_eq!(observations.borrow().len(), 2);
    assert_eq!(events.borrow().len(), 6);
}

#[test]
fn cell_range_row_append_context_retains_the_original_selection_before_tentative_collapse() {
    let (document, nodes) = fixture(&["intro"]);
    let document = transaction()
        .with_step(TransactionStep::InsertTable {
            parent: document.root(),
            index: 1,
            rows: 1,
            columns: 1,
        })
        .apply(&document)
        .unwrap();
    let cell = document
        .store()
        .iter()
        .find(|node| matches!(node.kind(), NodeKind::TableCell))
        .unwrap()
        .id();
    let (policy, observations) = append_policy();
    let mut session =
        DocumentSession::new_with_policy(document.clone(), caret(nodes[0], 0), Box::new(policy))
            .unwrap();
    session.set_cell_range_selection(cell, cell).unwrap();
    let original = session.selection();
    assert!(original.active_cell_range().is_some());
    let events = listen(&mut session);
    session.apply_intent(&EditIntent::MoveToNextCell).unwrap();
    assert_eq!(observations.borrow().len(), 1);
    let observed = observations.borrow()[0].clone();
    assert_eq!(observed.original, original);
    assert_eq!(observed.before.store(), document.store());
    assert!(observed.selection.active_cell_range().is_none());
    assert_eq!(
        observed
            .candidate
            .store()
            .iter()
            .filter(|node| matches!(node.kind(), NodeKind::TableRow))
            .count(),
        2
    );
    assert_eq!(children(&observed.candidate).len(), 2);
    assert_eq!(children(session.document()).len(), 3);
    assert_eq!(session.history_depths(), (1, 0));
    let after = session.document().clone();
    let after_selection = session.selection();
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    session.redo().unwrap();
    assert_eq!(session.document().store(), after.store());
    assert_eq!(session.selection(), after_selection);
    assert_eq!(observations.borrow().len(), 1);
    assert_eq!(events.borrow().len(), 3);
}
