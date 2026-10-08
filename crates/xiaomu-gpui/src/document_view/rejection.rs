//! Optional, payload-free feedback for rejected document-view commands and native input.

use super::DocumentView;
use crate::block_view::ParagraphView;
use gpui::{Context, Entity, EventEmitter};
use std::rc::Rc;
use xiaomu_core::document::DocumentRevision;
use xiaomu_runtime::session::SessionError;

/// The frontend boundary that refused an operation.
///
/// This identifies the failure stage, not the originating key or menu item.
/// A paste rejected by session policy has [`Self::Intent`] as its stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditorRejectionStage {
    /// A typed intent applied through `DocumentView` failed.
    Intent,
    /// The optional host edit-command router refused the command.
    CommandRouting,
    /// Copy could not project or losslessly transport the selection.
    ClipboardCopy,
    /// Cut could not project or losslessly transport the selection.
    ClipboardCut,
    /// Recognized native clipboard metadata was rejected before paste.
    ClipboardPaste,
    /// An opted-in native font-size view refused a transient composition.
    TextSizePreedit,
    /// An opted-in native font-size view's text/IME session intent failed.
    TextSizeInput,
    /// An opted-in size view could not produce safe current-frame geometry.
    TextSizeLayout,
    /// A native typing, explicit replacement or IME commit session intent failed.
    /// Font-size-capable input views retain [`Self::TextSizeInput`] instead.
    NativeInput,
}

/// A bounded, content-free classification for host rejection feedback.
///
/// Host policy messages and Core errors may contain document or clipboard
/// data. They are deliberately not copied into this public diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditorRejectionReason {
    /// A host policy refused the action or its candidate document.
    Policy,
    /// The underlying document transaction was invalid.
    InvalidTransaction,
    /// The action could not preserve a valid selection.
    InvalidSelection,
    /// The available planner cannot preserve this edit's semantics.
    UnsupportedEdit,
    /// The table operation needs a compatible span-aware planner.
    UnsupportedTableOperation,
    /// An input-rule undo payload exceeded the supported budget.
    InputRuleUndoBudget,
    /// Inline atoms cannot be preserved at this paste target.
    ClipboardAtoms,
    /// The clipboard source boundary requires a compatible paste planner.
    ClipboardBoundary,
    /// An atomic clipboard block cannot be placed at this target.
    ClipboardAtomic,
    /// The clipboard table does not fit the target or supported planner.
    ClipboardTable,
    /// Clipboard metadata could not be validated or transported losslessly.
    ClipboardMetadata,
    /// A future session error without a more specific frontend classification.
    Other,
    /// The native renderer cannot safely shape this font-size composition.
    UnsupportedTextSize,
}

impl EditorRejectionReason {
    /// Returns a fixed diagnostic suitable for display, without source data.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Policy => "The editor policy rejected this action.",
            Self::InvalidTransaction => "The document transaction could not be applied.",
            Self::InvalidSelection => "This action could not preserve a valid selection.",
            Self::UnsupportedEdit => "This edit is not supported at the current target.",
            Self::UnsupportedTableOperation => "This table operation is not supported.",
            Self::InputRuleUndoBudget => "This input-rule undo exceeds the supported size limit.",
            Self::ClipboardAtoms => "The clipboard's inline objects cannot be pasted here.",
            Self::ClipboardBoundary => "The clipboard's structure cannot be pasted here.",
            Self::ClipboardAtomic => "The clipboard's block objects cannot be pasted here.",
            Self::ClipboardTable => "The clipboard table does not fit this target or paste mode.",
            Self::ClipboardMetadata => "The clipboard's structured data could not be preserved.",
            Self::Other => "The editor could not complete this action.",
            Self::UnsupportedTextSize => {
                "This text cannot be safely rendered with the selected font sizes."
            }
        }
    }
}

