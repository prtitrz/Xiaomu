//! Scoped, single-use preparation for a host's explicit CellRange Cut command.

use super::commit::PreparedCommit;
use super::plan::HistoryPolicy;
use super::{DocumentSession, SessionError, SessionOutcome};
use crate::clipboard::{ClipboardExportPurpose, ClipboardSlice, export_prepared_cell_cut};

/// A fully preflighted Cut bound to its exclusively borrowed source session.
///
/// Inspect the detached Slice, prepare a lossless platform item, then publish
/// the platform item and consume this guard synchronously. Dropping the guard
/// leaves the session completely unchanged. Publication does not replan,
/// revalidate or allocate canonical identities a second time.
///
/// The exclusive borrow prevents stale selection/history/marks and publication
/// into another session, not merely stale document revisions. No unsafe or
/// detached-token escape hatch exists. A guard cannot survive a source edit:
///
/// ```compile_fail
/// use xiaomu_runtime::session::{DocumentSession, EditIntent};
/// fn stale(session: &mut DocumentSession) {
///     let prepared = session.prepare_cut().unwrap().unwrap();
///     session.apply_intent(&EditIntent::InsertText { text: "stale".into() }).unwrap();
///     prepared.publish();
/// }
/// ```
///
/// Selection-only changes are excluded for the same reason:
///
/// ```compile_fail
/// use xiaomu_runtime::session::{DocumentSelection, DocumentSession};
/// fn stale_selection(session: &mut DocumentSession, selection: DocumentSelection) {
///     let prepared = session.prepare_cut().unwrap().unwrap();
///     session.set_document_selection(selection).unwrap();
///     prepared.publish();
/// }
/// ```
///
/// History navigation cannot invalidate a live prepared operation:
///
/// ```compile_fail
/// use xiaomu_runtime::session::DocumentSession;
/// fn stale_history(session: &mut DocumentSession) {
///     let prepared = session.prepare_cut().unwrap().unwrap();
///     session.undo().unwrap();
///     prepared.publish();
/// }
/// ```
///
/// A source session cannot have two simultaneously usable guards:
///
/// ```compile_fail
/// use xiaomu_runtime::session::DocumentSession;
/// fn overlapping(session: &mut DocumentSession) {
///     let first = session.prepare_cut().unwrap().unwrap();
///     let second = session.prepare_cut().unwrap().unwrap();
///     first.publish();
///     second.publish();
/// }
/// ```
///
/// Publishing consumes the guard, so a second publication is impossible:
///
/// ```compile_fail
/// use xiaomu_runtime::session::DocumentSession;
/// fn duplicate(session: &mut DocumentSession) {
///     let prepared = session.prepare_cut().unwrap().unwrap();
///     prepared.publish();
///     prepared.publish();
/// }
/// ```
///
/// The owner cannot be swapped with another session while its guard is live:
///
/// ```compile_fail
/// use xiaomu_runtime::session::DocumentSession;
/// fn exchange(first: &mut DocumentSession, second: &mut DocumentSession) {
///     let prepared = first.prepare_cut().unwrap().unwrap();
///     std::mem::swap(first, second);
///     prepared.publish();
/// }
/// ```
///
/// This is a semantic-rejection boundary, not an acknowledgment from the OS
/// clipboard or a crash-atomic transaction across two independent systems.
#[must_use = "dropping a prepared Cut cancels it without changing the session"]
pub struct PreparedCut<'a> {
    session: &'a mut DocumentSession,
    commit: PreparedCommit,
    slice: ClipboardSlice,
}

impl PreparedCut<'_> {
    /// The exact, bounded source projection from the prepared snapshot.
    #[must_use]
    pub const fn clipboard_slice(&self) -> &ClipboardSlice {
        &self.slice
    }

    /// Publishes the already validated candidate and its one isolated undo unit.
    ///
    /// The caller must finish all fallible platform-item preparation first.
    /// This does not write a platform clipboard or confirm its ownership.
    pub fn publish(self) -> SessionOutcome {
        self.session.publish_prepared_commit(self.commit)
    }
}

impl DocumentSession {
    /// Prepares an explicitly opted-in CellRange Cut without live side effects.
    ///
    /// `Ok(None)` means no dedicated policy plan; existing callers may retain
    /// their legacy Cut route. `Some` keeps this session mutably borrowed until
    /// publication or cancellation. Policy, projection budget, Core, final
    /// selection, candidate admission and exact inverse/redo are all evaluated
    /// before returning the guard. Projection-only opted-in CellRange Cut remains
    /// closed; the historical no-export-spec unit-cell path stays unchanged.
    pub fn prepare_cut(&mut self) -> Result<Option<PreparedCut<'_>>, SessionError> {
        self.selection.validate(&self.document)?;
        let plan = self.prepare_cut_plan()?;
        let Some(mut plan) = plan else {
            return Ok(None);
        };
        if self.selection.active_cell_range().is_none() || plan.transaction().steps().is_empty() {
            return Err(SessionError::UnsupportedTableOperation);
        }
        let spec = self
            .clipboard_export_spec(ClipboardExportPurpose::Cut)?
            .ok_or(SessionError::UnsupportedTableOperation)?;
        let slice = export_prepared_cell_cut(&self.document, self.selection, spec)?
            .ok_or(SessionError::SelectionInvalid)?;
        // A Cut is isolated, clears pending marks and cannot install an input
        // rule rollback token. All these changes occur only on publication.
        plan.input_rule_undo = None;
        let plan = plan
            .with_stored_marks(None)
            .with_history_policy(HistoryPolicy::Isolated);
        let commit = self.prepare_commit(plan)?;
        Ok(Some(PreparedCut {
            session: self,
            commit,
            slice,
        }))
    }
}
