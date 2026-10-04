//! Mixed-inline range marks; explicit set/remove remain distinct from toggle.

use xiaomu_core::document::{InlineContent, Mark, MarkKind, MarkSet, NodeId, XiaomuDocument};
use xiaomu_core::selection::{InlinePoint, TextSelection};
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::intent::{PlannedAction, edit_transaction, map_existing_plan, ordered_range};
use super::{
    CellRange, DocumentPosition, DocumentSelection, EditIntent, EditPlan, SelectionUpdate,
    SessionError,
};

/// Plans one isolated mark edit over the full mixed-inline document range.
///
/// Text ranges and atom marks share the toggle decision, but retain their
/// independent canonical values. Same-byte atom ordinals delimit half-open
/// selections precisely; literal LF remains ordinary text. Structural gaps,
/// atomic-block endpoints and rectangular cell selections fail closed.
pub(super) fn plan_range_mark(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    intent: &EditIntent,
) -> Result<PlannedAction, SessionError> {
    selection.validate(document)?;
    if selection.is_collapsed() || selection.active_cell_range().is_some() {
        return Err(SessionError::SelectionInvalid);
    }
    let (DocumentPosition::Inline(head), DocumentPosition::Inline(tail)) =
        selection.ordered(document)?
    else {
        return Err(SessionError::SelectionInvalid);
    };
    // Keep the established atom-free single-node contract, including its
    // exact no-op and one-step behavior.
    if head.node_id() == tail.node_id() {
        let inline = inline_of(document, head.node_id())?;
        if inline.atoms().is_empty() {
            let text_selection = selection
                .as_single_node()
                .ok_or(SessionError::SelectionInvalid)?;
            return match intent {
                EditIntent::ToggleMark { mark } => plan_toggle_mark(inline, text_selection, mark),
                EditIntent::SetMark { mark } => plan_set_mark(inline, text_selection, mark),
                EditIntent::RemoveMark { kind } => plan_remove_mark(inline, text_selection, *kind),
                _ => Err(SessionError::SelectionInvalid),
            };
        }
    }

    plan_spans(selected_spans(document, head, tail)?, intent)
}

/// Marks all inline descendants of selected unique cell origins as one edit.
///
/// The selected cells are disjoint canonical subtrees, even when their slots
/// repeat through row/column spans. Nested table contents are visited once as
/// descendants of the selected outer cell. Non-inline blocks are preserved.
pub(super) fn plan_cell_range_mark(
    document: &XiaomuDocument,
    range: CellRange,
    intent: &EditIntent,
) -> Result<PlannedAction, SessionError> {
    let mut pending = range.unique_origins(document)?;
    pending.reverse();
    let mut spans = Vec::new();
    while let Some(id) = pending.pop() {
        let node = document.node(id).ok_or(SessionError::SelectionInvalid)?;
        if let Some(children) = node.content().as_children() {
            pending.extend(children.iter().rev().copied());
        }
        let Some(inline) = node.content().as_inline() else {
            continue;
        };
        let atoms = inline
            .atoms()
            .iter()
            .map(|placement| {
                let content = document
                    .node(placement.atom())
                    .and_then(|node| node.content().as_inline_atom())
                    .ok_or(SessionError::SelectionInvalid)?;
                Ok((placement.atom(), content.marks()))
            })
            .collect::<Result<_, SessionError>>()?;
        spans.push(SelectedSpan {
            node: id,
            inline,
            range: TextRange::new(
                inline.offset_at(0).map_err(SessionError::Core)?,
                inline
                    .offset_at(inline.len_bytes())
                    .map_err(SessionError::Core)?,
            )
            .map_err(SessionError::Core)?,
            atoms,
        });
    }
    plan_spans(spans, intent)
}

