//! Opt-in closed logical rectangles retain real physical origin rows.

use xiaomu_core::document::{NodeAttrs, NodeKind, XiaomuDocument};

use super::projection::whole_fragment;
use super::{ClipboardCellRangeRoot, ClipboardNode, ClipboardNodeContent, ClipboardSourceBoundary};
use crate::session::{CellRange, SessionError};

pub(super) fn capture(
    document: &XiaomuDocument,
    range: CellRange,
) -> Result<(Vec<ClipboardNode>, ClipboardSourceBoundary), SessionError> {
    let table = document
        .parent_of(range.anchor())
        .and_then(|row| document.parent_of(row))
        .ok_or(SessionError::SelectionInvalid)?;
    let grid = document.table_grid(table).map_err(SessionError::Core)?;
    let rect = grid
        .rect_between(range.anchor(), range.focus())
        .map_err(SessionError::Core)?;
    if !grid.is_closed_rect(rect) {
        return Err(SessionError::UnsupportedTableOperation);
    }
    let table = document.node(table).ok_or(SessionError::SelectionInvalid)?;
    let source_rows = table
        .content()
        .as_children()
        .ok_or(SessionError::SelectionInvalid)?;
    let mut rows = Vec::with_capacity(rect.bottom() - rect.top());
    let mut row_attrs = Vec::with_capacity(rows.capacity());
    for row_id in &source_rows[rect.top()..rect.bottom()] {
        let row = document
            .node(*row_id)
            .ok_or(SessionError::SelectionInvalid)?;
        let mut cells = Vec::new();
        for cell in row
            .content()
            .as_children()
            .ok_or(SessionError::SelectionInvalid)?
        {
            let placement = grid
                .placement(*cell)
                .ok_or(SessionError::SelectionInvalid)?;
            if (rect.left()..rect.right()).contains(&placement.column()) {
                cells.push(whole_fragment(document, *cell)?);
            }
        }
        rows.push(cells);
        row_attrs.push(row.attrs().clone());
    }
    if row_attrs.iter().all(NodeAttrs::is_empty) {
        row_attrs.clear();
    }
    let root_form = if rect.top() == 0
        && rect.left() == 0
        && rect.bottom() == grid.rows()
        && rect.right() == grid.columns()
    {
        ClipboardCellRangeRoot::Table
    } else {
        ClipboardCellRangeRoot::Rows
    };
    Ok((
        vec![ClipboardNode::new(
            NodeKind::Table,
            table.attrs().clone(),
            ClipboardNodeContent::Table { rows, row_attrs },
        )],
        ClipboardSourceBoundary::CellRange { root_form },
    ))
}
