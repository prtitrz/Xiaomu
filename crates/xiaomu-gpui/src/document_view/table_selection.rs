//! Native table-range gestures: drag between cells, Shift-click, Alt-click
//! for a single cell, and Alt-Shift-arrows to extend a rectangular range.

use super::{DocumentView, navigation};
use gpui::{Context, Pixels, Point, Window, actions};
use xiaomu_core::document::NodeId;
use xiaomu_runtime::session::DocumentPosition;

actions!(
    xiaomu_gpui,
    [
        /// Extend a rectangular table selection one cell to the left.
        ExtendCellLeft,
        /// Extend a rectangular table selection one cell to the right.
        ExtendCellRight,
        /// Extend a rectangular table selection one row up.
        ExtendCellUp,
        /// Extend a rectangular table selection one row down.
        ExtendCellDown,
    ]
);

impl DocumentView {
    pub(super) fn cell_at(&self, point: Point<Pixels>) -> Option<NodeId> {
        self.cell_bounds
            .borrow()
            .iter()
            .filter(|(_, bounds)| bounds.contains(&point))
            // Nested tables: choose the innermost containing cell.
            .min_by(|(_, a), (_, b)| {
                (f32::from(a.size.width) * f32::from(a.size.height))
                    .total_cmp(&(f32::from(b.size.width) * f32::from(b.size.height)))
            })
            .map(|(cell, _)| *cell)
    }

    pub(super) fn selection_anchor_cell(&self) -> Option<NodeId> {
        let session = self.session.borrow();
        let selection = session.selection();
        if let Some(range) = selection.active_cell_range() {
            return Some(range.anchor());
        }
        let node = match selection.anchor() {
            DocumentPosition::Inline(point) => point.node_id(),
            DocumentPosition::Atomic(node) => node,
            DocumentPosition::Gap(gap) => gap.parent(),
        };
        navigation::table_cell_ancestor(session.document(), node)
    }

    pub(super) fn select_cells(
        &mut self,
        anchor: NodeId,
        focus: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.focused_child_composing(window, cx) {
            return false;
        }
        if self
            .session
            .borrow_mut()
            .set_cell_range_selection(anchor, focus)
            .is_err()
        {
            return false;
        }
        self.desired_x = None;
        self.route_focus(window, cx);
        cx.notify();
        true
    }

    fn extend_cell(&mut self, dr: isize, dc: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(anchor) = self.selection_anchor_cell() else {
            return;
        };
        let target = {
            let session = self.session.borrow();
            let document = session.document();
            let focus = session
                .selection()
                .active_cell_range()
                .map_or(anchor, |range| range.focus());
            let Some(row) = document.parent_of(focus) else {
                return;
            };
            let Some(table) = document.parent_of(row) else {
                return;
            };
            let children = |node| {
                document
                    .node(node)
                    .and_then(|node| node.content().as_children())
            };
            let Some(rows) = children(table) else {
                return;
            };
            let Some(cells) = children(row) else {
                return;
            };
            let Some(r) = rows.iter().position(|id| *id == row) else {
                return;
            };
            let Some(c) = cells.iter().position(|id| *id == focus) else {
                return;
            };
            let r = r.saturating_add_signed(dr).min(rows.len() - 1);
            let c = c.saturating_add_signed(dc).min(cells.len() - 1);
            let Some(target) = children(rows[r]).and_then(|cells| cells.get(c)) else {
                return;
            };
            *target
        };
        self.select_cells(anchor, target, window, cx);
    }

    pub(super) fn extend_cell_left(
        &mut self,
        _: &ExtendCellLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.extend_cell(0, -1, window, cx);
    }
    pub(super) fn extend_cell_right(
        &mut self,
        _: &ExtendCellRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.extend_cell(0, 1, window, cx);
    }
    pub(super) fn extend_cell_up(
        &mut self,
        _: &ExtendCellUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.extend_cell(-1, 0, window, cx);
    }
    pub(super) fn extend_cell_down(
        &mut self,
        _: &ExtendCellDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.extend_cell(1, 0, window, cx);
    }
}
