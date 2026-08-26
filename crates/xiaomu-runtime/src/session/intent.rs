//! Typed editing intents and intent-specific selection policies.
//!
//! Intents and selection policies live in the runtime, not in the Core
//! transaction contract: the same Core steps can serve many commands, and
//! only the session knows which after-selection a command promises.

use xiaomu_core::document::{InlineContent, Mark, MarkKind, NodeId, NodeKind};
use xiaomu_core::selection::{TextPoint, TextSelection};
use xiaomu_core::text::{TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::SessionError;

/// One caret movement direction over the paragraph's logical text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaretMove {
    /// Previous Unicode scalar boundary (Left).
    Backward,
    /// Next Unicode scalar boundary (Right).
    Forward,
    /// Logical start of the paragraph (Home).
    ToStart,
    /// Logical end of the paragraph (End).
    ToEnd,
}

/// A typed editing intent.
///
/// Text intents act inside one inline node. Structural intents
/// ([`EditIntent::SplitBlock`], [`EditIntent::JoinWithPrevious`],
/// [`EditIntent::TurnInto`]) still require a single-node text selection in
/// this phase; cross-block structural commands belong to later slices.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditIntent {
    /// Insert `text` at the current selection, replacing a non-collapsed
    /// selection. Empty text over a collapsed caret is a no-op.
    InsertText {
        /// Replacement text; may be empty (deletes the selection).
        text: String,
    },
    /// Delete one Unicode scalar before the caret, or the whole selection.
    ///
    /// A collapsed caret at the start of an inline block joins that block
    /// with its previous sibling when one exists; otherwise it is a no-op.
    Backspace,
    /// Delete one Unicode scalar after the caret, or the whole selection.
    Delete,
    /// Move the caret focus without producing a transaction.
    MoveCaret {
        /// Movement direction over the logical text.
        caret_move: CaretMove,
        /// Keep the anchor and move only the focus (Shift).
        extend_selection: bool,
    },
    /// Place the caret focus at an absolute offset without producing a
    /// transaction (hit-testing, programmatic moves).
    ///
    /// The offset must be a valid boundary of the focused node's inline
    /// text; otherwise the intent fails with a typed error.
    PlaceCaret {
        /// Absolute target offset in the focused node's concatenated text.
        offset: TextOffset,
        /// Keep the anchor and move only the focus (Shift-click, drag).
        extend_selection: bool,
    },
    /// Toggle one mark over the whole selection.
    ///
    /// A collapsed selection is a no-op; P1 has no pending-mark state.
    ToggleMark {
        /// The mark to apply; an existing mark of the same kind over the
        /// whole selection is removed instead.
        mark: Mark,
    },
    /// Split the focused inline block at the caret.
    ///
    /// A non-collapsed selection is deleted first in the same transaction.
    /// The new sibling keeps the original kind and attributes; a split
    /// inside a run gives both halves that run's marks. After commit the
    /// caret sits at the start of the new (tail) block.
    SplitBlock,
    /// Merge the focused inline block into its immediately preceding sibling.
    ///
    /// No previous sibling is a no-op. After commit the caret sits at the
    /// join seam (the end of the surviving block's original text).
    JoinWithPrevious,
    /// Change the focused inline block's kind, keeping its identity and
    /// content.
    ///
    /// The same kind is a no-op. Shape-incompatible kinds (for example
    /// turning a paragraph into a quote container) are rejected by Core.
    ///
    /// List kinds compose with the surrounding structure instead of a plain
    /// kind rewrite: a paragraph becomes a single-item list, a paragraph
    /// inside a list item returns to a plain block (lifting out), and a
    /// bullet list converts to ordered (or back) by rekinding the list
    /// itself.
    TurnInto {
        /// Replacement semantic kind.
        kind: NodeKind,
    },
    /// Indent the focused block's list item under its previous sibling
    /// item, creating the nested list when needed.
    ///
    /// The first item of a list cannot indent; that is a no-op.
    IndentListItem,
    /// Outdent the focused block's nested list item into its enclosing
    /// list, directly after the item that contains the list.
    ///
    /// An item of a top-level list cannot outdent; that is a no-op.
    OutdentListItem,
    /// Place both selection endpoints at absolute text positions.
    ///
    /// This is the document-level form of [`EditIntent::PlaceCaret`]: it can
    /// move the caret or selection across blocks (cross-block navigation,
    /// mouse drag select). Both endpoints are validated against the current
    /// snapshot; an invalid endpoint fails with a typed error and leaves the
    /// session untouched. Produces no transaction.
    SetSelection {
        /// Selection anchor endpoint.
        anchor: TextPoint,
        /// Selection focus endpoint.
        focus: TextPoint,
    },
}

