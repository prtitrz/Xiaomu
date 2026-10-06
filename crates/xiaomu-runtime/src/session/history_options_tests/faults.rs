//! PRIVATE fault sentinels, not public host workflows or mutable policy rules.
//!
//! Every valid entry was admitted under the same immutable policy. The first
//! three tests deliberately corrupt one private entry to reach Core, selection
//! and candidate rejection gates. No unsafe code, guard bypass or consumer
//! callback mutation is involved. Other stack entries and transients must stay
//! byte-for-byte/logically identical, and a repaired retry must capture afresh.

use super::*;

struct StablePolicy(HistoryOptions);
impl SessionPolicy for StablePolicy {
    fn history_options(&self) -> HistoryOptions {
        self.0
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document
            .store()
            .iter()
            .any(|node| node.attrs().get("history-test-reject").is_some())
        {
            return Err(PolicyError::new("immutable test candidate rule"));
        }
        Ok(())
    }
}

fn seeded(f: &Fixture, options: HistoryOptions) -> (DocumentSession, Events) {
    let mut s = DocumentSession::new_with_policy(
        f.document.clone(),
        caret(f.first, 0),
        Box::new(StablePolicy(options)),
    )
    .unwrap();
    let events = listen(&mut s);
    timed_insert(&mut s, "a", 1_000);
    select(&mut s, caret(f.second, 0));
    timed_insert(&mut s, "b", 1_100);
    select(&mut s, caret(f.first, 1));
    timed_insert(&mut s, "c", 1_200);
    let grouping = s.history.grouping_state();
    s.undo().unwrap();
    assert_eq!(s.history_depths(), (2, 1));
    select(&mut s, caret(f.first, 3));

    // This combined private state is intentionally adversarial. Successful
    // traversal closes groups and real selection setters discard tokens. We
    // restore the previous full anchor only to exercise rollback sentinels;
    // do not call this combined state publicly reachable.
    s.stored_marks = Some(MarkSet::new([Mark::Bold]).unwrap());
    s.history.restore_grouping_state(grouping);
    s.history_selection_before = Some(caret(f.second, 1));
    let selection = s.selection();
    let spec = InputRuleUndoSpec::new(transaction(), selection).unwrap();
    s.input_rule_undo = Some(
        s.prepare_input_rule_undo(spec, s.document(), selection)
            .unwrap(),
    );
    assert!(s.input_rule_undo_available());
    (s, events)
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    Core,
    Selection,
    Candidate,
}

fn transaction_mut(entry: &mut HistoryEntry, direction: Direction) -> &mut Transaction {
    match direction {
        Direction::Undo => &mut entry.undo,
        Direction::Redo => &mut entry.redo,
    }
}
fn target_mut(entry: &mut HistoryEntry, direction: Direction) -> &mut DocumentSelection {
    match direction {
        Direction::Undo => &mut entry.before_selection,
        Direction::Redo => &mut entry.after_selection,
    }
}

fn corrupt(s: &mut DocumentSession, f: &Fixture, direction: Direction, fault: Fault) -> EntryImage {
    let grouping = s.history.grouping_state();
    let mut entry = direction.take(s);
    let original = EntryImage::from(&entry);
    match fault {
        Fault::Core => {
            // Force Core failure after a candidate allocation, rather than only
            // an early guard. No allocated node may leak to a later real edit.
            let tx = transaction_mut(&mut entry, direction);
            tx.push_step(TransactionStep::InsertNode {
                parent: f.document.root(),
                index: 0,
                kind: NodeKind::Paragraph,
                attrs: NodeAttrs::empty(),
                content: NodeContent::empty_inline(),
            });
            tx.push_step(TransactionStep::RemoveNode {
                node: f.document.root(),
            });
        }
        Fault::Selection => {
            *target_mut(&mut entry, direction) = caret(f.first, 999);
        }
        Fault::Candidate => {
            transaction_mut(&mut entry, direction).push_step(TransactionStep::SetNodeAttrs {
                node: f.first,
                attrs: NodeAttrs::new(
                    [("history-test-reject".into(), AttrValue::Bool(true))].into(),
                )
                .unwrap(),
            });
        }
    }
    direction.restore(s, entry);
    s.history.restore_grouping_state(grouping);
    original
}

