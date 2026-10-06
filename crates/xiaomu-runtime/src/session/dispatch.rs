//! Default host-neutral intent planning and dispatch.

use super::intent::{HistoryPolicy, PlannedAction};
use super::*;
use xiaomu_core::selection::TextSelection;

#[cfg(test)]
#[path = "cell_paste_boundary_tests.rs"]
mod cell_paste_boundary_tests;

impl DocumentSession {
    /// Applies one typed editing intent.
    ///
    /// Legal empty operations (Backspace at the start of the first block,
    /// caret moves at a boundary, TurnInto the kind already present) return
    /// [`SessionOutcome::NoChange`] without calling Core, advancing the
    /// revision, notifying document listeners, or writing history. A
    /// collapsed mark intent updates Runtime StoredMarks without a Core
    /// transaction.
    pub(super) fn apply_default_intent(
        &mut self,
        intent: &EditIntent,
        timestamp: Option<HistoryTimestamp>,
    ) -> Result<SessionOutcome, SessionError> {
        if matches!(intent, EditIntent::InsertHorizontalRule) {
            return Err(SessionError::UnsupportedEdit);
        }
        // Policy already saw the original logical command. Resolve these
        // against the live selection before generic node/cell-range guards;
        // unsupported targets are genuine no-ops, not range convergence.
        if let Some(outcome) = self.apply_table_cell_intent(intent)? {
            return Ok(outcome);
        }
        // Guard raw internal slices too, before closed/task fitting or any
        // tentative history/selection change can obscure unsupported spans.
        if let EditIntent::PasteSlice { slice } = intent {
            crate::clipboard::require_unit_tables(slice.roots())?;
        }
        // Whole-block selections carry identity, not an ordinary partial gap
        // range. A node-aware policy may replace this default; until then no
        // selection-driven edit may accidentally take an All/text/gap route.
        if self.selection.as_node_selection().is_some()
            && !matches!(
                intent,
                EditIntent::SetSelection { .. } | EditIntent::SetTaskChecked { .. }
            )
        {
            return Err(SessionError::UnsupportedEdit);
        }
        // Task fitting belongs to an explicit task-aware policy. Do not let
        // the generic single-leaf/table routes discard the task wrapper.
        if let EditIntent::PasteSlice { slice } = intent
            && slice.contains_tasks()
        {
            return Err(SessionError::UnsupportedEdit);
        }
        if let EditIntent::PasteSlice { slice } = intent
            && !slice.allows_default_fitting()
        {
            // Host policy has already had its chance to provide an exact plan.
            // A CellRange's open 1/1 Table carrier must not enter the historical
            // table-root fitter or collapse a target rectangle first.
            return Err(if slice.is_closed() {
                SessionError::ClipboardClosedUnsupported
            } else {
                SessionError::UnsupportedTableOperation
            });
        }
        // Policy has already seen the logical command. Normalize only inside
        // default dispatch, so hosts never need to guess a clipboard's origin.
        if matches!(intent, EditIntent::InsertLineBreak) {
            return self
                .apply_default_intent(&EditIntent::PasteText { text: "\n".into() }, timestamp);
        }
        // An explicitly addressed checkbox never edits or collapses the
        // current selection, including rectangular and structural selections.
        if let EditIntent::SetTaskChecked { item, checked } = intent {
            return self.set_task_checked(*item, *checked);
        }
        if let Some(outcome) = self.apply_cell_range_intent(intent)? {
            return Ok(outcome);
        }
        if let EditIntent::MoveCaret {
            caret_move,
            extend_selection,
        } = intent
        {
            return self.move_caret(*caret_move, *extend_selection);
        }
        if let EditIntent::PlaceCaret {
            offset,
            extend_selection,
        } = intent
        {
            return self.place_caret(*offset, *extend_selection);
        }
        if let EditIntent::SetSelection { anchor, focus } = intent {
            return self.set_selection(*anchor, *focus);
        }
        if let EditIntent::PasteSlice { slice } = intent {
            self.history.break_group();
            self.clear_stored_marks();
            let action = paste::plan_paste_slice(&self.document, self.selection, slice)?;
            return match action {
                PlannedAction::NoChange => Ok(SessionOutcome::NoChange),
                PlannedAction::Commit(plan) => self.commit(plan),
                PlannedAction::CommitStaged(staged) => self.commit_staged(staged),
            };
        }
        if let EditIntent::MoveToNextCell = intent {
            return self.move_to_next_cell();
        }
        if let EditIntent::MoveToPreviousCell = intent {
            return self.move_to_previous_cell();
        }

        // Table row/column operations are addressed by table + index and do
        // not require an inline focus; they are structural history
        // boundaries like the other structural commands.
        if matches!(
            intent,
            EditIntent::InsertTableRow { .. }
                | EditIntent::InsertTableColumn { .. }
                | EditIntent::DeleteTableRow { .. }
                | EditIntent::DeleteTableColumn { .. }
        ) {
            self.history.break_group();
            self.clear_stored_marks();
        }
        let table_action = match intent {
            EditIntent::InsertTableRow { table, index } => {
                Some(self.plan_insert_table_row(*table, *index))
            }
            EditIntent::InsertTableColumn { table, index } => {
                Some(self.plan_insert_table_column(*table, *index))
            }
            EditIntent::DeleteTableRow { table, index } => {
                Some(self.plan_delete_table_row(*table, *index))
            }
            EditIntent::DeleteTableColumn { table, index } => {
                Some(self.plan_delete_table_column(*table, *index))
            }
            _ => None,
        };
        if let Some(action) = table_action {
            return match action? {
                PlannedAction::NoChange => Ok(SessionOutcome::NoChange),
                PlannedAction::Commit(plan) => self.commit(plan),
                PlannedAction::CommitStaged(staged) => self.commit_staged(staged),
            };
        }

        // Backspace/Delete on a collapsed atomic node selection removes the
        // whole block as one logical history change.
        if matches!(intent, EditIntent::Backspace | EditIntent::Delete)
            && self.selection.as_atomic_node().is_some()
        {
            self.history.break_group();
            return match self.plan_atomic_removal()? {
                PlannedAction::NoChange => Ok(SessionOutcome::NoChange),
                PlannedAction::Commit(plan) => self.commit(plan),
                PlannedAction::CommitStaged(staged) => self.commit_staged(staged),
            };
        }

        // Backspace/Delete over a document-level text selection share the
        // same cross-block delete plan. Single-block forms continue through
        // the normal inline planners below.
        if matches!(intent, EditIntent::Backspace | EditIntent::Delete)
            && self.selection.as_same_node_inline().is_none()
        {
            self.history.break_group();
            let action = cross_block_atom::plan_delete_selection(&self.document, self.selection)?;
            return match action {
                PlannedAction::NoChange => Ok(SessionOutcome::NoChange),
                PlannedAction::Commit(plan) => self.commit(plan),
                PlannedAction::CommitStaged(staged) => self.commit_staged(staged),
            };
        }

        if !self.selection.is_collapsed()
            && matches!(
                intent,
                EditIntent::ToggleMark { .. }
                    | EditIntent::SetMark { .. }
                    | EditIntent::RemoveMark { .. }
            )
        {
            let action = marks::plan_range_mark(&self.document, self.selection, intent)?;
            if !matches!(action, PlannedAction::NoChange) {
                self.history.break_group();
                self.clear_stored_marks();
            }
            return match action {
                PlannedAction::NoChange => Ok(SessionOutcome::NoChange),
                PlannedAction::Commit(plan) => self.commit(plan),
                PlannedAction::CommitStaged(staged) => self.commit_staged(staged),
            };
        }

        // Remaining content and structural intents in this slice act from one
        // inline node. The endpoints keep their mixed-inline coordinates;
        // planners decide between the text-only and atom-aware contracts.
        let focus = self.inline_focus()?;
        let inline = self.inline_of(focus.node_id())?;
        let anchor = if self.selection.is_collapsed() {
            None
        } else {
            match self.selection.anchor() {
                DocumentPosition::Inline(point) if point.node_id() == focus.node_id() => {
                    Some(point)
                }
                _ => return Err(SessionError::SelectionInvalid),
            }
        };
        let action = match intent {
            EditIntent::InsertImage { image } => {
                self.history.break_group();
                self.plan_insert_image(image)?
            }
            EditIntent::InsertTable { rows, columns } => {
                self.history.break_group();
                self.plan_insert_table(*rows, *columns)?
            }
            EditIntent::InsertText { text } => atom_edit::plan_text_input(
                &self.document,
                &inline,
                anchor,
                focus,
                text,
                self.stored_marks.as_ref(),
                HistoryPolicy::Typing,
            )?,
            EditIntent::CommitComposition { range, text } => {
                self.history.break_group();
                inline
                    .validate_offset(range.start())
                    .map_err(SessionError::Core)?;
                inline
                    .validate_offset(range.end())
                    .map_err(SessionError::Core)?;
                if inline.atoms().is_empty() {
                    let ime_selection = TextSelection::new(
                        TextPoint::new(focus.node_id(), range.start(), focus.affinity()),
                        TextPoint::new(focus.node_id(), range.end(), focus.affinity()),
                    );
                    intent::plan_insert_text(
                        &inline,
                        ime_selection,
                        text,
                        self.stored_marks.as_ref(),
                        HistoryPolicy::Isolated,
                    )?
                } else {
                    atom_edit::plan_ime_commit(
                        &self.document,
                        &inline,
                        focus,
                        *range,
                        text,
                        self.stored_marks.as_ref(),
                    )?
                }
            }
            EditIntent::PasteText { text } => {
                self.history.break_group();
                atom_edit::plan_paste_text(
                    &self.document,
                    &inline,
                    anchor,
                    focus,
                    text,
                    self.stored_marks.as_ref(),
                )?
            }
            EditIntent::Backspace => {
                self.history.break_group();
                let at_block_start = self.selection.is_collapsed()
                    && focus.text_offset().as_usize() == 0
                    && focus.atom_index() == 0;
                // Priority at a block start: merge into the previous block
                // (same parent), then into the previous list item's tail,
                // then leave the list itself (outdent when nested, lift out
                // at the top level).
                if !at_block_start {
                    atom_edit::plan_backspace(&inline, anchor, focus)?
                } else {
                    match structure::plan_join_with_previous(&self.document, focus.node_id())? {
                        PlannedAction::NoChange => {
                            match structure::list_ancestry_of(&self.document, focus.node_id()) {
                                Some(ancestry) if ancestry.item_index > 0 => {
                                    structure::plan_merge_item_into_previous(
                                        &self.document,
                                        focus.node_id(),
                                    )?
                                }
                                Some(ancestry) => {
                                    let nested =
                                        structure::item_is_nested(&self.document, &ancestry)?;
                                    if nested {
                                        structure::plan_outdent_list_item(
                                            &self.document,
                                            focus.node_id(),
                                        )?
                                    } else {
                                        structure::plan_lift_out_of_list(&self.document, ancestry)?
                                    }
                                }
                                None => PlannedAction::NoChange,
                            }
                        }
                        planned => planned,
                    }
                }
            }
            EditIntent::Delete => {
                self.history.break_group();
                atom_edit::plan_delete(&inline, anchor, focus)?
            }
            EditIntent::ToggleMark { mark } if self.selection.is_collapsed() => {
                return self.toggle_stored_mark(mark);
            }
            EditIntent::SetMark { mark } if self.selection.is_collapsed() => {
                return self.set_stored_mark(mark);
            }
            EditIntent::RemoveMark { kind } if self.selection.is_collapsed() => {
                return self.remove_stored_mark(*kind);
            }
            EditIntent::SplitBlock => {
                // Split is an explicit history boundary, but pending marks are
                // intentionally inherited into the new tail block.
                self.history.break_group();
                split::plan_split_block(&self.document, anchor, focus)?
            }
            EditIntent::JoinWithPrevious => {
                self.history.break_group();
                self.clear_stored_marks();
                structure::plan_join_with_previous(&self.document, focus.node_id())?
            }
            EditIntent::TurnInto { kind } => {
                self.history.break_group();
                self.clear_stored_marks();
                structure::plan_turn_into(&self.document, focus.node_id(), kind)?
            }
            EditIntent::IndentListItem => {
                self.history.break_group();
                self.clear_stored_marks();
                structure::plan_indent_list_item(&self.document, focus.node_id())?
            }
            EditIntent::OutdentListItem => {
                self.history.break_group();
                self.clear_stored_marks();
                structure::plan_outdent_list_item(&self.document, focus.node_id())?
            }
            EditIntent::MoveCaret { .. }
            | EditIntent::ToggleMark { .. }
            | EditIntent::SetMark { .. }
            | EditIntent::RemoveMark { .. }
            | EditIntent::SetTaskChecked { .. }
            | EditIntent::InsertLineBreak
            | EditIntent::InsertHorizontalRule
            | EditIntent::MoveToNextCell
            | EditIntent::MoveToPreviousCell
            | EditIntent::InsertTableRow { .. }
            | EditIntent::InsertTableColumn { .. }
            | EditIntent::DeleteTableRow { .. }
            | EditIntent::DeleteTableColumn { .. }
            | EditIntent::MergeTableCells
            | EditIntent::SplitTableCell
            | EditIntent::PlaceCaret { .. }
            | EditIntent::PasteSlice { .. }
            | EditIntent::SetSelection { .. } => unreachable!("handled above"),
        };

        match action {
            PlannedAction::NoChange => Ok(SessionOutcome::NoChange),
            PlannedAction::Commit(mut plan) => {
                // Consume only after input is fully planned, so the canonical
                // inserted content still uses the pending marks. The existing
                // atomic commit applies this before listeners are notified.
                // Never turn default typing into a host Apply/isolation unit.
                if self.default_text_input_marks == DefaultTextInputMarks::ConsumePending
                    && matches!(intent,
                        EditIntent::InsertText { text }
                        | EditIntent::CommitComposition { text, .. } if !text.is_empty())
                {
                    plan = plan.with_stored_marks(None);
                }
                self.commit_at(plan, timestamp)
            }
            PlannedAction::CommitStaged(staged) => self.commit_staged(staged),
        }
    }
}
