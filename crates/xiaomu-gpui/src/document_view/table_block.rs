//! Table presentation (P5.4).
//!
//! A table renders as a bordered grid: one flex row per table row, one
//! bordered cell per table cell. Cells recurse through the ordinary block
//! renderer, so every block kind inside a cell keeps its existing
//! presentation, caret, IME, and hit-test seams. The grid and the cell
//! holding the selection gain a focus affordance; clicking a cell's padding
//! resolves through the shared paint registry to that row's nearest text
//! block, so no table-specific pointer path exists.

use gpui::{Context, IntoElement, ParentElement, Styled, div, px};
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
            let mut row_element = div().flex().flex_row();
            let last_cell = cells.len().saturating_sub(1);
            for (cell_index, cell) in cells.into_iter().enumerate() {
                let mut cell_element = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(28.0))
                    .px_2()
                    .py_1();
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
                row_element = row_element.child(cell_element);
            }
            grid = grid.child(row_element);
        }
        grid.into_any_element()
    }
}
