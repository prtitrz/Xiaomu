//! Publicly reachable empty-stack behavior and construction-time isolation.

use super::*;

fn bold() -> MarkSet {
    MarkSet::new([Mark::Bold]).unwrap()
}
fn pending_bold(s: &mut DocumentSession) {
    assert_eq!(
        s.apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold }),
        Ok(SessionOutcome::NoChange)
    );
    assert_eq!(s.stored_marks(), Some(&bold()));
}

#[test]
fn empty_preserve_keeps_pending_bold_selection_full_history_and_notifications_unchanged() {
    let f = fixture("abcdef");
    // Selection capture and empty-stack behavior are independent options.
    let options =
        HistoryOptions::new().with_empty_behavior(EmptyHistoryBehavior::PreserveEditingState);
    for has_undo in [false, true] {
        let mut s = session(&f, options);
        let events = listen(&mut s);
        if has_undo {
            insert(&mut s, "x");
        }
        pending_bold(&mut s);
        assert!(
            !s.history.typing_group_open(),
            "mark command already broke grouping"
        );
        let before = Snapshot::capture(&mut s, &events);
        if !has_undo {
            assert_eq!(s.undo(), Ok(SessionOutcome::NoChange));
            before.assert_unchanged(&mut s, &events);
        }
        assert_eq!(s.redo(), Ok(SessionOutcome::NoChange));
        before.assert_unchanged(&mut s, &events);
        insert(&mut s, "q");
        let inline = s
            .document()
            .node(f.first)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        assert!(
            inline
                .runs()
                .iter()
                .any(|run| run.text().as_str() == "q" && run.marks() == &bold())
        );
    }
}

#[test]
fn empty_undo_preserves_pending_marks_and_parked_redo_entry() {
    let f = fixture("abcdef");
    let mut s = session(&f, capture_options());
    let events = listen(&mut s);
    insert(&mut s, "x");
    let edited = s.document().clone();
    select(&mut s, caret(f.first, 3));
    s.undo().unwrap();
    assert_eq!(s.history_depths(), (0, 1));
    pending_bold(&mut s);
    let before = Snapshot::capture(&mut s, &events);
    assert_eq!(s.undo(), Ok(SessionOutcome::NoChange));
    before.assert_unchanged(&mut s, &events);
    traverse(
        &mut s,
        &events,
        Direction::Redo,
        &edited,
        caret(f.first, 3),
        (1, 0),
    );
    assert_eq!(s.stored_marks(), None);
}

#[test]
fn empty_legacy_behavior_clears_pending_marks_even_when_selection_capture_is_enabled() {
    let f = fixture("abcdef");
    assert_eq!(HistoryOptions::default(), HistoryOptions::new());
    assert_eq!(
        HistoryOptions::new().selection_mode(),
        HistorySelectionMode::Recorded
    );
    assert_eq!(
        HistoryOptions::new().empty_behavior(),
        EmptyHistoryBehavior::ClearPendingMarks
    );
    for selection_mode in [
        HistorySelectionMode::Recorded,
        HistorySelectionMode::CaptureOnTraversal,
    ] {
        let mut s = session(
            &f,
            HistoryOptions::new().with_selection_mode(selection_mode),
        );
        let events = listen(&mut s);
        for direction in [Direction::Undo, Direction::Redo] {
            pending_bold(&mut s);
            let revision = s.document().revision();
            let selection = s.selection();
            let before_history = history_image(&mut s);
            assert_eq!(direction.run(&mut s), Ok(SessionOutcome::NoChange));
            assert_eq!(s.stored_marks(), None);
            assert_eq!(s.document().revision(), revision);
            assert_eq!(s.document().store(), f.document.store());
            assert_eq!(s.selection(), selection);
            assert_eq!(history_image(&mut s), before_history);
            assert!(events.borrow().is_empty());
        }
    }
}

// A host rule reached entirely through the public intent/policy seam. Its
// reversal is deliberately simple; syntax and product input rules are not tested.
struct RulePolicy;
impl SessionPolicy for RulePolicy {
    fn history_options(&self) -> HistoryOptions {
        capture_options()
    }
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::Backspace) && context.input_rule_undo_available() {
            return Ok(IntentDisposition::UndoInputRule);
        }
        if let EditIntent::InsertText { text } = intent
            && text == "rule"
        {
            let (start, _) = context.selection().as_same_node_inline().unwrap();
            let node = start.node_id();
            let at = start.text_offset().as_usize();
            let spec = InputRuleUndoSpec::new(
                transaction().with_step(replace(node, at, at + 1, "rule")),
                caret(node, at + 4),
            )
            .unwrap();
            return Ok(IntentDisposition::Apply(
                EditPlan::new(
                    transaction().with_step(replace(node, at, at, "r")),
                    SelectionUpdate::Exact {
                        selection: caret(node, at + 1),
                    },
                    None,
                )
                .with_input_rule_undo(spec),
            ));
        }
        Ok(IntentDisposition::Continue)
    }
}

