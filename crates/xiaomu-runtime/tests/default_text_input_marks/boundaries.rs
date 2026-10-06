//! Atomic refusal, host-plan precedence and unaffected legacy entry points.

use super::*;

struct RejectingOptions;
impl SessionPolicy for RejectingOptions {
    fn default_text_input_marks(&self) -> DefaultTextInputMarks {
        DefaultTextInputMarks::ConsumePending
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::InsertText { text } if text == "skip") {
            Ok(IntentDisposition::NoChange)
        } else {
            Ok(IntentDisposition::Continue)
        }
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document
            .store()
            .iter()
            .filter_map(|node| node.content().as_inline())
            .flat_map(|content| content.runs())
            .any(|run| run.text().as_str().contains('!'))
        {
            Err(PolicyError::new("candidate rejected"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn policy_nochange_invalid_utf8_ranges_and_candidate_rejections_publish_nothing() {
    for empty in [false, true] {
        for open_group in [false, true] {
            let (document, node, _) = fixture("中", MarkSet::empty(), &[]);
            let selection = DocumentSelection::collapsed(point(&document, node, 3, 0));
            let mut session = DocumentSession::new_with_policy(
                document.clone(),
                selection,
                Box::new(RejectingOptions),
            )
            .unwrap();
            let counts = listen(&mut session);
            let expected = pending(&mut session, empty);
            if open_group {
                insert(&mut session, "a");
            }
            let before = Snapshot::capture(&session, &counts);
            assert_eq!(
                session
                    .apply_intent(&EditIntent::InsertText {
                        text: "skip".into()
                    })
                    .unwrap(),
                SessionOutcome::NoChange
            );
            before.assert_unchanged(&session, &counts);
            let target = DocumentSelection::collapsed(point(session.document(), node, 0, 0));
            assert_eq!(
                session
                    .apply_intent_with_selection(
                        target,
                        &EditIntent::InsertText {
                            text: "skip".into()
                        }
                    )
                    .unwrap(),
                SessionOutcome::NoChange
            );
            before.assert_unchanged(&session, &counts);
            assert!(matches!(
                session.apply_intent_with_selection(
                    target,
                    &EditIntent::InsertText { text: "!".into() }
                ),
                Err(SessionError::Policy(_))
            ));
            before.assert_unchanged(&session, &counts);
            // Both a mid-scalar endpoint and an out-of-bounds endpoint must
            // roll back the tentative composition history boundary.
            for invalid in [range(1, 1), range(0, 99)] {
                assert!(matches!(
                    session.apply_intent(&EditIntent::CommitComposition {
                        range: invalid,
                        text: "x".into()
                    }),
                    Err(SessionError::Core(_))
                ));
                before.assert_unchanged(&session, &counts);
            }
            for intent in [
                EditIntent::InsertText { text: "!".into() },
                EditIntent::CommitComposition {
                    range: range(0, 3),
                    text: "!".into(),
                },
            ] {
                assert!(matches!(
                    session.apply_intent(&intent),
                    Err(SessionError::Policy(_))
                ));
                before.assert_unchanged(&session, &counts);
            }
            insert(&mut session, "b");
            assert_eq!(
                text_marks(session.document(), node, if open_group { 4 } else { 3 }),
                &expected
            );
            assert_eq!(session.stored_marks(), None);
            assert_eq!(session.history_depths(), (1, 0));
            session.undo().unwrap();
            assert_eq!(session.document().store(), document.store());
            assert_eq!(session.selection(), selection);
        }
    }
}

#[test]
fn atom_crossing_composition_core_failure_does_not_consume_prepared_marks() {
    for empty in [false, true] {
        let (document, node, atoms) = fixture(
            "a中b",
            MarkSet::empty(),
            &[(1, AtomKind::hard_break(), marks([Mark::Italic]))],
        );
        let selection = DocumentSelection::collapsed(point(&document, node, 5, 0));
        let mut session = session(&document, selection, Mode::Consume);
        let counts = listen(&mut session);
        pending(&mut session, empty);
        let before = Snapshot::capture(&session, &counts);
        // Both UTF-8 endpoints are valid. Planning produces a nonempty input
        // plan and decorated marks-after; Core rejects the intervening atom.
        assert!(matches!(
            session.apply_intent(&EditIntent::CommitComposition {
                range: range(0, 5),
                text: "X".into(),
            }),
            Err(SessionError::Core(_))
        ));
        before.assert_unchanged(&session, &counts);
        assert_eq!(
            atom_marks(session.document(), atoms[0]),
            &marks([Mark::Italic])
        );
        insert(&mut session, "Y");
        assert_eq!(session.stored_marks(), None);
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
    }
}

#[test]
fn rejected_or_ignored_input_preserves_an_existing_redo_entry_and_pending_marks() {
    let (document, node, _) = fixture("中", MarkSet::empty(), &[]);
    let selection = DocumentSelection::collapsed(point(&document, node, 3, 0));
    let mut session =
        DocumentSession::new_with_policy(document.clone(), selection, Box::new(RejectingOptions))
            .unwrap();
    let counts = listen(&mut session);
    insert(&mut session, "a");
    let after = session.document().clone();
    let after_selection = session.selection();
    session.undo().unwrap();
    pending(&mut session, false);
    let before = Snapshot::capture(&session, &counts);
    assert_eq!(before.depths, (0, 1));
    for intent in [
        EditIntent::InsertText {
            text: String::new(),
        },
        EditIntent::InsertText {
            text: "skip".into(),
        },
    ] {
        assert_eq!(
            session.apply_intent(&intent).unwrap(),
            SessionOutcome::NoChange
        );
        before.assert_unchanged(&session, &counts);
    }
    assert!(
        session
            .apply_intent(&EditIntent::CommitComposition {
                range: range(1, 1),
                text: "x".into()
            })
            .is_err()
    );
    before.assert_unchanged(&session, &counts);
    assert!(
        session
            .apply_intent(&EditIntent::InsertText { text: "!".into() })
            .is_err()
    );
    before.assert_unchanged(&session, &counts);
    session.redo().unwrap();
    assert_eq!(session.document().store(), after.store());
    assert_eq!(session.selection(), after_selection);
}

struct HostPlan {
    after: Option<MarkSet>,
}
impl SessionPolicy for HostPlan {
    fn default_text_input_marks(&self) -> DefaultTextInputMarks {
        DefaultTextInputMarks::ConsumePending
    }

    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        let EditIntent::InsertText { text } = intent else {
            return Ok(IntentDisposition::Continue);
        };
        if text != "host" {
            return Ok(IntentDisposition::Continue);
        }
        let focus = context.selection().as_single_node().unwrap().focus();
        let range = TextRange::new(focus.offset(), focus.offset()).unwrap();
        Ok(IntentDisposition::Apply(
            EditPlan::new(
                Transaction::new(TransactionOrigin::UserInput).with_step(
                    TransactionStep::ReplaceText {
                        node: focus.node_id(),
                        range,
                        replacement: text.clone(),
                    },
                ),
                SelectionUpdate::CaretAfterReplacement,
                Some(PrimaryEdit::new(focus.node_id(), range, text.len())),
            )
            .with_stored_marks(self.after.clone()),
        ))
    }
}

#[test]
fn host_apply_on_insert_text_respects_explicit_some_italic_and_none_marks_after() {
    for after in [Some(marks([Mark::Italic])), None] {
        let (document, node, _) = fixture("", MarkSet::empty(), &[]);
        let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
        let mut session = DocumentSession::new_with_policy(
            document.clone(),
            selection,
            Box::new(HostPlan {
                after: after.clone(),
            }),
        )
        .unwrap();
        let counts = listen(&mut session);
        pending(&mut session, false);
        insert(&mut session, "host");
        assert_eq!(text(session.document(), node), "host");
        assert_eq!(text_marks(session.document(), node, 0), &MarkSet::empty());
        assert_eq!(session.stored_marks(), after.as_ref());
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(counts.get(), (1, 0));
        let hosted = session.document().clone();
        insert(&mut session, "中");
        assert_eq!(
            text_marks(session.document(), node, 4),
            &after.clone().unwrap_or_default()
        );
        assert_eq!(session.stored_marks(), None);
        assert_eq!(session.history_depths(), (2, 0));
        session.undo().unwrap();
        assert_eq!(session.document().store(), hosted.store());
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
    }
}

#[test]
fn paste_delete_backspace_enter_and_empty_composition_deletion_keep_legacy_pending_marks() {
    for mode in MODES {
        for empty in [false, true] {
            for intent in [
                EditIntent::PasteText { text: "P".into() },
                EditIntent::Delete,
                EditIntent::Backspace,
                EditIntent::SplitBlock,
                EditIntent::InsertLineBreak,
                EditIntent::CommitComposition {
                    range: range(0, 1),
                    text: String::new(),
                },
            ] {
                let (document, node, _) = fixture("ab", MarkSet::empty(), &[]);
                let selection = DocumentSelection::collapsed(point(&document, node, 1, 0));
                let mut session = session(&document, selection, mode);
                let counts = listen(&mut session);
                let expected = pending(&mut session, empty);
                assert_eq!(
                    session.apply_intent(&intent).unwrap(),
                    SessionOutcome::DocumentChanged,
                    "{intent:?}"
                );
                assert_eq!(
                    session.stored_marks(),
                    Some(&expected),
                    "{mode:?}: {intent:?}"
                );
                assert_eq!(session.history_depths(), (1, 0));
                assert_eq!(counts.get(), (1, 0));
                let after = session.document().clone();
                let after_selection = session.selection();
                insert(&mut session, "中");
                assert_eq!(session.history_depths(), (2, 0), "{intent:?}");
                session.undo().unwrap();
                assert_eq!(session.document().store(), after.store());
                assert_eq!(session.selection(), after_selection);
                session.undo().unwrap();
                assert_eq!(session.document().store(), document.store());
                assert_eq!(session.selection(), selection);
            }
        }
    }
}

#[test]
fn raw_empty_and_nonempty_transactions_keep_the_legacy_clear_and_history_boundary() {
    for mode in MODES {
        for nonempty in [false, true] {
            let (document, node, _) = fixture("ab", MarkSet::empty(), &[]);
            let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
            let mut session = session(&document, selection, mode);
            let counts = listen(&mut session);
            pending(&mut session, false);
            let mut transaction = Transaction::new(TransactionOrigin::UserInput);
            if nonempty {
                transaction.push_step(TransactionStep::ReplaceText {
                    node,
                    range: range(1, 1),
                    replacement: "R".into(),
                });
            }
            assert_eq!(
                session.apply(&transaction).unwrap(),
                SessionOutcome::DocumentChanged
            );
            assert_eq!(session.stored_marks(), None);
            assert_ne!(session.document().revision(), document.revision());
            assert_eq!(session.history_depths(), (1, 0));
            assert_eq!(counts.get(), (1, 0));
            let after = session.document().clone();
            insert(&mut session, "中");
            assert_eq!(session.history_depths(), (2, 0));
            session.undo().unwrap();
            assert_eq!(session.document().store(), after.store());
            session.undo().unwrap();
            assert_eq!(session.document().store(), document.store());
        }
    }
}
