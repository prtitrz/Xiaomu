//! Edit plans and their Runtime-owned selection and history contracts.
//!
//! Core transactions describe canonical changes; these types additionally
//! describe how a session publishes the resulting selection and undo unit.

use xiaomu_core::document::{MarkSet, NodeId};
use xiaomu_core::selection::{InlinePoint, NodeGap};
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::Transaction;

use super::DocumentSelection;

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
    /// Install exact endpoints in the final snapshot, after validating them.
    ///
    /// A host converting text bytes to typed inline atoms can compute the
    /// corresponding complete range without collapsing it or guessing a
    /// ChangeMap bias. Invalid endpoints reject the entire candidate before
    /// history or listeners are published. Undo retains the original range.
    Exact {
        /// Selection expressed against the transaction's final document.
        selection: DocumentSelection,
    },
    /// Select the complete root child range of the final snapshot.
    ///
    /// Host whole-document replacement/deletion uses this instead of retaining
    /// stale pre-edit child counts. No text-endpoint inference is performed.
    AllDocument,
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
    /// Explicit whole-block selections preserve the selected identity and
    /// refresh its surrounding gaps against the final snapshot instead.
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
    pub(super) input_rule_undo: Option<Box<super::InputRuleUndoSpec>>,
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
            input_rule_undo: None,
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
