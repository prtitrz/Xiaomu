//! Document session orchestration.
//!
//! The session is the single canonical orchestration owner: it holds the
//! current snapshot, the current selection, local editing state, undo/redo,
//! and the change-notification seam. Frontends translate input into
//! [`EditIntent`]s and never mutate the document directly.
//!
//! Every document edit flows through
//! `intent → EditPlan(Transaction + SelectionUpdate) → commit`, where the
//! new snapshot and the resolved selection become visible together or not
//! at all. The session selection is valid for the current snapshot at every
//! public read.

mod atom_edit;
mod atomic_block;
mod caret;
mod cell_edit;
mod cell_range;
mod commit;
mod cross_block;
mod cross_block_atom;
mod dispatch;
mod history;
mod history_options;
#[cfg(test)]
mod history_options_tests;
mod image;
mod input_rule_undo;
mod intent;
mod listener;
mod marks;
mod outcome;
mod paste;
mod paste_fragment;
pub(crate) mod paste_hierarchy;
mod paste_table;
mod plan;
mod policy;
mod prepared_cut;
#[cfg(test)]
mod prepared_cut_tests;
mod resolve;
mod selection;
mod split;
mod stored_marks;
mod structure;
mod table;
mod table_commands;
#[cfg(test)]
mod table_commands_tests;
#[cfg(test)]
mod table_geometry_tests;
mod task_checked;
mod text_input_marks;

pub use history::HistoryStack;
pub use history_options::{EmptyHistoryBehavior, HistoryOptions, HistorySelectionMode};
pub use input_rule_undo::InputRuleUndoSpec;
pub use intent::{CaretMove, EditIntent, EditPlan, PrimaryEdit, SelectionUpdate};
pub use listener::DocumentChangeListener;
pub use outcome::{SessionError, SessionOutcome};
pub use policy::{IntentDisposition, PolicyError, SessionContext, SessionPolicy};
pub use prepared_cut::PreparedCut;
pub use selection::CellRange;
pub use selection::DocumentPosition;
pub use selection::DocumentSelection;
pub use text_input_marks::DefaultTextInputMarks;

