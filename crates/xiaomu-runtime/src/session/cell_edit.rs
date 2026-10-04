//! Content commands over a rectangular selection. Cell identities and
//! table shape survive; the content change and selection/history commit once.
use super::intent::{EditPlan, HistoryPolicy, PlannedAction, SelectionUpdate};
use super::structure::{children_of, user_transaction};
use super::{CellRange, DocumentSession, EditIntent, SessionError, SessionOutcome};
use xiaomu_core::document::{InlineContent, MarkSet, NodeAttrs, NodeContent, NodeKind, TextRun};
use xiaomu_core::transaction::TransactionStep;

impl DocumentSession {
    /// Handles rectangular content commands before ordinary text planning.
    /// `None` allows an explicitly supported intent to continue dispatching.
    pub(super) fn apply_cell_range_intent(
        &mut self,
        intent: &EditIntent,
    ) -> Result<Option<SessionOutcome>, SessionError> {
        let Some(range) = self.selection.active_cell_range() else {
            return Ok(None);
        };
        let replacement = match intent {
            EditIntent::Backspace | EditIntent::Delete => Some(None),
            EditIntent::InsertText { text } | EditIntent::PasteText { text } => {
                Some(Some(text.as_str()))
            }
            EditIntent::CommitComposition {
                range: composition,
                text,
            } => {
                // A cell selection has no canonical inline text range. Its
                // native input proxy is empty and commits at exactly 0..0;
                // offsets into a former paragraph must never be ignored.
                if composition.start().as_usize() != 0 || composition.end().as_usize() != 0 {
                    return Err(SessionError::SelectionInvalid);
                }
                Some(Some(text.as_str()))
            }
            _ => None,
        };
        if let Some(replacement) = replacement {
            let action = self.plan_cell_content(range, replacement)?;
            self.history.break_group();
            self.clear_stored_marks();
            return match action {
                PlannedAction::Commit(plan) => self.commit(plan).map(Some),
                PlannedAction::NoChange => Ok(Some(SessionOutcome::NoChange)),
                PlannedAction::CommitStaged(_) => unreachable!(),
            };
        }
        if matches!(
            intent,
            EditIntent::ToggleMark { .. }
                | EditIntent::SetMark { .. }
                | EditIntent::RemoveMark { .. }
        ) {
            let action = super::marks::plan_cell_range_mark(&self.document, range, intent)?;
            if !matches!(action, PlannedAction::NoChange) {
                self.history.break_group();
                self.clear_stored_marks();
            }
            return match action {
                PlannedAction::Commit(plan) => self.commit(plan).map(Some),
                PlannedAction::NoChange => Ok(Some(SessionOutcome::NoChange)),
                PlannedAction::CommitStaged(_) => unreachable!(),
            };
        }
        match intent {
            EditIntent::PasteSlice { .. }
            | EditIntent::InsertTableRow { .. }
            | EditIntent::InsertTableColumn { .. }
            | EditIntent::DeleteTableRow { .. }
            | EditIntent::DeleteTableColumn { .. } => {}
            EditIntent::MoveCaret { .. }
            | EditIntent::MoveToNextCell
            | EditIntent::MoveToPreviousCell
            | EditIntent::PlaceCaret { .. }
            | EditIntent::SetSelection { .. } => self.collapse_cell_range(),
            // Other structural content commands need an explicit rectangular
            // contract; never silently edit just one cell.
            _ => return Err(SessionError::SelectionInvalid),
        }
        Ok(None)
    }

    fn plan_cell_content(
        &self,
        range: CellRange,
        replacement: Option<&str>,
    ) -> Result<PlannedAction, SessionError> {
        // Retain the generic anchor-based replacement contract for both unit
        // and spanning cells. Covered slots are never separate mutation
        // targets, and cells crossing in from above/left remain untouched.
        let mut cells = range.unique_origins(&self.document)?;
        let already_empty = cells.iter().all(|cell| {
            let children = children_of(&self.document, *cell);
            children.len() == 1
                && self.document.node(children[0]).is_some_and(|node| {
                    matches!(node.kind(), NodeKind::Paragraph)
                        && node.content().as_inline().is_some_and(|inline| {
                            inline.len_bytes() == 0 && inline.atoms().is_empty()
                        })
                })
        });
        if replacement.is_none() && already_empty {
            return Ok(PlannedAction::NoChange);
        }
        // The anchor's seed is allocated last so selection resolution does
        // not need a guessed fresh NodeId. All mutations still share a tx.
        cells.sort_by_key(|cell| *cell == range.anchor());
        let mut transaction = user_transaction();
        for cell in cells {
            let text = if cell == range.anchor() {
                replacement.unwrap_or("")
            } else {
                ""
            };
            let content = if text.is_empty() {
                InlineContent::empty()
            } else {
                InlineContent::new([
                    TextRun::new(text, MarkSet::empty()).map_err(SessionError::Core)?
                ])
                .map_err(SessionError::Core)?
            };
            transaction.push_step(TransactionStep::InsertNode {
                parent: cell,
                index: 0,
                kind: NodeKind::Paragraph,
                attrs: NodeAttrs::empty(),
                content: NodeContent::Inline(content),
            });
            for node in children_of(&self.document, cell) {
                transaction.push_step(TransactionStep::RemoveNode { node });
            }
        }
        let selection = match replacement {
            None => SelectionUpdate::MapExisting,
            Some(text) => SelectionUpdate::CaretAtLastInsertedOffset { offset: text.len() },
        };
        Ok(PlannedAction::Commit(
            EditPlan::new(transaction, selection, None)
                .with_history_policy(HistoryPolicy::Isolated),
        ))
    }
}
