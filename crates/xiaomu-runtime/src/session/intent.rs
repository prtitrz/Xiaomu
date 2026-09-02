//! Typed editing intents and intent-specific selection policies.
//!
//! Intents and selection policies live in the runtime, not in the Core
//! transaction contract: the same Core steps can serve many commands, and
//! only the session knows which after-selection a command promises.

use xiaomu_core::document::{InlineContent, Mark, MarkKind, MarkSet, NodeId, NodeKind};
use xiaomu_core::selection::{InlinePoint, TextPoint, TextSelection};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use crate::clipboard::ClipboardSlice;

use super::SessionError;

const MARK_KINDS: [MarkKind; 6] = [
    MarkKind::Bold,
    MarkKind::Italic,
    MarkKind::Code,
    MarkKind::Underline,
    MarkKind::Strike,
    MarkKind::Link,
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
    /// Commit one native IME composition over an explicit canonical range.
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

/// A planned edit: the Core transaction plus Runtime selection/history policy.
///
/// Plans are produced by the session from intents; callers never construct
/// them directly.
#[derive(Clone, Debug)]
pub struct EditPlan {
    transaction: Transaction,
    selection_update: SelectionUpdate,
    primary_edit: Option<PrimaryEdit>,
    history_policy: HistoryPolicy,
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
            history_policy: HistoryPolicy::Isolated,
        }
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

/// Builds the plan for Backspace.
///
/// Plain-text nodes keep the exact P0-P3 `ReplaceText` contract. Nodes with
/// inline atoms delete one caret unit atomically (ADR 0005): the unit before
/// the caret is either an atom (removed by identity) or one text scalar
/// (deleted without touching the seam atoms before it). A non-collapsed
/// selection deletes the whole region including any atoms inside it.
pub(crate) fn plan_backspace(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    if inline.atoms().is_empty() {
        return plan_backspace_text_only(inline, anchor, focus);
    }
    if !selection_is_collapsed(anchor, focus) {
        return plan_inline_replacement(inline, anchor, focus, "", None, HistoryPolicy::Isolated);
    }

    let node = focus.node_id();
    let affinity = focus.affinity();
    if focus.atom_index() > 0 {
        // The unit before the caret is the atom at the previous ordinal.
        let atom = atom_at_ordinal(inline, focus.text_offset(), focus.atom_index() - 1)
            .ok_or(SessionError::SelectionInvalid)?;
        let caret = InlinePoint::new(node, focus.text_offset(), focus.atom_index() - 1, affinity);
        let transaction = edit_transaction(TransactionStep::RemoveInlineAtom { atom });
        return Ok(PlannedAction::Commit(
            EditPlan::new(transaction, SelectionUpdate::CaretAtInline { caret }, None)
                .with_history_policy(HistoryPolicy::Isolated),
        ));
    }

    // Delete the previous text scalar; seam atoms at its start boundary sit
    // before the caret and survive.
    let text = concatenated(inline);
    let Some(start) = previous_boundary(&text, focus.text_offset().as_usize()) else {
        return Ok(PlannedAction::NoChange);
    };
    let start_offset = inline.offset_at(start).map_err(SessionError::Core)?;
    let start_ordinal = inline.atom_count_at(start_offset);
    plan_seam_deletion(
        node,
        InlinePoint::new(node, start_offset, start_ordinal, affinity),
        focus.text_offset(),
    )
}

/// Builds the plan for forward Delete.
///
/// Mirrors [`plan_backspace`]: the unit after the caret is either an atom or
/// one text scalar.
pub(crate) fn plan_delete(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    if inline.atoms().is_empty() {
        return plan_delete_text_only(inline, anchor, focus);
    }
    if !selection_is_collapsed(anchor, focus) {
        return plan_inline_replacement(inline, anchor, focus, "", None, HistoryPolicy::Isolated);
    }

    let node = focus.node_id();
    let affinity = focus.affinity();
    let count = inline.atom_count_at(focus.text_offset());
    if focus.atom_index() < count {
        // The unit after the caret is the atom at the caret's own ordinal.
        let atom = atom_at_ordinal(inline, focus.text_offset(), focus.atom_index())
            .ok_or(SessionError::SelectionInvalid)?;
        let caret = InlinePoint::new(node, focus.text_offset(), focus.atom_index(), affinity);
        let transaction = edit_transaction(TransactionStep::RemoveInlineAtom { atom });
        return Ok(PlannedAction::Commit(
            EditPlan::new(transaction, SelectionUpdate::CaretAtInline { caret }, None)
                .with_history_policy(HistoryPolicy::Isolated),
        ));
    }

    // Delete the next text scalar; the caret stays after the seam atoms.
    let text = concatenated(inline);
    let Some(end) = next_boundary(&text, focus.text_offset().as_usize()) else {
        return Ok(PlannedAction::NoChange);
    };
    let end_offset = inline.offset_at(end).map_err(SessionError::Core)?;
    plan_seam_deletion(
        node,
        InlinePoint::new(node, focus.text_offset(), count, affinity),
        end_offset,
    )
}

/// Plans the deletion of the text span from a seam gap to `end`.
fn plan_seam_deletion(
    node: NodeId,
    at: InlinePoint,
    end: TextOffset,
) -> Result<PlannedAction, SessionError> {
    let transaction = edit_transaction(TransactionStep::ReplaceInlineText {
        at,
        end,
        replacement: String::new(),
    });
    Ok(PlannedAction::Commit(
        EditPlan::new(
            transaction,
            SelectionUpdate::CaretAtInline { caret: at },
            Some(PrimaryEdit {
                node,
                range: TextRange::new(at.text_offset(), end).map_err(SessionError::Core)?,
                inserted_len: 0,
            }),
        )
        .with_history_policy(HistoryPolicy::Isolated),
    ))
}

/// Builds the InsertText plan at a mixed-inline caret gap.
///
/// Plain-text nodes keep the exact P0-P3 `ReplaceText` contract; nodes with
/// inline atoms insert through the atom-aware replacement so the caret's
/// seam ordinal decides whether typed text lands before or after the
/// same-boundary atoms. A selection spanning atoms deletes them explicitly
/// in the same transaction.
pub(crate) fn plan_text_input(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
    text: &str,
    stored_marks: Option<&MarkSet>,
    requested_history: HistoryPolicy,
) -> Result<PlannedAction, SessionError> {
    if inline.atoms().is_empty() {
        let selection = text_selection_from(anchor, focus)?;
        return plan_insert_text(inline, selection, text, stored_marks, requested_history);
    }
    plan_inline_replacement(inline, anchor, focus, text, stored_marks, requested_history)
}

/// Replaces the span between two mixed-inline endpoints of one node.
///
/// The selection's atoms (same-boundary atoms at or after the start gap, and
/// end-boundary atoms before the end gap) are removed by identity before the
/// atom-aware replacement, whose seam ordinal already excludes them. A
/// collapsed endpoint pair is a pure seam insertion and removes nothing.
fn plan_inline_replacement(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
    text: &str,
    stored_marks: Option<&MarkSet>,
    requested_history: HistoryPolicy,
) -> Result<PlannedAction, SessionError> {
    let node = focus.node_id();
    let collapsed = selection_is_collapsed(anchor, focus);
    let (start, end) = match anchor {
        Some(anchor) if !collapsed => {
            let a = (anchor.text_offset().as_usize(), anchor.atom_index());
            let f = (focus.text_offset().as_usize(), focus.atom_index());
            if a <= f {
                (anchor, focus)
            } else {
                (focus, anchor)
            }
        }
        _ => (focus, focus),
    };

    let mut transaction = Transaction::new(TransactionOrigin::UserInput);
    if !collapsed {
        // Atoms inside the selection are deleted with the region; they can
        // never be silently consumed by a text edit. When both endpoints
        // share one boundary the selection covers a half-open ordinal span.
        let start_raw = start.text_offset().as_usize();
        let end_raw = end.text_offset().as_usize();
        for placement in inline.atoms() {
            let offset = placement.text_offset().as_usize();
            let ordinal = same_boundary_ordinal(inline, placement.text_offset(), placement.atom());
            let inside = if start_raw == end_raw {
                ordinal >= start.atom_index() && ordinal < end.atom_index()
            } else {
                offset == start_raw && ordinal >= start.atom_index()
                    || (offset > start_raw && offset < end_raw)
                    || (offset == end_raw && ordinal < end.atom_index())
            };
            if inside {
                transaction.push_step(TransactionStep::RemoveInlineAtom {
                    atom: placement.atom(),
                });
            }
        }
    }
    transaction.push_step(TransactionStep::ReplaceInlineText {
        at: start,
        end: end.text_offset(),
        replacement: text.to_owned(),
    });
    if let Some(marks) = stored_marks
        && !text.is_empty()
    {
        let replaced =
            TextRange::new(start.text_offset(), end.text_offset()).map_err(SessionError::Core)?;
        push_exact_insert_marks(&mut transaction, inline, node, replaced, text, marks)?;
    }

    let selection_update = if !text.is_empty() {
        // The caret lands after the replacement text, before the seam atoms
        // that moved behind it.
        SelectionUpdate::CaretAfterReplacement
    } else {
        SelectionUpdate::CaretAtInline { caret: start }
    };
    let history_policy =
        if requested_history == HistoryPolicy::Typing && collapsed && !text.is_empty() {
            HistoryPolicy::Typing
        } else {
            HistoryPolicy::Isolated
        };

    Ok(PlannedAction::Commit(
        EditPlan::new(
            transaction,
            selection_update,
            Some(PrimaryEdit {
                node,
                range: TextRange::new(start.text_offset(), end.text_offset())
                    .map_err(SessionError::Core)?,
                inserted_len: text.len(),
            }),
        )
        .with_history_policy(history_policy),
    ))
}

/// Returns the same-boundary ordinal of one atom placement.
fn same_boundary_ordinal(
    inline: &InlineContent,
    offset: xiaomu_core::text::TextOffset,
    atom: NodeId,
) -> usize {
    inline
        .atoms()
        .iter()
        .take_while(|placement| placement.text_offset() <= offset)
        .filter(|placement| placement.text_offset() == offset)
        .take_while(|placement| placement.atom() != atom)
        .count()
}

/// Returns the atom identity at one same-boundary ordinal.
fn atom_at_ordinal(
    inline: &InlineContent,
    offset: xiaomu_core::text::TextOffset,
    ordinal: usize,
) -> Option<NodeId> {
    inline
        .atoms()
        .iter()
        .filter(|placement| placement.text_offset() == offset)
        .nth(ordinal)
        .map(|placement| placement.atom())
}

/// Returns whether an anchor/focus pair denotes a collapsed caret.
fn selection_is_collapsed(anchor: Option<InlinePoint>, focus: InlinePoint) -> bool {
    match anchor {
        Some(anchor) => anchor == focus,
        None => true,
    }
}

/// Rebuilds a text-only Core selection from mixed-inline endpoints.
pub(crate) fn text_selection_from(
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<TextSelection, SessionError> {
    let to_point = |point: InlinePoint| {
        point
            .to_text_point()
            .map_err(|_| SessionError::SelectionInvalid)
    };
    let focus = to_point(focus)?;
    Ok(match anchor {
        Some(anchor) => TextSelection::new(to_point(anchor)?, focus),
        None => TextSelection::new(focus, focus),
    })
}

/// Builds the plan for Backspace over a plain-text node.
fn plan_backspace_text_only(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    let selection = text_selection_from(anchor, focus)?;
    plan_backspace_legacy(inline, selection)
}

/// Builds the plan for forward Delete over a plain-text node.
fn plan_delete_text_only(
    inline: &InlineContent,
    anchor: Option<InlinePoint>,
    focus: InlinePoint,
) -> Result<PlannedAction, SessionError> {
    let selection = text_selection_from(anchor, focus)?;
    plan_delete_legacy(inline, selection)
}

/// Builds the plan for Backspace.
fn plan_backspace_legacy(
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
fn plan_delete_legacy(
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

/// Builds the plan for toggling one mark over a non-collapsed selection.
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

fn push_exact_insert_marks(
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