use xiaomu_core::document::{InlineContent, MarkSet, NodeId, XiaomuDocument};
use xiaomu_core::selection::{InlinePoint, TextPoint, TextSelection};
use xiaomu_core::transaction::Transaction;

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
    history_options: HistoryOptions,
    default_text_input_marks: DefaultTextInputMarks,
    stored_marks: Option<MarkSet>,
    listeners: Vec<Box<dyn DocumentChangeListener>>,
    policy: Option<Box<dyn SessionPolicy>>,
    // An atomic platform replacement plans at a tentative selection but
    // Undo must restore the selection from before the whole operation.
    history_selection_before: Option<DocumentSelection>,
    // At most one bounded, session-local token. Rollback shares ownership;
    // no canonical metadata or document/history snapshot is retained.
    input_rule_undo: Option<std::rc::Rc<input_rule_undo::InputRuleUndoToken>>,
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
            history_options: HistoryOptions::new(),
            default_text_input_marks: DefaultTextInputMarks::PreservePending,
            stored_marks: None,
            listeners: Vec::new(),
            policy: None,
            history_selection_before: None,
            input_rule_undo: None,
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

    /// Returns history options captured once when this session was created.
    #[must_use]
    pub const fn history_options(&self) -> HistoryOptions {
        self.history_options
    }

    /// Returns default text-input mark behavior captured at construction.
    #[must_use]
    pub const fn default_text_input_marks(&self) -> DefaultTextInputMarks {
        self.default_text_input_marks
    }

    /// Returns the single-block Core selection when the whole selection
    /// lives inside one inline node; `None` for gap or cross-block
    /// selections (P1 single-block frontends use this).
    #[must_use]
    pub fn text_selection(&self) -> Option<TextSelection> {
        self.selection.as_single_node()
    }

    /// Returns the selected content's plain-text fallback, or `None` for a
    /// collapsed selection.
    ///
    /// Cross-block boundaries are represented by `\n`. Marks and structure
    /// are omitted from this convenience view; callers that need them should
    /// use [`Self::clipboard_slice`].
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        self.clipboard_slice()
            .ok()
            .flatten()
            .map(|slice| slice.plain_text().to_owned())
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

    /// Applies a raw Core transaction with the map-existing selection
    /// policy.
    ///
    /// Unlike intents, raw applies have no no-op detection: even an empty
    /// transaction commits, advances the revision, and is recorded in
    /// history. The previous selection is mapped through the change map; a
    /// transaction that deletes a selection endpoint fails atomically.
    pub fn apply(&mut self, transaction: &Transaction) -> Result<SessionOutcome, SessionError> {
        self.with_transient_rollback(|session| {
            session.history.break_group();
            session.clear_stored_marks();
            session.commit(intent::map_existing_plan(transaction.clone()))
        })
    }

    /// Undoes the newest history entry.
    ///
    /// Undo replays the recorded inverse transaction (ADR 0003), restoring
    /// the exact previous store, and reinstates the entry's `before_selection`.
    /// An opted-in traversal captures the current selection for the next Redo
    /// only after success. Empty-stack editing state follows `history_options`.
    pub fn undo(&mut self) -> Result<SessionOutcome, SessionError> {
        self.with_transient_rollback(Self::undo_inner)
    }

    fn undo_inner(&mut self) -> Result<SessionOutcome, SessionError> {
        if self.history.undo_depth() == 0
            && self.history_options.empty_behavior() == EmptyHistoryBehavior::PreserveEditingState
        {
            return Ok(SessionOutcome::NoChange);
        }
        let before_traversal = self.selection;
        self.clear_stored_marks();
        let Some(mut entry) = self.history.take_undo() else {
            return Ok(SessionOutcome::NoChange);
        };

        match self.apply_history_transaction(&entry.undo, entry.before_selection) {
            Ok(()) => {
                if self.history_options.selection_mode() == HistorySelectionMode::CaptureOnTraversal
                {
                    entry.after_selection = before_traversal;
                }
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
    /// `after_selection`. Opted-in capture saves the current selection for the
    /// next Undo only after success, not for the current target document.
    /// Empty-stack editing state follows `history_options`.
    pub fn redo(&mut self) -> Result<SessionOutcome, SessionError> {
        self.with_transient_rollback(Self::redo_inner)
    }

    fn redo_inner(&mut self) -> Result<SessionOutcome, SessionError> {
        if self.history.redo_depth() == 0
            && self.history_options.empty_behavior() == EmptyHistoryBehavior::PreserveEditingState
        {
            return Ok(SessionOutcome::NoChange);
        }
        let before_traversal = self.selection;
        self.clear_stored_marks();
        let Some(mut entry) = self.history.take_redo() else {
            return Ok(SessionOutcome::NoChange);
        };

        match self.apply_history_transaction(&entry.redo, entry.after_selection) {
            Ok(()) => {
                if self.history_options.selection_mode() == HistorySelectionMode::CaptureOnTraversal
                {
                    entry.before_selection = before_traversal;
                }
                self.history.requeue_redone(entry);
                Ok(SessionOutcome::DocumentChanged)
            }
            Err(error) => {
                self.history.restore_redo(entry);
                Err(error)
            }
        }
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

    /// Returns the focused mixed-inline position; content-editing intents
    /// cannot act on a gap or an atomic node selection.
    fn inline_focus(&self) -> Result<InlinePoint, SessionError> {
        match self.selection.focus() {
            DocumentPosition::Inline(point) => Ok(point),
            DocumentPosition::Gap(_) | DocumentPosition::Atomic(_) => {
                Err(SessionError::SelectionInvalid)
            }
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