/// How the session derives the selection after a plan commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectionUpdate {
    /// Collapse the caret right after the replacement text of the primary
    /// edit (InsertText / paste / IME commit).
    CaretAfterReplacement,
    /// Collapse the caret at the start of the primary edit
    /// (Backspace / Delete).
    CaretAtEditStart,
    /// Map the previous selection through the change map with outward bias
    /// (mark edits, kind changes, and non-intent applies).
    MapExisting,
    /// After a split: caret at the start of the newly inserted tail sibling.
    CaretAtSplitTail,
    /// After a join: caret at the join seam of the surviving node.
    CaretAtJoinSeam,
    /// Collapse the caret at the start of the primary edit's range.
    ///
    /// Used when text appends at a container tail: the junction sits at the
    /// pre-edit seam, not after the inserted span.
    CaretAtJoinPoint,
    /// The focus endpoint keeps its node and offset; the selection
    /// collapses.
    ///
    /// Used by structural moves that preserve the focused block's identity
    /// (list wrap / lift / indent / outdent). The resolved selection is
    /// validated against the post-command snapshot.
    PreserveFocus,
}

/// The coordinates of the primary text edit of a plan.
///
/// Caret-oriented selection policies resolve against these coordinates in
/// the post-commit snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryEdit {
    pub(crate) node: NodeId,
    pub(crate) range: TextRange,
    pub(crate) inserted_len: usize,
}

impl PrimaryEdit {
    /// Returns the inline node the edit applies to.
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Returns the replaced half-open range in the pre-edit coordinates.
    #[must_use]
    pub const fn range(&self) -> TextRange {
        self.range
    }

    /// Returns the UTF-8 byte length of the inserted text.
    #[must_use]
    pub const fn inserted_len(&self) -> usize {
        self.inserted_len
    }
}

/// A planned edit: the Core transaction plus the runtime selection policy.
///
/// Plans are produced by the session from intents; callers never construct
/// them directly.
#[derive(Clone, Debug)]
pub struct EditPlan {
    transaction: Transaction,
    selection_update: SelectionUpdate,
    primary_edit: Option<PrimaryEdit>,
}

impl EditPlan {
    pub(crate) fn new(
        transaction: Transaction,
        selection_update: SelectionUpdate,
        primary_edit: Option<PrimaryEdit>,
    ) -> Self {
        Self {
            transaction,
            selection_update,
            primary_edit,
        }
    }

    /// Returns the Core transaction to apply.
    #[must_use]
    pub const fn transaction(&self) -> &Transaction {
        &self.transaction
    }

    /// Returns the after-selection policy.
    #[must_use]
    pub const fn selection_update(&self) -> &SelectionUpdate {
        &self.selection_update
    }

    /// Returns the primary text edit when the policy needs its coordinates.
    #[must_use]
    pub const fn primary_edit(&self) -> Option<&PrimaryEdit> {
        self.primary_edit.as_ref()
    }
}

/// What an intent resolves to before anything is committed.
pub(crate) enum PlannedAction {
    /// Commit a plan.
    Commit(EditPlan),
    /// Commit a multi-stage command as one history entry.
    CommitStaged(super::structure::StagedPlan),
    /// The intent is a legitimate no-op.
    NoChange,
}

/// Returns the greatest Unicode scalar boundary strictly before `offset`.
pub(crate) fn previous_boundary(text: &str, offset: usize) -> Option<usize> {
    let mut index = offset;
    while index > 0 {
        index -= 1;
        if text.is_char_boundary(index) {
            return Some(index);
        }
    }
    None
}

/// Returns the smallest Unicode scalar boundary strictly after `offset`.
pub(crate) fn next_boundary(text: &str, offset: usize) -> Option<usize> {
    if offset >= text.len() {
        return None;
    }

    let mut index = offset + 1;
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    Some(index)
}

