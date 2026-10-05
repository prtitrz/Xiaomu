//! Candidate preparation, validation and atomic document publication.

use super::history::{self, HistoryEntry, HistoryGroup};
use super::resolve::{
    affinity_of, collapsed_caret, preserved_focus, preserved_selection, resolve_selection,
};
use super::*;
use xiaomu_core::mapping::StepMap;
use xiaomu_core::transaction::{Transaction, TransactionOrigin};

/// A completely evaluated candidate. Only its owning session may publish it;
/// callers keep that session mutably borrowed from preparation to publication.
pub(super) struct PreparedCommit {
    document: XiaomuDocument,
    selection: DocumentSelection,
    history: HistoryEntry,
    timestamp: Option<HistoryTimestamp>,
    stored_marks_after: Option<Option<MarkSet>>,
    input_rule_undo: Option<std::rc::Rc<input_rule_undo::InputRuleUndoToken>>,
}

impl DocumentSession {
    pub(super) fn commit(&mut self, plan: EditPlan) -> Result<SessionOutcome, SessionError> {
        self.commit_at(plan, None)
    }

    pub(super) fn commit_at(
        &mut self,
        plan: EditPlan,
        timestamp: Option<HistoryTimestamp>,
    ) -> Result<SessionOutcome, SessionError> {
        let prepared = self.prepare_commit_at(plan, timestamp)?;
        Ok(self.publish_prepared_commit(prepared))
    }

    /// All fallible Core, selection, policy and inverse preparation is shared
    /// by ordinary commits and scoped external-publication commands.
    pub(super) fn prepare_commit(&self, plan: EditPlan) -> Result<PreparedCommit, SessionError> {
        self.prepare_commit_at(plan, None)
    }

    fn prepare_commit_at(
        &self,
        mut plan: EditPlan,
        timestamp: Option<HistoryTimestamp>,
    ) -> Result<PreparedCommit, SessionError> {
        let before_selection = self.selection;
        let group = history::history_group_for_plan(&plan);
        let applied = plan
            .transaction()
            .apply_with_changes(&self.document)
            .map_err(SessionError::Core)?;
        let after_selection = resolve_selection(
            &plan,
            applied.changes(),
            before_selection,
            &self.document,
            applied.document(),
        )?;
        let stored_marks_after = plan.stored_marks_after().cloned();
        if stored_marks_after.is_some()
            && (!after_selection.is_collapsed() || after_selection.as_same_node_inline().is_none())
        {
            return Err(SessionError::SelectionInvalid);
        }
        self.validate_candidate(applied.document())?;
        let undo = applied.inverse().clone();
        // Redo must reproduce the post-commit identities, not mint new ones.
        // `inverse(inverse(T))` restores allocated NodeIds (SplitNode tail)
        // via RestoreSubtree; replaying the original SplitNode would not.
        let redo = undo
            .apply_with_changes(applied.document())
            .map_err(SessionError::Core)?
            .inverse()
            .clone();

        let input_rule_undo = plan
            .take_input_rule_undo()
            .map(|spec| self.prepare_input_rule_undo(spec, applied.document(), after_selection))
            .transpose()?;

        Ok(PreparedCommit {
            document: applied.into_document(),
            selection: after_selection,
            history: HistoryEntry {
                redo,
                undo,
                before_selection: self.history_selection_before.unwrap_or(before_selection),
                after_selection,
                group,
            },
            timestamp,
            stored_marks_after,
            input_rule_undo,
        })
    }

    /// No ordinary validation or reapplication occurs after external writes.
    /// Allocation failure, listener panic and process termination are not an
    /// atomic transaction with another process's clipboard.
    pub(super) fn publish_prepared_commit(&mut self, prepared: PreparedCommit) -> SessionOutcome {
        self.history.record(
            prepared.history,
            prepared.timestamp,
            self.history_options.typing_group_delay_ms(),
        );
        self.document = prepared.document;
        self.selection = prepared.selection;
        self.input_rule_undo = prepared.input_rule_undo;
        if let Some(marks) = prepared.stored_marks_after {
            self.stored_marks = marks;
        }
        self.notify_document_changed();
        SessionOutcome::DocumentChanged
    }

