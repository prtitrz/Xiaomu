//! Atom-aware editing planners for mixed-inline carets.
//!
//! Split from `intent.rs`: these planners consume the session selection's
//! atom ordinal and decide between the text-only P0-P3 contract (plain-text
//! nodes, exact legacy steps) and the P4 atom-aware transaction contract
//! (seam insertions via `ReplaceInlineText`, atomic Backspace/Delete via
//! `RemoveInlineAtom`, and explicit selection-span atom removals).

use xiaomu_core::document::{InlineContent, MarkSet, NodeId};
use xiaomu_core::selection::{InlinePoint, TextSelection};
use xiaomu_core::text::{TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::SessionError;
use super::intent::{
    EditPlan, HistoryPolicy, PlannedAction, PrimaryEdit, SelectionUpdate, concatenated,
    deletion_plan, edit_transaction, next_boundary, ordered_range, plan_insert_text,
    previous_boundary, push_exact_insert_marks,
};

/// Builds the plan for Backspace.
///
/// Plain-text nodes keep the exact P0-P3 `ReplaceText` contract. Nodes with
/// inline atoms delete one caret unit atomically (ADR 0005): the unit before
/// the caret is either an atom (removed by identity) or one text scalar
/// (deleted without touching the seam atoms before it). A non-collapsed
/// selection deletes the whole region including any atoms inside it.
pub(crate) fn plan_backspace(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    if inline.atoms().is_empty() {
        return plan_backspace_text_only(inline, anchor, focus);
    }
    if !selection_is_collapsed(anchor, focus) {
        return plan_inline_replacement(inline, anchor, focus, "", None, HistoryPolicy::Isolated);
    }

    let node = focus.node_id();
    let affinity = focus.affinity();
    if focus.atom_index() > 0 {
        // The unit before the caret is the atom at the previous ordinal.
        let atom = atom_at_ordinal(inline, focus.text_offset(), focus.atom_index() - 1)
            .ok_or(SessionError::SelectionInvalid)?;
        let caret = InlinePoint::new(node, focus.text_offset(), focus.atom_index() - 1, affinity);
        let transaction = edit_transaction(TransactionStep::RemoveInlineAtom { atom });
        return Ok(PlannedAction::Commit(
            EditPlan::new(transaction, SelectionUpdate::CaretAtInline { caret }, None)
                .with_history_policy(HistoryPolicy::Isolated),
        ));
    }

    // Delete the previous text scalar; seam atoms at its start boundary sit
    // before the caret and survive.
    let text = concatenated(inline);
    let Some(start) = previous_boundary(&text, focus.text_offset().as_usize()) else {
        return Ok(PlannedAction::NoChange);
    };
    let start_offset = inline.offset_at(start).map_err(SessionError::Core)?;
    let start_ordinal = inline.atom_count_at(start_offset);
    plan_seam_deletion(
        node,
        InlinePoint::new(node, start_offset, start_ordinal, affinity),
        focus.text_offset(),
    )
}

/// Builds the plan for forward Delete.
///
/// Mirrors [`plan_backspace`]: the unit after the caret is either an atom or
/// one text scalar.
pub(crate) fn plan_delete(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    if inline.atoms().is_empty() {
        return plan_delete_text_only(inline, anchor, focus);
    }
    if !selection_is_collapsed(anchor, focus) {
        return plan_inline_replacement(inline, anchor, focus, "", None, HistoryPolicy::Isolated);
    }

    let node = focus.node_id();
    let affinity = focus.affinity();
    let count = inline.atom_count_at(focus.text_offset());
    if focus.atom_index() < count {
        // The unit after the caret is the atom at the caret's own ordinal.
        let atom = atom_at_ordinal(inline, focus.text_offset(), focus.atom_index())
            .ok_or(SessionError::SelectionInvalid)?;
        let caret = InlinePoint::new(node, focus.text_offset(), focus.atom_index(), affinity);
        let transaction = edit_transaction(TransactionStep::RemoveInlineAtom { atom });
        return Ok(PlannedAction::Commit(
            EditPlan::new(transaction, SelectionUpdate::CaretAtInline { caret }, None)
                .with_history_policy(HistoryPolicy::Isolated),
        ));
    }

    // Delete the next text scalar; the caret stays after the seam atoms.
    let text = concatenated(inline);
    let Some(end) = next_boundary(&text, focus.text_offset().as_usize()) else {
        return Ok(PlannedAction::NoChange);
    };
    let end_offset = inline.offset_at(end).map_err(SessionError::Core)?;
    plan_seam_deletion(
        node,
        InlinePoint::new(node, focus.text_offset(), count, affinity),
        end_offset,
    )
}

/// Plans the deletion of the text span from a seam gap to `end`.
fn plan_seam_deletion(
    node: NodeId,
    at: InlinePoint,
    end: TextOffset,
) -> Result<PlannedAction, SessionError> {
    let transaction = edit_transaction(TransactionStep::ReplaceInlineText {
        at,
        end,
        replacement: String::new(),
    });
    Ok(PlannedAction::Commit(
        EditPlan::new(
            transaction,
            SelectionUpdate::CaretAtInline { caret: at },
            Some(PrimaryEdit {
                node,
                range: TextRange::new(at.text_offset(), end).map_err(SessionError::Core)?,
                inserted_len: 0,
            }),
        )
        .with_history_policy(HistoryPolicy::Isolated),
    ))
}

/// Builds the InsertText plan at a mixed-inline caret gap.
///
/// Plain-text nodes keep the exact P0-P3 `ReplaceText` contract; nodes with
/// inline atoms insert through the atom-aware replacement so the caret's
/// seam ordinal decides whether typed text lands before or after the
/// same-boundary atoms. A selection spanning atoms deletes them explicitly
/// in the same transaction.
pub(crate) fn plan_text_input(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
    text: &str,
    stored_marks: Option<&MarkSet>,
    requested_history: HistoryPolicy,
) -> Result<PlannedAction, SessionError> {
    if inline.atoms().is_empty() {
        let selection = text_selection_from(anchor, focus)?;
        return plan_insert_text(inline, selection, text, stored_marks, requested_history);
    }
    plan_inline_replacement(inline, anchor, focus, text, stored_marks, requested_history)
}

/// Builds the plan for an IME composition commit in a node with atoms.
///
/// The composition range is a plain text span produced by the frontend, so
/// it carries no seam ordinal of its own: atoms anchored at either boundary
/// sit outside the composed text and survive it, while an atom strictly
/// inside the range fails the commit — IME composition can never enter an
/// atom.
pub(crate) fn plan_ime_commit(
    inline: &InlineContent,
    node: NodeId,
    range: TextRange,
    text: &str,
    stored_marks: Option<&MarkSet>,
) -> Result<PlannedAction, SessionError> {
    let start = range.start();
    let at = InlinePoint::new(
        node,
        start,
        inline.atom_count_at(start),
        xiaomu_core::selection::CursorAffinity::Before,
    );
    let mut transaction = edit_transaction(TransactionStep::ReplaceInlineText {
        at,
        end: range.end(),
        replacement: text.to_owned(),
    });
    if let Some(marks) = stored_marks
        && !text.is_empty()
    {
        push_exact_insert_marks(&mut transaction, inline, node, range, text, marks)?;
    }

    Ok(PlannedAction::Commit(
        EditPlan::new(
            transaction,
            SelectionUpdate::CaretAfterReplacement,
            Some(PrimaryEdit {
                node,
                range,
                inserted_len: text.len(),
            }),
        )
        .with_history_policy(HistoryPolicy::Isolated),
    ))
}

/// Replaces the span between two mixed-inline endpoints of one node.
///
/// The selection's atoms (same-boundary atoms at or after the start gap, and
/// end-boundary atoms before the end gap) are removed by identity before the
/// atom-aware replacement, whose seam ordinal already excludes them. A
/// collapsed endpoint pair is a pure seam insertion and removes nothing.
fn plan_inline_replacement(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
    text: &str,
    stored_marks: Option<&MarkSet>,
    requested_history: HistoryPolicy,
) -> Result<PlannedAction, SessionError> {
    let node = focus.node_id();
    let collapsed = selection_is_collapsed(anchor, focus);
    let (start, end) = match anchor {
        Some(anchor) if !collapsed => {
            let a = (anchor.text_offset().as_usize(), anchor.atom_index());
            let f = (focus.text_offset().as_usize(), focus.atom_index());
            if a <= f {
                (anchor, focus)
            } else {
                (focus, anchor)
            }
        }
        _ => (focus, focus),
    };

    let mut transaction = Transaction::new(TransactionOrigin::UserInput);
    if !collapsed {
        // Atoms inside the selection are deleted with the region; they can
        // never be silently consumed by a text edit.
        for atom in atoms_inside_span(inline, start, end) {
            transaction.push_step(TransactionStep::RemoveInlineAtom { atom });
        }
    }
    transaction.push_step(TransactionStep::ReplaceInlineText {
        at: start,
        end: end.text_offset(),
        replacement: text.to_owned(),
    });
    if let Some(marks) = stored_marks
        && !text.is_empty()
    {
        let replaced =
            TextRange::new(start.text_offset(), end.text_offset()).map_err(SessionError::Core)?;
        push_exact_insert_marks(&mut transaction, inline, node, replaced, text, marks)?;
    }

    let selection_update = if !text.is_empty() {
        // The caret lands after the replacement text, before the seam atoms
        // that moved behind it.
        SelectionUpdate::CaretAfterReplacement
    } else {
        SelectionUpdate::CaretAtInline { caret: start }
    };
    let history_policy =
        if requested_history == HistoryPolicy::Typing && collapsed && !text.is_empty() {
            HistoryPolicy::Typing
        } else {
            HistoryPolicy::Isolated
        };

    Ok(PlannedAction::Commit(
        EditPlan::new(
            transaction,
            selection_update,
            Some(PrimaryEdit {
                node,
                range: TextRange::new(start.text_offset(), end.text_offset())
                    .map_err(SessionError::Core)?,
                inserted_len: text.len(),
            }),
        )
        .with_history_policy(history_policy),
    ))
}

/// Returns the atoms inside the mixed-inline span between two gaps of one
/// node, in canonical order.
///
/// Same-boundary atoms at or after the start gap and atoms before the end
/// gap belong to the span; when both gaps share one boundary the span is a
/// half-open ordinal range. Callers remove these atoms explicitly so a text
/// replacement never consumes them as a side effect.
pub(crate) fn atoms_inside_span(
    inline: &InlineContent,
    start: InlinePoint,
    end: InlinePoint,
) -> Vec<NodeId> {
    let start_raw = start.text_offset().as_usize();
    let end_raw = end.text_offset().as_usize();
    let mut inside = Vec::new();
    for placement in inline.atoms() {
        let offset = placement.text_offset().as_usize();
        let ordinal = same_boundary_ordinal(inline, placement.text_offset(), placement.atom());
        let contained = if start_raw == end_raw {
            ordinal >= start.atom_index() && ordinal < end.atom_index()
        } else {
            offset == start_raw && ordinal >= start.atom_index()
                || (offset > start_raw && offset < end_raw)
                || (offset == end_raw && ordinal < end.atom_index())
        };
        if contained {
            inside.push(placement.atom());
        }
    }
    inside
}

/// Returns the same-boundary ordinal of one atom placement.
fn same_boundary_ordinal(
    inline: &InlineContent,
    offset: xiaomu_core::text::TextOffset,
    atom: NodeId,
) -> usize {
    inline
        .atoms()
        .iter()
        .take_while(|placement| placement.text_offset() <= offset)
        .filter(|placement| placement.text_offset() == offset)
        .take_while(|placement| placement.atom() != atom)
        .count()
}

/// Returns the atom identity at one same-boundary ordinal.
fn atom_at_ordinal(
    inline: &InlineContent,
    offset: xiaomu_core::text::TextOffset,
    ordinal: usize,
) -> Option<NodeId> {
    inline
        .atoms()
        .iter()
        .filter(|placement| placement.text_offset() == offset)
        .nth(ordinal)
        .map(|placement| placement.atom())
}

/// Returns whether an anchor/focus pair denotes a collapsed caret.
fn selection_is_collapsed(anchor: Option<InlinePoint>, focus: InlinePoint) -> bool {
    match anchor {
        Some(anchor) => anchor == focus,
        None => true,
    }
}

/// Rebuilds a text-only Core selection from mixed-inline endpoints.
pub(crate) fn text_selection_from(
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<TextSelection, SessionError> {
    let to_point = |point: InlinePoint| {
        point
            .to_text_point()
            .map_err(|_| SessionError::SelectionInvalid)
    };
    let focus = to_point(focus)?;
    Ok(match anchor {
        Some(anchor) => TextSelection::new(to_point(anchor)?, focus),
        None => TextSelection::new(focus, focus),
    })
}

/// Builds the plan for Backspace over a plain-text node.
fn plan_backspace_text_only(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    let selection = text_selection_from(anchor, focus)?;
    plan_backspace_legacy(inline, selection)
}

/// Builds the plan for forward Delete over a plain-text node.
fn plan_delete_text_only(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    let selection = text_selection_from(anchor, focus)?;
    plan_delete_legacy(inline, selection)
}

/// Builds the plan for Backspace.
fn plan_backspace_legacy(
    inline: &InlineContent,
    selection: TextSelection,
) -> Result<PlannedAction, SessionError> {
    let node = selection.focus().node_id();
    let range = if selection.is_collapsed() {
        let focus = selection.focus().offset().as_usize();
        let text = concatenated(inline);
        match previous_boundary(&text, focus) {
            Some(start) => TextRange::new(inline.offset_at(start)?, selection.focus().offset())
                .map_err(SessionError::Core)?,
            None => return Ok(PlannedAction::NoChange),
        }
    } else {
        ordered_range(selection)?
    };

    Ok(PlannedAction::Commit(deletion_plan(node, range)))
}

/// Builds the plan for forward Delete.
fn plan_delete_legacy(
    inline: &InlineContent,
    selection: TextSelection,
) -> Result<PlannedAction, SessionError> {
    let node = selection.focus().node_id();
    let range = if selection.is_collapsed() {
        let focus = selection.focus().offset().as_usize();
        let text = concatenated(inline);
        match next_boundary(&text, focus) {
            Some(end) => TextRange::new(selection.focus().offset(), inline.offset_at(end)?)
                .map_err(SessionError::Core)?,
            None => return Ok(PlannedAction::NoChange),
        }
    } else {
        ordered_range(selection)?
    };

    Ok(PlannedAction::Commit(deletion_plan(node, range)))
}
