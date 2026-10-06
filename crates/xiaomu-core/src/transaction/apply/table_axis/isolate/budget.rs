//! Borrowed conservative admission for one isolation and its exact inverse.
//!
//! Eight payload passes cover construction, exchange and a retained inverse
//! through one Undo/Redo cycle. This deliberately overcounts widths (the whole
//! source list per fragment) and inserted geometry keys. It is not an RSS
//! bound: allocator/BTree overhead, the shared store map, ancestor traversal,
//! grid allocations and arbitrarily many caller/history clones are separate.

use std::mem::size_of;

use crate::transaction::apply::table_span::attrs_budget::{
    MAX_ATTR_DEPTH, MAX_OUTPUT_ATTR_BYTES, MAX_OUTPUT_ATTR_VALUES,
};

use super::*;

const PAYLOAD_PASSES: usize = 8;
const MAX_EDIT_NODES: usize = 1_000_000;

#[cfg(test)]
mod tests;

pub(super) fn preflight(
    context: &ApplyContext,
    grid: &TableGrid,
    rect: TableRect,
) -> Result<usize> {
    let mut budget = OwnedBudget::default();
    let mut added = 0usize;
    let mut at_top = 0usize;
    let mut at_bottom = 0usize;
    for placement in grid.origins() {
        let fragments = Fragments::new(placement, rect);
        let fragments = fragments.as_slice();
        if fragments.len() == 1 {
            continue;
        }
        let extra = fragments.len() - 1;
        added = add(added, extra)?;
        let original = context
            .store
            .get(placement.cell())
            .ok_or(Error::UnknownNode)?;
        // Expected and replacement preserve only the old cell's child IDs.
        budget.node(original, 2)?;
        budget.empty_cells(original.attrs(), extra)?;
        // Changed spans can introduce keys absent from the original attrs.
        budget.scaled(
            (2 * (size_of::<String>() + 7 + size_of::<AttrValue>()), 2),
            fragments.len(),
        )?;
        for fragment in &fragments[1..] {
            if fragment.top != placement.row() {
                if fragment.top == rect.top() {
                    at_top = add(at_top, 1)?;
                } else if fragment.top == rect.bottom() {
                    at_bottom = add(at_bottom, 1)?;
                } else {
                    return Err(Error::InvalidTableStructure);
                }
            }
        }
    }
    if added == 0 {
        return Ok(0);
    }
    budget.node(
        context.store.get(grid.table()).ok_or(Error::UnknownNode)?,
        2,
    )?;
    // Each affected row is counted exactly once, without allocating a row map.
    // Horizontal fragments can enter only the rectangle's top or bottom row.
    for row_index in 0..grid.rows() {
        let row = context
            .store
            .get(grid.row_id(row_index).ok_or(Error::InvalidTableStructure)?)
            .ok_or(Error::UnknownNode)?;
        let children = row
            .content()
            .as_children()
            .ok_or(Error::InvalidTableStructure)?;
        let mut row_added = if row_index == rect.top() { at_top } else { 0 };
        if row_index == rect.bottom() {
            row_added = add(row_added, at_bottom)?;
        }
        for child in children {
            let placement = grid.placement(*child).ok_or(Error::InvalidTableStructure)?;
            let fragments = Fragments::new(placement, rect);
            row_added = add(
                row_added,
                fragments.as_slice()[1..]
                    .iter()
                    .filter(|piece| piece.top == row_index)
                    .count(),
            )?;
        }
        if row_added > 0 {
            budget.node(row, 2)?;
            budget.scaled((size_of::<NodeId>(), 1), row_added)?;
        }
    }
    // One insertion tuple and forward/inverse map payload per new cell. The
    // set's two NodeIds are accounted even though BTree allocator overhead is
    // not. Row output vectors were charged above, including old child IDs.
    budget.scaled(
        (
            size_of::<(usize, usize, NodeId, NodeId)>()
                + 2 * size_of::<StepMap>()
                + 2 * size_of::<NodeId>(),
            2,
        ),
        added,
    )?;
    let nodes = added.checked_mul(2).ok_or(Error::TableResourceLimit)?;
    let cells = add(grid.origins().len(), added)?;
    // Uses the authoritative transaction allocator checked-add rule. Public
    // callers can query can_allocate_node_ids first, but it reserves nothing.
    context.check_axis_growth(grid, grid.rows(), grid.columns(), cells, nodes)?;
    Ok(added)
}

fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or(Error::TableResourceLimit)
}

#[derive(Default)]
struct OwnedBudget {
    bytes: usize,
    values: usize,
    nodes: usize,
}

impl OwnedBudget {
    fn scaled(&mut self, (bytes, values): (usize, usize), copies: usize) -> Result<()> {
        let copies = copies
            .checked_mul(PAYLOAD_PASSES)
            .ok_or(Error::TableResourceLimit)?;
        self.bytes = add(
            self.bytes,
            bytes.checked_mul(copies).ok_or(Error::TableResourceLimit)?,
        )?;
        self.values = add(
            self.values,
            values
                .checked_mul(copies)
                .ok_or(Error::TableResourceLimit)?,
        )?;
        if self.bytes > MAX_OUTPUT_ATTR_BYTES || self.values > MAX_OUTPUT_ATTR_VALUES {
            return Err(Error::TableResourceLimit);
        }
        Ok(())
    }

    fn nodes(&mut self, count: usize) -> Result<()> {
        self.nodes = add(self.nodes, count)?;
        if self.nodes > MAX_EDIT_NODES {
            return Err(Error::TableResourceLimit);
        }
        self.scaled((size_of::<Node>(), 0), count)
    }

    fn node(&mut self, node: &Node, copies: usize) -> Result<()> {
        self.nodes(copies)?;
        self.attrs(node.attrs(), copies)?;
        let children = node
            .content()
            .as_children()
            .ok_or(Error::InvalidTableStructure)?;
        self.scaled(
            (
                children
                    .len()
                    .checked_mul(size_of::<NodeId>())
                    .ok_or(Error::TableResourceLimit)?,
                children.len(),
            ),
            copies,
        )
    }

    fn empty_cells(&mut self, attrs: &NodeAttrs, cells: usize) -> Result<()> {
        self.nodes(cells.checked_mul(2).ok_or(Error::TableResourceLimit)?)?;
        self.attrs(attrs, cells)?;
        self.scaled((size_of::<NodeId>(), 1), cells)
    }

    fn attrs(&mut self, attrs: &NodeAttrs, copies: usize) -> Result<()> {
        for (key, value) in attrs.iter() {
            self.key(key, copies)?;
            self.value(value, 0, copies)?;
        }
        Ok(())
    }

    fn key(&mut self, key: &str, copies: usize) -> Result<()> {
        self.scaled((add(size_of::<String>(), key.len())?, 0), copies)
    }

    fn value(&mut self, value: &AttrValue, depth: usize, copies: usize) -> Result<()> {
        if depth >= MAX_ATTR_DEPTH {
            return Err(Error::TableResourceLimit);
        }
        self.scaled((size_of::<AttrValue>(), 1), copies)?;
        match value {
            AttrValue::String(text) => self.scaled((text.len(), 0), copies)?,
            AttrValue::List(values) => {
                for value in values {
                    self.value(value, depth + 1, copies)?;
                }
            }
            AttrValue::Object(values) => {
                for (key, value) in values {
                    self.key(key, copies)?;
                    self.value(value, depth + 1, copies)?;
                }
            }
            AttrValue::Null | AttrValue::Integer(_) | AttrValue::Bool(_) => {}
        }
        Ok(())
    }
}