fn plan_spans(
    spans: Vec<SelectedSpan<'_>>,
    intent: &EditIntent,
) -> Result<PlannedAction, SessionError> {
    let kind = match intent {
        EditIntent::ToggleMark { mark } | EditIntent::SetMark { mark } => mark.kind(),
        EditIntent::RemoveMark { kind } => *kind,
        _ => return Err(SessionError::SelectionInvalid),
    };
    let fully_marked = spans
        .iter()
        .all(|span| span.all_marks(|marks| marks.contains(kind)));
    let replacement = match intent {
        EditIntent::ToggleMark { mark } if !fully_marked => Some(mark),
        EditIntent::SetMark { mark } => Some(mark),
        _ => None,
    };
    let unchanged =
        |marks: &MarkSet| marks.as_slice().iter().find(|mark| mark.kind() == kind) == replacement;
    if spans.iter().all(|span| span.all_marks(unchanged)) {
        return Ok(PlannedAction::NoChange);
    }

    let mut transaction = Transaction::new(TransactionOrigin::UserInput);
    for span in spans {
        if !span.range.is_empty() && !range_all(span.inline, span.range, unchanged) {
            transaction.push_step(match replacement {
                Some(mark) => TransactionStep::AddMark {
                    node: span.node,
                    range: span.range,
                    mark: mark.clone(),
                },
                None => TransactionStep::RemoveMark {
                    node: span.node,
                    range: span.range,
                    mark_kind: kind,
                },
            });
        }
        for (atom, marks) in span.atoms {
            if unchanged(marks) {
                continue;
            }
            let next = marks
                .as_slice()
                .iter()
                .filter(|mark| mark.kind() != kind)
                .cloned()
                .chain(replacement.cloned());
            transaction.push_step(TransactionStep::SetInlineAtomMarks {
                atom,
                marks: MarkSet::new(next).map_err(SessionError::Core)?,
            });
        }
    }
    Ok(PlannedAction::Commit(map_existing_plan(transaction)))
}

struct SelectedSpan<'a> {
    node: NodeId,
    inline: &'a InlineContent,
    range: TextRange,
    atoms: Vec<(NodeId, &'a MarkSet)>,
}

impl SelectedSpan<'_> {
    fn all_marks(&self, predicate: impl Fn(&MarkSet) -> bool) -> bool {
        range_all(self.inline, self.range, &predicate)
            && self.atoms.iter().all(|(_, marks)| predicate(marks))
    }
}

fn inline_of(document: &XiaomuDocument, node: NodeId) -> Result<&InlineContent, SessionError> {
    document
        .node(node)
        .and_then(|node| node.content().as_inline())
        .ok_or(SessionError::SelectionInvalid)
}

fn selected_spans(
    document: &XiaomuDocument,
    head: InlinePoint,
    tail: InlinePoint,
) -> Result<Vec<SelectedSpan<'_>>, SessionError> {
    let mut pending = vec![document.root()];
    let mut spans = Vec::new();
    let mut inside = false;
    while let Some(id) = pending.pop() {
        let node = document.node(id).ok_or(SessionError::SelectionInvalid)?;
        if let Some(children) = node.content().as_children() {
            pending.extend(children.iter().rev().copied());
        }
        let Some(inline) = node.content().as_inline() else {
            continue;
        };
        inside |= id == head.node_id();
        if !inside {
            continue;
        }
        let start = if id == head.node_id() {
            (head.text_offset(), head.atom_index())
        } else {
            (inline.offset_at(0).map_err(SessionError::Core)?, 0)
        };
        let end = if id == tail.node_id() {
            (tail.text_offset(), tail.atom_index())
        } else {
            let offset = inline
                .offset_at(inline.len_bytes())
                .map_err(SessionError::Core)?;
            (offset, inline.atom_count_at(offset))
        };
        let mut atoms = Vec::new();
        let mut previous_offset = None;
        let mut ordinal = 0;
        for placement in inline.atoms() {
            let offset = placement.text_offset();
            if previous_offset == Some(offset) {
                ordinal += 1;
            } else {
                ordinal = 0;
                previous_offset = Some(offset);
            }
            if (offset, ordinal) >= start && (offset, ordinal) < end {
                let content = document
                    .node(placement.atom())
                    .and_then(|node| node.content().as_inline_atom())
                    .ok_or(SessionError::SelectionInvalid)?;
                atoms.push((placement.atom(), content.marks()));
            }
        }
        spans.push(SelectedSpan {
            node: id,
            inline,
            range: TextRange::new(start.0, end.0).map_err(SessionError::Core)?,
            atoms,
        });
        if id == tail.node_id() {
            return Ok(spans);
        }
    }
    Err(SessionError::SelectionInvalid)
}

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