    /// Commits a multi-stage command as one history entry.
    ///
    /// Stages run against intermediate snapshots that never become visible:
    /// if any stage fails, the session keeps its previous state unchanged.
    /// The combined undo applies every stage's inverse in reverse order; the
    /// redo is `inverse(undo)` so restored identities are reused, matching
    /// single-transaction commits.
    pub(super) fn commit_staged(
        &mut self,
        staged: structure::StagedPlan,
    ) -> Result<SessionOutcome, SessionError> {
        let before_selection = self.selection;
        let mut current = self.document.clone();
        let mut inverse_groups: Vec<Transaction> = Vec::new();
        let mut split_tail = None;
        let mut last_inserted = None;
        // MapExisting resolves the after-selection by folding the mapped
        // selection through every stage's change map (cell ranges shrink or
        // survive through the same fold).
        let mut mapped = before_selection;

        for build in staged.stages {
            let transaction = build(&current)?;
            let applied = transaction
                .apply_with_changes(&current)
                .map_err(SessionError::Core)?;
            if matches!(staged.selection_update, SelectionUpdate::MapExisting) {
                mapped = mapped.map_through(applied.changes(), &current)?;
            }
            if split_tail.is_none() {
                split_tail = applied
                    .changes()
                    .steps()
                    .iter()
                    .rev()
                    .find_map(|step| match step {
                        StepMap::NodeSplit { inserted, .. }
                        | StepMap::InlineNodeSplit { inserted, .. } => Some(*inserted),
                        _ => None,
                    });
            }
            if let Some(inserted) =
                applied
                    .changes()
                    .steps()
                    .iter()
                    .rev()
                    .find_map(|step| match step {
                        StepMap::NodeInserted { inserted, .. } => Some(*inserted),
                        _ => None,
                    })
            {
                last_inserted = Some(inserted);
            }
            inverse_groups.push(applied.inverse().clone());
            current = applied.into_document();
        }

        let mut undo = Transaction::new(TransactionOrigin::UserInput);
        for transaction in inverse_groups.into_iter().rev() {
            for step in transaction.steps() {
                undo.push_step(step.clone());
            }
        }
        // Redo must reproduce the post-command identities (see `commit`).
        let redo = undo
            .apply_with_changes(&current)
            .map_err(SessionError::Core)?
            .inverse()
            .clone();

        let after_selection = match staged.selection_update {
            SelectionUpdate::Exact { selection } => {
                selection.validate(&current)?;
                selection
            }
            SelectionUpdate::AllDocument => DocumentSelection::all(&current),
            SelectionUpdate::PreserveFocus => preserved_focus(before_selection, &current)?,
            SelectionUpdate::PreserveSelection => preserved_selection(before_selection, &current)?,
            SelectionUpdate::CaretAtSplitTail => {
                let inserted = split_tail.ok_or(SessionError::SelectionInvalid)?;
                collapsed_caret(&current, inserted, 0, affinity_of(before_selection))?
            }
            SelectionUpdate::CaretAtLastInsertedOffset { offset } => {
                let inserted = last_inserted.ok_or(SessionError::SelectionInvalid)?;
                collapsed_caret(&current, inserted, offset, affinity_of(before_selection))?
            }
            SelectionUpdate::MapExisting => {
                mapped.validate(&current)?;
                mapped
            }
            _ => return Err(SessionError::SelectionInvalid),
        };

        self.validate_candidate(&current)?;
        self.history.record(
            HistoryEntry {
                redo,
                undo,
                before_selection: self.history_selection_before.unwrap_or(before_selection),
                after_selection,
                group: HistoryGroup::Isolated,
            },
            None,
            self.history_options.typing_group_delay_ms(),
        );
        self.document = current;
        self.selection = after_selection;
        self.input_rule_undo = None;
        self.notify_document_changed();

        Ok(SessionOutcome::DocumentChanged)
    }

    pub(super) fn apply_history_transaction(
        &mut self,
        transaction: &Transaction,
        selection: DocumentSelection,
    ) -> Result<(), SessionError> {
        let applied = transaction
            .apply_with_changes(&self.document)
            .map_err(SessionError::Core)?;
        selection
            .validate(applied.document())
            .map_err(|_| SessionError::SelectionInvalid)?;

        self.validate_candidate(applied.document())?;
        self.document = applied.into_document();
        self.selection = selection;
        self.input_rule_undo = None;
        self.notify_document_changed();

        Ok(())
    }
}

#[cfg(test)]
#[path = "exact_staged_tests.rs"]
mod exact_staged_tests;
