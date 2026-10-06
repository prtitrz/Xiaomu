//! Atomic host targeting through the existing typed-intent frontend path.

use super::DocumentView;
use gpui::{Context, Window};
use xiaomu_runtime::session::{DocumentSelection, EditIntent};

impl DocumentView {
    /// Applies a typed intent at an explicit target as one atomic operation.
    ///
    /// Unlike publishing a selection move first, this preserves the original
    /// pre-operation selection for Undo. Runtime validates the target before
    /// policy preflight; policy sees both the target and original selections.
    /// Host plans still require a valid final selection and candidate document.
    /// Invalid targets, policy rejection and policy `NoChange` leave canonical
    /// state, stored marks, history and document/selection listeners unchanged.
    /// An accepted default no-op can publish just the changed target selection.
    ///
    /// Uses the same composition/presentation guards, history clock, rejection
    /// events and view updates as [`Self::apply_edit_intent`]. Active native
    /// composition or hidden table endpoints (current or target) silently ignore
    /// the command. Accepted selection-only changes also restore native focus.
    /// Hosts must derive the target from the current snapshot without yielding
    /// before this call; product-specific target and editing rules remain in
    /// host policy, including read-only or no-applicable-target behavior.
    pub fn apply_edit_intent_with_selection(
        &mut self,
        target_selection: DocumentSelection,
        intent: EditIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_edit_intent_inner(Some(target_selection), intent, window, cx);
    }
}

#[cfg(test)]
#[path = "host_selection_intent_tests.rs"]
mod tests;