/// Optional GPUI feedback for one failed command or native-input boundary.
///
/// Subscribe to the particular `Entity<DocumentView>` with GPUI's `subscribe`
/// or `subscribe_in`; retain the subscription for as long as feedback is needed.
/// The emitter entity supplies identity, so hosts must associate it with the
/// correct editor/note and discard stale subscriptions when replacing a view.
/// No mandatory hook or global callback is installed.
///
/// Emission occurs after the rejected session call has returned and its borrow
/// has been released. Session rollback is complete before a subscriber can read
/// the view's session. Emission itself changes no document, selection, marks,
/// history, layout epoch or document/selection listener state. GPUI delivers
/// events through its normal event queue, not as synchronous session listeners;
/// the event's local document revision is captured when emitted, allowing hosts
/// to discard a queued diagnostic after a later canonical edit. It is not a
/// full immutable session snapshot, persistence timestamp or global sequence.
///
/// Coverage includes failed `apply_edit_intent` and
/// `apply_edit_intent_with_selection` calls, host edit command routing, Copy/Cut
/// projection or lossless transport, rejected native Paste metadata, and failed
/// `ParagraphView` typing, explicit replacement and IME commit session calls.
/// Native input failures use `NativeInput`, or the existing `TextSizeInput` for
/// font-size-capable input views. Paragraph and document-range input children
/// forward the original event and revision to their owning `DocumentView`.
/// Forwarding checks the existing session and view-epoch identities, so a
/// retained child cannot report into a view replaced in the same GPUI Entity.
/// Standalone `ParagraphView` hosts can subscribe to that entity directly.
/// Successful edits, `NoChange`, empty/unsupported foreign clipboards, legacy
/// Copy text fallback, cancelled composition and composition/presentation guards
/// do not emit, except opted-in font-size admission failures
/// (`TextSizePreedit` / `TextSizeLayout`). Direct session calls,
/// selection/navigation/checkbox actions, image import, history, persistence and
/// the explicitly returned errors from `apply_edit_transaction` are not covered.
/// This is not a universal engine error stream or a save-status event.
///
/// The event is bounded and contains no intent payload, clipboard data, node
/// attributes, host policy text or arbitrary error strings. Existing logging
/// remains independent. Hosts can localize feedback by stage and reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorRejection {
    stage: EditorRejectionStage,
    reason: EditorRejectionReason,
    document_revision: DocumentRevision,
}

impl EditorRejection {
    /// Returns the boundary that failed.
    #[must_use]
    pub const fn stage(self) -> EditorRejectionStage {
        self.stage
    }
    /// Returns a content-free classification of the rejection.
    #[must_use]
    pub const fn reason(self) -> EditorRejectionReason {
        self.reason
    }
    /// Returns the reason's fixed, bounded diagnostic, never source content.
    #[must_use]
    pub const fn message(self) -> &'static str {
        self.reason.message()
    }

    /// Returns the emitting view's canonical document revision at rejection.
    ///
    /// Compare only within that same editor Entity. A changed revision means a
    /// later canonical edit occurred before this queued event was inspected.
    /// This content-free stamp does not capture selection or persistence state.
    #[must_use]
    pub const fn document_revision(self) -> DocumentRevision {
        self.document_revision
    }

    pub(crate) const fn new(
        stage: EditorRejectionStage,
        reason: EditorRejectionReason,
        document_revision: DocumentRevision,
    ) -> Self {
        Self {
            stage,
            reason,
            document_revision,
        }
    }
    pub(crate) fn from_session(
        stage: EditorRejectionStage,
        error: &SessionError,
        document_revision: DocumentRevision,
    ) -> Self {
        let reason = match error {
            SessionError::Policy(_) => EditorRejectionReason::Policy,
            SessionError::Core(_) => EditorRejectionReason::InvalidTransaction,
            SessionError::SelectionDeleted | SessionError::SelectionInvalid => {
                EditorRejectionReason::InvalidSelection
            }
            SessionError::UnsupportedEdit => EditorRejectionReason::UnsupportedEdit,
            SessionError::UnsupportedTableOperation => {
                EditorRejectionReason::UnsupportedTableOperation
            }
            SessionError::InputRuleUndoBudgetExceeded => EditorRejectionReason::InputRuleUndoBudget,
            SessionError::ClipboardAtomsUnsupported => EditorRejectionReason::ClipboardAtoms,
            SessionError::ClipboardClosedUnsupported => EditorRejectionReason::ClipboardBoundary,
            SessionError::ClipboardAtomicUnsupported => EditorRejectionReason::ClipboardAtomic,
            SessionError::ClipboardTableUnsupported => EditorRejectionReason::ClipboardTable,
            _ => EditorRejectionReason::Other,
        };
        Self::new(stage, reason, document_revision)
    }
}

impl EventEmitter<EditorRejection> for DocumentView {}
impl EventEmitter<EditorRejection> for crate::block_view::ParagraphView {}

impl DocumentView {
    pub(super) fn forward_input_rejection(
        &self,
        input: &Entity<ParagraphView>,
        event: &EditorRejection,
        cx: &mut Context<Self>,
    ) {
        let input = input.read(cx);
        // Native handlers can retain a child after an in-place host view
        // replacement. Session revisions alone cannot identify its owner.
        if Rc::ptr_eq(&self.session, &input.session) && Rc::ptr_eq(&self.epoch, &input.epoch) {
            cx.emit(*event);
        }
    }

    pub(super) fn emit_rejection(
        &self,
        stage: EditorRejectionStage,
        reason: EditorRejectionReason,
        cx: &mut Context<Self>,
    ) {
        let revision = self.session.borrow().document().revision();
        cx.emit(EditorRejection::new(stage, reason, revision));
    }

    pub(super) fn emit_session_rejection(
        &self,
        stage: EditorRejectionStage,
        error: &SessionError,
        cx: &mut Context<Self>,
    ) {
        let revision = self.session.borrow().document().revision();
        cx.emit(EditorRejection::from_session(stage, error, revision));
    }
}

#[cfg(test)]
#[path = "rejection_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "rejection_input_tests.rs"]
mod input_tests;
