//! Immutable per-session choices for history traversal, not typing grouping.

/// Which selection is restored when traversing a recorded history entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum HistorySelectionMode {
    /// Preserve the command's originally recorded before/after selections.
    #[default]
    Recorded,
    /// On successful Undo, save the pre-Undo current selection for Redo; on
    /// successful Redo, save the pre-Redo selection for the next Undo.
    ///
    /// The captured selection belongs to the opposite traversal's snapshot.
    /// It must not be mapped into the current traversal's target document.
    CaptureOnTraversal,
}

/// Editing-state behavior when the requested history stack is empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum EmptyHistoryBehavior {
    /// Retain historical behavior: clear pending marks and end typing grouping.
    #[default]
    ClearPendingMarks,
    /// Treat an empty traversal as a complete no-op, preserving pending marks,
    /// typing grouping, input-rule state and selection without notifications.
    PreserveEditingState,
}

/// Fixed history traversal options chosen at session construction.
///
/// These do not add time-based grouping, transaction mapping for unrecorded
/// canonical edits, or a history-policy callback during live traversal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryOptions {
    selection: HistorySelectionMode,
    empty: EmptyHistoryBehavior,
}

impl HistoryOptions {
    /// Creates the unchanged default history behavior.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selection: HistorySelectionMode::Recorded,
            empty: EmptyHistoryBehavior::ClearPendingMarks,
        }
    }

    /// Selects recorded selections or capture at each successful traversal.
    #[must_use]
    pub const fn with_selection_mode(mut self, mode: HistorySelectionMode) -> Self {
        self.selection = mode;
        self
    }

    /// Selects the independent behavior of an empty Undo or Redo request.
    #[must_use]
    pub const fn with_empty_behavior(mut self, behavior: EmptyHistoryBehavior) -> Self {
        self.empty = behavior;
        self
    }

    /// Returns the fixed selection restoration mode.
    #[must_use]
    pub const fn selection_mode(self) -> HistorySelectionMode {
        self.selection
    }

    /// Returns the fixed empty-stack editing-state behavior.
    #[must_use]
    pub const fn empty_behavior(self) -> EmptyHistoryBehavior {
        self.empty
    }
}
