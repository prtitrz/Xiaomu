//! Immutable per-session choices for history traversal and explicit typing grouping.

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

/// Grouping behavior for ordinary successful selection installation.
///
/// Atomic target replacement and existing CellRange convergence paths retain
/// their separate barriers, even when a later navigation action is a no-op.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SelectionOnlyGrouping {
    /// End the current typing group, retaining historical behavior.
    #[default]
    Close,
    /// Keep the current group and its last edit time. The next edit must still
    /// satisfy exact selection continuity and native typing eligibility.
    /// Pending marks and input-rule tokens retain their existing semantics.
    Preserve,
}

/// Fixed history options chosen once at session construction.
///
/// Timed grouping and selection-only preservation are independent opt-ins.
/// No ambient clock, transaction mapping for unrecorded canonical edits, or
/// live history-policy callback is introduced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryOptions {
    selection: HistorySelectionMode,
    empty: EmptyHistoryBehavior,
    typing_group_delay_ms: Option<u64>,
    selection_only_grouping: SelectionOnlyGrouping,
}

impl HistoryOptions {
    /// Creates the unchanged default history behavior.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selection: HistorySelectionMode::Recorded,
            empty: EmptyHistoryBehavior::ClearPendingMarks,
            typing_group_delay_ms: None,
            selection_only_grouping: SelectionOnlyGrouping::Close,
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

    /// Enables a sliding typing timeout in explicit session-clock milliseconds.
    ///
    /// Only eligible typing edits participate. The inclusive delay is measured
    /// from the previous successful typing edit, never from selection movement.
    /// Missing or regressing timestamps commit normally as separate undo units
    /// and close the anchor; they do not lower the accepted clock high-water mark.
    #[must_use]
    pub const fn with_typing_group_delay_ms(mut self, delay_ms: u64) -> Self {
        self.typing_group_delay_ms = Some(delay_ms);
        self
    }

    /// Selects whether ordinary direct selection installation closes the group.
    /// Atomic target replacement and CellRange convergence keep their boundaries.
    #[must_use]
    pub const fn with_selection_only_grouping(mut self, grouping: SelectionOnlyGrouping) -> Self {
        self.selection_only_grouping = grouping;
        self
    }

    /// Returns the typing delay, or `None` for unchanged timeless grouping.
    #[must_use]
    pub const fn typing_group_delay_ms(self) -> Option<u64> {
        self.typing_group_delay_ms
    }

    /// Returns the independent selection-only grouping behavior.
    #[must_use]
    pub const fn selection_only_grouping(self) -> SelectionOnlyGrouping {
        self.selection_only_grouping
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
