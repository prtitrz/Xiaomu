//! Optional, construction-time host rules around the generic edit pipeline.

use std::fmt;

use crate::clipboard::{ClipboardExportPurpose, ClipboardExportSpec};
use xiaomu_core::document::{MarkSet, XiaomuDocument};

use super::{
    DocumentSelection, DocumentSession, EditIntent, EditPlan, SessionError, SessionOutcome,
};

/// A host-defined reason for refusing an intent or document snapshot.
///
/// The message is diagnostic, not a stable machine-readable error code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyError {
    message: String,
}

impl PolicyError {
    /// Creates a rejection with a host-provided diagnostic message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the host-provided diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PolicyError {}

/// Read-only editing state supplied before any intent changes session state.
/// For an atomic selection-plus-intent operation, this describes the proposed
/// target selection and the typing marks that target would inherit.
#[derive(Clone, Copy)]
pub struct SessionContext<'a> {
    document: &'a XiaomuDocument,
    selection: DocumentSelection,
    stored_marks: Option<&'a MarkSet>,
    input_rule_undo_available: bool,
}

impl<'a> SessionContext<'a> {
    /// Whether one exact rule reversal is eligible at this target selection.
    ///
    /// A different atomic target never inherits the current caret's token.
    #[must_use]
    pub const fn input_rule_undo_available(self) -> bool {
        self.input_rule_undo_available
    }

    /// Returns the current, policy-validated canonical snapshot.
    #[must_use]
    pub const fn document(self) -> &'a XiaomuDocument {
        self.document
    }

    /// Returns the intent's validated target selection.
    #[must_use]
    pub const fn selection(self) -> DocumentSelection {
        self.selection
    }

    /// Returns explicit typing marks; `None` means surrounding-run inheritance.
    #[must_use]
    pub const fn stored_marks(self) -> Option<&'a MarkSet> {
        self.stored_marks
    }

    /// Returns the marks typing would use at a collapsed inline caret.
    ///
    /// Explicit stored marks take precedence, including `Some(empty)`.
    /// Otherwise the exact mixed-inline gap selects the left child, or the
    /// right child at paragraph start, including independent atom marks. Returns
    /// `None` for a range, cell range, atomic selection or structural gap.
    #[must_use]
    pub fn effective_typing_marks(self) -> Option<MarkSet> {
        if !self.selection.is_collapsed() {
            return None;
        }
        let (_, focus) = self.selection.as_same_node_inline()?;
        match self.stored_marks {
            Some(marks) => Some(marks.clone()),
            None => self.document.inherited_inline_marks(focus).ok(),
        }
    }
}

/// A policy's decision before the default intent planner runs.
#[derive(Clone, Debug)]
// Exact final selections make EditPlan relatively large. Keep the established Apply
// API and stack-owned one-shot plans rather than allocating every edit merely
// to reduce the size of the two empty decision variants.
#[allow(clippy::large_enum_variant)]
pub enum IntentDisposition {
    /// Use the existing host-neutral intent behavior.
    Continue,
    /// Do nothing, including leaving stored marks and typing grouping intact.
    NoChange,
    /// Replace explicit typing marks and close the current typing group.
    ///
    /// Only a collapsed inline selection accepts this decision. `None`
    /// restores inheritance; `Some(empty)` explicitly requests unmarked text.
    /// No document revision, history entry or listener notification is made.
    StoredMarks(Option<MarkSet>),
    /// Consume the eligible rule reversal as one isolated forward edit.
    ///
    /// The host opts in for a verified input source, normally Backspace.
    /// Runtime rechecks eligibility and never falls through on failure. This
    /// restores exact rule input but does not reproduce grouped Ctrl+Z timing.
    UndoInputRule,
    /// Commit one host-planned transaction as an isolated undo unit.
    ///
    /// Stored marks are cleared unless the plan uses `with_stored_marks`.
    /// Core, after-selection and policy validation
    /// all run before publication; any error leaves session state unchanged.
    Apply(EditPlan),
}

