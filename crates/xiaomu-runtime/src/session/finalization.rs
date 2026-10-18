//! One bounded host amendment inside the original atomic edit.

use xiaomu_core::document::XiaomuDocument;
use xiaomu_core::transaction::Transaction;

use super::{DocumentChangeOrigin, DocumentSelection, DocumentSession, PolicyError, SessionError};

/// Immutable original and fully resolved candidate state for one local edit.
///
/// The candidate is Core-valid, but has not yet passed final host validation.
/// The original selection belongs to the before document; `selection` belongs
/// to the candidate. No mutable session or history capability is exposed.
#[derive(Clone, Copy)]
pub struct FinalizeEditContext<'a> {
    before: &'a XiaomuDocument,
    candidate: &'a XiaomuDocument,
    original_selection: DocumentSelection,
    selection: DocumentSelection,
    origin: DocumentChangeOrigin,
}

impl<'a> FinalizeEditContext<'a> {
    /// Snapshot before the complete operation, including atomic target changes.
    #[must_use]
    pub const fn before_document(self) -> &'a XiaomuDocument {
        self.before
    }

    /// Prepared candidate after all original stages and selection resolution.
    #[must_use]
    pub const fn candidate_document(self) -> &'a XiaomuDocument {
        self.candidate
    }

    /// Original session selection, before any atomic target-selection change.
    #[must_use]
    pub const fn original_selection(self) -> DocumentSelection {
        self.original_selection
    }

    /// Resolved selection in the candidate, before the optional amendment.
    #[must_use]
    pub const fn selection(self) -> DocumentSelection {
        self.selection
    }

    /// Original publication classification; external edits do not call the hook.
    #[must_use]
    pub const fn change_origin(self) -> DocumentChangeOrigin {
        self.origin
    }
}

/// One bounded transaction applied to an already prepared edit candidate.
///
/// Admission permits at most 64 steps, 256 owned payload nodes and 64 KiB of
/// accounted payload, with attribute nesting at most 32. Unknown payload shapes
/// fail closed. Core's document-dependent resource and validity checks still
/// apply. This bounds owned descriptions, not callback CPU or all session memory.
/// No nested finalization occurs. History grouping, marks and origin remain the
/// original command's responsibility.
#[derive(Clone, Debug)]
pub struct EditFinalization {
    transaction: Transaction,
    selection: Option<DocumentSelection>,
}

impl EditFinalization {
    /// Checks the borrowed payload before retaining a compact transaction clone.
    ///
    /// `None` maps the resolved candidate selection through the amendment.
    /// `Some` installs exact coordinates in the final document, validated before
    /// publication. An invalid or deleted selection refuses the complete edit.
    pub fn new(
        transaction: Transaction,
        selection: Option<DocumentSelection>,
    ) -> Result<Self, SessionError> {
        check_budget(&transaction)?;
        Ok(Self {
            transaction: transaction.clone(),
            selection,
        })
    }
}

fn check_budget(transaction: &Transaction) -> Result<(), SessionError> {
    super::input_rule_undo::account(transaction, None).map_err(|_| {
        SessionError::Policy(PolicyError::new(
            "edit finalization payload budget exceeded",
        ))
    })
}

impl DocumentSession {
    /// Extends the inverse in reverse order, without publishing any stage.
    pub(super) fn finalize_candidate(
        &self,
        candidate: &mut XiaomuDocument,
        selection: &mut DocumentSelection,
        undo: &mut Transaction,
        origin: DocumentChangeOrigin,
    ) -> Result<(), SessionError> {
        let Some(policy) = &self.policy else {
            return Ok(());
        };
        if origin != DocumentChangeOrigin::Local
            || (candidate.root() == self.document.root()
                && candidate.store() == self.document.store())
        {
            return Ok(());
        }
        let Some(amendment) = policy.finalize_edit(FinalizeEditContext {
            before: &self.document,
            candidate,
            original_selection: self
                .finalization_selection_before
                .or(self.history_selection_before)
                .unwrap_or(self.selection),
            selection: *selection,
            origin,
        })?
        else {
            return Ok(());
        };
        check_budget(&amendment.transaction)?;
        let applied = amendment.transaction.apply_with_changes(candidate)?;
        let after = match amendment.selection {
            Some(exact) => exact,
            None => selection.map_through(applied.changes(), candidate)?,
        };
        after.validate(applied.document())?;
        // Default typing can retain pending marks without an explicit plan
        // override. An amendment must not move those marks onto a range/gap.
        // Explicit overrides are additionally checked by commit preparation.
        if self.stored_marks.is_some()
            && (!after.is_collapsed() || after.as_same_node_inline().is_none())
        {
            return Err(SessionError::SelectionInvalid);
        }
        let mut combined = applied.inverse().clone();
        for step in undo.steps() {
            combined.push_step(step.clone());
        }
        *undo = combined;
        *selection = after;
        *candidate = applied.into_document();
        Ok(())
    }
}