fn repair(s: &mut DocumentSession, direction: Direction, fault: Fault, original: EntryImage) {
    let grouping = s.history.grouping_state();
    let mut entry = direction.take(s);
    // Restore only the intentionally corrupted field, not the entire entry. A
    // premature write to the opposite captured selection must not be hidden.
    match fault {
        Fault::Core | Fault::Candidate => {
            *transaction_mut(&mut entry, direction) = match direction {
                Direction::Undo => original.undo,
                Direction::Redo => original.redo,
            };
        }
        Fault::Selection => {
            *target_mut(&mut entry, direction) = match direction {
                Direction::Undo => original.before,
                Direction::Redo => original.after,
            };
        }
    }
    direction.restore(s, entry);
    s.history.restore_grouping_state(grouping);
}

fn assert_twins(
    s: &mut DocumentSession,
    control: &mut DocumentSession,
    events: &Events,
    control_events: &Events,
) {
    assert_eq!(s.document().store(), control.document().store());
    assert_eq!(s.document().root(), control.document().root());
    assert_eq!(s.document().revision(), control.document().revision());
    assert_eq!(s.selection(), control.selection());
    assert_eq!(s.stored_marks(), control.stored_marks());
    assert_eq!(history_image(s), history_image(control));
    assert_eq!(s.history_selection_before, control.history_selection_before);
    assert_eq!(
        s.input_rule_undo_available(),
        control.input_rule_undo_available()
    );
    assert_eq!(s.listeners.len(), control.listeners.len());
    assert_eq!(*events.borrow(), *control_events.borrow());
}

fn allocation_probe(s: &mut DocumentSession) -> NodeId {
    let parent = s.document().root();
    let index = s
        .document()
        .node(parent)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .len();
    s.apply(&transaction().with_step(TransactionStep::InsertNode {
        parent,
        index,
        kind: NodeKind::Paragraph,
        attrs: NodeAttrs::empty(),
        content: NodeContent::empty_inline(),
    }))
    .unwrap();
    s.document()
        .node(parent)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[index]
}

fn rejected_traversal_preserves_every_field_and_retry(fault: Fault, options: HistoryOptions) {
    for direction in [Direction::Undo, Direction::Redo] {
        let f = fixture("abcdef");
        let (mut s, events) = seeded(&f, options);
        let (mut control, control_events) = seeded(&f, options);
        let original = corrupt(&mut s, &f, direction, fault);
        let before = Snapshot::capture(&mut s, &events);
        let error = direction.run(&mut s).unwrap_err();
        match fault {
            Fault::Core => assert!(matches!(error, SessionError::Core(_))),
            Fault::Selection => assert_eq!(error, SessionError::SelectionInvalid),
            Fault::Candidate => assert_eq!(
                error,
                SessionError::Policy(PolicyError::new("immutable test candidate rule"))
            ),
        }
        before.assert_unchanged(&mut s, &events);
        repair(&mut s, direction, fault, original);
        assert_twins(&mut s, &mut control, &events, &control_events);

        // A later source selection differs from the rejected attempt's source.
        // It is this selection, not the rejected one, that the reverse trip sees.
        let retry_source = caret(f.first, 4);
        select(&mut s, retry_source);
        select(&mut control, retry_source);
        assert_eq!(direction.run(&mut s), Ok(SessionOutcome::DocumentChanged));
        assert_eq!(
            direction.run(&mut control),
            Ok(SessionOutcome::DocumentChanged)
        );
        assert_twins(&mut s, &mut control, &events, &control_events);
        assert_eq!(s.stored_marks(), None);
        assert!(!s.history.typing_group_open());
        assert!(!s.input_rule_undo_available());
        assert_eq!(
            direction.opposite().run(&mut s),
            Ok(SessionOutcome::DocumentChanged)
        );
        assert_eq!(
            direction.opposite().run(&mut control),
            Ok(SessionOutcome::DocumentChanged)
        );
        if options.selection_mode() == HistorySelectionMode::CaptureOnTraversal {
            assert_eq!(s.selection(), retry_source);
        }
        assert_twins(&mut s, &mut control, &events, &control_events);
        assert_eq!(allocation_probe(&mut s), allocation_probe(&mut control));
        assert_twins(&mut s, &mut control, &events, &control_events);
    }
}

