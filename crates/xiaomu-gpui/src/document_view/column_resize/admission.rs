//! Transfer an already-measured preview only to an identical width-only result.
//! This prevents the next native input from being dropped between release and
//! repaint. It does not authorize a transaction or infer a host width policy.

use super::ResizeDrag;
use crate::{document_view::DocumentView, table_layout::TableLayoutPlan};
use gpui::{Context, Window};
use std::{collections::BTreeSet, rc::Rc};
use xiaomu_core::document::XiaomuDocument;

pub(super) fn only_column_widths_changed(
    before: &XiaomuDocument,
    after: &XiaomuDocument,
    plan: &TableLayoutPlan,
) -> bool {
    if before.root() != after.root() || before.store().len() != after.store().len() {
        return false;
    }
    let cells: BTreeSet<_> = plan
        .cells()
        .iter()
        .map(|cell| cell.placement.cell())
        .collect();
    before
        .store()
        .iter()
        .zip(after.store().iter())
        .all(|(before, after)| {
            before.id() == after.id()
                && before.kind() == after.kind()
                && before.content() == after.content()
                && (before.attrs() == after.attrs()
                    || (cells.contains(&before.id())
                        && before
                            .attrs()
                            .iter()
                            .filter(|(key, _)| *key != "colwidth")
                            .eq(after.attrs().iter().filter(|(key, _)| *key != "colwidth"))))
        })
}

impl DocumentView {
    pub(super) fn readmit_committed_column_preview(
        &mut self,
        drag: &ResizeDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !Rc::ptr_eq(&self.session, &drag.session) {
            return;
        }
        let document = self.session.borrow().document().clone();
        if !only_column_widths_changed(&drag.document, &document, &drag.plan) {
            return;
        }
        let mut capability = self.table_capability.borrow_mut();
        if !capability.enabled() {
            return;
        }
        let Ok(key) = capability.key(&document, drag.intent.table) else {
            return;
        };
        if !key.same_configuration(&drag.key) {
            return;
        }
        let Ok(plan) =
            TableLayoutPlan::from_document(&document, drag.intent.table, Default::default())
        else {
            return;
        };
        let Ok(geometry) = plan.layout(drag.available, &vec![0.0; plan.cells().len()]) else {
            return;
        };
        let mut measurements = self.column_resize.measurements.borrow_mut();
        let Some(measured) = measurements
            .iter_mut()
            .find(|measured| measured.table == drag.intent.table)
        else {
            return;
        };
        if measured.geometry.column_edges != geometry.column_edges
            || measured
                .placements
                .iter()
                .ne(plan.cells().iter().map(|cell| &cell.placement))
        {
            return;
        }
        // Text, kinds, rich subtrees, metadata and decoration inputs are exact;
        // canonical columns now equal the actual measured preview, so its real
        // child heights/caret coordinates remain valid for this new key.
        capability.record(drag.intent.table, key.clone(), true);
        measured.key = key;
        measured.document = document;
        measured.revision = measured.document.revision();
        drop(measurements);
        drop(capability);
        let owned = self
            .focus_handle
            .as_ref()
            .is_some_and(|handle| handle.is_focused(window))
            || self.focused_child(window, cx).is_some();
        if owned {
            self.route_focus(window, cx);
        }
    }
}
