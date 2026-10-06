//! Native entry and focus routing for rectangular cell selections.
//! Runtime owns the rectangle and all edits. The frontend input proxy uses
//! ParagraphView's existing empty-text/IME path, with no canonical fake node.
use gpui::{AppContext as _, Context, Focusable as _, Pixels, Point, Window, actions};
use xiaomu_core::document::{NodeId, XiaomuDocument};
use xiaomu_core::selection::InlinePoint;
use xiaomu_runtime::session::DocumentPosition;

use super::{DocumentView, navigation, visual_navigation::NavStep};
use crate::block_view::ParagraphView;

actions!(xiaomu_gpui, [SelectCell, EscapeCellRange]);

#[cfg(test)]
#[path = "cell_selection_tests.rs"]
mod tests;

fn children(document: &XiaomuDocument, node: NodeId) -> &[NodeId] {
    document
        .node(node)
        .and_then(|node| node.content().as_children())
        .unwrap_or(&[])
}

impl DocumentView {
    pub(super) fn uses_range_input(&self) -> bool {
        self.range_input_anchor().is_some()
    }

    /// The identity of the virtual input surface for the current selection.
    /// It can change without leaving range-selection mode.
    pub(super) fn range_input_anchor(&self) -> Option<NodeId> {
        self.range_input_anchor_for(false)
    }

    fn range_input_anchor_for(&self, building: bool) -> Option<NodeId> {
        let session = self.session.borrow();
        let selection = session.selection();
        let hidden = if building {
            crate::table_capability::selection_has_hidden_table_endpoint_for_build(
                session.document(),
                selection,
                &self.table_capability.borrow(),
            )
        } else {
            self.selection_has_hidden_table_endpoint()
        };
        if hidden {
            return None;
        }
        selection
            .active_cell_range()
            .map(|range| range.anchor())
            .or_else(|| selection.as_node_selection())
            .or_else(|| {
                selection
                    .is_all(session.document())
                    .then_some(session.document().root())
            })
    }

    pub(super) fn sync_range_input(&mut self, cx: &mut Context<Self>) {
        let anchor = self.range_input_anchor_for(self.table_capability.borrow().enabled());
        if self.range_input.as_ref().map(|(cell, _)| *cell) == anchor {
            return;
        }
        self.range_input = anchor.map(|cell| {
            let view = cx.new(|cx| {
                ParagraphView::for_document_range(
                    self.session.clone(),
                    self.epoch.clone(),
                    cell,
                    self.history_clock.clone(),
                    cx,
                )
            });
            view.update(cx, |view, _| {
                view.attach_scroll_handle(self.scroll_handle.clone());
                view.attach_table_capability(self.table_capability.clone());
            });
            cx.observe(&view, |_, _, cx| cx.notify()).detach();
            (cell, view)
        });
    }

    pub(super) fn cell_at_position(&self, position: Point<Pixels>) -> Option<NodeId> {
        let session = self.session.borrow();
        self.cell_registry
            .borrow()
            .iter()
            .rev()
            .find(|(cell, bounds)| {
                bounds.contains(&position)
                    && self
                        .hidden_table_ancestor(session.document(), *cell)
                        .is_none()
            })
            .map(|(cell, _)| *cell)
    }

    pub(super) fn install_cell_range(
        &mut self,
        anchor: NodeId,
        mut focus: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.focused_child_composing(window, cx) {
            return;
        }
        {
            let session = self.session.borrow();
            if [anchor, focus].into_iter().any(|cell| {
                self.hidden_table_ancestor(session.document(), cell)
                    .is_some()
            }) {
                return;
            }
        }
        // Dragging an outer range over a nested cell addresses its ancestor
        // in the anchor's table. A nested range never escapes its own table.
        {
            let session = self.session.borrow();
            let document = session.document();
            let table = document
                .parent_of(anchor)
                .and_then(|row| document.parent_of(row));
            while document
                .parent_of(focus)
                .and_then(|row| document.parent_of(row))
                != table
            {
                let Some(parent) = document.parent_of(focus) else {
                    return;
                };
                let Some(outer) = navigation::table_cell_ancestor(document, parent) else {
                    return;
                };
                focus = outer;
            }
        }
        let outcome = self
            .session
            .borrow_mut()
            .set_cell_range_selection(anchor, focus);
        if outcome.is_ok() {
            self.desired_x = None;
            self.sync_range_input(cx);
            self.route_focus(window, cx);
            cx.notify();
        }
    }

    pub(super) fn begin_cell_range(
        &mut self,
        cell: NodeId,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .hidden_table_ancestor(self.session.borrow().document(), cell)
            .is_some()
        {
            return;
        }
        let anchor = if extend {
            self.session
                .borrow()
                .selection()
                .active_cell_range()
                .map(|range| range.anchor())
                .unwrap_or(cell)
        } else {
            cell
        };
        self.install_cell_range(anchor, cell, window, cx);
        self.is_dragging = false;
        self.cell_drag_anchor = Some(anchor);
    }

