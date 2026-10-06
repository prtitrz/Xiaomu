//! Measured, per-view transient resize state and window-wide pointer ownership.

mod admission;
mod geometry;
mod pointer;
#[cfg(test)]
mod tests;

use std::{cell::RefCell, rc::Rc};

use gpui::{App, Context, Pixels, Point, Window};
use xiaomu_core::document::XiaomuDocument;

use super::DocumentView;
use crate::block_view::SharedSession;
use crate::table_capability::TableCapabilityKey;
use crate::table_column_resize::{TableColumnResize, TableColumnResizeIntent};
use crate::table_layout::{TableLayoutError, TableLayoutPlan};

pub(super) use geometry::ResizeMeasurement;
pub(super) use pointer::{paint_resize_cursor, register_resize_pointer_handlers};

#[derive(Default)]
pub(super) struct ColumnResizeState {
    capability: Option<TableColumnResize>,
    pub measurements: Rc<RefCell<Vec<ResizeMeasurement>>>,
    drag: RefCell<Option<ResizeDrag>>,
    hovered: bool,
}

struct ResizeDrag {
    intent: TableColumnResizeIntent,
    start_x: f32,
    session: SharedSession,
    document: XiaomuDocument,
    plan: TableLayoutPlan,
    key: Rc<TableCapabilityKey>,
    available: f32,
    origin: Point<Pixels>,
    released: bool,
    commit_queued: bool,
    token: Rc<()>,
}

impl ColumnResizeState {
    pub(super) fn enabled(&self) -> bool {
        self.capability
            .as_ref()
            .is_some_and(|capability| capability.config.valid())
    }

    pub(super) fn cancel(&self) -> bool {
        self.drag.borrow_mut().take().is_some()
    }

    pub(super) fn clear_measurements(&self) {
        self.measurements.borrow_mut().clear();
    }

    pub(super) fn preview_plan(
        &self,
        mut plan: TableLayoutPlan,
        available: f32,
    ) -> Result<TableLayoutPlan, TableLayoutError> {
        let mut drag = self.drag.borrow_mut();
        if let Some(active) = drag.as_ref()
            && active.intent.table == plan.table()
        {
            if active.available != available {
                *drag = None;
            } else {
                plan.override_column_width(active.intent.column, active.intent.width)?;
            }
        }
        Ok(plan)
    }
}

impl DocumentView {
    /// Installs this view's optional measured column-drag capability.
    ///
    /// Disabled by default and dependent on `set_measured_table_layout(true)`.
    /// Every call cancels any current preview, including replacing callbacks.
    /// Notify a mounted context after changing this setting. The host owns its
    /// authorization, canonical attribute mapping and persistence; this API
    /// never changes Core/runtime behavior or enables edits on its own.
    pub fn set_table_column_resize(&mut self, capability: Option<TableColumnResize>) {
        self.column_resize.cancel();
        self.column_resize.capability = capability;
        self.column_resize.hovered = false;
        if !self.column_resize.enabled() {
            self.column_resize.clear_measurements();
        }
    }

    pub(super) fn validate_column_resize(&self, cx: &App) -> bool {
        let valid = self
            .column_resize
            .drag
            .borrow()
            .as_ref()
            .is_none_or(|drag| {
                let Some(capability) = &self.column_resize.capability else {
                    return false;
                };
                let session = self.session.borrow();
                let document = session.document();
                Rc::ptr_eq(&self.session, &drag.session)
                    && document.root() == drag.document.root()
                    && document.revision() == drag.intent.revision
                    && document.store() == drag.document.store()
                    && self.table_capability.borrow().enabled()
                    && self
                        .table_capability
                        .borrow()
                        .hidden_ancestor(document, drag.intent.table)
                        .is_none()
                    && !self.has_active_composition(cx)
                    && (capability.guard)(&session, &drag.intent)
                    && self
                        .column_resize
                        .measurements
                        .borrow()
                        .iter()
                        .any(|measured| {
                            measured.table == drag.intent.table
                                && measured.key == drag.key
                                && measured.available == drag.available
                                && measured.origin == drag.origin
                        })
            });
        if !valid {
            self.column_resize.cancel();
        }
        valid
    }

    pub(super) fn finish_column_resize_measurement(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.validate_column_resize(cx) {
            cx.notify();
            return;
        }
        let mut state = self.column_resize.drag.borrow_mut();
        let Some(drag) = state.as_mut() else { return };
        if !drag.released || drag.commit_queued {
            return;
        }
        // The release position itself may differ from the final move. Never
        // commit until that exact preview has successfully measured real child
        // trees, including heights, nesting, clipping and native input bounds.
        let measured = self.column_resize.measurements.borrow();
        let exact = measured
            .iter()
            .find(|table| table.table == drag.intent.table)
            .is_some_and(|table| {
                table.geometry.column_edges[drag.intent.column + 1]
                    - table.geometry.column_edges[drag.intent.column]
                    == drag.intent.width as f32
            });
        if !exact {
            *state = None;
            cx.notify();
            return;
        }
        drag.commit_queued = true;
        let token = drag.token.clone();
        let weak = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let _ = weak.update(cx, |view, cx| {
                view.commit_measured_column_resize(&token, window, cx)
            });
        });
    }

    fn commit_measured_column_resize(
        &mut self,
        token: &Rc<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .column_resize
            .drag
            .borrow()
            .as_ref()
            .is_none_or(|drag| !Rc::ptr_eq(&drag.token, token))
        {
            return;
        }
        if !self.validate_column_resize(cx) {
            cx.notify();
            return;
        }
        let ready = self
            .column_resize
            .drag
            .borrow()
            .as_ref()
            .is_some_and(|drag| drag.released && drag.commit_queued);
        if !ready {
            return;
        }
        let drag = self
            .column_resize
            .drag
            .borrow_mut()
            .take()
            .expect("ready gesture");
        let commit = self
            .column_resize
            .capability
            .as_ref()
            .expect("validated capability")
            .commit
            .clone();
        // No preview or session borrow survives into host code. It owns the
        // immediate final policy check and one canonical transaction.
        commit(drag.intent, self, window, cx);
        self.readmit_committed_column_preview(&drag, window, cx);
        cx.notify();
    }
}

impl Drop for ColumnResizeState {
    fn drop(&mut self) {
        // Retained frame elements never own a committing gesture after disposal.
        self.cancel();
    }
}
