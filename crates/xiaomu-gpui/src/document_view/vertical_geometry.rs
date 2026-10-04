//! Cross-block vertical navigation follows painted rows and table columns.
//! A nested table exits into its enclosing cell before leaving the outer row.
use gpui::{Bounds, Pixels, point, size};
use xiaomu_core::document::{NodeId, XiaomuDocument};

use super::{DocumentView, navigation};

fn children(document: &XiaomuDocument, node: NodeId) -> &[NodeId] {
    document
        .node(node)
        .and_then(|node| node.content().as_children())
        .unwrap_or(&[])
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
        if navigation::spanning_table_ancestor(document, node).is_some() {
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
            let rows = children(document, table);
            let row_index = rows.iter().position(|id| *id == row)?;
            let column = children(document, row).iter().position(|id| *id == cell)?;
            let indices: Vec<_> = if down {
                (row_index + 1..rows.len()).collect()
            } else {
                (0..row_index).rev().collect()
            };
            // Atomic-only cells have no editable visual line; Tab still
            // visits them as whole-node selections. Up/Down seek text rows.
            for index in indices {
                let target = *children(document, rows[index]).get(column)?;
                let bounds = self
                    .cell_registry
                    .borrow()
                    .iter()
                    .find(|(id, _)| *id == target)?
                    .1;
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
            if navigation::spanning_table_ancestor(document, *node).is_some()
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
