//! Capture before text/cell gestures, retaining only weak view references.

use gpui::{
    App, Context, CursorStyle, DispatchPhase, Entity, Hitbox, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Window,
};

use super::{ResizeDrag, geometry::drag_width};
use crate::{document_view::DocumentView, table_column_resize::TableColumnResizeIntent};

impl DocumentView {
    fn column_resize_hit(
        &self,
        position: Point<Pixels>,
        cx: &App,
    ) -> Option<(usize, TableColumnResizeIntent)> {
        let capability = self.column_resize.capability.as_ref()?;
        if !self.table_capability.borrow().enabled() || self.has_active_composition(cx) {
            return None;
        }
        let measurements = self.column_resize.measurements.borrow();
        // Outer tables register before descendants. An inner table without a
        // resize edge must not accidentally activate an enclosing table's edge.
        let (index, measured) = measurements
            .iter()
            .enumerate()
            .rev()
            .find(|(_, table)| table.contains(position, capability.config.handle_width))?;
        let intent = measured.hit(position, capability.config)?;
        let session = self.session.borrow();
        if session.document().root() != measured.document.root()
            || session.document().store() != measured.document.store()
            || session.document().revision() != intent.revision
            || self
                .table_capability
                .borrow()
                .hidden_ancestor(session.document(), intent.table)
                .is_some()
            || !(capability.guard)(&session, &intent)
        {
            return None;
        }
        Some((index, intent))
    }

    pub(super) fn begin_column_resize(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.button != MouseButton::Left {
            return false;
        }
        if self.column_resize.drag.borrow().is_some() {
            return true;
        }
        let Some((index, intent)) = self.column_resize_hit(event.position, cx) else {
            return false;
        };
        let Ok(plan) = crate::table_layout::TableLayoutPlan::from_document(
            self.session.borrow().document(),
            intent.table,
            Default::default(),
        ) else {
            return false;
        };
        let (key, available, origin) = {
            let measurements = self.column_resize.measurements.borrow();
            let measured = &measurements[index];
            (measured.key.clone(), measured.available, measured.origin)
        };
        self.focus_selection(window, cx);
        self.is_dragging = false;
        self.cell_drag_anchor = None;
        *self.column_resize.drag.borrow_mut() = Some(ResizeDrag {
            intent,
            start_x: f32::from(event.position.x),
            session: self.session.clone(),
            document: self.session.borrow().document().clone(),
            plan,
            key,
            available,
            origin,
            released: false,
            commit_queued: false,
            token: std::rc::Rc::new(()),
        });
        cx.notify();
        true
    }

    fn update_column_resize(&self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        if !self.validate_column_resize(cx) {
            cx.notify();
            return false;
        }
        let capability = self
            .column_resize
            .capability
            .as_ref()
            .expect("validated capability");
        let mut state = self.column_resize.drag.borrow_mut();
        let Some(drag) = state.as_mut() else {
            return false;
        };
        if drag.released {
            return false;
        }
        let Some(width) = drag_width(
            drag.intent.initial_width,
            drag.start_x,
            f32::from(position.x),
            capability.config.min_column_width,
        ) else {
            *state = None;
            cx.notify();
            return false;
        };
        let mut intent = drag.intent;
        intent.width = width;
        if !(capability.guard)(&self.session.borrow(), &intent) {
            *state = None;
            cx.notify();
            return false;
        }
        // Reject impossible extents even if release arrives before a render.
        let mut plan = drag.plan.clone();
        let valid_layout = plan
            .override_column_width(intent.column, intent.width)
            .and_then(|()| plan.layout(drag.available, &vec![0.0; plan.cells().len()]))
            .is_ok();
        if !valid_layout {
            *state = None;
            cx.notify();
            return false;
        }
        drag.intent = intent;
        cx.notify();
        true
    }

    pub(super) fn end_column_resize(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.update_column_resize(position, cx) {
            return;
        }
        let ready = {
            let mut state = self.column_resize.drag.borrow_mut();
            let drag = state.as_mut().expect("updated gesture");
            drag.released = true;
            let measured = self.column_resize.measurements.borrow();
            let exact = measured
                .iter()
                .find(|table| table.table == drag.intent.table)
                .is_some_and(|table| {
                    table.geometry.column_edges[drag.intent.column + 1]
                        - table.geometry.column_edges[drag.intent.column]
                        == drag.intent.width as f32
                });
            if exact {
                drag.commit_queued = true;
                Some(drag.token.clone())
            } else {
                None
            }
        };
        if let Some(token) = ready {
            self.commit_measured_column_resize(&token, window, cx);
        } else {
            // Unmeasured release coordinates require one real child-layout pass.
            cx.notify();
        }
    }
}

pub(in crate::document_view) fn paint_resize_cursor(
    view: &Entity<DocumentView>,
    hitbox: &Hitbox,
    window: &mut Window,
    cx: &mut App,
) {
    let (active, hovered) = view.update(cx, |view, cx| {
        let active = view.column_resize.drag.borrow().is_some();
        let hovered = hitbox.is_hovered(window)
            && view
                .column_resize_hit(window.mouse_position(), cx)
                .is_some();
        // Keep move invalidation aligned with the actual cursor request in
        // this frame, including guard/layout changes without pointer motion.
        view.column_resize.hovered = hovered;
        (active, hovered)
    });
    if active || hovered {
        window.set_cursor_style(CursorStyle::ResizeLeftRight, hitbox);
    }
    if active {
        window.set_window_cursor_style(CursorStyle::ResizeLeftRight);
    }
}

pub(in crate::document_view) fn register_resize_pointer_handlers(
    view: &Entity<DocumentView>,
    hitbox: Hitbox,
    window: &mut Window,
    cx: &mut App,
) {
    if !view.read(cx).column_resize.enabled() {
        return;
    }
    let weak = view.downgrade();
    let down_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture || !down_hitbox.is_hovered(window) {
            return;
        }
        let _ = weak.update(cx, |view, cx| {
            if view.begin_column_resize(event, window, cx) {
                cx.stop_propagation();
            }
        });
    });
    let weak = view.downgrade();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let _ = weak.update(cx, |view, cx| {
            if view.column_resize.drag.borrow().is_some() {
                match event.pressed_button {
                    Some(MouseButton::Left) => {
                        view.update_column_resize(event.position, cx);
                    }
                    None => {
                        // A release outside the editor/window may arrive only
                        // as the next button-free motion. Match that position.
                        view.end_column_resize(event.position, window, cx);
                    }
                    Some(_) => {}
                }
                cx.stop_propagation();
            } else {
                let hovered = hitbox.is_hovered(window)
                    && view.column_resize_hit(event.position, cx).is_some();
                if hovered != view.column_resize.hovered {
                    view.column_resize.hovered = hovered;
                    cx.notify();
                }
            }
        });
    });
    let weak = view.downgrade();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
            return;
        }
        let _ = weak.update(cx, |view, cx| {
            if view.column_resize.drag.borrow().is_some() {
                view.end_column_resize(event.position, window, cx);
                cx.stop_propagation();
            }
        });
    });
}
