//! Public API rejection/no-op paths with full private state observation.
//! Deliberate history corruption lives separately in `faults`.

use super::super::{HistoryTimestamp, SelectionOnlyGrouping};
use super::*;

fn stamp(millis: u64) -> HistoryTimestamp {
    HistoryTimestamp::from_millis(millis)
}
fn options() -> HistoryOptions {
    capture_options()
        .with_typing_group_delay_ms(500)
        .with_selection_only_grouping(SelectionOnlyGrouping::Preserve)
}

struct RejectPolicy;
impl SessionPolicy for RejectPolicy {
    fn history_options(&self) -> HistoryOptions {
        options()
    }
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        let EditIntent::InsertText { text } = intent else {
            return Ok(IntentDisposition::Continue);
        };
        if text == "preflight" {
            return Err(PolicyError::new("test preflight rejection"));
        }
        if text == "noop" {
            return Ok(IntentDisposition::NoChange);
        }
        if !matches!(text.as_str(), "core" | "selection" | "rule_failure") {
            return Ok(IntentDisposition::Continue);
        }
        let (point, _) = context.selection().as_same_node_inline().unwrap();
        let node = point.node_id();
        let at = point.text_offset().as_usize();
        let mut tx = transaction();
        match text.as_str() {
            "core" => {
                tx.push_step(TransactionStep::InsertNode {
                    parent: context.document().root(),
                    index: 0,
                    kind: NodeKind::Paragraph,
                    attrs: NodeAttrs::empty(),
                    content: NodeContent::empty_inline(),
                });
                tx.push_step(TransactionStep::RemoveNode {
                    node: context.document().root(),
                });
            }
            "selection" | "rule_failure" => tx.push_step(replace(node, at, at, "Q")),
            _ => return Ok(IntentDisposition::Continue),
        }
        let after = caret(node, if text == "selection" { 999 } else { at + 1 });
        let mut plan = EditPlan::new(tx, SelectionUpdate::Exact { selection: after }, None);
        if text == "rule_failure" {
            let spec = InputRuleUndoSpec::new(
                transaction().with_step(TransactionStep::RemoveNode {
                    node: context.document().root(),
                }),
                after,
            )
            .unwrap();
            plan = plan.with_input_rule_undo(spec);
        }
        Ok(IntentDisposition::Apply(plan))
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.store().iter().any(|node| {
            node.content().as_inline().is_some_and(|inline| {
                inline
                    .runs()
                    .iter()
                    .any(|run| run.text().as_str().contains('!'))
            })
        }) {
            return Err(PolicyError::new("immutable forbidden text rule"));
        }
        Ok(())
    }
}

fn seeded(f: &Fixture) -> (DocumentSession, Events) {
    let mut s = DocumentSession::new_with_policy(
        f.document.clone(),
        caret(f.first, 0),
        Box::new(RejectPolicy),
    )
    .unwrap();
    let events = listen(&mut s);
    s.apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    timed_insert(&mut s, "X", 1_000);
    assert!(s.history.typing_group_open());
    assert!(s.stored_marks().is_some());
    (s, events)
}

#[test]
fn timed_public_failures_restore_full_state_and_do_not_consume_future_time() {
    let f = fixture("abcdef");
    for changed_target in [false, true] {
        for text in ["preflight", "core", "selection", "rule_failure", "!"] {
            let (mut s, events) = seeded(&f);
            let before = Snapshot::capture(&mut s, &events);
            let target = if changed_target {
                caret(f.second, 1)
            } else {
                s.selection()
            };
            assert!(
                s.apply_intent_with_selection_at(
                    target,
                    &EditIntent::InsertText { text: text.into() },
                    stamp(9_000),
                )
                .is_err(),
                "{text}"
            );
            before.assert_unchanged(&mut s, &events);
            timed_insert(&mut s, "Y", 1_400);
            assert_eq!(s.history_depths(), (1, 0), "{text}: no leaked break/time");
        }
    }
}

#[test]
fn invalid_target_planner_and_raw_core_failures_preserve_timed_snapshot() {
    let f = fixture("abcdef");
    let (mut s, events) = seeded(&f);
    let before = Snapshot::capture(&mut s, &events);
    assert!(
        s.apply_intent_with_selection_at(
            caret(f.first, 999),
            &EditIntent::InsertText { text: "Q".into() },
            stamp(9_000),
        )
        .is_err()
    );
    before.assert_unchanged(&mut s, &events);
    assert_eq!(
        s.apply_intent_with_selection_at(
            caret(f.second, 0),
            &EditIntent::InsertHorizontalRule,
            stamp(9_000),
        ),
        Err(SessionError::UnsupportedEdit)
    );
    before.assert_unchanged(&mut s, &events);
    assert!(
        s.apply(&transaction().with_step(TransactionStep::RemoveNode {
            node: f.document.root()
        }))
        .is_err()
    );
    before.assert_unchanged(&mut s, &events);
    timed_insert(&mut s, "Y", 1_400);
    assert_eq!(s.history_depths(), (1, 0));
}