/// Optional per-session host editing rules, fixed at construction time.
///
/// Implementations must be pure, read-only and non-reentrant: do not mutate
/// external state or recursively borrow/apply to the session in a callback.
/// In particular, validation must use stable rules for the session's lifetime
/// so undo snapshots cannot become invalid after a configuration change.
/// Callbacks receive no mutable session. Return one plan to replace an intent
/// rather than recursively dispatching another intent or repairing listeners.
/// The session can roll back its own state, not a callback's external effects.
pub trait SessionPolicy {
    /// Chooses fixed mark consumption for default nonempty inline text input.
    ///
    /// Independent from history options. Host plans and other commands retain
    /// their existing explicit marks-after semantics.
    fn default_text_input_marks(&self) -> super::DefaultTextInputMarks {
        super::DefaultTextInputMarks::PreservePending
    }

    /// Chooses immutable history traversal behavior at session construction.
    ///
    /// Defaults retain recorded selections and historical empty-stack behavior.
    /// The value is captured once, never queried during Undo/Redo or publication.
    fn history_options(&self) -> super::HistoryOptions {
        super::HistoryOptions::new()
    }

    /// Optionally supplies one dedicated, isolated CellRange Cut plan.
    ///
    /// `None` preserves the frontend's legacy route. A supplied plan is used
    /// only by [`DocumentSession::prepare_cut`], together with this policy's
    /// explicit Cut export spec. Projection-only Cut stays independently
    /// refused. No platform write or live mutation happens in this callback.
    /// Generic Delete is not a substitute for the host's exact Cut contract.
    fn prepare_cut(&self, _context: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        Ok(None)
    }

    /// Selects explicit clipboard export rules before projection or writes.
    ///
    /// This is pure and read-only like other policy callbacks. `None` retains
    /// historical unit-cell geometry, plain text and metadata behavior. An
    /// error rejects Copy/Cut before touching the platform clipboard. Hosts
    /// must reject unsupported Cut purposes here, not in the later Delete.
    /// Projection-only opted-in CellRange Cut is always rejected by Runtime.
    /// A host supplying a dedicated [`Self::prepare_cut`] plan can use the
    /// scoped session preparation API instead. The callback cannot supply
    /// arbitrary text or authorize a later fallible generic Delete.
    fn clipboard_export_spec(
        &self,
        _context: SessionContext<'_>,
        _purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        Ok(None)
    }
    /// Checks or replaces an intent before any session state changes.
    ///
    /// This also runs before structured-paste planning, stored-mark clearing,
    /// cell-range collapse and history boundaries. An error is a rejection.
    fn prepare_intent(
        &self,
        _context: SessionContext<'_>,
        _intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        Ok(IntentDisposition::Continue)
    }

    /// Validates the initial document and every final candidate snapshot.
    ///
    /// Called for normal, staged, raw, undo and redo commits before history,
    /// document or listeners are published. Hidden intermediate stages are
    /// not host-validated. A rejection never reaches document listeners.
    fn validate_document(&self, _document: &XiaomuDocument) -> Result<(), PolicyError> {
        Ok(())
    }
}

impl DocumentSession {
    pub(super) fn prepare_cut_plan(&self) -> Result<Option<EditPlan>, PolicyError> {
        self.policy.as_ref().map_or(Ok(None), |policy| {
            policy.prepare_cut(SessionContext {
                document: &self.document,
                selection: self.selection,
                stored_marks: self.stored_marks.as_ref(),
                input_rule_undo_available: self.input_rule_undo_available_at(self.selection),
            })
        })
    }

    pub(crate) fn clipboard_export_spec(
        &self,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        self.policy.as_ref().map_or(Ok(None), |policy| {
            policy.clipboard_export_spec(
                SessionContext {
                    document: &self.document,
                    selection: self.selection,
                    stored_marks: self.stored_marks.as_ref(),
                    input_rule_undo_available: self.input_rule_undo_available_at(self.selection),
                },
                purpose,
            )
        })
    }

    /// Creates a session with host rules that cannot be replaced later.
    ///
    /// Both the initial selection and document must be valid. A rejected
    /// initial document produces an error without constructing a session.
    pub fn new_with_policy(
        document: XiaomuDocument,
        selection: DocumentSelection,
        policy: Box<dyn SessionPolicy>,
    ) -> Result<Self, SessionError> {
        let mut session = Self::new(document, selection)?;
        policy.validate_document(&session.document)?;
        session.history_options = policy.history_options();
        session.default_text_input_marks = policy.default_text_input_marks();
        session.policy = Some(policy);
        Ok(session)
    }

