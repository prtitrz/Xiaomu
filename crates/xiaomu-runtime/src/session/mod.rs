//! Document session orchestration.
//!
//! The session is the single canonical orchestration owner: it holds the
//! current snapshot, the current selection, the basic undo/redo stacks, and
//! the change-notification seam. Frontends translate input into
//! [`EditIntent`]s and never mutate the document directly.
//!
//! Every document edit flows through
//! `intent → EditPlan(Transaction + SelectionUpdate) → commit`, where the
//! new snapshot and the resolved selection become visible together or not
//! at all. The session selection is valid for the current snapshot at every
//! public read.

mod caret;
mod history;
mod intent;
mod listener;
mod outcome;
mod resolve;
mod selection;
mod split;
mod structure;

pub use history::HistoryStack;
pub use intent::{CaretMove, EditIntent, EditPlan, PrimaryEdit, SelectionUpdate};
pub use listener::DocumentChangeListener;
pub use outcome::{SessionError, SessionOutcome};
pub use selection::DocumentPosition;
pub use selection::DocumentSelection;

use xiaomu_core::document::{InlineContent, NodeId, XiaomuDocument};
use xiaomu_core::mapping::StepMap;
use xiaomu_core::selection::{TextPoint, TextSelection};
use xiaomu_core::transaction::{Transaction, TransactionOrigin};

use self::history::HistoryEntry;
use self::intent::PlannedAction;
use self::resolve::{affinity_of, collapsed_caret, preserved_focus, resolve_selection};

/// Orchestrates one editing session over an immutable Core snapshot.
///
/// The session keeps its selection valid for the current snapshot at every
/// public read. Commits are atomic: if a transaction is rejected, the
/// selection update cannot be resolved, or the resolved selection fails
/// validation, the session keeps its previous state unchanged.
pub struct DocumentSession {
    document: XiaomuDocument,
    selection: DocumentSelection,
    history: HistoryStack,
    listeners: Vec<Box<dyn DocumentChangeListener>>,
}

impl DocumentSession {
    /// Creates a session over `document` with an initial selection.
    ///
    /// The selection must be valid for the snapshot.
    pub fn new(
        document: XiaomuDocument,
        selection: DocumentSelection,
    ) -> Result<Self, SessionError> {
        selection
            .validate(&document)
            .map_err(|_| SessionError::SelectionInvalid)?;
        Ok(Self {
            document,
            selection,
            history: HistoryStack::new(),
            listeners: Vec::new(),
        })
    }

    /// Returns the current snapshot.
    #[must_use]
    pub const fn document(&self) -> &XiaomuDocument {
        &self.document
    }

    /// Returns the selection; always valid for the current snapshot.
    #[must_use]
    pub const fn selection(&self) -> DocumentSelection {
        self.selection
    }

    /// Returns the single-block Core selection when the whole selection
    /// lives inside one inline node; `None` for gap or cross-block
    /// selections (P1 single-block frontends use this).
    #[must_use]
    pub fn text_selection(&self) -> Option<TextSelection> {
        self.selection.as_single_node()
    }

    /// Returns the selected text as plain text, or `None` for a collapsed
    /// selection. Runs are concatenated in logical order; marks do not
    /// participate (plain-text clipboard, P1 scope).
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        let selection = self.text_selection()?;
        if selection.is_collapsed() {
            return None;
        }
        let range = selection.ordered_range().ok()?;
        let inline = self.inline_of(selection.focus().node_id()).ok()?;

        let mut selected = String::new();
        let mut cursor = 0usize;
        for run in inline.runs() {
            let run_start = cursor;
            let run_end = run_start + run.len_bytes();
            cursor = run_end;

            let overlap_start = range.start().as_usize().max(run_start);
            let overlap_end = range.end().as_usize().min(run_end);
            if overlap_start < overlap_end {
                selected.push_str(
                    &run.text().as_str()[overlap_start - run_start..overlap_end - run_start],
                );
            }
        }