#[test]
fn timed_empty_input_policy_nochange_and_empty_preserve_do_not_refresh_anchor() {
    let f = fixture("abcdef");
    for next_time in [1_400, 1_501] {
        let (mut s, events) = seeded(&f);
        let before = Snapshot::capture(&mut s, &events);
        for text in ["", "noop"] {
            assert_eq!(
                s.apply_intent_at(&EditIntent::InsertText { text: text.into() }, stamp(9_000),),
                Ok(SessionOutcome::NoChange)
            );
            before.assert_unchanged(&mut s, &events);
        }
        assert_eq!(
            s.apply_intent_with_selection_at(
                caret(f.second, 0),
                &EditIntent::InsertText {
                    text: "noop".into()
                },
                stamp(9_000),
            ),
            Ok(SessionOutcome::NoChange)
        );
        before.assert_unchanged(&mut s, &events);
        assert_eq!(s.redo(), Ok(SessionOutcome::NoChange));
        before.assert_unchanged(&mut s, &events);
        timed_insert(&mut s, "Y", next_time);
        assert_eq!(
            s.history_depths(),
            (if next_time == 1_400 { 1 } else { 2 }, 0)
        );
    }
}

#[test]
fn before_only_close_preserves_marks_token_and_every_non_grouping_field() {
    let f = fixture("abcdef");
    let (mut s, events) = seeded(&f);
    let document = s.document().clone();
    let selection = s.selection();
    let marks = s.stored_marks().cloned();
    let history_before = history_image(&mut s);
    let notifications = events.borrow().clone();
    s.close_history_group();
    s.close_history_group();
    let history_after = history_image(&mut s);
    assert_eq!(history_before.undo, history_after.undo);
    assert_eq!(history_before.redo, history_after.redo);
    assert!(!s.history.typing_group_open());
    assert_eq!(s.document().store(), document.store());
    assert_eq!(s.document().revision(), document.revision());
    assert_eq!(s.selection(), selection);
    assert_eq!(s.stored_marks(), marks.as_ref());
    assert_eq!(*events.borrow(), notifications);
    timed_insert(&mut s, "Y", 1_100);
    timed_insert(&mut s, "Z", 1_200);
    assert_eq!(s.history_depths(), (2, 0));

    // A real admitted host rule token, independently of an open typing group.
    // The sibling fixture's policy uses the same immutable rule contract.
    let mut ruled = DocumentSession::new_with_policy(
        f.document.clone(),
        caret(f.first, 0),
        Box::new(TimedRulePolicy),
    )
    .unwrap();
    let rule_events = listen(&mut ruled);
    timed_insert(&mut ruled, "rule", 1_000);
    assert!(ruled.input_rule_undo_available());
    ruled
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
    let before = Snapshot::capture(&mut ruled, &rule_events);
    ruled.close_history_group();
    ruled.close_history_group();
    before.assert_unchanged(&mut ruled, &rule_events);
    let selection = ruled.selection();
    ruled.set_document_selection(selection).unwrap();
    assert!(
        !ruled.input_rule_undo_available(),
        "same-coordinate selection invalidates token"
    );
    assert!(
        ruled.stored_marks().is_some(),
        "unchanged selection retains old marks behavior"
    );
    ruled.set_document_selection(caret(f.second, 0)).unwrap();
    assert_eq!(
        ruled.stored_marks(),
        None,
        "Preserve does not preserve pending marks"
    );
}

struct TimedRulePolicy;
impl SessionPolicy for TimedRulePolicy {
    fn history_options(&self) -> HistoryOptions {
        options()
    }
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if let EditIntent::InsertText { text } = intent
            && text == "rule"
        {
            let (point, _) = context.selection().as_same_node_inline().unwrap();
            let node = point.node_id();
            let at = point.text_offset().as_usize();
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
fn public_preserved_cell_selection_away_and_back_keeps_timing_but_cell_edit_is_isolated() {
    let f = fixture("abcdef");
    let (mut s, events) = seeded(&f);
    let original = s.selection();
    let grouping = s.history.grouping_state();
    select(&mut s, cell_range(&f));
    assert_eq!(s.history.grouping_state(), grouping);
    select(&mut s, original);
    timed_insert(&mut s, "Y", 1_400);
    assert_eq!(s.history_depths(), (1, 0));
    select(&mut s, cell_range(&f));
    let count = events.borrow().len();
    assert_eq!(
        s.apply_intent_at(
            &EditIntent::InsertText {
                text: "cell".into()
            },
            stamp(u64::MAX)
        ),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(s.history_depths(), (2, 0));
    assert!(!s.history.typing_group_open());
    assert_eq!(events.borrow().len(), count + 1);
    timed_insert(&mut s, "Z", 1_500);
    timed_insert(&mut s, "W", 1_600);
    assert_eq!(
        s.history_depths(),
        (3, 0),
        "isolated cell edit did not publish MAX time"
    );
}

#[test]
fn preserved_selection_option_keeps_the_existing_cell_convergence_barrier() {
    let f = fixture("abcdef");
    let (mut s, _) = seeded(&f);
    let original = s.selection();
    select(&mut s, cell_range(&f));
    assert!(s.history.typing_group_open());
    assert_eq!(
        s.apply_intent_at(
            &EditIntent::MoveCaret {
                caret_move: crate::session::CaretMove::ToStart,
                extend_selection: false,
            },
            stamp(9_000),
        ),
        Ok(SessionOutcome::SelectionChanged)
    );
    assert!(!s.history.typing_group_open());
    select(&mut s, original);
    timed_insert(&mut s, "Y", 1_100);
    timed_insert(&mut s, "Z", 1_200);
    assert_eq!(s.history_depths(), (2, 0));
    assert_eq!(plain(&s, f.first), "XYZabcdef");
}