/// Returns the concatenated text of an inline node.
pub(crate) fn concatenated(inline: &InlineContent) -> String {
    inline
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

/// Builds the plan for inserting text at the current selection.
pub(crate) fn plan_insert_text(
    selection: TextSelection,
    text: &str,
) -> Result<PlannedAction, SessionError> {
    if text.is_empty() && selection.is_collapsed() {
        return Ok(PlannedAction::NoChange);
    }

    let node = selection.focus().node_id();
    let range = ordered_range(selection)?;
    let transaction = edit_transaction(TransactionStep::ReplaceText {
        node,
        range,
        replacement: text.to_owned(),
    });

    Ok(PlannedAction::Commit(EditPlan::new(
        transaction,
        SelectionUpdate::CaretAfterReplacement,
        Some(PrimaryEdit {
            node,
            range,
            inserted_len: text.len(),
        }),
    )))
}

/// Builds the plan for Backspace.
pub(crate) fn plan_backspace(
    inline: &InlineContent,
    selection: TextSelection,
) -> Result<PlannedAction, SessionError> {
    let node = selection.focus().node_id();
    let range = if selection.is_collapsed() {
        let focus = selection.focus().offset().as_usize();
        let text = concatenated(inline);
        match previous_boundary(&text, focus) {
            Some(start) => TextRange::new(inline.offset_at(start)?, selection.focus().offset())
                .map_err(SessionError::Core)?,
            None => return Ok(PlannedAction::NoChange),
        }
    } else {
        ordered_range(selection)?
    };

    Ok(PlannedAction::Commit(deletion_plan(node, range)))
}

/// Builds the plan for forward Delete.
pub(crate) fn plan_delete(
    inline: &InlineContent,
    selection: TextSelection,
) -> Result<PlannedAction, SessionError> {
    let node = selection.focus().node_id();
    let range = if selection.is_collapsed() {
        let focus = selection.focus().offset().as_usize();
        let text = concatenated(inline);
        match next_boundary(&text, focus) {
            Some(end) => TextRange::new(selection.focus().offset(), inline.offset_at(end)?)
                .map_err(SessionError::Core)?,
            None => return Ok(PlannedAction::NoChange),
        }
    } else {
        ordered_range(selection)?
    };

    Ok(PlannedAction::Commit(deletion_plan(node, range)))
}

/// Builds the plan for toggling one mark over the selection.
pub(crate) fn plan_toggle_mark(
    inline: &InlineContent,
    selection: TextSelection,
    mark: &Mark,
) -> Result<PlannedAction, SessionError> {
    if selection.is_collapsed() {
        return Ok(PlannedAction::NoChange);
    }

    let node = selection.focus().node_id();
    let range = ordered_range(selection)?;
    let step = if range_fully_marked(inline, range, mark.kind()) {
        TransactionStep::RemoveMark {
            node,
            range,
            mark_kind: mark.kind(),
        }
    } else {
        TransactionStep::AddMark {
            node,
            range,
            mark: mark.clone(),
        }
    };

    Ok(PlannedAction::Commit(EditPlan::new(
        edit_transaction(step),
        SelectionUpdate::MapExisting,
        None,
    )))
}

/// Wraps a raw transaction with the map-existing selection policy.
pub(crate) fn map_existing_plan(transaction: Transaction) -> EditPlan {
    EditPlan::new(transaction, SelectionUpdate::MapExisting, None)
}

fn ordered_range(selection: TextSelection) -> Result<TextRange, SessionError> {
    selection
        .ordered_range()
        .map_err(|_| SessionError::SelectionInvalid)
}

fn edit_transaction(step: TransactionStep) -> Transaction {
    Transaction::new(TransactionOrigin::UserInput).with_step(step)
}

fn deletion_plan(node: NodeId, range: TextRange) -> EditPlan {
    EditPlan::new(
        edit_transaction(TransactionStep::ReplaceText {
            node,
            range,
            replacement: String::new(),
        }),
        SelectionUpdate::CaretAtEditStart,
        Some(PrimaryEdit {
            node,
            range,
            inserted_len: 0,
        }),
    )
}

fn range_fully_marked(inline: &InlineContent, range: TextRange, kind: MarkKind) -> bool {
    let start = range.start().as_usize();
    let end = range.end().as_usize();

    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;

        let overlap_start = start.max(run_start);
        let overlap_end = end.min(run_end);
        if overlap_start < overlap_end && !run.marks().contains(kind) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_walk_unicode_scalars() {
        // "a👍中": a=0..1, 👍=1..5, 中=5..8.
        let text = "a👍中";

        assert_eq!(previous_boundary(text, 0), None);
        assert_eq!(previous_boundary(text, 1), Some(0));
        assert_eq!(previous_boundary(text, 5), Some(1));
        assert_eq!(previous_boundary(text, 8), Some(5));

        assert_eq!(next_boundary(text, 0), Some(1));
        assert_eq!(next_boundary(text, 1), Some(5));
        assert_eq!(next_boundary(text, 5), Some(8));
        assert_eq!(next_boundary(text, 8), None);
    }
}