#[test]
fn private_core_fault_preserves_undo_and_redo_entries_transients_listeners_and_retry() {
    rejected_traversal_preserves_every_field_and_retry(Fault::Core, capture_options());
}

#[test]
fn private_invalid_target_selection_preserves_both_stacks_and_does_not_capture_on_failure() {
    rejected_traversal_preserves_every_field_and_retry(Fault::Selection, capture_options());
}

#[test]
fn private_candidate_fault_uses_stable_policy_and_preserves_both_stacks_and_retry() {
    rejected_traversal_preserves_every_field_and_retry(Fault::Candidate, capture_options());
}

#[test]
fn unrecorded_canonical_mapping_is_not_provided_by_selection_capture() {
    let f = fixture("abcdef");
    let mut public = session(&f, capture_options());
    select(&mut public, caret(f.first, 2));
    insert(&mut public, "X");
    public.undo().unwrap();
    assert_eq!(public.history_depths(), (0, 1));
    // Supported public raw apply is recorded and clears redo. It is not an
    // addToHistory=false / map-only channel, even with selection capture on.
    public
        .apply(&transaction().with_step(replace(f.first, 0, 0, "NN")))
        .unwrap();
    assert_eq!(public.history_depths(), (1, 0));
    assert_eq!(plain(&public, f.first), "NNabcdef");
    let public_events = listen(&mut public);
    let before = Snapshot::capture(&mut public, &public_events);
    assert_eq!(public.redo(), Ok(SessionOutcome::NoChange));
    before.assert_unchanged(&mut public, &public_events);

    let mut bypassed = session(&f, capture_options());
    select(&mut bypassed, caret(f.first, 2));
    insert(&mut bypassed, "X");
    select(&mut bypassed, cell_range(&f));
    bypassed.undo().unwrap();
    let history = history_image(&mut bypassed);
    // Deliberate PRIVATE canonical replacement outside Session orchestration.
    // There is no supported public call for this while retaining live history.
    // Unlike PM map-only history, stale Cell selections have no remap/fallback.
    bypassed.document = transaction()
        .with_step(TransactionStep::RemoveNode { node: f.table })
        .apply(bypassed.document())
        .unwrap();
    bypassed.selection().validate(bypassed.document()).unwrap();
    assert_eq!(history_image(&mut bypassed), history);
    let events = listen(&mut bypassed);
    let before = Snapshot::capture(&mut bypassed, &events);
    assert_eq!(bypassed.redo(), Err(SessionError::SelectionInvalid));
    before.assert_unchanged(&mut bypassed, &events);
}

#[test]
fn private_timed_traversal_faults_restore_anchor_and_high_water_for_all_options() {
    for selection_mode in [
        HistorySelectionMode::Recorded,
        HistorySelectionMode::CaptureOnTraversal,
    ] {
        for empty in [
            EmptyHistoryBehavior::ClearPendingMarks,
            EmptyHistoryBehavior::PreserveEditingState,
        ] {
            let options = HistoryOptions::new()
                .with_typing_group_delay_ms(500)
                .with_selection_only_grouping(super::super::SelectionOnlyGrouping::Preserve)
                .with_selection_mode(selection_mode)
                .with_empty_behavior(empty);
            for fault in [Fault::Core, Fault::Selection, Fault::Candidate] {
                rejected_traversal_preserves_every_field_and_retry(fault, options);
            }
        }
    }
}
