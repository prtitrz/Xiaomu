//! Fixed construction-time history clock ownership.

use super::DocumentView;
use xiaomu_runtime::session::{DocumentSelection, EditIntent, SessionError, SessionOutcome};

use crate::{block_view::SharedSession, history_clock::SharedHistoryClock};

#[cfg(test)]
#[path = "history_clock_tests.rs"]
mod tests;

impl DocumentView {
    // Only the guarded central command path calls this helper. Sample once
    // before borrowing Runtime mutably; no timestamp is ambient or retained.
    pub(super) fn apply_runtime_intent(
        &self,
        target: Option<DocumentSelection>,
        intent: &EditIntent,
    ) -> Result<SessionOutcome, SessionError> {
        let timestamp = self.history_clock.as_ref().map(|clock| clock.now());
        let mut session = self.session.borrow_mut();
        match (target, timestamp) {
            (Some(target), Some(timestamp)) => {
                session.apply_intent_with_selection_at(target, intent, timestamp)
            }
            (Some(target), None) => session.apply_intent_with_selection(target, intent),
            (None, Some(timestamp)) => session.apply_intent_at(intent, timestamp),
            (None, None) => session.apply_intent(intent),
        }
    }

    /// Creates a view with the shared session's explicit history clock.
    ///
    /// Every normal, nested, table and range child inherits this provider,
    /// including children created after editing. All views over the same
    /// session must share its clock domain for the session's lifetime.
    #[must_use]
    pub fn new_with_history_clock(
        session: SharedSession,
        history_clock: SharedHistoryClock,
    ) -> Self {
        Self {
            history_clock: Some(history_clock),
            ..Self::new(session)
        }
    }
}
