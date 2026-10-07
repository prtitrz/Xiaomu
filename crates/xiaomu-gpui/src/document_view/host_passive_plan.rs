//! Passive publication of a checked host plan without changing pane ownership.

use gpui::{Context, Window};
use xiaomu_runtime::session::{EditPlan, SessionError, SessionOutcome};

use super::DocumentView;

impl DocumentView {
    /// Applies a host plan without taking another control's focus or scrolling.
    ///
    /// Runtime validates the complete candidate and after-selection, preserves
    /// ordinary change notifications, and records one isolated Undo unit. Hosts
    /// retain responsibility for revision/cleanliness checks and change
    /// classification; do not yield between those checks and this call.
    /// Fixed text-size and semantic admission belong to the bound session
    /// policy. Fresh table identities require a new view measurement; until
    /// then, and on presentation failure, normal hidden-input guards apply.
    /// A successful commit alone does not certify presentation or saving.
    ///
    /// Returns `None` while any receiving paragraph or range input has active
    /// composition, even when that surface is not focused. Preedit is never
    /// committed or cancelled here. Refusal, errors and `NoChange` leave the
    /// view's geometry, gestures and focus unchanged.
    ///
    /// After success, stale geometry and gestures are discarded and children
    /// are synchronized. Native focus follows the resulting selection only if
    /// this view already owned it, including child, range and root surfaces.
    /// Fresh input surfaces do not request caret scrolling. Viewport offsets
    /// remain subject to the normal layout clamp when content becomes smaller.
    pub fn apply_passive_edit_plan(
        &mut self,
        plan: &EditPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<SessionOutcome>, SessionError> {
        if self.has_active_composition(cx) {
            return Ok(None);
        }
        // Capture ownership before fresh identities retire the focused entity.
        let owned_focus = self.focused_child(window, cx).is_some()
            || self
                .focus_handle
                .as_ref()
                .is_some_and(|handle| handle.is_focused(window));
        let outcome = self.session.borrow_mut().apply_plan(plan.clone())?;
        if outcome == SessionOutcome::NoChange {
            return Ok(Some(outcome));
        }

        self.epoch.set(self.epoch.get() + 1);
        self.desired_x = None;
        self.is_dragging = false;
        self.cell_drag_anchor = None;
        self.column_resize.cancel();
        self.column_resize.clear_measurements();
        self.registry.borrow_mut().clear();
        self.cell_registry.borrow_mut().clear();
        self.table_clips.borrow_mut().clear();
        // Retired children may still own queued next-frame caret scrolling.
        self.invalidate_passive_inputs(cx);
        self.sync_children(cx);
        self.invalidate_passive_inputs(cx);
        if owned_focus {
            self.route_focus(window, cx);
        }
        cx.notify();
        Ok(Some(outcome))
    }

    fn invalidate_passive_inputs(&self, cx: &mut Context<Self>) {
        for (_, child) in self.children.iter().chain(self.range_input.iter()) {
            child.update(cx, |child, _| child.invalidate_passive_layout());
        }
    }
}

#[cfg(test)]
#[path = "host_passive_plan_tests.rs"]
mod tests;