    pub(super) fn select_cell(
        &mut self,
        _: &SelectCell,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cell = {
            let session = self.session.borrow();
            let selection = session.selection();
            selection
                .active_cell_range()
                .map(|range| range.focus())
                .or_else(|| {
                    let node = match selection.focus() {
                        DocumentPosition::Inline(point) => point.node_id(),
                        DocumentPosition::Atomic(node) => node,
                        DocumentPosition::Gap(gap) => gap.parent(),
                    };
                    navigation::table_cell_ancestor(session.document(), node)
                })
        };
        if let Some(cell) = cell {
            self.install_cell_range(cell, cell, window, cx);
        }
    }

    pub(super) fn escape_cell_range(
        &mut self,
        _: &EscapeCellRange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.focused_child_composing(window, cx) {
            return;
        }
        self.exit_cell_range(window, cx);
    }

    fn exit_cell_range(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = {
            let session = self.session.borrow();
            let Some(range) = session.selection().active_cell_range() else {
                return;
            };
            self.rendered_nav_units(session.document())
                .into_iter()
                .find(|unit| {
                    let node = match unit {
                        navigation::NavUnit::Text(block) => block.node,
                        navigation::NavUnit::Atomic(node) => *node,
                    };
                    navigation::node_is_within(session.document(), node, range.focus())
                })
        };
        match target {
            Some(navigation::NavUnit::Text(block)) => {
                self.place(InlinePoint::at_start_of(block.node), window, cx)
            }
            Some(navigation::NavUnit::Atomic(node)) => {
                let _ = self.session.borrow_mut().set_atomic_selection(node);
                self.route_focus(window, cx);
                cx.notify();
            }
            None => {}
        }
    }

    pub(super) fn navigate_cell_range(
        &mut self,
        step: &NavStep,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(range) = self.session.borrow().selection().active_cell_range() else {
            return false;
        };
        if self
            .hidden_table_ancestor(self.session.borrow().document(), range.focus())
            .is_some()
        {
            // Stale/unsupported tables cannot use even the logical route until
            // their current geometry is actually admitted by this instance.
            return true;
        }
        if !extend {
            self.exit_cell_range(window, cx);
            return true;
        }
        let next = {
            let session = self.session.borrow();
            let document = session.document();
            let cell = range.focus();
            let row = document.parent_of(cell).expect("validated cell");
            let table = document.parent_of(row).expect("validated row");
            if let Ok(grid) = document.table_grid(table)
                && grid.has_spans()
            {
                let Some(at) = grid.placement(cell) else {
                    return true;
                };
                let target = match step {
                    NavStep::Left => at.column().checked_sub(1).map(|c| (at.row(), c)),
                    NavStep::Right => Some((at.row(), at.column() + at.colspan())),
                    NavStep::Up => at.row().checked_sub(1).map(|r| (r, at.column())),
                    NavStep::Down => Some((at.row() + at.rowspan(), at.column())),
                    NavStep::LineStart => Some((at.row(), 0)),
                    NavStep::LineEnd => grid.columns().checked_sub(1).map(|c| (at.row(), c)),
                };
                let next = target.and_then(|(row, column)| grid.slot(row, column));
                drop(session);
                if let Some(cell) = next {
                    self.install_cell_range(range.anchor(), cell, window, cx);
                }
                return true;
            }
            let rows = children(document, table);
            let cells = children(document, row);
            let r = rows
                .iter()
                .position(|id| *id == row)
                .expect("validated row");
            let c = cells
                .iter()
                .position(|id| *id == cell)
                .expect("validated cell");
            match step {
                NavStep::Left => c.checked_sub(1).and_then(|c| cells.get(c)).copied(),
                NavStep::Right => cells.get(c + 1).copied(),
                NavStep::Up => r
                    .checked_sub(1)
                    .and_then(|r| rows.get(r))
                    .and_then(|row| children(document, *row).get(c))
                    .copied(),
                NavStep::Down => rows
                    .get(r + 1)
                    .and_then(|row| children(document, *row).get(c))
                    .copied(),
                NavStep::LineStart => cells.first().copied(),
                NavStep::LineEnd => cells.last().copied(),
            }
        };
        if let Some(cell) = next {
            self.install_cell_range(range.anchor(), cell, window, cx);
        }
        true
    }

    pub(super) fn range_input_is_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.range_input
            .as_ref()
            .is_some_and(|(_, input)| input.read(cx).focus_handle(cx).is_focused(window))
    }
}
