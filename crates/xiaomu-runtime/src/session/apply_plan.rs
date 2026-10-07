//! Checked host plans use the same atomic publication path as typed intents.

use super::{DocumentSession, EditPlan, SessionError, SessionOutcome};

impl DocumentSession {
    /// Applies a host-planned transaction with explicit after-selection.
    ///
    /// Core validation, final selection validation and the construction-time
    /// document policy all run before publication. This bypasses typed-intent
    /// planning, not final policy validation. Hosts must plan against the
    /// current snapshot and perform their own revision/authority checks without
    /// yielding before this call.
    ///
    /// A successful plan is one isolated ordinary Undo unit, retains earlier
    /// Undo entries, clears Redo and pending typing marks (unless the plan
    /// explicitly supplies them), and emits one document-change notification.
    /// Unlike intent no-ops, even an empty transaction commits the requested
    /// selection, advances revision and records history. Every failure leaves
    /// document, selection, history, pending marks and listeners unchanged.
    pub fn apply_plan(&mut self, plan: EditPlan) -> Result<SessionOutcome, SessionError> {
        self.with_transient_rollback(|session| {
            session.history.break_group();
            session.clear_stored_marks();
            session.commit(plan)
        })
    }
}
