//! Cross-block vertical navigation follows painted rows and table columns.
//! A nested table exits into its enclosing cell before leaving the outer row.
use std::collections::{HashMap, HashSet};

use gpui::{Bounds, Pixels, point, size};
use xiaomu_core::document::{NodeId, TableGrid, XiaomuDocument};

use super::{DocumentView, navigation};

/// Resolves the next distinct cells in travel order from checked logical rows.
/// Unit grids retain the legacy fixed-column policy; spanning grids use the
/// measured full-cell x intervals, which already share the layout's column
/// edges. Looking at paragraph widths or interpolating equal columns would
/// misroute navigation for short text, colspans and unequal column widths.
fn vertical_cell_candidates(
    grid: &TableGrid,
    cell_bounds: &[(NodeId, Bounds<Pixels>)],
    source: NodeId,
    x: Pixels,
    down: bool,
) -> Option<Vec<(NodeId, Bounds<Pixels>)>> {
    let placement = grid.placement(source)?;
    let rows: Vec<_> = if down {
        (placement.row().checked_add(placement.rowspan())?..grid.rows()).collect()
    } else {
        (0..placement.row()).rev().collect()
    };
    let bounds_by_cell: HashMap<_, _> = cell_bounds.iter().copied().collect();
    let has_spans = grid.has_spans();
    let mut visited = HashSet::from([source]);
    let mut candidates = Vec::new();
    for row in rows {
        let target = if has_spans {
            let mut best: Option<(NodeId, (Pixels, bool))> = None;
            let mut previous = None;
            for column in 0..grid.columns() {
                let cell = grid.slot(row, column)?;
                // One origin may cover several adjacent logical columns.
                if previous == Some(cell) {
                    continue;
                }
                previous = Some(cell);
                let bounds = bounds_by_cell.get(&cell)?;
                let horizontal = (bounds.left() - x)
                    .max(x - bounds.right())
                    .max(Pixels::ZERO);
                // Shared edges belong to the cell on their right. Include the
                // final outer edge so it remains inside the last column.
                let target = grid.placement(cell)?;
                let last_column = target.column() + target.colspan() == grid.columns();
                let contains = x >= bounds.left()
                    && (x < bounds.right() || (last_column && x == bounds.right()));
                let score = (horizontal, !contains);
                if best.is_none_or(|(_, previous)| score < previous) {
                    best = Some((cell, score));
                }
            }
            best?.0
        } else {
            grid.slot(row, placement.column())?
        };
        // Covered rows repeat an origin. Never revisit its editable subtree
        // (or return the source) when an atomic-only cell needs to be skipped.
        if visited.insert(target) {
            candidates.push((target, *bounds_by_cell.get(&target)?));
        }
    }
    Some(candidates)
}

impl DocumentView {
    pub(super) fn block_bounds(&self, node: NodeId) -> Option<Bounds<Pixels>> {
        self.registry
            .borrow()
            .iter()
            .find(|(id, _)| *id == node)
            .map(|(_, bounds)| *bounds)
    }

    pub(super) fn vertical_neighbor(&self, node: NodeId, x: Pixels, down: bool) -> Option<NodeId> {
        let session = self.session.borrow();
        let document = session.document();
        if self.hidden_table_ancestor(document, node).is_some() {
            return None;
        }
        let mut scope = navigation::table_cell_ancestor(document, node);
        let mut source = self.block_bounds(node)?;
        let mut excluded = Some(node);
        loop {
            if let Some(target) =
                self.nearest_vertical_block(document, scope, excluded, source, x, down)
            {
                return Some(target);
            }
            let cell = scope?;
            let row = document.parent_of(cell)?;
            let table = document.parent_of(row)?;
            let grid = document.table_grid(table).ok()?;
            let candidates =
                vertical_cell_candidates(&grid, &self.cell_registry.borrow(), cell, x, down)?;
            // Atomic-only cells have no editable visual line; Tab still
            // visits them as whole-node selections. Up/Down seek text rows.
            for (target, bounds) in candidates {
                let edge = if down { bounds.top() } else { bounds.bottom() };
                let origin =
                    Bounds::new(point(bounds.left(), edge), size(Pixels::ZERO, Pixels::ZERO));
                if let Some(node) =
                    self.nearest_vertical_block(document, Some(target), None, origin, x, down)
                {
                    return Some(node);
                }
            }
            // No more rows: treat this whole table as the source block
            // while searching the enclosing cell (or ordinary document).
            let bounds: Vec<_> = self
                .cell_registry
                .borrow()
                .iter()
                .filter(|(id, _)| navigation::node_is_within(document, *id, table))
                .map(|(_, bounds)| *bounds)
                .collect();
            let first = *bounds.first()?;
            let (mut left, mut top, mut right, mut bottom) =
                (first.left(), first.top(), first.right(), first.bottom());
            for b in bounds {
                left = left.min(b.left());
                top = top.min(b.top());
                right = right.max(b.right());
                bottom = bottom.max(b.bottom());
            }
            source = Bounds::new(point(left, top), size(right - left, bottom - top));
            excluded = Some(table);
            scope = document
                .parent_of(table)
                .and_then(|parent| navigation::table_cell_ancestor(document, parent));
        }
    }