    /// Applies one typed editing intent after optional policy preflight.
    ///
    /// No-op intents do not advance the revision, write history or notify
    /// document listeners. Collapsed mark intents change only stored marks.
    /// On error, the document, selection, stored marks and history grouping
    /// remain unchanged and no listener is notified.
    pub fn apply_intent(&mut self, intent: &EditIntent) -> Result<SessionOutcome, SessionError> {
        self.apply_intent_with_selection(self.selection, intent)
    }

    /// Applies an intent at a validated target selection as one atomic action.
    ///
    /// Platform replacement ranges can use this instead of first publishing
    /// a selection change. Policy preflight sees the target selection; stored
    /// marks are inherited normally if it differs from the current selection.
    /// On success only the final selection/document is notified and Undo
    /// restores the selection from before this whole operation. On rejection
    /// or a policy `NoChange`, the original editing state stays unchanged.
    pub fn apply_intent_with_selection(
        &mut self,
        selection: DocumentSelection,
        intent: &EditIntent,
    ) -> Result<SessionOutcome, SessionError> {
        selection.validate(&self.document)?;
        // Preflight deliberately precedes even transient state changes.
        let disposition = match &self.policy {
            Some(policy) => policy.prepare_intent(
                SessionContext {
                    document: &self.document,
                    selection,
                    stored_marks: if selection == self.selection {
                        self.stored_marks.as_ref()
                    } else {
                        None
                    },
                    input_rule_undo_available: self.input_rule_undo_available_at(selection),
                },
                intent,
            )?,
            None => IntentDisposition::Continue,
        };
        if matches!(disposition, IntentDisposition::NoChange) {
            return Ok(SessionOutcome::NoChange);
        }
        self.with_transient_rollback(|session| {
            let before = session.selection;
            if selection != before {
                session.history_selection_before = Some(before);
                session.selection = selection;
                session.input_rule_undo = None;
                session.clear_stored_marks();
                session.history.break_group();
            }
            let outcome = match disposition {
                IntentDisposition::Continue => session.apply_default_intent(intent),
                IntentDisposition::NoChange => Ok(SessionOutcome::NoChange),
                IntentDisposition::StoredMarks(marks) => {
                    if !session.selection.is_collapsed()
                        || session.selection.as_same_node_inline().is_none()
                    {
                        return Err(SessionError::SelectionInvalid);
                    }
                    session.stored_marks = marks;
                    session.history.break_group();
                    Ok(SessionOutcome::NoChange)
                }
                IntentDisposition::UndoInputRule => session.undo_input_rule(),
                IntentDisposition::Apply(plan) => {
                    session.history.break_group();
                    session.clear_stored_marks();
                    session.commit(plan)
                }
            }?;
            // Target selection and cell-range convergence are tentative
            // until all planning succeeds. Other successful outcomes have
            // already published the final selection or document.
            if outcome == SessionOutcome::NoChange && session.selection != before {
                session.input_rule_undo = None;
                session.history.break_group();
                session.notify_selection_changed();
                return Ok(SessionOutcome::SelectionChanged);
            }
            Ok(outcome)
        })
    }

    pub(super) fn validate_candidate(&self, document: &XiaomuDocument) -> Result<(), SessionError> {
        if let Some(policy) = &self.policy {
            policy.validate_document(document)?;
        }
        Ok(())
    }

    /// Save only cheap transient state. Commit paths publish once, after
    /// their last fallible step; failed undo/redo put back their taken entry.
    /// Neither the document nor either transaction stack is cloned here.
    pub(super) fn with_transient_rollback(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<SessionOutcome, SessionError>,
    ) -> Result<SessionOutcome, SessionError> {
        let selection = self.selection;
        let marks = self.stored_marks.clone();
        let group_open = self.history.typing_group_open();
        let history_selection_before = self.history_selection_before;
        let input_rule_undo = self.input_rule_undo.clone();
        let result = operation(self);
        self.history_selection_before = history_selection_before;
        if result.is_err() {
            self.selection = selection;
            self.stored_marks = marks;
            self.history.restore_typing_group(group_open);
            self.input_rule_undo = input_rule_undo;
        }
        result
    }
}
