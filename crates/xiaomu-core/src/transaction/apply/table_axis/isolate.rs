//! Bounded rectangle-boundary isolation using the logical-axis staged exchange.

mod budget;
mod geometry;

#[cfg(test)]
mod tests;

use crate::document::{CellPlacement, TableRect};

use super::*;
use geometry::{Fragment, Fragments};

impl ApplyContext {
    pub(in crate::transaction::apply) fn apply_isolate_table_rect(
        &mut self,
        table: NodeId,
        rect: TableRect,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let grid = self.logical_grid(table)?;
        if rect.bottom() > grid.rows() || rect.right() > grid.columns() {
            return Err(Error::InvalidSelection);
        }
        let added = budget::preflight(self, &grid, rect)?;
        if added == 0 {
            return Ok((Vec::new(), Vec::new()));
        }
        // No owned patch, fragment vector, attribute clone or fresh identity
        // exists until all borrowed payload, grid and allocator checks pass.
        let mut edit = TableEdit::new(self, table)?;
        let mut insertions = Vec::new();
        insertions
            .try_reserve_exact(added)
            .map_err(|_| Error::TableResourceLimit)?;
        for placement in grid.origins() {
            let fragments = Fragments::new(placement, rect);
            let fragments = fragments.as_slice();
            if fragments.len() == 1 {
                continue;
            }
            let original = self.store.get(placement.cell()).ok_or(Error::UnknownNode)?;
            // The first piece contains the original top-left; descendants are
            // referenced by their old IDs, never cloned into the empty pieces.
            edit.replace(
                self,
                Node::new(
                    original.id(),
                    original.kind().clone(),
                    fragment_attrs(original.attrs(), placement, fragments[0])?,
                    original.content().clone(),
                )?,
            )?;
            for fragment in &fragments[1..] {
                let paragraph = edit.allocate(NodeKind::Paragraph, NodeContent::empty_inline())?;
                let cell = edit.allocate_with_attrs(
                    original.kind().clone(),
                    fragment_attrs(original.attrs(), placement, *fragment)?,
                    NodeContent::children([paragraph]),
                )?;
                insertions.push((fragment.top, fragment.left, cell, paragraph));
            }
        }
        insertions.sort_unstable_by_key(|entry| (entry.0, entry.1));
        let mut start = 0;
        while start < insertions.len() {
            let row_index = insertions[start].0;
            let mut end = start + 1;
            while end < insertions.len() && insertions[end].0 == row_index {
                end += 1;
            }
            let row = grid.row_id(row_index).ok_or(Error::InvalidTableStructure)?;
            let old = self.store.get(row).ok_or(Error::UnknownNode)?;
            let children = old
                .content()
                .as_children()
                .ok_or(Error::InvalidTableStructure)?;
            let count = children
                .len()
                .checked_add(end - start)
                .ok_or(Error::TableResourceLimit)?;
            let mut next = Vec::new();
            next.try_reserve_exact(count)
                .map_err(|_| Error::TableResourceLimit)?;
            let mut old_index = 0;
            for (_, column, cell, paragraph) in &insertions[start..end] {
                while let Some(child) = children.get(old_index) {
                    let old_column = grid
                        .placement(*child)
                        .ok_or(Error::InvalidTableStructure)?
                        .column();
                    if old_column >= *column {
                        break;
                    }
                    next.push(*child);
                    old_index += 1;
                }
                let index = next.len();
                next.push(*cell);
                edit.record(
                    StepMap::NodeInserted {
                        parent: row,
                        index,
                        inserted: *cell,
                    },
                    StepMap::NodeRemoved {
                        parent: row,
                        index,
                        removed: BTreeSet::from([*cell, *paragraph]),
                    },
                );
            }
            next.extend_from_slice(&children[old_index..]);
            edit.rewrite_children(self, row, next)?;
            start = end;
        }
        edit.finish(self)
    }
}

fn fragment_attrs(
    attrs: &NodeAttrs,
    placement: &CellPlacement,
    fragment: Fragment,
) -> Result<NodeAttrs> {
    let columns = fragment.right - fragment.left;
    let rows = fragment.bottom - fragment.top;
    let sliced = columns != placement.colspan();
    let mut values: BTreeMap<_, _> = attrs
        .iter()
        .filter(|(key, _)| !sliced || *key != "colwidth")
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect();
    for (key, old, new) in [
        ("rowspan", placement.rowspan(), rows),
        ("colspan", placement.colspan(), columns),
    ] {
        if old != new {
            values.insert(
                key.to_owned(),
                AttrValue::Integer(i64::try_from(new).map_err(|_| Error::InvalidTableAttrs)?),
            );
        }
    }
    if sliced && let Some(widths) = attrs.get("colwidth") {
        let value = match widths {
            AttrValue::List(widths) => {
                let start = fragment.left - placement.column();
                let widths = widths
                    .get(start..start + columns)
                    .ok_or(Error::InvalidTableAttrs)?;
                if widths.iter().all(|width| *width == AttrValue::Integer(0)) {
                    AttrValue::Null
                } else {
                    AttrValue::List(widths.to_vec())
                }
            }
            other => other.clone(),
        };
        values.insert("colwidth".to_owned(), value);
    }
    NodeAttrs::new(values)
}
