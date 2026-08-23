//! Basic undo/redo stack: one entry per committed transaction.

use xiaomu_core::selection::TextSelection;
use xiaomu_core::transaction::Transaction;

/// One undo unit: the redo/undo transaction pair plus the selections
/// recorded in both snapshot coordinate spaces.
pub(crate) struct HistoryEntry {
    pub(crate) redo: Transaction,
    pub(crate) undo: Transaction,
    pub(crate) before_selection: TextSelection,
    pub(crate) after_selection: TextSelection,
}

/// Basic undo/redo stacks for one session.
///
/// P1 records exactly one entry per document-changing transaction; typing
/// coalescing and history grouping are later-phase concerns. Recording a new
/// edit clears the redo stack.
pub struct HistoryStack {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl HistoryStack {
    /// Creates empty stacks.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Returns how many undo entries are available.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Returns how many redo entries are available.
    #[must_use]
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Records a committed edit and clears the redo stack.
    pub(crate) fn record(&mut self, entry: HistoryEntry) {
        self.redo.clear();
        self.undo.push(entry);
    }

    /// Takes the newest undo entry.
    pub(crate) fn take_undo(&mut self) -> Option<HistoryEntry> {
        self.undo.pop()
    }

    /// Puts a failed undo attempt back in place.
    pub(crate) fn restore_undo(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
    }

    /// Parks a successfully undone entry for redo.
    pub(crate) fn park_undone(&mut self, entry: HistoryEntry) {
        self.redo.push(entry);
    }

    /// Takes the newest redo entry.
    pub(crate) fn take_redo(&mut self) -> Option<HistoryEntry> {
        self.redo.pop()
    }

    /// Puts a failed redo attempt back in place.
    pub(crate) fn restore_redo(&mut self, entry: HistoryEntry) {
        self.redo.push(entry);
    }

    /// Requeues a successfully redone entry for undo without clearing the
    /// remaining redo stack.
    pub(crate) fn requeue_redone(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
    }
}

impl Default for HistoryStack {
    fn default() -> Self {
        Self::new()
    }
}
