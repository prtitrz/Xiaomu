//! Opt-in completed atom clicks, using the ordinary painted-text hit test.

use std::rc::Rc;

use gpui::{
    App, Bounds, Context, DispatchPhase, Entity, FocusHandle, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Subscription, Window,
};
use xiaomu_core::document::DocumentRevision;
use xiaomu_runtime::session::{DocumentSelection, DocumentSessionIdentity};

use super::{DocumentView, mouse::ChipHit};
use crate::atom_capability::{ATOM_ACTION_CLICK, AtomAction, SharedAtomCapability};

/// Div move listeners skip positions outside their hitbox. Observe motion in
/// capture phase so leaving the surface cannot revive a pending click later.
pub(super) fn observe_pointer_motion(
    view: &Entity<DocumentView>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &App,
) {
    if view.read(cx).atom_capability.is_none() {
        return;
    }
    let weak = view.downgrade();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let _ = weak.update(cx, |view, cx| {
            if !bounds.contains(&event.position) {
                if let Some(gesture) = view.atom_pointer.as_mut() {
                    gesture.cancelled = true;
                }
            } else {
                view.update_atom_pointer(event, window, cx);
            }
        });
    });
}

pub(super) struct AtomPointerGesture {
    chip: ChipHit,
    capability: SharedAtomCapability,
    session: DocumentSessionIdentity,
    revision: DocumentRevision,
    selection: DocumentSelection,
    epoch: u64,
    cancelled: bool,
    focus: Option<FocusHandle>,
    subscriptions: Vec<Subscription>,
}

pub(super) struct AtomPointerFrame {
    session: DocumentSessionIdentity,
    revision: DocumentRevision,
    epoch: u64,
}

impl AtomPointerGesture {
    fn action(&self) -> AtomAction {
        AtomAction {
            node: self.chip.node,
            kind: self.chip.kind.clone(),
            action: ATOM_ACTION_CLICK.into(),
            attrs: self.chip.attrs.clone(),
        }
    }

    fn is_current(&self, view: &DocumentView) -> bool {
        let session = view.session.borrow();
        !self.cancelled
            && view.atom_pointer_frame.as_ref().is_some_and(|frame| {
                frame.session == self.session
                    && frame.revision == self.revision
                    && frame.epoch == self.epoch
            })
            && self.session == session.identity()
            && self.revision == session.document().revision()
            && self.selection == session.selection()
            && self.epoch == view.epoch.get()
            && view
                .atom_capability
                .as_ref()
                .is_some_and(|capability| Rc::ptr_eq(capability, &self.capability))
    }
}

impl DocumentView {
    pub(super) fn record_atom_pointer_frame(&mut self) {
        let session = self.session.borrow();
        self.atom_pointer_frame = Some(AtomPointerFrame {
            session: session.identity(),
            revision: session.document().revision(),
            epoch: self.epoch.get(),
        });
    }

    pub(super) fn capture_atom_pointer_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.atom_pointer = None;
        self.begin_atom_pointer_down(event, true, window, cx);
    }

    /// Only a visible chip owns capture. Clamped text hits wait for bubble so
    /// sibling images, rules and controls can handle their own press first.
    pub(super) fn begin_atom_pointer_down(
        &mut self,
        event: &MouseDownEvent,
        capture_phase: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.button != MouseButton::Left {
            return false;
        }
        let Some(capability) = self.atom_capability.clone() else {
            return false;
        };
        let Some(hit) = self.hit_test(event.position, cx) else {
            return false;
        };
        if hit.inside_visible_bounds != capture_phase {
            return false;
        }
        let Some(chip) = hit.chip else {
            return false;
        };
        let session = self.session.borrow();
        let mut gesture = AtomPointerGesture {
            chip,
            capability: capability.clone(),
            session: session.identity(),
            revision: session.document().revision(),
            selection: session.selection(),
            epoch: self.epoch.get(),
            cancelled: false,
            focus: window.focused(cx),
            subscriptions: Vec::new(),
        };
        drop(session);
        if !capability.capture_pointer_click(&gesture.action(), event.modifiers) {
            return false;
        }
        // A host eligibility query may itself invalidate the original snapshot.
        // Consume that press, but never activate a replacement target.
        gesture.cancelled = !hit.inside_visible_bounds || !gesture.is_current(self);
        if let Some(focus) = &gesture.focus {
            gesture
                .subscriptions
                .push(cx.on_blur(focus, window, |view, _, _| {
                    view.atom_pointer = None;
                }));
        }
        gesture
            .subscriptions
            .push(cx.observe_window_activation(window, |view, _, _| {
                view.atom_pointer = None;
            }));
        self.atom_pointer = Some(gesture);
        self.is_dragging = false;
        self.cell_drag_anchor = None;
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    pub(super) fn capture_atom_pointer_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        let Some(gesture) = self.atom_pointer.take() else {
            return;
        };
        let matches = gesture.is_current(self)
            && gesture.focus == window.focused(cx)
            && self.hit_test(event.position, cx).is_some_and(|hit| {
                hit.inside_visible_bounds && hit.chip.as_ref() == Some(&gesture.chip)
            });
        self.is_dragging = false;
        self.cell_drag_anchor = None;
        window.prevent_default();
        cx.stop_propagation();
        if matches {
            gesture
                .capability
                .atom_pointer_click(gesture.action(), event.modifiers);
        }
    }

    pub(super) fn update_atom_pointer(
        &mut self,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &App,
    ) -> bool {
        let Some(gesture) = self.atom_pointer.as_ref() else {
            return false;
        };
        let matches = event.pressed_button == Some(MouseButton::Left)
            && gesture.is_current(self)
            && gesture.focus == window.focused(cx)
            && self.hit_test(event.position, cx).is_some_and(|hit| {
                hit.inside_visible_bounds && hit.chip.as_ref() == Some(&gesture.chip)
            });
        if !matches && let Some(gesture) = self.atom_pointer.as_mut() {
            gesture.cancelled = true;
        }
        true
    }
}
