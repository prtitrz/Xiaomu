//! Typed editing intents and intent-specific selection policies.
//!
//! Intents and selection policies live in the runtime, not in the Core
//! transaction contract: the same Core steps can serve many commands, and
//! only the session knows which after-selection a command promises.

use xiaomu_core::document::{ImageAttrs, InlineContent, Mark, MarkKind, MarkSet, NodeId, NodeKind};
use xiaomu_core::selection::{InlinePoint, NodeGap, TextPoint, TextSelection};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use crate::clipboard::ClipboardSlice;

use super::SessionError;

pub(super) const MARK_KINDS: [MarkKind; 7] = [
    MarkKind::Bold,
    MarkKind::Italic,
    MarkKind::Code,
    MarkKind::Underline,
    MarkKind::Strike,
    MarkKind::Link,
    MarkKind::TextStyle,
];

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
/// Text intents act inside one inline node unless their contract explicitly
/// carries a document-level fragment. Structural intents
/// ([`EditIntent::SplitBlock`], [`EditIntent::JoinWithPrevious`],
/// [`EditIntent::TurnInto`]) still require a single-node text selection in
/// this phase.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditIntent {
    /// Insert `text` at the current selection, replacing a non-collapsed
    /// selection. Adjacent collapsed insertions are eligible for Runtime
    /// typing-history coalescing. Empty text over a collapsed caret is a no-op.
    InsertText {
        /// Replacement text; may be empty (deletes the selection).
        text: String,
    },
    /// Insert one logical inline line break as an isolated edit.
    ///
    /// The host-neutral default inserts canonical LF, retaining the existing
    /// stored-mark and history behavior. A product policy can distinguish this
    /// keyboard command from pasting a clipboard string containing LF.
    InsertLineBreak,
    /// Inserts one image atomic block after the focused block (P4.7).
    ///
    /// The payload is the typed canonical image semantics; pixels and host
    /// file objects never travel through the document.
    InsertImage {
        /// Typed canonical image attrs (source, alt, optional metadata).
        image: ImageAttrs,
    },
    /// Inserts a `rows × columns` table after the focused block (P5.1).
    ///
    /// Core allocates the whole subtree (rows, cells, one empty paragraph
    /// per cell) as one semantic step; the command is one isolated history
    /// entry and the caret stays where it was.
    InsertTable {
        /// Number of rows; must be at least one.
        rows: usize,
        /// Number of columns; must be at least one.
        columns: usize,
    },
    /// Inserts one row into a table at `index` (P5.3).
    ///
    /// The new row mirrors the table's column count with one empty
    /// paragraph per cell. `index` may be the current row count to append.
    /// One isolated history entry; the caret maps through unchanged.
    InsertTableRow {
        /// The target table.
        table: NodeId,
        /// Number of existing rows before the insertion point.
        index: usize,
    },
    /// Inserts one column into a table at `index` (P5.3).
    ///
    /// Every row gains one cell carrying one empty paragraph. One isolated
    /// history entry; carets in existing cells map through unchanged.
    InsertTableColumn {
        /// The target table.
        table: NodeId,
        /// Number of existing cells (columns) before the insertion point
        /// in every row.
        index: usize,
    },
    /// Deletes one row of a table by `index` (P5.3).
    ///
    /// Deleting the last row fails closed. A caret inside the deleted row
    /// converges to the structural seam where the row was; other selections
    /// map through unchanged.
    DeleteTableRow {
        /// The target table.
        table: NodeId,
        /// The row to delete.
        index: usize,
    },
    /// Deletes one column of a table by `index` (P5.3).
    ///
    /// Every row loses its cell at `index` in one transaction. Deleting the
    /// last column fails closed. A caret inside a deleted cell converges to
    /// that row's structural seam; other selections map through unchanged.
    DeleteTableColumn {
        /// The target table.
        table: NodeId,
        /// The column to delete.
        index: usize,
    },
    /// Commit one native IME composition over an explicit canonical range.
    /// A collapsed range at the current caret preserves its mixed-inline
    /// atom ordinal. Nonempty ranges keep boundary atoms and reject atoms
    /// strictly inside the replaced text.
    ///
    /// Composition updates remain frontend-transient. The final committed
    /// text uses the same StoredMarks semantics as normal typing but owns one
    /// isolated history entry rather than joining an adjacent typing group.
    CommitComposition {
        /// Replacement range in the focused inline node.
        range: TextRange,
        /// Final composition text.
        text: String,
    },
    /// Paste unstructured platform text over the current selection.
    ///
    /// Plain text inherits the current typing marks but always owns an
    /// isolated history entry, so paste never coalesces with adjacent typing.
    PasteText {
        /// Normalized platform text.
        text: String,
    },
    /// Paste a detached Xiaomu structured clipboard fragment.
    ///
    /// Marks and multi-block boundaries are preserved. A cross-block target
    /// selection is replaced atomically as part of the same history entry.
    PasteSlice {
        /// Structured clipboard value to insert.
        slice: ClipboardSlice,
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
    /// Move the caret to the next table cell in reading order (P5.2).
    ///
    /// Tab semantics: the next sibling cell, the first cell of the next
    /// row, or — from the table's last cell — one appended trailing row
    /// entered at its first cell. Outside a table this is a no-op.
    MoveToNextCell,
    /// Move the caret to the previous table cell in reading order (P5.2).
    ///
    /// Shift+Tab semantics: the previous sibling cell or the last cell of
    /// the previous row. From the table's first cell, and outside a table,
    /// this is a no-op.
    MoveToPreviousCell,
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
    /// Toggle one mark over the selection or at a collapsed caret.
    ///
    /// Non-collapsed selections change canonical marks. At a collapsed caret
    /// the session updates Runtime StoredMarks without advancing the document
    /// revision; later typing/IME commit uses that explicit mark set.
    ToggleMark {
        /// The mark to apply; an existing mark of the same kind over the
        /// whole selection is removed instead.
        mark: Mark,
    },
    /// Set one exact mark, replacing any mark of the same semantic kind.
    ///
    /// A non-collapsed single-node text selection changes canonical marks in
    /// one undo unit. A collapsed inline caret updates Runtime StoredMarks
    /// using the explicit marks or surrounding-run inheritance. Other mark
    /// kinds are preserved, including when setting inline code. Setting the
    /// already-effective value is a no-op, preserving typing history grouping.
    SetMark {
        /// Exact mark value, including all attributes for an attributed mark.
        mark: Mark,
    },
    /// Remove one mark kind from the selection or pending typing marks.
    ///
    /// Uses the same selection and history contract as [`EditIntent::SetMark`].
    /// At a collapsed caret, removing the last effective mark leaves explicit
    /// empty StoredMarks so later typing does not re-inherit that mark.
    /// An already-absent kind is a no-op, preserving typing history grouping.
    RemoveMark {
        /// Semantic mark kind to remove, irrespective of its attributes.
        kind: MarkKind,
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

/// How one plan participates in Runtime history grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryPolicy {
    /// Always record an independent undo unit.
    Isolated,
    /// Allow adjacency-based coalescing with the currently open typing group.
    Typing,
}

/// How the session derives the selection after a plan commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectionUpdate {
    /// Collapse the caret right after the replacement text of the primary
    /// edit (InsertText / single-block paste / IME commit).
    CaretAfterReplacement,
    /// Collapse inside the last node inserted by a transaction at `offset`.
    ///
    /// Multi-block structured paste uses this to place the caret after the
    /// pasted portion but before the target block's relocated suffix.
    CaretAtLastInsertedOffset {
        /// UTF-8 byte offset in the last inserted inline-bearing node.
        offset: usize,
    },
    /// Enter the first editable descendant of the last inserted subtree.
    /// Used when Tab appends a table row; mapping still names the actual row.
    CaretAtStartOfLastInsertedSubtree,
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
    /// Collapse the caret onto an exact mixed-inline gap in the
    /// post-command snapshot.
    ///
    /// Atom edits know their resulting caret gap (for example the gap an
    /// atomic Backspace leaves behind); the point is validated against the
    /// post-command snapshot like any other stale coordinate.
    CaretAtInline {
        /// The exact post-edit caret gap.
        caret: InlinePoint,
    },
    /// The focus endpoint keeps its node and offset; the selection
    /// collapses.
    ///
    /// Used by structural moves that preserve the focused block's identity
    /// (list wrap / lift / indent / outdent). The resolved selection is
    /// validated against the post-command snapshot.
    PreserveFocus,
    /// Keep the complete selection exactly as it was before the command.
    ///
    /// Both endpoints, their direction, affinity and mixed-inline ordinals,
    /// and any active cell range are preserved without mapping or collapse.
    /// The complete selection must validate against the final document;
    /// otherwise the command fails atomically. This supports identity-
    /// preserving edits whose intermediate steps temporarily remove nodes.
    PreserveSelection,
    /// Collapse onto an exact structural gap in the post-command snapshot.
    ///
    /// Atomic node removal leaves no inline caret behind; the selection
    /// converges to the boundary the removed block occupied.
    CaretAtGap {
        /// The exact post-edit structural gap.
        gap: NodeGap,
    },
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
    /// Describes a text replacement for a plan's caret-resolution policy.
    ///
    /// `range` uses pre-edit UTF-8 byte coordinates; `inserted_len` is the
    /// replacement's byte length. The resolved post-edit point is validated
    /// when the session commits the plan.
    #[must_use]
    pub const fn new(node: NodeId, range: TextRange, inserted_len: usize) -> Self {
        Self {
            node,
            range,
            inserted_len,
        }
    }

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

/// A planned edit: the Core transaction plus Runtime selection/history policy.
///
/// Plans are produced by the default planner or a host's [`super::SessionPolicy`].
/// Host-constructed plans always form one isolated undo unit.
#[derive(Clone, Debug)]
pub struct EditPlan {
    transaction: Transaction,
    selection_update: SelectionUpdate,
    primary_edit: Option<PrimaryEdit>,
    history_policy: HistoryPolicy,
    stored_marks_after: Option<Option<MarkSet>>,
}

impl EditPlan {
    /// Creates a single-transaction plan with isolated undo semantics.
    ///
    /// Caret policies referring to a primary text replacement require
    /// `primary_edit`. Core, selection and host policy validation happen
    /// atomically on commit, not when constructing this description.
    #[must_use]
    pub fn new(
        transaction: Transaction,
        selection_update: SelectionUpdate,
        primary_edit: Option<PrimaryEdit>,
    ) -> Self {
        Self {
            transaction,
            selection_update,
            primary_edit,
            history_policy: HistoryPolicy::Isolated,
            stored_marks_after: None,
        }
    }

    /// Installs explicit typing marks together with a successful commit.
    ///
    /// The resolved selection must be a collapsed inline caret; otherwise
    /// the whole plan fails before publication. `None` restores inheritance,
    /// while `Some(empty)` requests unmarked typing. Without this option,
    /// the calling intent's existing stored-mark behavior is unchanged.
    #[must_use]
    pub fn with_stored_marks(mut self, marks: Option<MarkSet>) -> Self {
        self.stored_marks_after = Some(marks);
        self
    }

    pub(crate) fn stored_marks_after(&self) -> Option<&Option<MarkSet>> {
        self.stored_marks_after.as_ref()
    }

    pub(crate) fn with_history_policy(mut self, history_policy: HistoryPolicy) -> Self {
        self.history_policy = history_policy;
        self
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

    pub(crate) const fn history_policy(&self) -> HistoryPolicy {
        self.history_policy
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

/// Builds a text replacement plan using optional explicit StoredMarks.
pub(crate) fn plan_insert_text(
    inline: &InlineContent,
    selection: TextSelection,
    text: &str,
    stored_marks: Option<&MarkSet>,
    requested_history: HistoryPolicy,
) -> Result<PlannedAction, SessionError> {
    if text.is_empty() && selection.is_collapsed() {
        return Ok(PlannedAction::NoChange);
    }

    let node = selection.focus().node_id();
    let range = ordered_range(selection)?;
    let mut transaction = edit_transaction(TransactionStep::ReplaceText {
        node,
        range,
        replacement: text.to_owned(),
    });
    if let Some(marks) = stored_marks
        && !text.is_empty()
    {
        push_exact_insert_marks(&mut transaction, inline, node, range, text, marks)?;
    }

    let history_policy = if requested_history == HistoryPolicy::Typing
        && selection.is_collapsed()
        && !text.is_empty()
    {
        HistoryPolicy::Typing
    } else {
        HistoryPolicy::Isolated
    };

    Ok(PlannedAction::Commit(
        EditPlan::new(
            transaction,
            SelectionUpdate::CaretAfterReplacement,
            Some(PrimaryEdit {
                node,
                range,
                inserted_len: text.len(),
            }),
        )
        .with_history_policy(history_policy),
    ))
}

/// Wraps a raw transaction with the map-existing selection policy.
pub(crate) fn map_existing_plan(transaction: Transaction) -> EditPlan {
    EditPlan::new(transaction, SelectionUpdate::MapExisting, None)
}

pub(crate) fn push_exact_insert_marks(
    transaction: &mut Transaction,
    inline: &InlineContent,
    node: NodeId,
    replaced: TextRange,
    inserted_text: &str,
    marks: &MarkSet,
) -> Result<(), SessionError> {
    let source = concatenated(inline);
    let start = replaced.start().as_usize();
    let end = replaced.end().as_usize();
    let post_text = format!("{}{}{}", &source[..start], inserted_text, &source[end..]);
    let buffer = TextBuffer::from_string(post_text);
    let inserted_range = buffer
        .range(
            buffer.offset_at(start).map_err(SessionError::Core)?,
            buffer
                .offset_at(start + inserted_text.len())
                .map_err(SessionError::Core)?,
        )
        .map_err(SessionError::Core)?;

    for kind in MARK_KINDS {
        transaction.push_step(TransactionStep::RemoveMark {
            node,
            range: inserted_range,
            mark_kind: kind,
        });
    }
    for mark in marks.as_slice() {
        transaction.push_step(TransactionStep::AddMark {
            node,
            range: inserted_range,
            mark: mark.clone(),
        });
    }
    Ok(())
}

pub(crate) fn ordered_range(selection: TextSelection) -> Result<TextRange, SessionError> {
    selection
        .ordered_range()
        .map_err(|_| SessionError::SelectionInvalid)
}

pub(crate) fn edit_transaction(step: TransactionStep) -> Transaction {
    Transaction::new(TransactionOrigin::UserInput).with_step(step)
}

pub(crate) fn deletion_plan(node: NodeId, range: TextRange) -> EditPlan {
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
