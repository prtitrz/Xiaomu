//! Table presentation (P5.4).
//!
//! A table renders as a bordered grid: one flex row per table row, one
//! bordered cell per table cell. Cells recurse through the ordinary block
//! renderer, so every block kind inside a cell keeps its existing
//! presentation, caret, IME, and hit-test seams. The grid and the cell
//! holding the selection gain a focus affordance. Full cell bounds constrain
//! pointer hits before the ordinary per-block caret projection, including
//! blank space below a shorter cell's content and nested table cells.

use gpui::{
    Context, InteractiveElement as _, IntoElement, MouseButton, ParentElement, Styled, canvas, div,
    px,
};
use xiaomu_core::document::NodeId;
use xiaomu_runtime::session::DocumentPosition;

use super::{DocumentView, navigation};

impl DocumentView {
    /// Renders one table as a bordered grid of cell columns.
    pub(super) fn render_table(
        &self,
        table: NodeId,
        index: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if navigation::table_needs_placeholder(self.session.borrow().document(), table) {
            // The legacy flex-row renderer is only correct for unit cells.
            // Never paint a ragged physical row as a different logical table.
            // No product profile admits these tables until the span renderer
            // and its geometry/selection/IME tests are connected.
            return div()
                .debug_selector(move || format!("unsupported-spanning-table-{table:?}"))
                .border_1()
                .p_2()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child("Merged-cell table layout is not yet enabled")
                .into_any_element();
        }
        let rows: Vec<NodeId> = {
            let session = self.session.borrow();
            session
                .document()
                .node(table)
                .and_then(|node| node.content().as_children().map(<[NodeId]>::to_vec))
                .unwrap_or_default()
        };
        let (focused, highlight) = {
            let session = self.session.borrow();
            let document = session.document();
            let selection = session.selection();
            let focus = selection.focus();
            let focused = navigation::selection_is_within(document, focus, table);
            let focus_cell = match focus {
                DocumentPosition::Inline(point) => {
                    navigation::table_cell_ancestor(document, point.node_id())
                }
                DocumentPosition::Atomic(node) => navigation::table_cell_ancestor(document, node),
                DocumentPosition::Gap(gap) => {
                    navigation::table_cell_ancestor(document, gap.parent())
                }
            };
            // An active cell range highlights its whole rectangle; otherwise
            // only the focused cell lights up.
            let mut highlight = focus_cell.into_iter().collect::<Vec<_>>();
            if let Some(range) = selection.active_cell_range()
                && let Some(rect) = navigation::cell_range_rect(document, range)
            {
                highlight = rect;
            }
            (focused, highlight)
        };

        let border = if focused {
            gpui::rgba(0x2b6cb8ff)
        } else {
            gpui::rgba(0xccccccff)
        };
        let mut grid = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .border_1()
            .border_color(border)
            .my_3();
        let last_row = rows.len().saturating_sub(1);
        for (row_index, row) in rows.into_iter().enumerate() {
            let cells: Vec<NodeId> = {
                let session = self.session.borrow();
                session
                    .document()
                    .node(row)
                    .and_then(|node| node.content().as_children().map(<[NodeId]>::to_vec))
                    .unwrap_or_default()
            };
            let mut row_element = div().flex().flex_row().w_full().min_w_0();
            let last_cell = cells.len().saturating_sub(1);
            for (cell_index, cell) in cells.into_iter().enumerate() {
                let cell_registry = self.cell_registry.clone();
                let mut cell_element = div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h(px(28.0))
                    .px_2()
                    .py_1()
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, (), _, _| {
                                cell_registry.borrow_mut().push((cell, bounds));
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    );
                if highlight.contains(&cell) {
                    cell_element = cell_element.bg(gpui::rgba(0xeef4fbff));
                }
                if cell_index != last_cell {
                    cell_element = cell_element
                        .border_r_1()
                        .border_color(gpui::rgba(0xccccccff));
                }
                if row_index != last_row {
                    cell_element = cell_element
                        .border_b_1()
                        .border_color(gpui::rgba(0xccccccff));
                }
                cell_element = cell_element.child(self.render_block_tree(
                    cell,
                    false,
                    0,
                    index + row_index + cell_index,
                    cx,
                ));
                if let Some((anchor, input)) = &self.range_input
                    && *anchor == cell
                {
                    let mut proxy = div().absolute().top_0().left_0().w_full().px_2().py_1();
                    if input.read(cx).is_composing() {
                        proxy = proxy.bg(gpui::white());
                    }
                    cell_element = cell_element.child(proxy.child(input.clone()));
                }
                cell_element = cell_element.child(
                    div()
                        .id(gpui::SharedString::from(format!("cell-select-{cell:?}")))
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(px(7.0))
                        .h(px(7.0))
                        .bg(gpui::rgba(0x718096ff))
                        .cursor(gpui::CursorStyle::Crosshair)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.begin_cell_range(cell, event.modifiers.shift, window, cx);
                            }),
                        ),
                );
                row_element = row_element.child(cell_element);
            }
            grid = grid.child(row_element);
        }
        grid.into_any_element()
    }
}