        Some(selected)
    }

    /// Returns the `(undo, redo)` history depths.
    #[must_use]
    pub fn history_depths(&self) -> (usize, usize) {
        (self.history.undo_depth(), self.history.redo_depth())
    }

    /// Registers a change listener.
    pub fn add_listener(&mut self, listener: Box<dyn DocumentChangeListener>) {
        self.listeners.push(listener);
    }

    /// Applies one typed editing intent.
    ///
    /// Legal empty operations (Backspace at the start of the first block,
    /// caret moves at the boundary, toggling a mark with a collapsed
    /// selection, TurnInto the kind already present) return
    /// [`SessionOutcome::NoChange`] without calling Core, advancing the
    /// revision, notifying listeners, or writing history.
    pub fn apply_intent(&mut self, intent: &EditIntent) -> Result<SessionOutcome, SessionError> {
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

        // Content and structural intents still act from a single inline
        // node; cross-block forms gain dedicated commands in later slices.
        let focus = self.text_focus()?;
        let inline = self.inline_of(focus.node_id())?;
        let selection = self
            .selection
            .as_single_node()
            .ok_or(SessionError::SelectionInvalid)?;
        let action = match intent {
            EditIntent::InsertText { text } => intent::plan_insert_text(selection, text)?,
            EditIntent::Backspace => {
                let at_block_start = selection.is_collapsed() && focus.offset().as_usize() == 0;
                // Priority at a block start: merge into the previous block
                // (same parent), then into the previous list item's tail,
                // then leave the list itself (outdent when nested, lift out
                // at the top level).
                if !at_block_start {
                    intent::plan_backspace(&inline, selection)?
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
            EditIntent::Delete => intent::plan_delete(&inline, selection)?,
            EditIntent::ToggleMark { mark } => intent::plan_toggle_mark(&inline, selection, mark)?,
            EditIntent::SplitBlock => split::plan_split_block(&self.document, selection)?,
            EditIntent::JoinWithPrevious => {
                structure::plan_join_with_previous(&self.document, focus.node_id())?
            }
            EditIntent::TurnInto { kind } => {
                structure::plan_turn_into(&self.document, focus.node_id(), kind)?
            }
            EditIntent::IndentListItem => {
                structure::plan_indent_list_item(&self.document, focus.node_id())?
            }
            EditIntent::OutdentListItem => {
                structure::plan_outdent_list_item(&self.document, focus.node_id())?
            }
            EditIntent::MoveCaret { .. }
            | EditIntent::PlaceCaret { .. }
            | EditIntent::SetSelection { .. } => unreachable!("handled above"),
        };

        match action {
            PlannedAction::NoChange => Ok(SessionOutcome::NoChange),
            PlannedAction::Commit(plan) => self.commit(plan),
            PlannedAction::CommitStaged(staged) => self.commit_staged(staged),
        }
    }

    /// Applies a raw Core transaction with the map-existing selection
    /// policy.
    ///
    /// Unlike intents, raw applies have no no-op detection: even an empty
    /// transaction commits, advances the revision, and is recorded in
    /// history. The previous selection is mapped through the change map; a
    /// transaction that deletes a selection endpoint fails atomically.
    pub fn apply(&mut self, transaction: &Transaction) -> Result<SessionOutcome, SessionError> {
        self.commit(intent::map_existing_plan(transaction.clone()))
    }

    /// Undoes the newest history entry.
    ///
    /// Undo replays the recorded inverse transaction (ADR 0003), restoring
    /// the exact previous store, and reinstates the recorded
    /// `before_selection` directly. Undo on an empty history is a no-op.
    pub fn undo(&mut self) -> Result<SessionOutcome, SessionError> {
        let Some(entry) = self.history.take_undo() else {
            return Ok(SessionOutcome::NoChange);
        };

        match self.apply_history_transaction(&entry.undo, entry.before_selection) {
            Ok(()) => {
                self.history.park_undone(entry);
                Ok(SessionOutcome::DocumentChanged)
            }
            Err(error) => {
                self.history.restore_undo(entry);
                Err(error)
            }
        }
    }

    /// Redoes the newest undone entry.
    ///
    /// Redo replays the original transaction and reinstates the recorded
    /// `after_selection`. Redo on an empty redo stack is a no-op.
    pub fn redo(&mut self) -> Result<SessionOutcome, SessionError> {
        let Some(entry) = self.history.take_redo() else {
            return Ok(SessionOutcome::NoChange);
        };

        match self.apply_history_transaction(&entry.redo, entry.after_selection) {
            Ok(()) => {
                self.history.requeue_redone(entry);
                Ok(SessionOutcome::DocumentChanged)
            }
            Err(error) => {
                self.history.restore_redo(entry);
                Err(error)
            }
        }
    }

    fn commit(&mut self, plan: EditPlan) -> Result<SessionOutcome, SessionError> {
        let before_selection = self.selection;
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
        let undo = applied.inverse().clone();
        // Redo must reproduce the post-commit identities, not mint new ones.
        // `inverse(inverse(T))` restores allocated NodeIds (SplitNode tail)
        // via RestoreSubtree; replaying the original SplitNode would not.
        let redo = undo
            .apply_with_changes(applied.document())
            .map_err(SessionError::Core)?
            .inverse()
            .clone();

        self.history.record(HistoryEntry {
            redo,
            undo,
            before_selection,
            after_selection,
        });
        self.document = applied.into_document();
        self.selection = after_selection;
        self.notify_document_changed();

        Ok(SessionOutcome::DocumentChanged)
    }

    /// Commits a multi-stage command as one history entry.
    ///
    /// Stages run against intermediate snapshots that never become visible:
    /// if any stage fails, the session keeps its previous state unchanged.
    /// The combined undo applies every stage's inverse in reverse order; the
    /// redo is `inverse(undo)` so restored identities are reused, matching
    /// single-transaction commits.
    fn commit_staged(
        &mut self,
        staged: structure::StagedPlan,
    ) -> Result<SessionOutcome, SessionError> {
        let before_selection = self.selection;
        let mut current = self.document.clone();
        let mut inverse_groups: Vec<Transaction> = Vec::new();
        let mut split_tail = None;

        for build in staged.stages {
            let transaction = build(&current)?;
            let applied = transaction
                .apply_with_changes(&current)
                .map_err(SessionError::Core)?;
            if split_tail.is_none() {
                split_tail = applied
                    .changes()
                    .steps()
                    .iter()
                    .rev()
                    .find_map(|step| match step {
                        StepMap::NodeSplit { inserted, .. } => Some(*inserted),
                        _ => None,
                    });
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
            SelectionUpdate::PreserveFocus => preserved_focus(before_selection, &current)?,
            SelectionUpdate::CaretAtSplitTail => {
                let inserted = split_tail.ok_or(SessionError::SelectionInvalid)?;
                collapsed_caret(&current, inserted, 0, affinity_of(before_selection))?
            }
            _ => return Err(SessionError::SelectionInvalid),
        };

        self.history.record(HistoryEntry {
            redo,
            undo,
            before_selection,
            after_selection,
        });
        self.document = current;
        self.selection = after_selection;
        self.notify_document_changed();

        Ok(SessionOutcome::DocumentChanged)
    }

    fn apply_history_transaction(
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

        self.document = applied.into_document();
        self.selection = selection;
        self.notify_document_changed();

        Ok(())
    }

    fn inline_of(&self, node: NodeId) -> Result<InlineContent, SessionError> {
        self.document
            .node(node)
            .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
            .content()
            .as_inline()
            .cloned()
            .ok_or(SessionError::SelectionInvalid)
    }

    /// Returns the focused text position when the focus endpoint carries
    /// text coordinates; content-editing intents cannot act on a gap.
    fn text_focus(&self) -> Result<TextPoint, SessionError> {
        match self.selection.focus() {
            DocumentPosition::Text(point) => Ok(point),
            DocumentPosition::Gap(_) => Err(SessionError::SelectionInvalid),
        }
    }

    fn notify_document_changed(&mut self) {
        for listener in &mut self.listeners {
            listener.document_changed(&self.document, self.selection);
        }
    }

    fn notify_selection_changed(&mut self) {
        let selection = self.selection;
        for listener in &mut self.listeners {
            listener.selection_changed(selection);
        }
    }
}