    fn nearest_vertical_block(
        &self,
        document: &XiaomuDocument,
        scope: Option<NodeId>,
        excluded: Option<NodeId>,
        source: Bounds<Pixels>,
        x: Pixels,
        down: bool,
    ) -> Option<NodeId> {
        let mut best: Option<(NodeId, (Pixels, Pixels, Pixels, Pixels))> = None;
        for (node, bounds) in self.registry.borrow().iter() {
            if self.hidden_table_ancestor(document, *node).is_some()
                || scope.is_some_and(|scope| !navigation::node_is_within(document, *node, scope))
                || excluded
                    .is_some_and(|excluded| navigation::node_is_within(document, *node, excluded))
            {
                continue;
            }
            // When entering a table, full row-cell bounds make a short
            // column and a tall column equally near vertically. Choose
            // the visual column before looking at its individual blocks.
            let band = self
                .entry_cell_bounds(document, *node, scope)
                .unwrap_or(*bounds);
            let vertical = if down {
                band.top() - source.bottom()
            } else {
                source.top() - band.bottom()
            };
            if vertical < Pixels::ZERO {
                continue;
            }
            let horizontal = (band.left() - x).max(x - band.right()).max(Pixels::ZERO);
            let inner_vertical = if down {
                bounds.top() - band.top()
            } else {
                band.bottom() - bounds.bottom()
            };
            let inner_horizontal = (bounds.left() - x)
                .max(x - bounds.right())
                .max(Pixels::ZERO);
            let score = (vertical, horizontal, inner_vertical, inner_horizontal);
            if best.is_none_or(|(_, previous)| score < previous) {
                best = Some((*node, score));
            }
        }
        best.map(|(node, _)| node)
    }

    fn entry_cell_bounds(
        &self,
        document: &XiaomuDocument,
        node: NodeId,
        scope: Option<NodeId>,
    ) -> Option<Bounds<Pixels>> {
        let mut cell = navigation::table_cell_ancestor(document, node)?;
        if Some(cell) == scope {
            return None;
        }
        loop {
            let parent = document.parent_of(cell)?;
            match navigation::table_cell_ancestor(document, parent) {
                Some(outer) if Some(outer) != scope => cell = outer,
                _ => break,
            }
        }
        self.cell_registry
            .borrow()
            .iter()
            .find(|(id, _)| *id == cell)
            .map(|(_, bounds)| *bounds)
    }
}

#[cfg(test)]
mod tests {
    use gpui::px;
    use xiaomu_core::document::{AttrValue, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder};

    use super::*;

