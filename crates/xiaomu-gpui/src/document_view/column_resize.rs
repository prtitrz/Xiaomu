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
    viewport: Option<super::table_scroll::TableScrollMeasurement>,
    observed_viewport: Option<super::table_scroll::TableScrollMeasurement>,
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
        Self::discard(&mut self.drag.borrow_mut())
    }

    fn discard(state: &mut Option<ResizeDrag>) -> bool {
        let drag = state.take();
        if let Some(viewport) = drag.as_ref().and_then(|drag| drag.viewport.as_ref()) {
            viewport.handle.set_offset(viewport.offset);
        }
        drag.is_some()
    }

    pub(super) fn scroll_preview(
        &self,
        table: xiaomu_core::document::NodeId,
    ) -> Option<super::table_scroll::TableScrollPreview> {
        let drag = self.drag.borrow();
        let drag = drag.as_ref().filter(|drag| drag.intent.table == table)?;
        let original = drag.viewport.clone()?;
        let observed = drag.observed_viewport.clone()?;
        Some(super::table_scroll::TableScrollPreview { original, observed })
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
                Self::discard(&mut drag);
            } else {
                plan.override_column_width(active.intent.column, active.intent.width)?;
            }
        }
        Ok(plan)
    }
}

impl DocumentView {
    /// Reports whether this view owns an unfinished table-column resize.
    ///
    /// Includes a drag on any measured table (including nested tables), a
    /// released preview awaiting measurement, and a queued commit. Returns
    /// false when idle or after cancellation, and before the host commit
    /// callback runs. Hovering alone is not pending work.
    ///
    /// This only observes the current state: it does not validate guards,
    /// cancel or commit a preview, change selection/history, or move focus.
    #[must_use]
    pub fn has_pending_table_column_resize(&self) -> bool {
        self.column_resize.drag.borrow().is_some()
    }

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
                                && measured.matches_origin(drag)
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
        // The next render clears the shared frame registry before constructing
        // its tree. Retain only this validated observation for preview clamping.
        drag.observed_viewport = self
            .column_resize
            .measurements
            .borrow()
            .iter()
            .find(|table| table.table == drag.intent.table)
            .and_then(|table| table.viewport.clone());
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
            ColumnResizeState::discard(&mut state);
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
