//! Single-node mark planners; explicit set/remove remain distinct from toggle.

use xiaomu_core::document::{InlineContent, Mark, MarkKind, MarkSet};
use xiaomu_core::selection::TextSelection;
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::TransactionStep;

use super::intent::{PlannedAction, edit_transaction, map_existing_plan, ordered_range};
use super::{EditPlan, SelectionUpdate, SessionError};

/// Builds the plan for toggling one mark over a non-collapsed selection.
pub(crate) fn plan_toggle_mark(
    inline: &InlineContent,
    selection: TextSelection,
    mark: &Mark,
) -> Result<PlannedAction, SessionError> {
    if selection.is_collapsed() {
        return Ok(PlannedAction::NoChange);
    }

    let node = selection.focus().node_id();
    let range = ordered_range(selection)?;
    let step = if range_fully_marked(inline, range, mark.kind()) {
        TransactionStep::RemoveMark {
            node,
            range,
            mark_kind: mark.kind(),
        }
    } else {
        TransactionStep::AddMark {
            node,
            range,
            mark: mark.clone(),
        }
    };

    Ok(PlannedAction::Commit(EditPlan::new(
        edit_transaction(step),
        SelectionUpdate::MapExisting,
        None,
    )))
}

/// Sets an exact value without interpreting same-kind presence as a toggle.
pub(super) fn plan_set_mark(
    inline: &InlineContent,
    selection: TextSelection,
    mark: &Mark,
) -> Result<PlannedAction, SessionError> {
    let range = ordered_range(selection)?;
    if range_all(inline, range, |marks| marks.as_slice().contains(mark)) {
        return Ok(PlannedAction::NoChange);
    }
    Ok(PlannedAction::Commit(map_existing_plan(edit_transaction(
        TransactionStep::AddMark {
            node: selection.focus().node_id(),
            range,
            mark: mark.clone(),
        },
    ))))
}

/// Removes by kind only when some selected text actually carries that kind.
pub(super) fn plan_remove_mark(
    inline: &InlineContent,
    selection: TextSelection,
    kind: MarkKind,
) -> Result<PlannedAction, SessionError> {
    let range = ordered_range(selection)?;
    if range_all(inline, range, |marks| !marks.contains(kind)) {
        return Ok(PlannedAction::NoChange);
    }
    Ok(PlannedAction::Commit(map_existing_plan(edit_transaction(
        TransactionStep::RemoveMark {
            node: selection.focus().node_id(),
            range,
            mark_kind: kind,
        },
    ))))
}

fn range_fully_marked(inline: &InlineContent, range: TextRange, kind: MarkKind) -> bool {
    range_all(inline, range, |marks| marks.contains(kind))
}

/// Applies a predicate to each run overlapping the half-open text range.
fn range_all(
    inline: &InlineContent,
    range: TextRange,
    predicate: impl Fn(&MarkSet) -> bool,
) -> bool {
    let start = range.start().as_usize();
    let end = range.end().as_usize();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;
        if start.max(run_start) < end.min(run_end) && !predicate(run.marks()) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{
        LinkMark, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    };
    use xiaomu_core::selection::{CursorAffinity, TextPoint};

    #[test]
    fn explicit_range_plans_use_one_core_step_and_map_existing_selection() {
        let inline = InlineContent::new([TextRun::new(
            "ab",
            MarkSet::new([Mark::Link(LinkMark::new("old", None))]).unwrap(),
        )
        .unwrap()])
        .unwrap();
        let mut builder = NodeStoreBuilder::new();
        let node = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(inline.clone()),
            )
            .unwrap();
        let selection = TextSelection::new(
            TextPoint::at_start_of(node),
            TextPoint::new(node, inline.offset_at(2).unwrap(), CursorAffinity::Before),
        );
        let range = ordered_range(selection).unwrap();
        let mark = Mark::Link(LinkMark::new("new", None));
        let set = plan_set_mark(&inline, selection, &mark).unwrap();
        let remove = plan_remove_mark(&inline, selection, MarkKind::Link).unwrap();
        for (action, expected) in [
            (set, TransactionStep::AddMark { node, range, mark }),
            (
                remove,
                TransactionStep::RemoveMark {
                    node,
                    range,
                    mark_kind: MarkKind::Link,
                },
            ),
        ] {
            let PlannedAction::Commit(plan) = action else {
                panic!("expected one committed plan");
            };
            assert_eq!(plan.transaction().steps(), &[expected]);
            assert_eq!(*plan.selection_update(), SelectionUpdate::MapExisting);
            assert_eq!(
                plan.history_policy(),
                super::super::intent::HistoryPolicy::Isolated
            );
        }
    }
}
