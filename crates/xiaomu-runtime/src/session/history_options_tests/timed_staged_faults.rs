//! Private staged orchestration sentinels; hosts have no public staged API.

use super::super::structure::StagedPlan;
use super::super::{HistoryTimestamp, SelectionOnlyGrouping};
use super::*;

#[test]
fn private_staged_failure_after_tentative_boundary_restores_full_timing_snapshot() {
    let f = fixture("abcdef");
    let options = capture_options()
        .with_typing_group_delay_ms(500)
        .with_selection_only_grouping(SelectionOnlyGrouping::Preserve);
    let mut s = session(&f, options);
    let events = listen(&mut s);
    timed_insert(&mut s, "X", 1_000);
    let before = Snapshot::capture(&mut s, &events);
    let root = f.document.root();
    let staged = StagedPlan::new(SelectionUpdate::PreserveSelection)
        .stage(move |_| {
            Ok(transaction().with_step(TransactionStep::InsertNode {
                parent: root,
                index: 0,
                kind: NodeKind::Paragraph,
                attrs: NodeAttrs::empty(),
                content: NodeContent::empty_inline(),
            }))
        })
        .stage(move |_| Ok(transaction().with_step(TransactionStep::RemoveNode { node: root })));
    assert!(
        s.with_transient_rollback(|s| {
            s.history.break_group();
            s.clear_stored_marks();
            s.commit_staged(staged)
        })
        .is_err()
    );
    before.assert_unchanged(&mut s, &events);
    timed_insert(&mut s, "Y", 1_400);
    assert_eq!(s.history_depths(), (1, 0));
}

#[test]
fn private_staged_publication_is_isolated_and_retains_previous_high_water() {
    let f = fixture("abcdef");
    let mut s = session(&f, capture_options().with_typing_group_delay_ms(500));
    timed_insert(&mut s, "X", 1_000);
    let node = f.first;
    let staged = StagedPlan::new(SelectionUpdate::Exact {
        selection: caret(node, 3),
    })
    .stage(move |_| Ok(transaction().with_step(replace(node, 1, 1, "S"))))
    .stage(move |_| Ok(transaction().with_step(replace(node, 2, 2, "T"))));
    assert_eq!(s.commit_staged(staged), Ok(SessionOutcome::DocumentChanged));
    assert_eq!(s.history_depths(), (2, 0));
    assert!(!s.history.typing_group_open());
    timed_insert(&mut s, "Y", 900);
    timed_insert(&mut s, "Z", 950);
    assert_eq!(s.history_depths(), (4, 0));
    assert!(!s.history.typing_group_open());
    assert_eq!(
        s.apply_intent_at(
            &EditIntent::InsertText { text: "A".into() },
            HistoryTimestamp::from_millis(1_000)
        ),
        Ok(SessionOutcome::DocumentChanged)
    );
    timed_insert(&mut s, "B", 1_100);
    assert_eq!(s.history_depths(), (5, 0));
}
