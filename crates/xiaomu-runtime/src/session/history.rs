//! Runtime undo/redo stack with explicit history grouping.

use super::selection::DocumentSelection;
use xiaomu_core::document::NodeId;
use xiaomu_core::transaction::{Transaction, TransactionOrigin};

/// Runtime policy attached to one recorded history entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryGroup {
    /// A command that always owns its own undo unit.
    Isolated,
    /// Consecutive plain typing inside one node may extend this undo unit.
    Typing {
        node: NodeId,
        start: usize,
        end: usize,
    },
}

impl HistoryGroup {
    fn merge(self, next: Self) -> Option<Self> {
        match (self, next) {
            (
                Self::Typing {
                    node,
                    start,
                    end,
                },
                Self::Typing {
                    node: next_node,
                    start: next_start,
                    end: next_end,
                },
            ) if node == next_node && end == next_start => Some(Self::Typing {
                node,
                start,
                end: next_end,
            }),
            _ => None,
        }
    }

    const fn is_typing(self) -> bool {
        matches!(self, Self::Typing { .. })
    }
}

/// One undo unit: the redo/undo transaction pair plus the selections
/// recorded in both snapshot coordinate spaces.
///
/// `undo` is the exact inverse of the committed transaction or grouped
/// transactions. `redo` replays the grouped changes in original order so
/// allocated identities are restored rather than minted again.
pub(crate) struct HistoryEntry {
    pub(crate) redo: Transaction,
    pub(crate) undo: Transaction,
    pub(crate) before_selection: DocumentSelection,
    pub(crate) after_selection: DocumentSelection,
    pub(crate) group: HistoryGroup,
}

/// Undo/redo stacks for one session.
///
/// P3.4 keeps grouping explicit: only adjacent `Typing` entries can coalesce,
/// and only while the current typing group remains open. Selection movement,
/// formatting commands, structural edits, paste/cut, undo/redo, and explicit
/// session boundaries close that group. No clock or hidden timeout determines
/// canonical history semantics.
pub struct HistoryStack {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    typing_group_open: bool,
}

impl HistoryStack {
    /// Creates empty stacks.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            typing_group_open: false,
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

    /// Ends the currently open typing group without changing either stack.
    pub(crate) fn break_group(&mut self) {
        self.typing_group_open = false;
    }

    /// Records a committed edit and clears the redo stack.
    pub(crate) fn record(&mut self, entry: HistoryEntry) {
        self.redo.clear();

        if self.typing_group_open
            && entry.group.is_typing()
            && let Some(previous) = self.undo.pop()
        {
            if let Some(merged_group) = previous.group.merge(entry.group)
                && previous.after_selection == entry.before_selection
            {
                self.undo
                    .push(merge_entries(previous, entry, merged_group));
                self.typing_group_open = true;
                return;
            }
            self.undo.push(previous);
        }

        self.typing_group_open = entry.group.is_typing();
        self.undo.push(entry);
    }

    /// Takes the newest undo entry.
    pub(crate) fn take_undo(&mut self) -> Option<HistoryEntry> {
        self.break_group();
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
        self.break_group();
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

fn merge_entries(
    previous: HistoryEntry,
    next: HistoryEntry,
    group: HistoryGroup,
) -> HistoryEntry {
    let mut redo = Transaction::new(TransactionOrigin::System);
    for step in previous.redo.steps() {
        redo.push_step(step.clone());
    }
    for step in next.redo.steps() {
        redo.push_step(step.clone());
    }

    let mut undo = Transaction::new(TransactionOrigin::System);
    for step in next.undo.steps() {
        undo.push_step(step.clone());
    }
    for step in previous.undo.steps() {
        undo.push_step(step.clone());
    }

    HistoryEntry {
        redo,
        undo,
        before_selection: previous.before_selection,
        after_selection: next.after_selection,
        group,
    }
}

impl Default for HistoryStack {
    fn default() -> Self {
        Self::new()
    }
}