#[test]
fn empty_redo_preserves_real_typing_group_separately_from_marks_and_preserves_rule_token() {
    let f = fixture("abcdef");
    for preserve in [false, true] {
        let options = HistoryOptions::new().with_empty_behavior(if preserve {
            EmptyHistoryBehavior::PreserveEditingState
        } else {
            EmptyHistoryBehavior::ClearPendingMarks
        });
        let mut s = session(&f, options);
        let events = listen(&mut s);
        insert(&mut s, "a");
        let one_char = s.document().clone();
        assert!(s.history.typing_group_open());
        assert_eq!(
            s.stored_marks(),
            None,
            "no mark command disguises the grouping check"
        );
        events.borrow_mut().clear();
        let revision = s.document().revision();
        assert_eq!(s.redo(), Ok(SessionOutcome::NoChange));
        assert_eq!(s.document().revision(), revision);
        assert!(events.borrow().is_empty());
        assert_eq!(s.history.typing_group_open(), preserve);
        insert(&mut s, "b");
        assert_eq!(s.history_depths(), (if preserve { 1 } else { 2 }, 0));
        traverse(
            &mut s,
            &events,
            Direction::Undo,
            if preserve { &f.document } else { &one_char },
            caret(f.first, if preserve { 0 } else { 1 }),
            (if preserve { 0 } else { 1 }, 1),
        );
    }

    let mut s = DocumentSession::new_with_policy(
        f.document.clone(),
        caret(f.first, 0),
        Box::new(RulePolicy),
    )
    .unwrap();
    let events = listen(&mut s);
    insert(&mut s, "rule");
    assert_eq!(plain(&s, f.first), "rabcdef");
    assert!(s.input_rule_undo_available());
    let before = Snapshot::capture(&mut s, &events);
    assert_eq!(s.redo(), Ok(SessionOutcome::NoChange));
    before.assert_unchanged(&mut s, &events);
    assert_eq!(
        s.apply_intent(&EditIntent::Backspace),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(plain(&s, f.first), "ruleabcdef");
    assert!(!s.input_rule_undo_available());
}

#[test]
fn policy_options_are_read_once_at_construction_and_remain_per_session() {
    struct CountReads {
        options: HistoryOptions,
        calls: Rc<Cell<usize>>,
    }
    impl SessionPolicy for CountReads {
        fn history_options(&self) -> HistoryOptions {
            // Observation-only spy: its returned configuration never changes.
            self.calls.set(self.calls.get() + 1);
            self.options
        }
    }
    let f = fixture("abcdef");
    let capture_calls = Rc::new(Cell::new(0));
    let default_calls = Rc::new(Cell::new(0));
    let mut capture = DocumentSession::new_with_policy(
        f.document.clone(),
        caret(f.first, 0),
        Box::new(CountReads {
            options: capture_options(),
            calls: capture_calls.clone(),
        }),
    )
    .unwrap();
    let mut recorded = DocumentSession::new_with_policy(
        f.document.clone(),
        caret(f.first, 0),
        Box::new(CountReads {
            options: HistoryOptions::new(),
            calls: default_calls.clone(),
        }),
    )
    .unwrap();
    assert_eq!((capture_calls.get(), default_calls.get()), (1, 1));
    for s in [&mut capture, &mut recorded] {
        insert(s, "X");
        select(s, caret(f.first, 2));
        s.undo().unwrap();
        select(s, caret(f.first, 4));
        s.redo().unwrap();
    }
    assert_eq!(capture.selection(), caret(f.first, 2));
    assert_eq!(recorded.selection(), caret(f.first, 1));
    pending_bold(&mut capture);
    pending_bold(&mut recorded);
    capture.redo().unwrap();
    recorded.redo().unwrap();
    assert_eq!(capture.stored_marks(), Some(&bold()));
    assert_eq!(recorded.stored_marks(), None);
    assert_eq!(capture.history_options(), capture_options());
    assert_eq!(recorded.history_options(), HistoryOptions::new());
    assert_eq!((capture_calls.get(), default_calls.get()), (1, 1));
    let plain = DocumentSession::new(f.document.clone(), caret(f.first, 0)).unwrap();
    assert_eq!(plain.history_options(), HistoryOptions::new());
}