    fn grid(rows: &[&[(usize, usize)]]) -> (TableGrid, Vec<NodeId>) {
        let mut builder = NodeStoreBuilder::new();
        let mut cells = Vec::new();
        let mut row_ids = Vec::new();
        for row in rows {
            let mut row_cells = Vec::new();
            for &(rowspan, colspan) in *row {
                let paragraph = builder
                    .insert(
                        NodeKind::Paragraph,
                        NodeAttrs::empty(),
                        NodeContent::empty_inline(),
                    )
                    .unwrap();
                let attrs = NodeAttrs::new(
                    [
                        ("rowspan".to_owned(), AttrValue::Integer(rowspan as i64)),
                        ("colspan".to_owned(), AttrValue::Integer(colspan as i64)),
                    ]
                    .into(),
                )
                .unwrap();
                let cell = builder
                    .insert(
                        NodeKind::TableCell,
                        attrs,
                        NodeContent::children([paragraph]),
                    )
                    .unwrap();
                cells.push(cell);
                row_cells.push(cell);
            }
            row_ids.push(
                builder
                    .insert(
                        NodeKind::TableRow,
                        NodeAttrs::empty(),
                        NodeContent::children(row_cells),
                    )
                    .unwrap(),
            );
        }
        let table = builder
            .insert(
                NodeKind::Table,
                NodeAttrs::empty(),
                NodeContent::children(row_ids),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([table]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        (document.table_grid(table).unwrap(), cells)
    }

    fn measured_cells(grid: &TableGrid, edges: &[f32]) -> Vec<(NodeId, Bounds<Pixels>)> {
        grid.origins()
            .map(|cell| {
                let left = edges[cell.column()];
                let right = edges[cell.column() + cell.colspan()];
                (
                    cell.cell(),
                    Bounds::new(
                        point(px(left), px(cell.row() as f32 * 30.0)),
                        size(px(right - left), px(cell.rowspan() as f32 * 30.0)),
                    ),
                )
            })
            .collect()
    }

    fn targets(
        grid: &TableGrid,
        bounds: &[(NodeId, Bounds<Pixels>)],
        source: NodeId,
        x: f32,
        down: bool,
    ) -> Vec<NodeId> {
        vertical_cell_candidates(grid, bounds, source, px(x), down)
            .unwrap()
            .into_iter()
            .map(|(cell, _)| cell)
            .collect()
    }

    #[test]
    fn rowspan_navigation_starts_after_full_source_extent() {
        let (grid, cells) = grid(&[&[(2, 1), (1, 1)], &[(1, 1)], &[(1, 1), (1, 1)]]);
        let bounds = measured_cells(&grid, &[0.0, 45.0, 300.0]);
        assert_eq!(targets(&grid, &bounds, cells[0], 20.0, true), [cells[3]]);
        assert_eq!(targets(&grid, &bounds, cells[3], 20.0, false), [cells[0]]);
        assert!(targets(&grid, &bounds, cells[0], 20.0, false).is_empty());
    }

    #[test]
    fn colspan_navigation_uses_real_shared_column_edges() {
        let (grid, cells) = grid(&[&[(1, 2)], &[(1, 1), (1, 1)]]);
        let bounds = measured_cells(&grid, &[100.0, 140.0, 400.0]);
        assert_eq!(targets(&grid, &bounds, cells[0], 125.0, true), [cells[1]]);
        // x=160 is still in the left half of the spanning source, but it is
        // already in logical column 1 because the first column is only 40px.
        assert_eq!(targets(&grid, &bounds, cells[0], 160.0, true), [cells[2]]);
        assert_eq!(targets(&grid, &bounds, cells[2], 160.0, false), [cells[0]]);
    }

    #[test]
    fn shared_boundary_chooses_right_cell_and_outer_edges_clamp() {
        let (grid, cells) = grid(&[&[(1, 2)], &[(1, 1), (1, 1)]]);
        let bounds = measured_cells(&grid, &[100.0, 140.0, 400.0]);
        assert_eq!(targets(&grid, &bounds, cells[0], 140.0, true), [cells[2]]);
        assert_eq!(targets(&grid, &bounds, cells[0], 400.0, true), [cells[2]]);
        assert_eq!(targets(&grid, &bounds, cells[0], 450.0, true), [cells[2]]);
        assert_eq!(targets(&grid, &bounds, cells[0], 50.0, true), [cells[1]]);
    }

    #[test]
    fn repeated_covered_origins_are_visited_once() {
        let (grid, cells) = grid(&[&[(1, 1), (3, 1)], &[(1, 1)], &[(1, 1)], &[(1, 1), (1, 1)]]);
        let bounds = measured_cells(&grid, &[0.0, 40.0, 300.0]);
        // A carried x may lie outside the source cell. The right-hand rowspan
        // is one candidate, even though subsequent logical rows repeat it.
        assert_eq!(
            targets(&grid, &bounds, cells[0], 100.0, true),
            [cells[1], cells[5]]
        );
        assert_eq!(targets(&grid, &bounds, cells[5], 100.0, false), [cells[1]]);
    }

    #[test]
    fn fully_covered_rows_do_not_return_source_cell() {
        let (grid, cells) = grid(&[&[(3, 2)], &[], &[], &[(1, 1), (1, 1)]]);
        let bounds = measured_cells(&grid, &[0.0, 40.0, 300.0]);
        assert_eq!(targets(&grid, &bounds, cells[0], 100.0, true), [cells[2]]);
        assert_eq!(targets(&grid, &bounds, cells[2], 100.0, false), [cells[0]]);
    }

    #[test]
    fn unit_grid_preserves_legacy_column_despite_carried_x() {
        let (grid, cells) = grid(&[&[(1, 1), (1, 1)], &[(1, 1), (1, 1)]]);
        let bounds = measured_cells(&grid, &[0.0, 40.0, 300.0]);
        assert_eq!(targets(&grid, &bounds, cells[0], 250.0, true), [cells[2]]);
        assert_eq!(targets(&grid, &bounds, cells[3], 0.0, false), [cells[1]]);
    }

    #[test]
    fn missing_measured_cell_bounds_refuse_horizontal_guess() {
        let (grid, cells) = grid(&[&[(1, 2)], &[(1, 1), (1, 1)]]);
        let mut bounds = measured_cells(&grid, &[0.0, 40.0, 300.0]);
        bounds.retain(|(cell, _)| *cell != cells[2]);
        assert!(vertical_cell_candidates(&grid, &bounds, cells[0], px(100.0), true).is_none());
    }
}
