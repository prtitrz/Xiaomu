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

mod history;
mod intent;
mod listener;
mod outcome;

pub use history::HistoryStack;
pub use intent::{CaretMove, EditIntent, EditPlan, PrimaryEdit, SelectionUpdate};
pub use listener::DocumentChangeListener;
pub use outcome::{SessionError, SessionOutcome};

use xiaomu_core::document::{InlineContent, NodeId, XiaomuDocument};
use xiaomu_core::mapping::{ChangeMap, MappedPosition};
use xiaomu_core::selection::{TextPoint, TextSelection};
use xiaomu_core::transaction::Transaction;

use self::history::HistoryEntry;
use self::intent::PlannedAction;

/// Orchestrates one editing session over an immutable Core snapshot.
///
/// The session keeps its selection valid for the current snapshot at every
/// public read. Commits are atomic: if a transaction is rejected, the
/// selection update cannot be resolved, or the resolved selection fails
/// validation, the session keeps its previous state unchanged.
pub struct DocumentSession {
    document: XiaomuDocument,
    selection: TextSelection,
    history: HistoryStack,
    listeners: Vec<Box<dyn DocumentChangeListener>>,
}

impl DocumentSession {
    /// Creates a session over `document` with an initial selection.
    ///
    /// The selection must be valid for the snapshot.
    pub fn new(document: XiaomuDocument, selection: TextSelection) -> Result<Self, SessionError> {
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
    pub const fn selection(&self) -> TextSelection {
        self.selection
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
    /// Legal empty operations (Backspace at the paragraph start, caret moves
    /// at the boundary, toggling a mark with a collapsed selection) return
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

        let inline = self.inline_of(self.selection.focus().node_id())?;
        let action = match intent {
            EditIntent::InsertText { text } => intent::plan_insert_text(self.selection, text)?,
            EditIntent::Backspace => intent::plan_backspace(&inline, self.selection)?,
            EditIntent::Delete => intent::plan_delete(&inline, self.selection)?,
            EditIntent::ToggleMark { mark } => {
                intent::plan_toggle_mark(&inline, self.selection, mark)?
            }
            EditIntent::MoveCaret { .. } => unreachable!("handled above"),
        };

        match action {
            PlannedAction::NoChange => Ok(SessionOutcome::NoChange),
            PlannedAction::Commit(plan) => self.commit(plan),
        }
    }

    /// Applies a raw Core transaction with the map-existing selection
    /// policy.
    ///
    /// Unlike intents, raw applies have no no-op detection: even an empty
    /// transaction commits, advances the revision, and is recorded in
    /// history. The previous selection is mapped through the change map; a
    /// transaction that deletes the selection's node fails atomically.
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
            applied.document(),
        )?;

        self.history.record(HistoryEntry {
            redo: plan.transaction().clone(),
            undo: applied.inverse().clone(),
            before_selection,
            after_selection,
        });
        self.document = applied.into_document();
        self.selection = after_selection;
        self.notify_document_changed();

        Ok(SessionOutcome::DocumentChanged)
    }

    fn apply_history_transaction(
        &mut self,
        transaction: &Transaction,
        selection: TextSelection,
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

    fn move_caret(
        &mut self,
        caret_move: CaretMove,
        extend_selection: bool,
    ) -> Result<SessionOutcome, SessionError> {
        let inline = self.inline_of(self.selection.focus().node_id())?;
        let text = intent::concatenated(&inline);

        let focus = self.selection.focus();
        let current = focus.offset().as_usize();
        let target = match caret_move {
            CaretMove::Backward => intent::previous_boundary(&text, current),
            CaretMove::Forward => intent::next_boundary(&text, current),
            CaretMove::ToStart => (current != 0).then_some(0),
            CaretMove::ToEnd => (current != text.len()).then_some(text.len()),
        };

        let Some(raw) = target else {
            return Ok(SessionOutcome::NoChange);
        };
        let offset = inline.offset_at(raw).map_err(SessionError::Core)?;
        let moved = TextPoint::new(focus.node_id(), offset, focus.affinity());
        let next = if extend_selection {
            TextSelection::new(self.selection.anchor(), moved)
        } else {
            TextSelection::collapsed(moved)
        };

        if next == self.selection {
            return Ok(SessionOutcome::NoChange);
        }
        self.selection = next;
        self.notify_selection_changed();

        Ok(SessionOutcome::SelectionChanged)
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

/// Resolves the after-selection of one committed plan.
fn resolve_selection(
    plan: &EditPlan,
    changes: &ChangeMap,
    before: TextSelection,
    document: &XiaomuDocument,
) -> Result<TextSelection, SessionError> {
    match plan.selection_update() {
        SelectionUpdate::CaretAfterReplacement | SelectionUpdate::CaretAtEditStart => {
            let edit = plan.primary_edit().ok_or(SessionError::SelectionInvalid)?;
            let raw = match plan.selection_update() {
                SelectionUpdate::CaretAfterReplacement => {
                    edit.range().start().as_usize() + edit.inserted_len()
                }
                _ => edit.range().start().as_usize(),
            };

            let inline = document
                .node(edit.node())
                .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
                .content()
                .as_inline()
                .ok_or(SessionError::SelectionInvalid)?;
            let offset = inline.offset_at(raw).map_err(SessionError::Core)?;
            let selection = TextSelection::collapsed(TextPoint::new(
                edit.node(),
                offset,
                before.focus().affinity(),
            ));
            selection
                .validate(document)
                .map_err(|_| SessionError::SelectionInvalid)?;
            Ok(selection)
        }
        SelectionUpdate::MapExisting => match changes.map_text_selection(before) {
            MappedPosition::Mapped(mapped) => {
                mapped
                    .validate(document)
                    .map_err(|_| SessionError::SelectionInvalid)?;
                Ok(mapped)
            }
            MappedPosition::Deleted => Err(SessionError::SelectionDeleted),
        },
    }
}
